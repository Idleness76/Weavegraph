//! Frontier-based scheduler with version gating and bounded concurrency.
//!
//! This module provides the core scheduling logic for the Weavegraph workflow framework.
//! The scheduler manages concurrent execution of nodes while ensuring version-based
//! consistency and preventing unnecessary re-execution through intelligent gating.
//!
//! # Core Concepts
//!
//! - **Frontier**: The set of nodes eligible for execution in the current step
//! - **Version Gating**: Skip nodes that have already processed the current state
//! - **Bounded Concurrency**: Control parallel execution with configurable limits
//! - **Superstep**: A single execution phase over the frontier
//!
//! # Examples
//!
//! ```rust
//! use weavegraph::schedulers::{Scheduler, SchedulerState};
//! use weavegraph::state::StateSnapshot;
//! use weavegraph::types::NodeKind;
//! use weavegraph::event_bus::EventBus;
//! use rustc_hash::FxHashMap;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create scheduler with concurrency limit
//! let scheduler = Scheduler::new(4);
//! let mut state = SchedulerState::default();
//!
//! // Check if a node should run based on version changes
//! let snapshot = StateSnapshot {
//!     messages: vec![],
//!     messages_version: 2,
//!     extra: FxHashMap::default(),
//!     extra_version: 1,
//!     errors: vec![],
//!     errors_version: 1,
//! };
//!
//! let should_run = scheduler.should_run(&state, "node_id", &snapshot);
//! if should_run {
//!     println!("Node should run - state has changed");
//! }
//! # Ok(())
//! # }
//! ```

use crate::event_bus::Event;
use crate::node::{Node, NodeContext, NodeError, NodePartial};
use crate::state::StateSnapshot;
use crate::types::NodeKind;
use futures_util::stream::{self, StreamExt};
use miette::Diagnostic;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::instrument;

/// Result of executing a single superstep in the scheduler.
///
/// This structure provides comprehensive information about what happened
/// during a superstep execution, including which nodes ran, which were
/// skipped, and their outputs. This enables detailed monitoring and
/// debugging of workflow execution.
///
/// # Fields
///
/// - `ran_nodes`: Nodes that executed, preserving scheduling order
/// - `skipped_nodes`: Nodes that were skipped (End nodes or version-gated)
/// - `outputs`: Results from executed nodes as (NodeKind, NodePartial) pairs
///
/// # Examples
///
/// ```rust
/// use weavegraph::schedulers::StepRunResult;
/// use weavegraph::types::NodeKind;
///
/// fn analyze_step_result(result: &StepRunResult) {
///     println!("Executed {} nodes, skipped {}",
///              result.ran_nodes.len(),
///              result.skipped_nodes.len());
///     
///     for (node_kind, partial) in &result.outputs {
///         if let Some(messages) = &partial.messages {
///             println!("Node {:?} produced {} messages", node_kind, messages.len());
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct StepRunResult {
    /// Nodes that were executed this step, in the order they were scheduled.
    pub ran_nodes: Vec<NodeKind>,
    /// Nodes that were skipped this step (End nodes or no new versions seen).
    pub skipped_nodes: Vec<NodeKind>,
    /// Outputs from nodes that ran: (node_kind, NodePartial)
    pub outputs: Vec<(NodeKind, NodePartial)>,
}

/// Tracks version information for nodes to enable intelligent scheduling.
///
/// The scheduler uses this state to determine whether a node needs to run
/// based on whether it has already processed the current state versions.
/// This prevents unnecessary re-execution and enables efficient incremental
/// processing.
///
/// # Internal Structure
///
/// The `versions_seen` map uses a two-level structure:
/// - Outer key: Node identifier (string representation of NodeKind)
/// - Inner key: Channel name ("messages", "extra", etc.)
/// - Value: Last version number the node processed for that channel
///
/// # Examples
///
/// ```rust
/// use weavegraph::schedulers::{Scheduler, SchedulerState};
/// use weavegraph::state::StateSnapshot;
/// use rustc_hash::FxHashMap;
///
/// let mut state = SchedulerState::default();
/// let scheduler = Scheduler::new(2);
///
/// // Simulate a snapshot with version changes
/// let snapshot = StateSnapshot {
///     messages: vec![],
///     messages_version: 3,
///     extra: FxHashMap::default(),
///     extra_version: 1,
///     errors: vec![],
///     errors_version: 1,
/// };
///
/// // Record that a node has seen these versions
/// scheduler.record_seen(&mut state, "node_a", &snapshot);
///
/// // Later checks will use this information for gating
/// assert!(!scheduler.should_run(&state, "node_a", &snapshot));
/// ```
#[derive(Debug, Default, Clone)]
pub struct SchedulerState {
    /// versions_seen[node_id][channel_name] = last version observed when the node ran
    pub versions_seen: FxHashMap<String, FxHashMap<String, u64>>,
}

