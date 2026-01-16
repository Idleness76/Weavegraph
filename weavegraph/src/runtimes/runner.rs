use crate::app::{App, BarrierOutcome};
use crate::channels::Channel;
use crate::channels::errors::{ErrorEvent, ErrorScope, LadderError};
use crate::control::{FrontierCommand, NodeRoute};
use crate::event_bus::{Event, EventBus, EventStream, STREAM_END_SCOPE};
use crate::node::NodePartial;
use crate::runtimes::CheckpointerType;
use crate::runtimes::{
    Checkpoint, Checkpointer, CheckpointerError, InMemoryCheckpointer, restore_session_state,
};
use crate::schedulers::{Scheduler, SchedulerError, SchedulerState};
use crate::state::VersionedState;
use crate::types::NodeKind;
use miette::Diagnostic;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinError;
use tracing::instrument;

/// Result of executing one superstep in a session.
///
/// The embedded [`BarrierOutcome`] carries the
/// canonical ordering of updates/errors so callers can persist and resume
/// without drift.
#[derive(Debug, Clone)]
pub struct StepReport {
    pub step: u64,
    pub ran_nodes: Vec<NodeKind>,
    pub skipped_nodes: Vec<NodeKind>,
    pub barrier_outcome: BarrierOutcome,
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

/// Outcome from scheduler after normalization (ordered partials)
struct SchedulerOutcome {
    ran_nodes: Vec<NodeKind>,
    skipped_nodes: Vec<NodeKind>,
    partials: Vec<NodePartial>,
}

enum StreamEndReason {
    Completed { step: u64 },
    Error { step: Option<u64>, error: String },
}

/// Runtime execution engine for workflow graphs with session management and event streaming.
///
/// `AppRunner` wraps an [`App`] and manages the runtime execution environment,
/// including:
/// - **Session Management**: Multiple isolated workflow executions
/// - **Event Streaming**: Custom EventBus with pluggable sinks
/// - **Checkpointing**: State persistence and recovery
/// - **Step Control**: Pausing, resuming, and interrupting execution
///
/// # Architecture: App vs AppRunner
///
/// - **`App`**: The workflow graph structure (nodes, edges, topology)
/// - **`AppRunner`**: The runtime environment (sessions, events, checkpoints)
///
/// This separation allows:
/// - One `App` to be reused across multiple `AppRunner` instances
/// - Each runner to have isolated EventBus configuration
/// - Per-request event streaming in web servers
///
/// # EventBus Integration
///
/// The `AppRunner` owns the [`EventBus`] that receives events
/// from workflow nodes. When you need custom event handling:
///
/// ```text
/// ❌ WRONG: App.invoke() → Uses default EventBus (stdout only)
/// ✅ RIGHT: AppRunner::with_options_and_bus() → Custom EventBus with your sinks
/// ```
///
/// # Usage Patterns
///
/// ## Simple Execution (via App.invoke)
///
/// For basic workflows where stdout logging is sufficient:
///
/// ```rust,no_run
/// # use weavegraph::app::App;
/// # use weavegraph::state::VersionedState;
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
/// // App.invoke() creates an AppRunner internally with default EventBus
/// let final_state = app.invoke(
///     VersionedState::new_with_user_message("Hello")
/// ).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Advanced Execution (Direct AppRunner)
///
/// For production systems needing event streaming, use `AppRunner` directly:
///
/// ```rust,no_run
/// # use weavegraph::app::App;
/// # use weavegraph::state::VersionedState;
/// use weavegraph::event_bus::{EventBus, ChannelSink};
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
///
/// // Create channel for event streaming
/// let (tx, rx) = flume::unbounded();
///
/// // Build custom EventBus
/// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
///
/// // Create runner with custom EventBus
/// let mut runner = AppRunner::with_options_and_bus(
///     app,
///     CheckpointerType::InMemory,
///     false,
///     bus,
///     true,
/// ).await;
///
/// let session_id = "my-session".to_string();
/// runner.create_session(
///     session_id.clone(),
///     VersionedState::new_with_user_message("Hello")
/// ).await?;
///
/// // Events stream to the channel while workflow runs
/// tokio::spawn(async move {
///     while let Ok(event) = rx.recv_async().await {
///         println!("Event: {:?}", event);
///     }
/// });
///
/// runner.run_until_complete(&session_id).await?;
/// # Ok(())
/// # }
/// ```
///
/// # See Also
///
/// - [`with_options_and_bus()`](Self::with_options_and_bus) - Recommended for custom event handling
/// - [`App::invoke()`](crate::app::App::invoke) - Simple execution with defaults
/// - Example: `examples/streaming_events.rs` - Complete streaming demonstration
pub struct AppRunner {
    app: Arc<App>,
    sessions: FxHashMap<String, SessionState>,
    checkpointer: Option<Arc<dyn Checkpointer>>, // optional pluggable persistence
    autosave: bool,
    event_bus: EventBus,
    event_stream_taken: bool,
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

