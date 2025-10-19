use crate::app::App;
use crate::channels::errors::{ErrorEvent, ErrorScope, LadderError};
use crate::channels::Channel;
use crate::event_bus::{Event, EventBus, EventStream, STREAM_END_SCOPE};
use crate::node::NodePartial;
use crate::runtimes::CheckpointerType;
use crate::runtimes::{
    restore_session_state, Checkpoint, Checkpointer, CheckpointerError, InMemoryCheckpointer,
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

enum StreamEndReason {
    Completed { step: u64 },
    Error { step: Option<u64>, error: String },
}

/// Runtime execution engine for workflow graphs with session management and event streaming.
///
/// `AppRunner` wraps an [`App`](crate::app::App) and manages the runtime execution environment,
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
/// The `AppRunner` owns the [`EventBus`](crate::event_bus::EventBus) that receives events
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

    /// Variant that accepts a preconfigured EventBus for an existing Arc<App>.
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
    pub fn event_stream(&mut self) -> EventStream {
        if self.event_stream_taken {
            panic!("event stream already requested for this runner");
        }
        self.event_stream_taken = true;
        self.event_bus.subscribe()
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
                self.event_bus.get_emitter(),
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
            for ce in conditional_edges.iter().filter(|ce| ce.from() == id) {
                println!("running conditional edge from {:?}", ce.from());
                let target_names = (ce.predicate())(snapshot.clone());

                for target_name in target_names {
                    // Convert target name to NodeKind
                    let target = if target_name == "End" {
                        NodeKind::End
                    } else if target_name == "Start" {
                        NodeKind::Start
                    } else {
                        NodeKind::Custom(target_name.clone())
                    };

                    println!("conditional edge routing to {:?}", &target);

                    // Validate that the target node exists or is a virtual endpoint
                    let is_valid_target = match &target {
                        NodeKind::End | NodeKind::Start => true, // Virtual endpoints are always valid
                        NodeKind::Custom(_) => {
                            // Check if the node is registered in the app
                            self.app.nodes().contains_key(&target)
                        }
                    };

                    if is_valid_target {
                        if !next_frontier.contains(&target) {
                            next_frontier.push(target);
                        }
                    } else {
                        // Log a warning but don't fail the execution
                        println!("Warning: Conditional edge target '{}' does not exist in the graph. Skipping.", target_name);
                    }
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

        println!("\n== Final state ==");
        let (
            final_state,
            messages_snapshot,
            messages_version,
            extra_snapshot,
            extra_version,
            final_step,
        ) = {
            let final_session =
                self.sessions
                    .get(session_id)
                    .ok_or_else(|| RunnerError::SessionNotFound {
                        session_id: session_id.to_string(),
                    })?;
            let final_state = final_session.state.clone();
            let messages_snapshot = final_state.messages.snapshot();
            let messages_version = final_state.messages.version();
            let extra_snapshot = final_state.extra.snapshot();
            let extra_version = final_state.extra.version();
            let final_step = final_session.step;
            (
                final_state,
                messages_snapshot,
                messages_version,
                extra_snapshot,
                extra_version,
                final_step,
            )
        };

        // Print final state summary (matching App::invoke output)
        for (i, m) in messages_snapshot.iter().enumerate() {
            println!("#{:02} [{}] {}", i, m.role, m.content);
        }
        println!("messages.version = {}", messages_version);

        println!("extra (v {}) keys={}", extra_version, extra_snapshot.len());
        for (k, v) in extra_snapshot.iter() {
            println!("  {k}: {v}");
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