/// High-performance frontier scheduler with version gating and bounded concurrency.
///
/// The `Scheduler` is the core execution engine for workflow steps. It manages
/// parallel node execution while ensuring consistency through version-based
/// gating. The scheduler is stateless by design - all persistence is handled
/// through the separate `SchedulerState`.
///
/// # Architecture
///
/// The scheduler implements a "superstep" execution model:
/// 1. **Frontier Analysis**: Determine which nodes are eligible to run
/// 2. **Version Gating**: Skip nodes that have already processed current state
/// 3. **Concurrent Execution**: Run eligible nodes with bounded parallelism
/// 4. **Result Collection**: Gather outputs preserving execution metadata
///
/// # Performance Characteristics
///
/// - **Concurrency**: Configurable parallelism with `concurrency_limit`
/// - **Efficiency**: Version gating prevents redundant node execution
/// - **Scalability**: Stateless design enables easy distribution
/// - **Determinism**: Execution order is preserved in results
///
/// # Examples
///
/// ```rust
/// use weavegraph::schedulers::Scheduler;
///
/// // Create scheduler with specific concurrency limit
/// let scheduler = Scheduler::new(8); // Max 8 concurrent nodes
/// assert_eq!(scheduler.concurrency_limit, 8);
///
/// // Zero concurrency defaults to 1 for safety
/// let safe_scheduler = Scheduler::new(0);
/// assert_eq!(safe_scheduler.concurrency_limit, 1);
/// ```
#[derive(Debug, Default, Clone)]
pub struct Scheduler {
    pub concurrency_limit: usize,
}

/// Errors that can occur during scheduler execution.
///
/// This enum represents the various failure modes that can happen during
/// workflow execution in the scheduler. Each variant provides specific
/// context about the failure to enable appropriate error handling and
/// debugging.
///
/// # Error Handling
///
/// These errors typically indicate either:
/// - **Node failures**: Issues within individual node execution
/// - **System failures**: Problems with the async runtime or task management
///
/// # Examples
///
/// ```rust
/// use weavegraph::schedulers::SchedulerError;
/// use weavegraph::node::NodeError;
/// use weavegraph::types::NodeKind;
///
/// fn handle_scheduler_error(error: SchedulerError) {
///     match error {
///         SchedulerError::NodeRun { kind, step, source } => {
///             eprintln!("Node {:?} failed at step {}: {}", kind, step, source);
///             // Handle node-specific failure
///         }
///         SchedulerError::Join(join_error) => {
///             eprintln!("Task coordination failed: {}", join_error);
///             // Handle system-level failure
///         }
///     }
/// }
/// ```
#[derive(Debug, Error, Diagnostic)]
pub enum SchedulerError {
    /// A node failed during execution.
    ///
    /// This error occurs when a node's `run` method returns an error.
    /// It includes the node kind, execution step, and the underlying node error
    /// to provide comprehensive context for debugging.
    ///
    /// # Fields
    /// - `kind`: The type of node that failed
    /// - `step`: The workflow step number when the failure occurred  
    /// - `source`: The underlying `NodeError` that caused the failure
    #[error("node run error at step {step} for {kind:?}: {source}")]
    #[diagnostic(code(weavegraph::scheduler::node))]
    NodeRun {
        kind: NodeKind,
        step: u64,
        #[source]
        source: NodeError,
    },

