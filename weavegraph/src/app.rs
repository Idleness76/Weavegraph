use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::channels::Channel;
use crate::message::*;
use crate::node::*;
use crate::reducers::ReducerRegistry;
use crate::runtimes::runner::RunnerError;
use crate::runtimes::{CheckpointerType, RuntimeConfig, SessionInit};
use crate::state::*;
use crate::types::*;
use crate::utils::collections::new_extra_map;
use tracing::instrument;

/// Orchestrates graph execution and applies reducers at barriers.
///
/// `App` is the central coordination point for workflow execution, managing:
/// - Node graph topology (nodes, edges, conditional routing)
/// - State reduction through configurable reducers
/// - Runtime configuration and checkpointing
///
/// # Examples
///
/// ```rust,no_run
/// use weavegraph::graphs::GraphBuilder;
/// use weavegraph::runtimes::CheckpointerType;
/// use weavegraph::state::VersionedState;
/// use weavegraph::types::NodeKind;
/// use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
/// use async_trait::async_trait;
///
/// # struct MyNode;
/// # #[async_trait]
/// # impl Node for MyNode {
/// #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
/// #         Ok(NodePartial::default())
/// #     }
/// # }
/// #
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let app = GraphBuilder::new()
///     .add_node(NodeKind::Custom("process".into()), MyNode)
///     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
///     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
///     .compile()?;
///
/// let initial_state = VersionedState::new_with_user_message("Hello");
/// let final_state = app.invoke(initial_state).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct App {
    nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    conditional_edges: Vec<crate::graphs::ConditionalEdge>,
    reducer_registry: ReducerRegistry,
    runtime_config: RuntimeConfig,
}

impl App {
    /// Internal (crate) factory to build an App while keeping nodes/edges private.
    pub(crate) fn from_parts(
        nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
        edges: FxHashMap<NodeKind, Vec<NodeKind>>,
        conditional_edges: Vec<crate::graphs::ConditionalEdge>,
        runtime_config: RuntimeConfig,
    ) -> Self {
        App {
            nodes,
            edges,
            conditional_edges,
            reducer_registry: ReducerRegistry::default(),
            runtime_config,
        }
    }

    /// Returns a reference to the conditional edges in this graph.
    ///
    /// Conditional edges enable dynamic routing based on runtime state,
    /// allowing workflows to branch based on computed conditions. Predicates
    /// return a String which is interpreted as the next target node:
    /// - "End" and "Start" are recognized as virtual endpoints
    /// - any other string is treated as the name of a custom node
    ///
    /// At runtime, targets are validated before being pushed to the frontier.
    /// Unknown custom targets are skipped with a warning, preserving progress.
    ///
    /// # Returns
    /// A slice of conditional edge specifications.
    #[must_use]
    pub fn conditional_edges(&self) -> &Vec<crate::graphs::ConditionalEdge> {
        &self.conditional_edges
    }

    /// Returns a reference to the nodes registry.
    ///
    /// Provides access to all registered node implementations in the graph.
    /// Nodes are keyed by their `NodeKind` identifier.
    ///
    /// # Returns
    /// A map from `NodeKind` to the corresponding `Node` implementation.
    #[must_use]
    pub fn nodes(&self) -> &FxHashMap<NodeKind, Arc<dyn Node>> {
        &self.nodes
    }

    /// Returns a reference to the unconditional edges in this graph.
    ///
    /// Unconditional edges define the static topology of the workflow graph,
    /// specifying which nodes can transition to which other nodes.
    ///
    /// # Returns
    /// A map from source `NodeKind` to a list of destination `NodeKind`s.
    #[must_use]
    pub fn edges(&self) -> &FxHashMap<NodeKind, Vec<NodeKind>> {
        &self.edges
    }

    /// Returns a reference to the runtime configuration.
    ///
    /// Runtime configuration includes checkpointer settings, session IDs,
    /// and other execution parameters.
    ///
    /// # Returns
    /// The current runtime configuration.
    #[must_use]
    pub fn runtime_config(&self) -> &RuntimeConfig {
        &self.runtime_config
    }

    /// Execute the entire workflow until completion or no nodes remain.
    ///
    /// This is the primary entry point for workflow execution. It creates an
    /// `AppRunner`, manages session state, and coordinates execution through
    /// to completion.
    ///
    /// # Parameters
    /// * `initial_state` - The starting state for workflow execution
    ///
    /// # Returns
    /// * `Ok(VersionedState)` - The final state after workflow completion
    /// * `Err(RunnerError)` - If execution fails due to node errors,
    ///   checkpointer issues, or other runtime problems
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use weavegraph::state::VersionedState;
    /// use weavegraph::channels::Channel;
    /// # use weavegraph::app::App;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    /// let initial = VersionedState::new_with_user_message("Start workflow");
    /// let final_state = app.invoke(initial).await?;
    /// println!("Workflow completed with {} messages", final_state.messages.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Workflow Lifecycle
    /// 1. Creates an `AppRunner` with the configured checkpointer
    /// 2. Initializes or resumes a session
    /// 3. Executes supersteps until End nodes or empty frontier
    /// 4. Returns the final accumulated state
    #[instrument(skip(self, initial_state), err)]
    pub async fn invoke(
        &self,
        initial_state: VersionedState,
    ) -> Result<VersionedState, RunnerError> {
        use crate::runtimes::AppRunner;

        // Determine checkpointer type (default to InMemory if none supplied)
        let checkpointer_type = self
            .runtime_config
            .checkpointer
            .clone()
            .unwrap_or(CheckpointerType::InMemory);

        // Create async runner
        let mut runner = AppRunner::new(self.clone(), checkpointer_type).await;

        let session_id = self
            .runtime_config
            .session_id
            .clone()
            .unwrap_or_else(|| "temp_invoke_session".to_string());

        let init_state = runner
            .create_session(session_id.clone(), initial_state)
            .await?;

        if let SessionInit::Resumed { checkpoint_step } = init_state {
            println!(
                "Resuming session '{}' from checkpoint at step {}",
                session_id, checkpoint_step
            );
        }
        runner.run_until_complete(&session_id).await
    }