    #[error("workflow task join error: {0}")]
    #[diagnostic(code(weavegraph::runner::join))]
    Join(#[from] JoinError),

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
    /// Create a new AppRunner with default EventBus (stdout only).
    ///
    /// This is the simplest constructor, used internally by [`App::invoke()`](crate::app::App::invoke).
    /// For custom event handling (streaming to web clients, etc.), use
    /// [`with_options_and_bus()`](Self::with_options_and_bus) instead.
    ///
    /// # Parameters
    ///
    /// * `app` - The compiled workflow graph
    /// * `checkpointer_type` - Persistence strategy (InMemory or SQLite)
    ///
    /// # Returns
    ///
    /// An AppRunner with:
    /// - Default EventBus (stdout sink only)
    /// - Autosave enabled
    /// - Event listener started
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use weavegraph::app::App;
    /// use weavegraph::runtimes::{AppRunner, CheckpointerType};
    /// use weavegraph::state::VersionedState;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    ///
    /// let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
    ///
    /// let session_id = "my-session".to_string();
    /// runner.create_session(
    ///     session_id.clone(),
    ///     VersionedState::new_with_user_message("Hello")
    /// ).await?;
    ///
    /// runner.run_until_complete(&session_id).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # See Also
    ///
    /// - [`with_options_and_bus()`](Self::with_options_and_bus) - For custom EventBus
    /// - [`App::invoke()`](crate::app::App::invoke) - Higher-level API using this internally
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
            #[cfg(feature = "sqlite")]
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
                        tracing::error!(
                            url = %db_url,
                            error = %e,
                            "SQLiteCheckpointer initialization failed"
                        );
                        None
                    }
                }
            }
            #[cfg(feature = "postgres")]
            CheckpointerType::Postgres => {
                let db_url = std::env::var("WEAVEGRAPH_POSTGRES_URL")
                    .ok()
                    .or_else(|| std::env::var("DATABASE_URL").ok())
                    .unwrap_or_else(|| "postgresql://localhost/weavegraph".to_string());
                match crate::runtimes::PostgresCheckpointer::connect(&db_url).await {
                    Ok(cp) => Some(Arc::new(cp) as Arc<dyn Checkpointer>),
                    Err(e) => {
                        tracing::error!(
                            url = %db_url,
                            error = %e,
                            "PostgresCheckpointer initialization failed"
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
        let bus = app.runtime_config().event_bus.build_event_bus();
        let app = Arc::new(app);
        Self::with_arc_and_bus(app, checkpointer_type, autosave, bus, true).await
    }

    pub async fn with_options_arc(
        app: Arc<App>,
        checkpointer_type: CheckpointerType,
        autosave: bool,
    ) -> Self {
        let bus = app.runtime_config().event_bus.build_event_bus();
        Self::with_arc_and_bus(app, checkpointer_type, autosave, bus, true).await
    }

    /// Create an AppRunner with a custom EventBus for advanced event handling.
    ///
    /// Use this method when you need to stream events to custom sinks (e.g., web clients,
    /// logging systems, monitoring dashboards). This is the **preferred method** for
    /// production applications that need real-time event streaming.
    ///
    /// # Why Use This Instead of `App.invoke()`?
    ///
    /// - `App.invoke()` uses a **default EventBus** (stdout only)
    /// - This method lets you **inject a custom EventBus** with multiple sinks
    /// - Essential for streaming events to web clients via SSE, WebSocket, etc.
    /// - Allows per-request event isolation in web servers
    ///
    /// # Architecture
    ///
    /// The EventBus is a **runtime concern** managed by `AppRunner`, not `App`:
    ///
    /// ```text
    /// GraphBuilder → App (graph structure)
    ///                 ↓
    ///      AppRunner::with_options_and_bus(app, ..., custom_bus)
    ///                 ↓
    ///      AppRunner { app, event_bus: custom_bus }
    ///                 ↓
    ///      NodeContext gets event_emitter
    ///                 ↓
    ///      Events → EventBus → Your custom sinks
    /// ```
    ///
    /// This design allows multiple AppRunners to share the same App with different
    /// EventBus configurations (e.g., one EventBus per HTTP client connection).
    ///
    /// # Parameters
    ///
    /// * `app` - The compiled workflow graph
    /// * `checkpointer_type` - Persistence strategy (InMemory or SQLite)
    /// * `autosave` - Whether to automatically save checkpoints after each step
    /// * `event_bus` - Your custom EventBus with desired sinks
    /// * `start_listener` - Whether to start the EventBus listener immediately
    ///
    /// # Returns
    ///
    /// A configured `AppRunner` ready to execute workflows with custom event handling.
    ///
    /// # Examples
    ///
    /// ## Streaming Events to Web Clients (SSE)
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::{EventBus, ChannelSink, StdOutSink};
    /// use weavegraph::runtimes::{AppRunner, CheckpointerType};
    /// use weavegraph::state::VersionedState;
    /// # use weavegraph::app::App;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    ///
    /// // Create a streaming channel (one per client in production)
    /// let (tx, rx) = flume::unbounded();
    ///
    /// // Create EventBus with both stdout and channel sinks
    /// let bus = EventBus::with_sinks(vec![
    ///     Box::new(StdOutSink::default()),    // For server logs
    ///     Box::new(ChannelSink::new(tx)),     // For client streaming
    /// ]);
    ///
    /// // Create runner with custom EventBus
    /// let mut runner = AppRunner::with_options_and_bus(
    ///     app,
    ///     CheckpointerType::InMemory,
    ///     false,  // Don't autosave
    ///     bus,    // Our custom EventBus
    ///     true,   // Start listener
    /// ).await;
    ///
    /// // Run workflow - events stream to the channel
    /// let session_id = "client-123".to_string();
    /// let initial_state = VersionedState::new_with_user_message("Process this");
    /// runner.create_session(session_id.clone(), initial_state).await?;
    ///
    /// // Consume events in parallel
    /// tokio::spawn(async move {
    ///     while let Ok(event) = rx.recv_async().await {
    ///         // Send to web client via SSE, WebSocket, etc.
    ///         println!("Stream to client: {:?}", event);
    ///     }
    /// });
    ///
    /// runner.run_until_complete(&session_id).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Per-Request Event Isolation (Web Server Pattern)
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    /// use weavegraph::event_bus::{EventBus, ChannelSink};
    /// use weavegraph::runtimes::{AppRunner, CheckpointerType};
    /// use weavegraph::state::VersionedState;
    /// # use weavegraph::app::App;
    /// # async fn handle_request(app: Arc<App>, request_id: String) -> Result<(), Box<dyn std::error::Error>> {
    ///
    /// // Each request gets its own EventBus and channel
    /// let (tx, rx) = flume::unbounded();
    /// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
    ///
    /// // Clone the app (cheap Arc clone), create isolated runner
    /// let mut runner = AppRunner::with_options_and_bus(
    ///     Arc::try_unwrap(app.clone()).unwrap_or_else(|arc| (*arc).clone()),
    ///     CheckpointerType::InMemory,
    ///     false,
    ///     bus,
    ///     true,
    /// ).await;
    ///
    /// let session_id = format!("request-{}", request_id);
    /// let initial = VersionedState::new_with_user_message("User request");
    /// runner.create_session(session_id.clone(), initial).await?;
    ///
    /// // Events are isolated to this request's channel
    /// runner.run_until_complete(&session_id).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # See Also
    ///
    /// - [`App::invoke()`](crate::app::App::invoke) - Simple execution with default EventBus
    /// - [`EventBus::with_sinks()`](crate::event_bus::EventBus::with_sinks) - Create EventBus with custom sinks
    /// - [`ChannelSink`](crate::event_bus::ChannelSink) - Stream events to async channels
    /// - Example: `examples/streaming_events.rs` - Complete streaming demonstration
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

    /// Variant that accepts a preconfigured EventBus for an existing `Arc<App>`.
    ///
    /// Same as [`with_options_and_bus()`](Self::with_options_and_bus) but accepts
    /// an `Arc<App>` to avoid unnecessary cloning when you already have the app
    /// wrapped in an Arc.
    ///
    /// See [`with_options_and_bus()`](Self::with_options_and_bus) for detailed
    /// documentation and examples.
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
            event_stream_taken: false,
        }
    }

    /// Subscribe to the underlying event stream.
    ///
    /// Returns a handle that yields events as they are emitted by workflow nodes.
    /// Subsequent calls after the first return `None` until the stream is
    /// finalized (e.g., when a session completes and the runner resets the flag).
    pub fn event_stream(&mut self) -> Option<EventStream> {
        if self.event_stream_taken {
            return None;
        }
        self.event_stream_taken = true;
        Some(self.event_bus.subscribe())
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
        // Phase 3.1 (Clone Reduction - A): capture minimal snapshots without cloning full session
        let (current_step, current_frontier, current_versions) = {
            let current_session_state =
                self.sessions
                    .get(session_id)
                    .ok_or_else(|| RunnerError::SessionNotFound {
                        session_id: session_id.to_string(),
                    })?;
            let versions = StateVersions {
                messages_version: current_session_state.state.messages.version(),
                extra_version: current_session_state.state.extra.version(),
            };
            (
                current_session_state.step,
                current_session_state.frontier.clone(),
                versions,
            )
        };

        // Check if already completed
        if current_frontier.is_empty() || current_frontier.iter().all(|n| *n == NodeKind::End) {
            return Ok(StepResult::Completed(StepReport {
                step: current_step,
                ran_nodes: vec![],
                skipped_nodes: current_frontier.clone(),
                barrier_outcome: BarrierOutcome::default(),
                next_frontier: vec![],
                state_versions: current_versions,
                completed: true,
            }));
        }

        // Check for interrupt_before
        for node in &current_frontier {
            if options.interrupt_before.contains(node) {
                let session_state = self
                    .sessions
                    .get(session_id)
                    .expect("session exists after initial lookup")
                    .clone();
                return Ok(StepResult::Paused(PausedReport {
                    session_state,
                    reason: PausedReason::BeforeNode(node.clone()),
                }));
            }
        }

        // Take ownership of session state for execution (eliminates full clone)
        let mut session_state = self
            .sessions
            .remove(session_id)
            .expect("session exists after initial lookup");

        // Execute one superstep; on error, emit an ErrorEvent and rethrow
        let step_report = match self.run_one_superstep(&mut session_state).await {
            Ok(rep) => rep,
            Err(e) => {
                // Build error event
                let event = match &e {
                    RunnerError::Scheduler(source) => match source {
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
                    frontier: None,
                };
                // Apply directly using reducer registry through App
                let _ = self
                    .app
                    .apply_barrier(&mut update_state, &[], vec![partial])
                    .await;
                session_state.state = update_state;
                // Save back to sessions map so callers can inspect accumulated errors
                self.sessions.insert(session_id.to_string(), session_state);
                // Re-persist if autosave
                if self.autosave
                    && let Some(cp) = &self.checkpointer
                    && let Some(s) = self.sessions.get(session_id)
                {
                    let _ = cp.save(Checkpoint::from_session(session_id, s)).await;
                }
                return Err(e);
            }
        };

        // Evaluate post-execution interrupts BEFORE reinserting to minimize clones
        // If an interrupt triggers, we insert a clone for persistence and move original into PausedReport.
        if let Some(node) = step_report
            .ran_nodes
            .iter()
            .find(|n| options.interrupt_after.contains(n))
        {
            // Persist a clone, return original in PausedReport
            let persisted = session_state.clone();
            self.sessions.insert(session_id.to_string(), persisted);
            // Re-persist via helper
            self.maybe_checkpoint(session_id, step_report.step).await;
            return Ok(StepResult::Paused(PausedReport {
                session_state,
                reason: PausedReason::AfterNode(node.clone()),
            }));
        }
        if options.interrupt_each_step {
            let persisted = session_state.clone();
            self.sessions.insert(session_id.to_string(), persisted);
            // Re-persist via helper
            self.maybe_checkpoint(session_id, step_report.step).await;
            return Ok(StepResult::Paused(PausedReport {
                session_state,
                reason: PausedReason::AfterStep(step_report.step),
            }));
        }

        // Normal completion path: reinsert owned session_state directly (no clone)
        self.sessions.insert(session_id.to_string(), session_state);
        // Persist via helper
        self.maybe_checkpoint(session_id, step_report.step).await;
        Ok(StepResult::Completed(step_report))
    }

    /// Schedule one step: invoke scheduler and normalize outputs to ordered partials.
    #[inline]
    async fn schedule_step(
        &self,
        session_state: &mut SessionState,
        step: u64,
    ) -> Result<SchedulerOutcome, RunnerError> {
        let snapshot = session_state.state.snapshot();
        let result = session_state
            .scheduler
            .superstep(
                &mut session_state.scheduler_state,
                self.app.nodes(),
                session_state.frontier.clone(),
                snapshot.clone(),
                step,
                self.event_bus.get_emitter(),
            )
            .await?;

        let mut partials_by_kind: FxHashMap<NodeKind, NodePartial> = FxHashMap::default();
        for (k, partial) in result.outputs {
            partials_by_kind.insert(k, partial);
        }
        let executed_nodes = result.ran_nodes.clone();
        let partials = executed_nodes
            .iter()
            .cloned()
            .filter_map(|k| partials_by_kind.remove(&k))
            .collect();

        Ok(SchedulerOutcome {
            ran_nodes: executed_nodes,
            skipped_nodes: result.skipped_nodes,
            partials,
        })
    }

    /// Apply barrier and update session state with the results.
    #[tracing::instrument(skip(self, session_state, partials, ran), err)]
    async fn apply_barrier_and_update(
        &self,
        session_state: &mut SessionState,
        ran: &[NodeKind],
        partials: Vec<NodePartial>,
    ) -> Result<BarrierOutcome, RunnerError> {
        let mut update_state = session_state.state.clone();
        let outcome = self
            .app
            .apply_barrier(&mut update_state, ran, partials)
            .await
            .map_err(RunnerError::AppBarrier)?;
        session_state.state = update_state;
        Ok(outcome)
    }

    /// Compute next frontier from barrier outcome, resolving commands and conditional edges.
    #[inline]
    fn compute_next_frontier(
        &self,
        session_state: &SessionState,
        ran: &[NodeKind],
        barrier: &BarrierOutcome,
        step: u64,
    ) -> Vec<NodeKind> {
        let mut next_frontier: Vec<NodeKind> = Vec::new();
        let graph_edges = self.app.edges();
        let conditional_edges = self.app.conditional_edges();
        let state_snapshot = session_state.state.snapshot();

        let mut frontier_commands_by_node: FxHashMap<NodeKind, Vec<FrontierCommand>> =
            FxHashMap::default();
        for (origin, command) in &barrier.frontier_commands {
            frontier_commands_by_node
                .entry(origin.clone())
                .or_default()
                .push(command.clone());
        }

        for id in ran.iter() {
            let default_edges = graph_edges.get(id).cloned().unwrap_or_default();
            let mut next_targets: Vec<NodeKind> = Vec::new();
            let mut frontier_replaced = false;

            if let Some(commands) = frontier_commands_by_node.get(id) {
                // Commands are processed in emission order to preserve author intent.
                for command in commands {
                    match command {
                        FrontierCommand::Replace(entries) => {
                            if frontier_replaced {
                                tracing::warn!(
                                    step,
                                    origin = %id.encode(),
                                    target = %entries.iter().fold(String::new(),
                                        |acc, e| format!("{} + {}", acc, e.to_node_kind())
                                    ),
                                    "Replace frontier command has been issued once already during this step, skipping."
                                );
                                continue;
                            }
                            next_targets = entries.iter().map(NodeRoute::to_node_kind).collect();
                            frontier_replaced = true;
                        }
                        FrontierCommand::Append(entries) => {
                            if next_targets.is_empty() && !frontier_replaced {
                                next_targets.extend(default_edges.clone());
                            }
                            next_targets.extend(entries.iter().map(NodeRoute::to_node_kind));
                        }
                    }
                }

                if next_targets.is_empty() && !frontier_replaced {
                    next_targets.extend(default_edges.clone());
                }
            } else {
                next_targets.extend(default_edges.clone());
            }

            if !frontier_replaced {
                for conditional_edge in conditional_edges.iter().filter(|ce| ce.from() == id) {
                    tracing::debug!(from = ?conditional_edge.from(), step, "evaluating conditional edge");
                    let target_node_names = (conditional_edge.predicate())(state_snapshot.clone());

                    for target_name in target_node_names {
                        let target = if target_name == "End" {
                            NodeKind::End
                        } else if target_name == "Start" {
                            NodeKind::Start
                        } else {
                            NodeKind::Custom(target_name.clone())
                        };

                        tracing::debug!(target = ?target, step, "conditional edge routed");

                        next_targets.push(target);
                    }
                }
            }

            for target in next_targets {
                let is_valid_target = match &target {
                    NodeKind::End | NodeKind::Start => true,
                    NodeKind::Custom(_) => self.app.nodes().contains_key(&target),
                };

                if is_valid_target {
                    if !next_frontier.contains(&target) {
                        next_frontier.push(target);
                    }
                } else {
                    tracing::warn!(
                        step,
                        origin = %id.encode(),
                        target = %target.encode(),
                        "frontier target not found; skipping"
                    );
                }
            }
        }

        next_frontier
    }

    /// Conditionally persist a checkpoint for the given session if autosave is enabled.
    async fn maybe_checkpoint(&self, session_id: &str, step: u64) {
        let checkpoint_span = tracing::info_span!("checkpoint", step);
        checkpoint_span
            .in_scope(|| async {
                if self.autosave
                    && let Some(checkpointer) = &self.checkpointer
                    && let Some(session_state) = self.sessions.get(session_id)
                {
                    let _ = checkpointer
                        .save(Checkpoint::from_session(session_id, session_state))
                        .await;
                }
            })
            .await;
    }

    /// Helper method that executes exactly one superstep on the given session state.
    ///
    /// Applies barrier outcomes (including frontier commands) and returns the updated
    /// step report with deterministic routing decisions.
    #[instrument(skip(self, session_state), err)]
    async fn run_one_superstep(
        &self,
        session_state: &mut SessionState,
    ) -> Result<StepReport, RunnerError> {
        session_state.step += 1;
        let step = session_state.step;

        tracing::debug!(step, "starting superstep");

        // Phase 1: schedule and normalize outputs
        let schedule_span = tracing::info_span!(
            "schedule",
            step,
            frontier_len = session_state.frontier.len()
        );
        let scheduler_outcome = schedule_span
            .in_scope(|| self.schedule_step(session_state, step))
            .await?;

        // Phase 2: apply barrier and update state
        let errors_in_partials = scheduler_outcome
            .partials
            .iter()
            .filter_map(|p| p.errors.as_ref())
            .map(|e| e.len())
            .sum::<usize>();
        let barrier_span = tracing::info_span!(
            "barrier",
            ran_nodes_len = scheduler_outcome.ran_nodes.len(),
            errors_in_partials
        );
        let barrier_outcome = barrier_span
            .in_scope(|| {
                self.apply_barrier_and_update(
                    session_state,
                    &scheduler_outcome.ran_nodes,
                    scheduler_outcome.partials,
                )
            })
            .await?;

        // Phase 3: compute next frontier
        let commands_count = barrier_outcome.frontier_commands.len();
        let conditional_edges_evaluated = self.app.conditional_edges().len();
        let frontier_span =
            tracing::info_span!("frontier", commands_count, conditional_edges_evaluated);
        let next_frontier = frontier_span.in_scope(|| {
            self.compute_next_frontier(
                session_state,
                &scheduler_outcome.ran_nodes,
                &barrier_outcome,
                step,
            )
        });

        tracing::debug!(
            step,
            updated_channels = ?barrier_outcome.updated_channels,
            error_count = barrier_outcome.errors.len(),
            "barrier applied"
        );
        tracing::debug!(step, next_frontier = ?next_frontier, "computed next frontier");

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
            ran_nodes: scheduler_outcome.ran_nodes,
            skipped_nodes: scheduler_outcome.skipped_nodes,
            barrier_outcome,
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
        tracing::info!(session = %session_id, "workflow run started");

        loop {
            // Check if we're done before trying to run
            let session_state =
                self.sessions
                    .get(session_id)
                    .ok_or_else(|| RunnerError::SessionNotFound {
                        session_id: session_id.to_string(),
                    })?;

            if self.is_session_complete(session_state) {
                tracing::info!(
                    session = %session_id,
                    step = session_state.step,
                    "frontier reached terminal state"
                );
                break;
            }

            // Run one step
            let step_result = match self.run_step(session_id, StepOptions::default()).await {
                Ok(res) => res,
                Err(err) => {
                    let reason = err.to_string();
                    let step = self.sessions.get(session_id).map(|state| state.step);
                    self.finalize_event_stream(
                        session_id,
                        StreamEndReason::Error {
                            step,
                            error: reason,
                        },
                    );
                    return Err(err);
                }
            };

            match step_result {
                StepResult::Completed(report) => {
                    if report.completed {
                        break;
                    }
                }
                StepResult::Paused(_) => {
                    // This shouldn't happen with default options, but handle gracefully
                    let step = self.sessions.get(session_id).map(|state| state.step);
                    self.finalize_event_stream(
                        session_id,
                        StreamEndReason::Error {
                            step,
                            error: "execution paused unexpectedly".to_string(),
                        },
                    );
                    return Err(RunnerError::UnexpectedPause);
                }
            }
        }

        tracing::info!(session = %session_id, "workflow run completed");
        let (final_state, versions, final_step) = self.finalize_state_snapshot(session_id)?;
        let messages_snapshot = final_state.messages.snapshot();
        let extra_snapshot = final_state.extra.snapshot();
        let messages_version = versions.messages_version;
        let extra_version = versions.extra_version;

        // Print final state summary (matching App::invoke output)
        for (i, m) in messages_snapshot.iter().enumerate() {
            tracing::debug!(
                session = %session_id,
                message_index = i,
                role = %m.role,
                content = %m.content,
                "final message snapshot entry"
            );
        }
        tracing::debug!(
            session = %session_id,
            messages_version,
            "messages channel version"
        );

        tracing::debug!(
            session = %session_id,
            extra_version,
            keys = extra_snapshot.len(),
            "extra channel summary"
        );
        for (k, v) in extra_snapshot.iter() {
            tracing::debug!(
                session = %session_id,
                key = %k,
                value = %v,
                "final extra entry"
            );
        }

        self.finalize_event_stream(session_id, StreamEndReason::Completed { step: final_step });
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

impl AppRunner {
    /// Determine if a session has reached a terminal frontier (no work or only End nodes).
    #[inline]
    fn is_session_complete(&self, session_state: &SessionState) -> bool {
        session_state.frontier.is_empty()
            || session_state.frontier.iter().all(|n| *n == NodeKind::End)
    }

    /// Return the final state clone, channel versions, and last step for the session.
    /// Logging should occur after retrieval by the caller.
    #[inline]
    fn finalize_state_snapshot(
        &self,
        session_id: &str,
    ) -> Result<(VersionedState, StateVersions, u64), RunnerError> {
        let session_state =
            self.sessions
                .get(session_id)
                .ok_or_else(|| RunnerError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;

        let final_state = session_state.state.clone();
        let state_versions = StateVersions {
            messages_version: final_state.messages.version(),
            extra_version: final_state.extra.version(),
        };
        let final_step = session_state.step;
        Ok((final_state, state_versions, final_step))
    }

    fn finalize_event_stream(&mut self, session_id: &str, reason: StreamEndReason) {
        let message = match reason {
            StreamEndReason::Completed { step } => {
                format!("session={session_id} status=completed step={step}")
            }
            StreamEndReason::Error { step, error } => step
                .map(|s| format!("session={session_id} status=error step={s} error={error}"))
                .unwrap_or_else(|| format!("session={session_id} status=error error={error}")),
        };

        if let Err(err) = self
            .event_bus
            .get_emitter()
            .emit(Event::diagnostic(STREAM_END_SCOPE, message.clone()))
        {
            tracing::debug!(
                session = %session_id,
                scope = STREAM_END_SCOPE,
                completion_message = %message,
                error = ?err,
                "failed to emit stream termination event"
            );
        }

        if self.event_stream_taken {
            self.event_bus.close_channel();
            self.event_stream_taken = false;
        }
    }
}