    /// A task join operation failed.
    ///
    /// This error occurs when there's a problem with the async task coordination,
    /// such as a task being cancelled or panicking. This typically indicates
    /// a system-level issue rather than a node logic problem.
    ///
    /// Common causes:
    /// - Task panic during execution
    /// - Runtime shutdown during execution
    /// - Task cancellation due to timeout or external signal
    #[error("task join error: {0}")]
    #[diagnostic(code(weavegraph::scheduler::join))]
    Join(#[from] tokio::task::JoinError),
}

impl Scheduler {
    /// Create a new scheduler with the specified concurrency limit.
    ///
    /// If concurrency_limit is 0, it will be set to 1 to ensure at least
    /// one concurrent task can run.
    ///
    /// # Parameters
    /// * `concurrency_limit` - Maximum number of concurrent node executions
    ///
    /// # Returns
    /// A new Scheduler instance configured with the given concurrency limit
    #[must_use]
    pub fn new(concurrency_limit: usize) -> Self {
        Self {
            concurrency_limit: if concurrency_limit == 0 {
                1
            } else {
                concurrency_limit
            },
        }
    }

    /// Helper to expose channel versions as generic (name, version) pairs.
    #[inline]
    fn channel_versions(snap: &StateSnapshot) -> [(&'static str, u64); 2] {
        [
            ("messages", snap.messages_version as u64),
            ("extra", snap.extra_version as u64),
        ]
    }

    /// Decide if a node should run given the pre-barrier snapshot.
    ///
    /// Returns true if any channel version increased since this node last ran.
    /// This enables efficient incremental execution by skipping nodes that
    /// have already processed the current state.
    ///
    /// # Parameters
    /// * `state` - Current scheduler state tracking execution history
    /// * `node_id` - Identifier of the node to check
    /// * `snap` - Current state snapshot with version information
    ///
    /// # Returns
    /// `true` if the node should run, `false` if it can be skipped
    #[must_use]
    pub fn should_run(&self, state: &SchedulerState, node_id: &str, snap: &StateSnapshot) -> bool {
        let channels = Self::channel_versions(snap);
        self.should_run_with(state, node_id, &channels)
    }

    /// Generic form of should_run: decide based on provided (channel_name, version) pairs.
    ///
    /// This method provides the core scheduling logic and can be used with
    /// custom channel configurations or for testing purposes.
    ///
    /// # Parameters
    /// * `state` - Current scheduler state tracking execution history
    /// * `node_id` - Identifier of the node to check
    /// * `channels` - Array of (channel_name, version) pairs to check
    ///
    /// # Returns
    /// `true` if the node should run based on version changes, `false` otherwise
    #[must_use]
    pub fn should_run_with(
        &self,
        state: &SchedulerState,
        node_id: &str,
        channels: &[(&str, u64)],
    ) -> bool {
        let seen = match state.versions_seen.get(node_id) {
            Some(v) => v,
            None => return true, // never ran -> run
        };
        for (name, ver) in channels.iter() {
            let last = seen.get::<str>(name).copied().unwrap_or(0);
            if *ver > last {
                return true;
            }
        }
        false
    }

    /// Record the versions seen for a node at the start of its execution.
    ///
    /// This method updates the scheduler state to track which versions of each
    /// channel a node has processed. This information is used by version gating
    /// to determine whether a node needs to run again in future supersteps.
    ///
    /// The versions are captured from the pre-barrier snapshot to ensure
    /// consistency with the state the node actually processes.
    ///
    /// # Parameters
    /// * `state` - Mutable scheduler state to update with version information
    /// * `node_id` - String identifier of the node (typically `format!("{:?}", node_kind)`)
    /// * `snap` - State snapshot containing current channel versions
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::schedulers::{Scheduler, SchedulerState};
    /// use weavegraph::state::StateSnapshot;
    /// use rustc_hash::FxHashMap;
    ///
    /// let scheduler = Scheduler::new(2);
    /// let mut state = SchedulerState::default();
    ///
    /// let snapshot = StateSnapshot {
    ///     messages: vec![],
    ///     messages_version: 5,
    ///     extra: FxHashMap::default(),
    ///     extra_version: 3,
    ///     errors: vec![],
    ///     errors_version: 1,
    /// };
    ///
    /// // Record that node_a has processed versions 5 and 3
    /// scheduler.record_seen(&mut state, "node_a", &snapshot);
    ///
    /// // Future checks will use this information
    /// assert!(!scheduler.should_run(&state, "node_a", &snapshot));
    /// ```
    pub fn record_seen(&self, state: &mut SchedulerState, node_id: &str, snap: &StateSnapshot) {
        let channels = Self::channel_versions(snap);
        self.record_seen_with(state, node_id, &channels);
    }

    /// Generic form of record_seen: store versions for provided channel/version pairs.
    ///
    /// This is the low-level version tracking method that allows recording
    /// arbitrary channel versions. It's used internally by `record_seen` and
    /// can be used directly for custom channel configurations or testing.
    ///
    /// The method updates the internal `versions_seen` map structure:
    /// `versions_seen[node_id][channel_name] = version`
    ///
    /// # Parameters
    /// * `state` - Mutable scheduler state to update
    /// * `node_id` - String identifier of the node
    /// * `channels` - Slice of (channel_name, version) pairs to record
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::schedulers::{Scheduler, SchedulerState};
    ///
    /// let scheduler = Scheduler::new(2);
    /// let mut state = SchedulerState::default();
    ///
    /// // Record custom channel versions
    /// let channels = [("messages", 10), ("custom_channel", 5)];
    /// scheduler.record_seen_with(&mut state, "node_x", &channels);
    ///
    /// // Verify the versions were recorded
    /// let node_versions = &state.versions_seen["node_x"];
    /// assert_eq!(node_versions["messages"], 10);
    /// assert_eq!(node_versions["custom_channel"], 5);
    /// ```
    pub fn record_seen_with(
        &self,
        state: &mut SchedulerState,
        node_id: &str,
        channels: &[(&str, u64)],
    ) {
        let entry = state.versions_seen.entry(node_id.to_string()).or_default();
        for (name, ver) in channels.iter() {
            entry.insert((*name).to_string(), *ver);
        }
    }

    /// Execute a single superstep over a frontier with bounded concurrency.
    ///
    /// This is the core execution method of the scheduler. It processes a frontier
    /// of nodes, applying version gating to skip unnecessary work, and executes
    /// eligible nodes concurrently with the configured parallelism limit.
    ///
    /// # Execution Flow
    ///
    /// 1. **Frontier Partitioning**: Separate nodes into "to run" vs "skipped"
    ///    - Skip `NodeKind::End` nodes (terminal nodes)
    ///    - Skip nodes that have already processed current state versions
    /// 2. **Task Creation**: Build async tasks for eligible nodes
    /// 3. **Concurrent Execution**: Run tasks with bounded parallelism
    /// 4. **Result Collection**: Gather outputs, preserving execution metadata
    /// 5. **Version Recording**: Update state with processed versions
    ///
    /// # Version Gating
    ///
    /// Nodes are skipped if they have already processed the current state versions,
    /// preventing redundant computation. This enables efficient incremental execution
    /// in long-running workflows.
    ///
    /// # Concurrency Model
    ///
    /// - **Bounded Parallelism**: Respects `concurrency_limit` to control resource usage
    /// - **Unordered Completion**: Tasks may complete out of order for efficiency
    /// - **Deterministic Results**: `ran_nodes` preserves scheduling order
    ///
    /// # Parameters
    /// * `state` - Mutable scheduler state for version tracking
    /// * `nodes` - Registry mapping node kinds to their implementations
    /// * `frontier` - Vector of nodes eligible for execution this step
    /// * `snap` - Pre-barrier state snapshot for version gating
    /// * `step` - Current workflow step number (for context and logging)
    /// * `event_bus_sender` - Channel for sending execution events
    ///
    /// # Returns
    /// * `Ok(StepRunResult)` - Execution results with ran/skipped nodes and outputs
    /// * `Err(SchedulerError)` - Node execution failure or task coordination error
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::schedulers::{Scheduler, SchedulerState};
    /// use weavegraph::state::StateSnapshot;
    /// use weavegraph::types::NodeKind;
    /// use weavegraph::event_bus::EventBus;
    /// use rustc_hash::FxHashMap;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let scheduler = Scheduler::new(4);
    /// let mut state = SchedulerState::default();
    /// let nodes = FxHashMap::default(); // Node registry
    /// let event_bus = EventBus::default();
    ///
    /// let frontier = vec![NodeKind::Start, NodeKind::Other("process".into())];
    /// let snapshot = StateSnapshot {
    ///     messages: vec![],
    ///     messages_version: 1,
    ///     extra: FxHashMap::default(),
    ///     extra_version: 1,
    ///     errors: vec![],
    ///     errors_version: 1,
    /// };
    ///
    /// let result = scheduler.superstep(
    ///     &mut state,
    ///     &nodes,
    ///     frontier,
    ///     snapshot,
    ///     1,
    ///     event_bus.get_sender(),
    /// ).await?;
    ///
    /// println!("Executed {} nodes, skipped {}",
    ///          result.ran_nodes.len(),
    ///          result.skipped_nodes.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Error Handling
    ///
    /// - **Node Failures**: If any node returns an error, the entire superstep fails
    /// - **Task Panics**: Panicking nodes result in `SchedulerError::Join`
    /// - **Missing Nodes**: Panics if frontier contains nodes not in registry
    #[instrument(skip(self, state, nodes, frontier, snap))]
    pub async fn superstep(
        &self,
        state: &mut SchedulerState,
        nodes: &FxHashMap<NodeKind, Arc<dyn Node>>, // registry
        frontier: Vec<NodeKind>,                    // frontier for this step
        snap: StateSnapshot,                        // pre-barrier snapshot
        step: u64,
        event_bus_sender: flume::Sender<Event>,
    ) -> Result<StepRunResult, SchedulerError> {
        // Partition frontier into to_run vs skipped using a skip predicate and version gating.
        let channels = Self::channel_versions(&snap);
        let skip_predicate = |k: &NodeKind| matches!(k, NodeKind::End);
        let mut to_run: Vec<NodeKind> = Vec::new();
        let mut skipped_kinds: Vec<NodeKind> = Vec::new();
        for k in frontier.into_iter() {
            if skip_predicate(&k) {
                skipped_kinds.push(k);
                continue;
            }
            let id_str = format!("{:?}", k);
            if self.should_run_with(state, &id_str, &channels) {
                to_run.push(k);
            } else {
                skipped_kinds.push(k);
            }
        }

        // Build tasks for the nodes to run.
        let to_run_ids: Vec<String> = to_run.iter().map(|k| format!("{:?}", k)).collect();
        let tasks = to_run_ids
            .iter()
            .cloned()
            .zip(to_run.clone().into_iter())
            .map(|(id_str, kind)| {
                let node = nodes
                    .get(&kind)
                    .expect("node in frontier not found")
                    .clone();
                let event_bus_sender = event_bus_sender.clone();
                let ctx = NodeContext {
                    node_id: id_str.clone(),
                    step,
                    event_bus_sender,
                };
                let s = snap.clone();
                async move {
                    // Return Result and let caller collect
                    let out = node.run(s, ctx).await;
                    (kind, out)
                }
            });

        // Execute with bounded concurrency; completion order may differ.
        let mut outputs: Vec<(NodeKind, NodePartial)> = Vec::new();
        let mut stream = stream::iter(tasks).buffer_unordered(self.concurrency_limit);
        while let Some((kind, res)) = stream.next().await {
            match res {
                Ok(part) => outputs.push((kind, part)),
                Err(e) => {
                    return Err(SchedulerError::NodeRun {
                        kind,
                        step,
                        source: e,
                    });
                }
            }
        }

        // Record versions seen for nodes that ran.
        for id in &to_run_ids {
            self.record_seen_with(state, id, &channels);
        }

        Ok(StepRunResult {
            ran_nodes: to_run,
            skipped_nodes: skipped_kinds,
            outputs,
        })
    }
}