    /// Merge node outputs and apply state reductions after a superstep.
    ///
    /// This method coordinates the barrier synchronization phase of workflow
    /// execution, where all node outputs from a superstep are collected,
    /// merged, and applied to the global state via registered reducers.
    ///
    /// # Parameters
    /// * `state` - Mutable reference to the current versioned state
    /// * `run_ids` - Slice of node kinds that executed in this superstep
    /// * `node_partials` - Vector of partial updates from each executed node
    ///
    /// # Returns
    /// * `Ok(Vec<&'static str>)` - Names of channels that were updated
    /// * `Err(Box<dyn Error>)` - If reducer application fails
    ///
    /// # State Management
    /// - Aggregates messages, extra data, and errors from all nodes
    /// - Applies registered reducers to merge updates into global state
    /// - Intelligently bumps version numbers only when content changes
    /// - Preserves deterministic merge behavior for reproducible execution
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use weavegraph::app::App;
    /// # use weavegraph::node::NodePartial;
    /// # use weavegraph::state::VersionedState;
    /// # use weavegraph::types::NodeKind;
    /// # use weavegraph::message::Message;
    /// # async fn example(app: App, state: &mut VersionedState) -> Result<(), String> {
    /// let partials = vec![NodePartial {
    ///     messages: Some(vec![Message::assistant("test")]),
    ///     ..Default::default()
    /// }];
    /// let updated_channels = app.apply_barrier(state, &[NodeKind::Custom("process".into())], partials).await
    ///     .map_err(|e| format!("Error: {}", e))?;
    /// println!("Updated channels: {:?}", updated_channels);
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(skip(self, state, run_ids, node_partials), err)]
    pub async fn apply_barrier(
        &self,
        state: &mut VersionedState,
        run_ids: &[NodeKind],
        node_partials: Vec<NodePartial>,
    ) -> Result<Vec<&'static str>, Box<dyn std::error::Error + Send + Sync>> {
        let mut msgs_all: Vec<Message> = Vec::new();
        let mut extra_all = new_extra_map();
        let mut errors_all: Vec<crate::channels::errors::ErrorEvent> = Vec::new();

        for (i, p) in node_partials.iter().enumerate() {
            let fallback = NodeKind::Custom("?".to_string());
            let nid = run_ids.get(i).unwrap_or(&fallback);

            if let Some(ms) = &p.messages {
                if !ms.is_empty() {
                    tracing::debug!(node = ?nid, count = ms.len(), "Node produced messages");
                    msgs_all.extend(ms.clone());
                }
            }

            if let Some(ex) = &p.extra {
                if !ex.is_empty() {
                    tracing::debug!(node = ?nid, keys = ex.len(), "Node produced extra data");
                    for (k, v) in ex {
                        extra_all.insert(k.clone(), v.clone());
                    }
                }
            }

            if let Some(errs) = &p.errors {
                if !errs.is_empty() {
                    tracing::debug!(node = ?nid, count = errs.len(), "Node produced errors");
                    errors_all.extend(errs.clone());
                }
            }
        }

        let merged_updates = NodePartial {
            messages: if msgs_all.is_empty() {
                None
            } else {
                Some(msgs_all)
            },
            extra: if extra_all.is_empty() {
                None
            } else {
                Some(extra_all)
            },
            errors: if errors_all.is_empty() {
                None
            } else {
                Some(errors_all)
            },
        };

        // Record before-states for version bump decisions
        let msgs_before_len = state.messages.len();
        let msgs_before_ver = state.messages.version();
        let extra_before = state.extra.snapshot();
        let extra_before_ver = state.extra.version();

        // Apply reducers (they do NOT bump versions)
        self.reducer_registry
            .apply_all(&mut *state, &merged_updates)?;

        // Detect changes & bump versions responsibly
        let mut updated: Vec<&'static str> = Vec::new();

        let msgs_changed = state.messages.len() != msgs_before_len;
        if msgs_changed {
            state
                .messages
                .set_version(msgs_before_ver.saturating_add(1));
            tracing::info!(
                "Messages channel updated: {} -> {} messages, version {} -> {}",
                msgs_before_len,
                state.messages.len(),
                msgs_before_ver,
                state.messages.version()
            );
            updated.push("messages");
        }

        let extra_after = state.extra.snapshot();
        let extra_changed = extra_after != extra_before;
        if extra_changed {
            state.extra.set_version(extra_before_ver.saturating_add(1));
            tracing::info!(
                "Extra channel updated: {} -> {} keys, version {} -> {}",
                extra_before.len(),
                extra_after.len(),
                extra_before_ver,
                state.extra.version()
            );
            updated.push("extra");
        }

        Ok(updated)
    }
}
