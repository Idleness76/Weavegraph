use rustc_hash::FxHashMap;
use std::sync::Arc;

use super::checkpointer::{restore_session_state, Checkpoint, Checkpointer, CheckpointerError};
use crate::app::App;
use crate::channels::errors::{ErrorEvent, ErrorScope, LadderError};
use crate::channels::Channel;
use crate::event_bus::EventBus;
use crate::node::NodePartial;
use crate::runtimes::{CheckpointerType, InMemoryCheckpointer};
use crate::schedulers::{Scheduler, SchedulerError, SchedulerState};
use crate::state::VersionedState;
use crate::types::NodeKind;
use miette::Diagnostic;
use thiserror::Error;
use tracing::instrument;

/// Result of executing one superstep in a session.
#[derive(Debug, Clone)]
pub struct StepReport {
    pub step: u64,
    pub ran_nodes: Vec<NodeKind>,
    pub skipped_nodes: Vec<NodeKind>,
    pub updated_channels: Vec<&'static str>,
    pub next_frontier: Vec<NodeKind>,
    pub state_versions: StateVersions,
    pub completed: bool,
}

/// Snapshot of channel versions for tracking state evolution
#[derive(Debug, Clone)]
pub struct StateVersions {
    pub messages_version: u32,
    pub extra_version: u32,
}

/// Session state that needs to be persisted across steps
#[derive(Debug, Clone)]
pub struct SessionState {
    pub state: VersionedState,
    pub step: u64,
    pub frontier: Vec<NodeKind>,
    pub scheduler: Scheduler,
    pub scheduler_state: SchedulerState,
}

/// Options for step execution
#[derive(Debug, Clone, Default)]
pub struct StepOptions {
    pub interrupt_before: Vec<NodeKind>,
    pub interrupt_after: Vec<NodeKind>,
    pub interrupt_each_step: bool,
}

/// Paused execution context
#[derive(Debug, Clone)]
pub enum PausedReason {
    BeforeNode(NodeKind),
    AfterNode(NodeKind),
    AfterStep(u64),
}

/// Extended step report when execution is paused
#[derive(Debug, Clone)]
pub struct PausedReport {
    pub session_state: SessionState,
    pub reason: PausedReason,
}

/// Result of attempting to run a step
#[derive(Debug, Clone)]
pub enum StepResult {
    Completed(StepReport),
    Paused(PausedReport),
}

/// Stepwise execution wrapper around App that supports sessions and interrupts
pub struct AppRunner {
    app: Arc<App>,
    sessions: FxHashMap<String, SessionState>,
    checkpointer: Option<Arc<dyn Checkpointer>>, // optional pluggable persistence
    autosave: bool,
    event_bus: EventBus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionInit {
    Fresh,
    Resumed { checkpoint_step: u64 },
}

#[derive(Debug, Error, Diagnostic)]
pub enum RunnerError {
    #[error("session not found: {session_id}")]
    #[diagnostic(code(weavegraph::runner::session_not_found))]
    SessionNotFound { session_id: String },

    #[error("no nodes to run from START (empty frontier)")]
    #[diagnostic(
        code(weavegraph::runner::no_start_nodes),
        help("Add edges from Start or set the entry node correctly.")
    )]
    NoStartNodes,

    #[error("unexpected pause during run_until_complete")]
    #[diagnostic(code(weavegraph::runner::unexpected_pause))]
    UnexpectedPause,

    #[error(transparent)]
    #[diagnostic(code(weavegraph::runner::checkpointer))]
    Checkpointer(#[from] CheckpointerError),

    #[error("app barrier error: {0}")]
    #[diagnostic(code(weavegraph::runner::barrier))]
    AppBarrier(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error(transparent)]
    #[diagnostic(code(weavegraph::runner::scheduler))]
    Scheduler(#[from] SchedulerError),
}

impl AppRunner {
    /// Create a new AppRunner wrapping the given App
    #[must_use]
    pub async fn new(app: App, checkpointer_type: CheckpointerType) -> Self {
        Self::with_options(app, checkpointer_type, true).await
    }

    #[must_use]
    pub async fn from_arc(app: Arc<App>, checkpointer_type: CheckpointerType) -> Self {
        Self::with_options_arc(app, checkpointer_type, true).await
    }

    async fn create_checkpointer(
        checkpointer_type: CheckpointerType,
        sqlite_db_name: Option<String>,
    ) -> Option<Arc<dyn Checkpointer>> {
        match checkpointer_type {
            CheckpointerType::InMemory => Some(Arc::new(InMemoryCheckpointer::new())),
            CheckpointerType::SQLite => {
                let db_url = std::env::var("WEAVEGRAPH_SQLITE_URL")
                    .ok()
                    .or_else(|| {
                        sqlite_db_name
                            .as_ref()
                            .map(|name| format!("sqlite://{name}"))
                    })
                    .unwrap_or_else(|| {
                        let fallback = std::env::var("SQLITE_DB_NAME")
                            .unwrap_or_else(|_| "weavegraph.db".to_string());
                        format!("sqlite://{fallback}")
                    });
                // Ensure underlying sqlite file exists. Steps:
                // 1. Strip "sqlite://" scheme to get filesystem path.
                // 2. Create parent directories if needed.
                // 3. Attempt to create the file (ignore errors if it already exists or any failure).
                if let Some(path) = db_url.strip_prefix("sqlite://") {
                    let path = path.trim();
                    if !path.is_empty() {
                        let p = std::path::Path::new(path);
                        if let Some(parent) = p.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if !p.exists() {
                            // Ignore result; if it already exists or we lack permission we proceed anyway.
                            let _ = std::fs::File::create_new(p);
                        }
                    }
                }
                match crate::runtimes::SQLiteCheckpointer::connect(&db_url).await {
                    Ok(cp) => Some(Arc::new(cp) as Arc<dyn Checkpointer>),
                    Err(e) => {
                        eprintln!(
                            "SQLiteCheckpointer initialization failed ({}): {}",
                            db_url, e
                        );
                        None
                    }
                }
            }
        }
    }

    /// Create with explicit checkpointer + autosave toggle
    pub async fn with_options(
        app: App,
        checkpointer_type: CheckpointerType,
        autosave: bool,
    ) -> Self {
        let app = Arc::new(app);
        Self::with_arc_and_bus(app, checkpointer_type, autosave, EventBus::default(), true).await
    }

    pub async fn with_options_arc(
        app: Arc<App>,
        checkpointer_type: CheckpointerType,
        autosave: bool,
    ) -> Self {
        Self::with_arc_and_bus(app, checkpointer_type, autosave, EventBus::default(), true).await
    }

    /// Variant that accepts a preconfigured EventBus.
    pub async fn with_options_and_bus(
        app: App,
        checkpointer_type: CheckpointerType,
        autosave: bool,
        event_bus: EventBus,
        start_listener: bool,
    ) -> Self {
        let app = Arc::new(app);
        Self::with_arc_and_bus(app, checkpointer_type, autosave, event_bus, start_listener).await
    }

    /// Variant that accepts a preconfigured EventBus for an existing Arc<App>.
    pub async fn with_options_arc_and_bus(
        app: Arc<App>,
        checkpointer_type: CheckpointerType,
        autosave: bool,
        event_bus: EventBus,
        start_listener: bool,
    ) -> Self {
        Self::with_arc_and_bus(app, checkpointer_type, autosave, event_bus, start_listener).await
    }

    async fn with_arc_and_bus(
        app: Arc<App>,
        checkpointer_type: CheckpointerType,
        autosave: bool,
        event_bus: EventBus,
        start_listener: bool,
    ) -> Self {
        let sqlite_db_name = app.runtime_config().sqlite_db_name.clone();
        let checkpointer = Self::create_checkpointer(checkpointer_type, sqlite_db_name).await;
        if start_listener {
            event_bus.listen_for_events();
        }
        Self {
            app,
            sessions: FxHashMap::default(),
            checkpointer,
            autosave,
            event_bus,
        }
    }

    /// Initialize a new session with the given initial state
    #[instrument(skip(self, initial_state, session_id), err)]
    pub async fn create_session(
        &mut self,
        session_id: String,
        initial_state: VersionedState,
    ) -> Result<SessionInit, RunnerError> {
        // If checkpointer present and session exists, load instead of creating anew
        let restored_checkpoint = if let Some(cp) = &self.checkpointer {
            cp.load_latest(&session_id)
                .await
                .map_err(RunnerError::Checkpointer)?
        } else {
            None
        };

        if let Some(stored) = restored_checkpoint {
            let restored = restore_session_state(&stored);
            self.sessions.insert(session_id, restored);
            return Ok(SessionInit::Resumed {
                checkpoint_step: stored.step,
            });
        }

        let frontier = self
            .app
            .edges()
            .get(&NodeKind::Start)
            .cloned()
            .unwrap_or_default();
        if frontier.is_empty() {
            return Err(RunnerError::NoStartNodes);
        }
        let default_limit = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let scheduler = Scheduler::new(default_limit);
        let session_state = SessionState {
            state: initial_state,
            step: 0,
            frontier,
            scheduler,
            scheduler_state: SchedulerState::default(),
        };
        self.sessions
            .insert(session_id.clone(), session_state.clone());
        if let Some(cp) = &self.checkpointer {
            let _ = cp
                .save(Checkpoint::from_session(&session_id, &session_state))
                .await;
        }
        Ok(SessionInit::Fresh)
    }

    /// Execute one superstep for the given session
    #[instrument(skip(self, options), err)]
    pub async fn run_step(
        &mut self,
        session_id: &str,
        options: StepOptions,
    ) -> Result<StepResult, RunnerError> {
        // Clone session state to avoid borrowing issues
        let mut session_state = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RunnerError::SessionNotFound {
                session_id: session_id.to_string(),
            })?
            .clone();

        // Check if already completed
        if session_state.frontier.is_empty()
            || session_state.frontier.iter().all(|n| *n == NodeKind::End)
        {
            let versions = StateVersions {
                messages_version: session_state.state.messages.version(),
                extra_version: session_state.state.extra.version(),
            };
            return Ok(StepResult::Completed(StepReport {
                step: session_state.step,
                ran_nodes: vec![],
                skipped_nodes: session_state.frontier.clone(),
                updated_channels: vec![],
                next_frontier: vec![],
                state_versions: versions,
                completed: true,
            }));
        }

        // Check for interrupt_before
        for node in &session_state.frontier {
            if options.interrupt_before.contains(node) {
                return Ok(StepResult::Paused(PausedReport {
                    session_state: session_state.clone(),
                    reason: PausedReason::BeforeNode(node.clone()),
                }));
            }
        }

        // Execute one superstep; on error, emit an ErrorEvent and rethrow
        let step_report = match self.run_one_superstep(&mut session_state).await {
            Ok(rep) => rep,
            Err(e) => {
                // Build error event
                let event = match &e {
                    RunnerError::Scheduler(s) => match s {
                        crate::schedulers::SchedulerError::NodeRun { kind, step, source } => {
                            ErrorEvent {
                                when: chrono::Utc::now(),
                                scope: ErrorScope::Node {
                                    kind: kind.encode().to_string(),
                                    step: *step,
                                },
                                error: LadderError::msg(format!("{}", source)),
                                tags: vec!["node".into()],
                                context: serde_json::json!({}),
                            }
                        }
                        crate::schedulers::SchedulerError::Join(_) => ErrorEvent {
                            when: chrono::Utc::now(),
                            scope: ErrorScope::Scheduler {
                                step: session_state.step,
                            },
                            error: LadderError::msg(format!("{}", e)),
                            tags: vec!["scheduler".into()],
                            context: serde_json::json!({}),
                        },
                    },
                    _ => ErrorEvent {
                        when: chrono::Utc::now(),
                        scope: ErrorScope::Runner {
                            session: session_id.to_string(),
                            step: session_state.step,
                        },
                        error: LadderError::msg(format!("{}", e)),
                        tags: vec!["runner".into()],
                        context: serde_json::json!({
                            "frontier": session_state.frontier.iter().map(|k| k.encode()).collect::<Vec<_>>()
                        }),
                    },
                };
                // Inject via barrier mechanics by applying a synthetic NodePartial with errors field
                let mut update_state = session_state.state.clone();
                let partial = NodePartial {
                    messages: None,
                    extra: None,
                    errors: Some(vec![event]),
                };
                // Apply directly using reducer registry through App
                let _ = self
                    .app
                    .apply_barrier(&mut update_state, &[], vec![partial])
                    .await;
                session_state.state = update_state;
                // Save back to sessions map so callers can inspect accumulated errors
                self.sessions
                    .insert(session_id.to_string(), session_state.clone());
                // Re-persist if autosave
                if self.autosave {
                    if let Some(cp) = &self.checkpointer {
                        let _ = cp
                            .save(Checkpoint::from_session(session_id, &session_state))
                            .await;
                    }
                }
                return Err(e);
            }
        };

        // Update the session in map & persist if configured
        self.sessions
            .insert(session_id.to_string(), session_state.clone());
        if self.autosave {
            if let Some(cp) = &self.checkpointer {
                let _ = cp
                    .save(Checkpoint::from_session(session_id, &session_state))
                    .await;
            }
        }

        // Check for interrupt_after
        for node in &step_report.ran_nodes {
            if options.interrupt_after.contains(node) {
                return Ok(StepResult::Paused(PausedReport {
                    session_state: session_state.clone(),
                    reason: PausedReason::AfterNode(node.clone()),
                }));
            }
        }

        // Check for interrupt_each_step
        if options.interrupt_each_step {
            return Ok(StepResult::Paused(PausedReport {
                session_state: session_state.clone(),
                reason: PausedReason::AfterStep(step_report.step),
            }));
        }

        Ok(StepResult::Completed(step_report))
    }

    /// Helper method that executes exactly one superstep on the given session state
    #[instrument(skip(self, session_state), err)]
    async fn run_one_superstep(
        &self,
        session_state: &mut SessionState,
    ) -> Result<StepReport, RunnerError> {
        session_state.step += 1;
        let step = session_state.step;

        println!("\n-- Superstep {} --", step);

        let snapshot = session_state.state.snapshot();
        println!(
            "msgs={} v{}; extra_keys={} v{}",
            snapshot.messages.len(),
            snapshot.messages_version,
            snapshot.extra.len(),
            snapshot.extra_version
        );

        // Execute via scheduler
        let step_result = session_state
            .scheduler
            .superstep(
                &mut session_state.scheduler_state,
                self.app.nodes(),
                session_state.frontier.clone(),
                snapshot.clone(),
                step,
                self.event_bus.get_sender(),
            )
            .await?;

        // Reorder outputs to match ran_nodes order expected by the barrier
        let mut by_kind: FxHashMap<NodeKind, NodePartial> = FxHashMap::default();
        for (kind, part) in step_result.outputs {
            by_kind.insert(kind, part);
        }
        let run_ids: Vec<NodeKind> = step_result.ran_nodes.clone();
        let node_partials: Vec<NodePartial> = run_ids
            .iter()
            .cloned()
            .filter_map(|k| by_kind.remove(&k))
            .collect();

        // Apply barrier using the app's existing method
        let mut update_state = session_state.state.clone();
        let updated_channels = self
            .app
            .apply_barrier(&mut update_state, &run_ids, node_partials)
            .await
            .map_err(RunnerError::AppBarrier)?;

        // Update session state with the modified state
        session_state.state = update_state;

        // Compute next frontier: unconditional edges + conditional edges
        let mut next_frontier: Vec<NodeKind> = Vec::new();
        let app_edges = self.app.edges();
        let conditional_edges = self.app.conditional_edges();
        let snapshot = session_state.state.snapshot();
        for id in run_ids.iter() {
            // Unconditional edges
            if let Some(dests) = app_edges.get(id) {
                for d in dests {
                    if !next_frontier.contains(d) {
                        next_frontier.push(d.clone());
                    }
                }
            }
            // Conditional edges
            for ce in conditional_edges.iter().filter(|ce| &ce.from == id) {
                println!("running conditional edge from {:?}", ce.from);
                let target = if (ce.predicate)(snapshot.clone()) {
                    println!("conditional edge routing to {:?}", &ce.yes);
                    &ce.yes
                } else {
                    println!("conditional edge routing to {:?}", &ce.no);
                    &ce.no
                };
                if !next_frontier.contains(target) {
                    next_frontier.push(target.clone());
                }
            }
        }

        println!("Updated channels this step: {:?}", updated_channels);
        println!("Next frontier: {:?}", next_frontier);

        let completed =
            next_frontier.is_empty() || next_frontier.iter().all(|n| *n == NodeKind::End);

        // Update session state
        session_state.frontier = next_frontier.clone();

        let state_versions = StateVersions {
            messages_version: session_state.state.messages.version(),
            extra_version: session_state.state.extra.version(),
        };

        Ok(StepReport {
            step,
            ran_nodes: run_ids,
            skipped_nodes: step_result.skipped_nodes,
            updated_channels,
            next_frontier,
            state_versions,
            completed,
        })
    }

    /// Run until completion (End nodes or no frontier) - the canonical execution method
    #[instrument(skip(self, session_id), err)]
    pub async fn run_until_complete(
        &mut self,
        session_id: &str,
    ) -> Result<VersionedState, RunnerError> {
        println!("== Begin run ==");

        loop {
            // Check if we're done before trying to run
            let session_state =
                self.sessions
                    .get(session_id)
                    .ok_or_else(|| RunnerError::SessionNotFound {
                        session_id: session_id.to_string(),
                    })?;

            if session_state.frontier.is_empty()
                || session_state.frontier.iter().all(|n| *n == NodeKind::End)
            {
                println!("Reached END at step {}", session_state.step);
                break;
            }

            // Run one step
            let step_result = self.run_step(session_id, StepOptions::default()).await?;

            match step_result {
                StepResult::Completed(report) => {
                    if report.completed {
                        break;
                    }
                }
                StepResult::Paused(_) => {
                    // This shouldn't happen with default options, but handle gracefully
                    return Err(RunnerError::UnexpectedPause);
                }
            }
        }

        println!("\n== Final state ==");
        let final_session =
            self.sessions
                .get(session_id)
                .ok_or_else(|| RunnerError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;
        let final_state = final_session.state.clone();

        // Print final state summary (matching App::invoke output)
        for (i, m) in final_state.messages.snapshot().iter().enumerate() {
            println!("#{:02} [{}] {}", i, m.role, m.content);
        }
        println!("messages.version = {}", final_state.messages.version());

        let extra_snapshot = final_state.extra.snapshot();
        println!(
            "extra (v {}) keys={}",
            final_state.extra.version(),
            extra_snapshot.len()
        );
        for (k, v) in extra_snapshot.iter() {
            println!("  {k}: {v}");
        }

        Ok(final_state)
    }

    /// Get a snapshot of the current session state.
    ///
    /// # Parameters
    ///
    /// * `session_id` - The session identifier
    ///
    /// # Returns
    ///
    /// `Some(&SessionState)` if the session exists, `None` otherwise
    #[must_use]
    pub fn get_session(&self, session_id: &str) -> Option<&SessionState> {
        self.sessions.get(session_id)
    }

    /// List all active session IDs.
    ///
    /// # Returns
    ///
    /// A vector of session ID references
    #[must_use]
    pub fn list_sessions(&self) -> Vec<&String> {
        self.sessions.keys().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgePredicate, GraphBuilder};
    use crate::message::Message;
    use crate::node::{NodeContext, NodeError, NodePartial};
    use crate::state::{StateSnapshot, VersionedState};
    use crate::utils::testing::{FailingNode, TestNode};
    use async_trait::async_trait;
    use rustc_hash::FxHashMap;
    use serde_json::json;

    // Simple test nodes for runner testing
    #[derive(Debug, Clone)]
    struct NodeA;

    #[async_trait]
    impl crate::node::Node for NodeA {
        async fn run(
            &self,
            _snapshot: StateSnapshot,
            _ctx: NodeContext,
        ) -> Result<NodePartial, NodeError> {
            let mut extra = FxHashMap::default();
            extra.insert("node_a_executed".into(), json!(true));

            Ok(NodePartial {
                messages: Some(vec![Message {
                    role: "assistant".into(),
                    content: "NodeA executed".into(),
                }]),
                extra: Some(extra),
                ..Default::default()
            })
        }
    }

    #[derive(Debug, Clone)]
    struct NodeB;

    #[async_trait]
    impl crate::node::Node for NodeB {
        async fn run(
            &self,
            _snapshot: StateSnapshot,
            _ctx: NodeContext,
        ) -> Result<NodePartial, NodeError> {
            let mut extra = FxHashMap::default();
            extra.insert("node_b_executed".into(), json!(true));

            Ok(NodePartial {
                messages: Some(vec![Message {
                    role: "assistant".into(),
                    content: "NodeB executed".into(),
                }]),
                extra: Some(extra),
                ..Default::default()
            })
        }
    }

    fn make_test_app() -> App {
        let mut builder = GraphBuilder::new();
        builder = builder.add_node(NodeKind::Start, NodeA);
        builder = builder.add_node(NodeKind::Other("test".into()), TestNode { name: "test" });
        builder = builder.add_node(NodeKind::End, NodeB);
        builder = builder.add_edge(NodeKind::Start, NodeKind::Other("test".into()));
        builder = builder.add_edge(NodeKind::Other("test".into()), NodeKind::End);
        builder = builder.set_entry(NodeKind::Start);
        builder.compile().unwrap()
    }

    #[tokio::test]
    async fn test_conditional_edge_routing() {
        // Predicate: true if extra contains key "go_yes"
        let pred: EdgePredicate =
            std::sync::Arc::new(|snap: StateSnapshot| snap.extra.contains_key("go_yes"));
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, TestNode { name: "start" })
            .add_node(NodeKind::Other("Y".into()), TestNode { name: "yes path" })
            .add_node(NodeKind::Other("N".into()), TestNode { name: "no path" })
            .add_edge(NodeKind::Start, NodeKind::Start) // Add unconditional edge to START itself for initial frontier
            .add_conditional_edge(
                NodeKind::Start,
                NodeKind::Other("Y".into()),
                NodeKind::Other("N".into()),
                pred.clone(),
            )
            .set_entry(NodeKind::Start);
        let app = gb.compile().unwrap();
        let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
        // State with go_yes present
        let mut state = VersionedState::new_with_user_message("hi");
        state
            .extra
            .get_mut()
            .insert("go_yes".to_string(), serde_json::json!(1));
        match runner
            .create_session("sess1".to_string(), state.clone())
            .await
            .unwrap()
        {
            SessionInit::Fresh => {}
            SessionInit::Resumed { .. } => panic!("expected fresh session"),
        }
        let report = runner
            .run_step("sess1", StepOptions::default())
            .await
            .unwrap();
        if let StepResult::Completed(rep) = report {
            assert!(rep.next_frontier.contains(&NodeKind::Other("Y".into())));
            assert!(!rep.next_frontier.contains(&NodeKind::Other("N".into())));
        } else {
            panic!("Expected completed step");
        }
        // State without go_yes
        let state2 = VersionedState::new_with_user_message("hi");
        match runner
            .create_session("sess2".to_string(), state2.clone())
            .await
            .unwrap()
        {
            SessionInit::Fresh => {}
            SessionInit::Resumed { .. } => panic!("expected fresh session"),
        }
        let report2 = runner
            .run_step("sess2", StepOptions::default())
            .await
            .unwrap();
        if let StepResult::Completed(rep2) = report2 {
            assert!(rep2.next_frontier.contains(&NodeKind::Other("N".into())));
            assert!(!rep2.next_frontier.contains(&NodeKind::Other("Y".into())));
        } else {
            panic!("Expected completed step");
        }
    }

    #[tokio::test]
    async fn test_create_session() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
        let initial_state = VersionedState::new_with_user_message("hello");

        let result = runner
            .create_session("test_session".into(), initial_state)
            .await
            .unwrap();
        assert_eq!(result, SessionInit::Fresh);
        assert!(runner.get_session("test_session").is_some());
    }

    #[tokio::test]
    async fn test_run_step_basic() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
        let initial_state = VersionedState::new_with_user_message("hello");

        assert_eq!(
            runner
                .create_session("test_session".into(), initial_state)
                .await
                .unwrap(),
            SessionInit::Fresh
        );

        let result = runner
            .run_step("test_session", StepOptions::default())
            .await;
        assert!(result.is_ok());

        if let Ok(StepResult::Completed(report)) = result {
            assert_eq!(report.step, 1);
            assert_eq!(report.ran_nodes.len(), 1);
            assert!(report.updated_channels.contains(&"messages"));
        } else {
            panic!("Expected completed step, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_run_until_complete() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
        let initial_state = VersionedState::new_with_user_message("hello");

        assert_eq!(
            runner
                .create_session("test_session".into(), initial_state)
                .await
                .unwrap(),
            SessionInit::Fresh
        );

        let result = runner.run_until_complete("test_session").await;
        assert!(result.is_ok());

        let final_state = result.unwrap();
        assert_eq!(final_state.messages.len(), 2); // user + test node message
        assert_eq!(final_state.messages.version(), 2);
    }

    #[tokio::test]
    async fn test_interrupt_before() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
        let initial_state = VersionedState::new_with_user_message("hello");

        assert_eq!(
            runner
                .create_session("test_session".into(), initial_state)
                .await
                .unwrap(),
            SessionInit::Fresh
        );

        // Set interrupt before the test node
        let options = StepOptions {
            interrupt_before: vec![NodeKind::Other("test".into())],
            ..Default::default()
        };

        let result = runner.run_step("test_session", options).await;
        assert!(result.is_ok());

        if let Ok(StepResult::Paused(paused)) = result {
            assert!(matches!(paused.reason, PausedReason::BeforeNode(_)));
        } else {
            panic!("Expected paused step, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_interrupt_after() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
        let initial_state = VersionedState::new_with_user_message("hello");

        assert_eq!(
            runner
                .create_session("test_session".into(), initial_state)
                .await
                .unwrap(),
            SessionInit::Fresh
        );

        // Set interrupt after the "test" node (which runs in the first step)
        let options = StepOptions {
            interrupt_after: vec![NodeKind::Other("test".into())],
            ..Default::default()
        };

        let result = runner.run_step("test_session", options).await;
        assert!(result.is_ok());

        if let Ok(StepResult::Paused(paused)) = result {
            assert!(matches!(paused.reason, PausedReason::AfterNode(_)));
        } else {
            panic!("Expected paused step, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_resume_from_checkpoint() {
        // Test resuming from a SQLite checkpoint
        let app = make_test_app();
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_resume.db");

        // Set up environment variable for SQLite URL
        std::env::set_var(
            "WEAVEGRAPH_SQLITE_URL",
            format!("sqlite://{}", db_path.display()),
        );

        // Create first runner and session
        let mut runner1 = AppRunner::new(app.clone(), CheckpointerType::SQLite).await;
        let initial_state = VersionedState::new_with_user_message("hello from checkpoint test");

        let session_id = "checkpoint_test_session";
        assert_eq!(
            runner1
                .create_session(session_id.into(), initial_state.clone())
                .await
                .unwrap(),
            SessionInit::Fresh
        );

        // Run one step and verify it completes
        let step1_result = runner1
            .run_step(session_id, StepOptions::default())
            .await
            .unwrap();

        if let StepResult::Completed(report) = step1_result {
            assert_eq!(report.step, 1);
            assert!(!report.ran_nodes.is_empty());
        } else {
            panic!("Expected completed step");
        }

        // Get the session state after first step
        let session_after_step1 = runner1.get_session(session_id).unwrap().clone();
        assert_eq!(session_after_step1.step, 1);

        // Drop the first runner to simulate process restart
        drop(runner1);

        // Create second runner with same SQLite DB - should resume from checkpoint
        let mut runner2 = AppRunner::new(app, CheckpointerType::SQLite).await;

        // Try to create session with same ID - should resume from checkpoint
        let resume_result = runner2
            .create_session(session_id.into(), initial_state)
            .await
            .unwrap();

        assert!(matches!(
            resume_result,
            SessionInit::Resumed { checkpoint_step: 1 }
        ));

        // Verify the resumed session has the same state
        let resumed_session = runner2.get_session(session_id).unwrap();
        assert_eq!(resumed_session.step, session_after_step1.step);
        assert_eq!(resumed_session.frontier, session_after_step1.frontier);
        assert_eq!(
            resumed_session.state.messages.len(),
            session_after_step1.state.messages.len()
        );

        // Clean up environment variable
        std::env::remove_var("WEAVEGRAPH_SQLITE_URL");
    }

    #[tokio::test]
    async fn test_error_event_appended_on_failure() {
        let mut gb = GraphBuilder::new();
        gb = gb.add_node(NodeKind::Start, NodeA);
        gb = gb.add_node(NodeKind::Other("X".into()), FailingNode::default());
        gb = gb.add_edge(NodeKind::Start, NodeKind::Other("X".into()));
        gb = gb.set_entry(NodeKind::Start);
        let app = gb.compile().unwrap();
        let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
        let initial_state = VersionedState::new_with_user_message("hello");

        assert!(matches!(
            runner
                .create_session("err_sess".into(), initial_state)
                .await
                .unwrap(),
            SessionInit::Fresh
        ));

        let res = runner.run_step("err_sess", StepOptions::default()).await;
        assert!(res.is_err());

        // Inspect session state for errors in the errors channel
        let sess = runner.get_session("err_sess").unwrap();
        let errors_snapshot = sess.state.errors.snapshot();
        assert!(
            !errors_snapshot.is_empty(),
            "expected errors to be present in errors channel"
        );

        // Verify the error contains expected information
        let error_event = &errors_snapshot[0];
        // The error should be from the Node scope since it's a node failure
        assert!(matches!(
            error_event.scope,
            crate::channels::errors::ErrorScope::Node { .. }
        ));

        // Verify it's the failing node "X"
        if let crate::channels::errors::ErrorScope::Node { kind, step } = &error_event.scope {
            assert_eq!(kind, "Other:X");
            assert_eq!(*step, 1);
        }
    }
}
