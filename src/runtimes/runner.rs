//! Main workflow runner coordinating session management, execution, and event streaming.
//!
//! The `AppRunner` is the central coordinator that brings together:
//! - Session state management (from [`session`](super::session))
//! - Step execution logic (from [`execution`](super::execution))
//! - Event stream handling (see [`App::event_stream`](crate::app::App::event_stream) and
//!   [`App::invoke_streaming`](crate::app::App::invoke_streaming))
//!
//! For most use cases, interact with `AppRunner` directly rather than
//! the constituent modules.

use crate::app::{App, BarrierOutcome};
use crate::channels::Channel;
use crate::channels::errors::{ErrorEvent, ErrorScope, WeaveError};
use crate::control::{FrontierCommand, NodeRoute};
use crate::event_bus::{EventBus, EventStream};
use crate::node::NodePartial;
use crate::runtimes::CheckpointerType;
use crate::runtimes::execution::{
    PausedReason, PausedReport, SchedulerOutcome, StepOptions, StepReport, StepResult,
};
use crate::runtimes::session::{SessionInit, SessionState, StateVersions};
use crate::runtimes::streaming::{StreamEndReason, emit_invocation_end, finalize_event_stream};
use crate::runtimes::{
    Checkpoint, Checkpointer, CheckpointerError, InMemoryCheckpointer, restore_session_state,
};
use crate::schedulers::{Scheduler, SchedulerError, SchedulerRunContext, SchedulerState};
use crate::state::VersionedState;
use crate::types::NodeKind;
use crate::utils::clock::Clock;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinError;
use tracing::instrument;

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
/// ✅ RIGHT: AppRunner::builder() with .event_bus(bus) → Custom EventBus with your sinks
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
/// let mut runner = AppRunner::builder()
///     .app(app)
///     .checkpointer(CheckpointerType::InMemory)
///     .autosave(false)
///     .event_bus(bus)
///     .build()
///     .await;
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
/// - [`builder()`](Self::builder) - Recommended for custom event handling
/// - [`App::invoke()`](crate::app::App::invoke) - Simple execution with defaults
/// - Example: `examples/streaming_events.rs` - Complete streaming demonstration
pub struct AppRunner {
    app: Arc<App>,
    sessions: FxHashMap<String, SessionState>,
    checkpointer: Option<Arc<dyn Checkpointer>>, // optional pluggable persistence
    autosave: bool,
    event_bus: EventBus,
    event_stream_taken: bool,
    clock: Option<Arc<dyn Clock>>,
    checkpointer_descriptor: String,
}

/// Errors that can occur during workflow execution.
#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum RunnerError {
    /// The requested session was not found.
    #[error("session not found: {session_id}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::runner::session_not_found))
    )]
    SessionNotFound {
        /// The session ID that was not found.
        session_id: String,
    },

    /// No nodes are reachable from the Start node.
    #[error("no nodes to run from START (empty frontier)")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::runner::no_start_nodes),
            help("Add edges from Start or set the entry node correctly.")
        )
    )]
    NoStartNodes,

    /// The requested entry node cannot be used to start an iterative invocation.
    #[error("invalid iterative entry node: {node}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::runner::invalid_iterative_entry),
            help(
                "Use NodeKind::Start or a registered custom node. NodeKind::End is terminal and cannot be used as an entry."
            )
        )
    )]
    InvalidIterativeEntry {
        /// The invalid entry node.
        node: NodeKind,
    },

    /// Execution paused unexpectedly during run_until_complete.
    #[error("unexpected pause during run_until_complete")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::runner::unexpected_pause))
    )]
    UnexpectedPause,

    /// The join handle was already consumed by a previous call.
    #[error("join handle already consumed")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::runner::join_handle_consumed),
            help("InvocationHandle::join() can only be called once.")
        )
    )]
    JoinHandleConsumed,

    /// The workflow task failed to join.
    #[error("workflow task join error: {0}")]
    #[cfg_attr(feature = "diagnostics", diagnostic(code(weavegraph::runner::join)))]
    Join(#[from] JoinError),

    /// Checkpointer operation failed.
    #[error(transparent)]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::runner::checkpointer))
    )]
    Checkpointer(#[from] CheckpointerError),

    /// Barrier application failed.
    #[error("app barrier error: {0}")]
    #[cfg_attr(feature = "diagnostics", diagnostic(code(weavegraph::runner::barrier)))]
    AppBarrier(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Scheduler error during step execution.
    #[error(transparent)]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::runner::scheduler))
    )]
    Scheduler(#[from] SchedulerError),
}

/// Runtime metadata useful for audit, replay, and checkpoint labels.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunMetadata {
    /// Weavegraph crate version compiled into this binary.
    pub weavegraph_version: String,
    /// Deterministic graph definition hash.
    pub graph_hash: String,
    /// Deterministic runtime configuration hash.
    pub runtime_config_hash: String,
    /// Descriptor for the configured checkpointer backend.
    pub checkpointer_backend: String,
    /// Descriptor for runtime clock injection mode.
    pub clock_mode: String,
}

struct RunnerRuntimeMetadata {
    clock: Option<Arc<dyn Clock>>,
    checkpointer_descriptor: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionEventPolicy {
    CloseStream,
    KeepStreamOpen,
}

// ============================================================================
// Builder Pattern
// ============================================================================

/// Builder for constructing [`AppRunner`] instances with a fluent API.
///
/// This builder is the canonical way to construct `AppRunner` instances.
/// It provides a single, discoverable interface for all configuration options.
///
/// # Examples
///
/// ## Basic usage with defaults
///
/// ```rust,no_run
/// # use weavegraph::app::App;
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
///
/// let runner = AppRunner::builder()
///     .app(app)
///     .checkpointer(CheckpointerType::InMemory)
///     .build()
///     .await;
/// # Ok(())
/// # }
/// ```
///
/// ## Full configuration with custom EventBus
///
/// ```rust,no_run
/// # use weavegraph::app::App;
/// use weavegraph::event_bus::{EventBus, ChannelSink, StdOutSink};
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
///
/// let (tx, rx) = flume::unbounded();
/// let bus = EventBus::with_sinks(vec![
///     Box::new(StdOutSink::default()),
///     Box::new(ChannelSink::new(tx)),
/// ]);
///
/// let runner = AppRunner::builder()
///     .app(app)
///     .checkpointer(CheckpointerType::SQLite)
///     .event_bus(bus)
///     .autosave(true)
///     .start_listener(true)
///     .build()
///     .await;
/// # Ok(())
/// # }
/// ```
///
/// ## Using `Arc<App>` for shared workflows
///
/// ```rust,no_run
/// # use weavegraph::app::App;
/// use std::sync::Arc;
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
///
/// let shared_app = Arc::new(app);
///
/// // Create multiple runners sharing the same App
/// let runner1 = AppRunner::builder()
///     .app_arc(shared_app.clone())
///     .checkpointer(CheckpointerType::InMemory)
///     .build()
///     .await;
///
/// let runner2 = AppRunner::builder()
///     .app_arc(shared_app)
///     .checkpointer(CheckpointerType::InMemory)
///     .build()
///     .await;
/// # Ok(())
/// # }
/// ```
pub struct AppRunnerBuilder {
    app: Option<Arc<App>>,
    checkpointer_type: CheckpointerType,
    checkpointer_custom: Option<Arc<dyn Checkpointer>>,
    autosave: bool,
    event_bus: Option<EventBus>,
    start_listener: bool,
    clock: Option<Arc<dyn Clock>>,
}

impl Default for AppRunnerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AppRunnerBuilder {
    /// Create a new builder with default settings.
    ///
    /// Defaults:
    /// - `checkpointer`: `InMemory`
    /// - `autosave`: `true`
    /// - `event_bus`: Uses the app's runtime config when built
    /// - `start_listener`: `true`
    #[must_use]
    pub fn new() -> Self {
        Self {
            app: None,
            checkpointer_type: CheckpointerType::InMemory,
            checkpointer_custom: None,
            autosave: true,
            event_bus: None,
            start_listener: true,
            clock: None,
        }
    }

    /// Set the workflow application (takes ownership).
    ///
    /// This is required before calling [`build()`](Self::build).
    #[must_use]
    pub fn app(mut self, app: App) -> Self {
        self.app = Some(Arc::new(app));
        self
    }

    /// Set the workflow application from an existing `Arc<App>`.
    ///
    /// Use this when sharing an `App` across multiple runners to avoid cloning.
    #[must_use]
    pub fn app_arc(mut self, app: Arc<App>) -> Self {
        self.app = Some(app);
        self
    }

    /// Set the checkpointer type for state persistence.
    ///
    /// Defaults to [`CheckpointerType::InMemory`].
    #[must_use]
    pub fn checkpointer(mut self, checkpointer_type: CheckpointerType) -> Self {
        self.checkpointer_type = checkpointer_type;
        self
    }

    /// Set a custom checkpointer implementation.
    ///
    /// When both enum-based and custom checkpointers are configured, the custom
    /// checkpointer takes precedence.
    #[must_use]
    pub fn checkpointer_custom(mut self, checkpointer: Arc<dyn Checkpointer>) -> Self {
        self.checkpointer_custom = Some(checkpointer);
        self
    }

    /// Set whether to automatically save checkpoints after each step.
    ///
    /// Defaults to `true`.
    #[must_use]
    pub fn autosave(mut self, autosave: bool) -> Self {
        self.autosave = autosave;
        self
    }

    /// Set a custom EventBus for event handling.
    ///
    /// If not set, the runner will use the EventBus configured in the app's
    /// [`RuntimeConfig`](crate::runtimes::RuntimeConfig).
    #[must_use]
    pub fn event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Set whether to start the event listener immediately.
    ///
    /// Defaults to `true`. Set to `false` if you need to configure
    /// the EventBus further before starting.
    #[must_use]
    pub fn start_listener(mut self, start: bool) -> Self {
        self.start_listener = start;
        self
    }

    /// Set a runtime clock that will be injected into node contexts.
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Build the [`AppRunner`].
    ///
    /// # Panics
    ///
    /// Panics if [`app()`](Self::app) or [`app_arc()`](Self::app_arc) was not called.
    /// Use [`try_build()`](Self::try_build) for a fallible version.
    pub async fn build(self) -> AppRunner {
        self.try_build()
            .await
            .expect("AppRunnerBuilder requires an app to be set")
    }

    /// Build the [`AppRunner`], returning `None` if no app was provided.
    pub async fn try_build(self) -> Option<AppRunner> {
        let app = self.app?;
        let event_bus = self
            .event_bus
            .unwrap_or_else(|| app.runtime_config().event_bus.build_event_bus());
        let clock = self.clock.or_else(|| app.runtime_config().clock());
        let checkpointer_descriptor = if self.checkpointer_custom.is_some() {
            "custom".to_string()
        } else {
            AppRunner::checkpointer_type_label(&self.checkpointer_type).to_string()
        };
        let runtime_metadata = RunnerRuntimeMetadata {
            clock,
            checkpointer_descriptor,
        };

        Some(
            AppRunner::with_arc_and_bus(
                app,
                self.checkpointer_type,
                self.checkpointer_custom,
                self.autosave,
                event_bus,
                self.start_listener,
                runtime_metadata,
            )
            .await,
        )
    }
}

impl AppRunner {
    /// Create a new [`AppRunnerBuilder`] for fluent configuration.
    ///
    /// This is the **preferred method** for constructing `AppRunner` instances.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use weavegraph::app::App;
    /// use weavegraph::runtimes::{AppRunner, CheckpointerType};
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    ///
    /// let runner = AppRunner::builder()
    ///     .app(app)
    ///     .checkpointer(CheckpointerType::InMemory)
    ///     .autosave(true)
    ///     .build()
    ///     .await;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn builder() -> AppRunnerBuilder {
        AppRunnerBuilder::new()
    }

    async fn create_checkpointer(
        checkpointer_type: CheckpointerType,
        _sqlite_db_name: Option<String>,
    ) -> Option<Arc<dyn Checkpointer>> {
        match checkpointer_type {
            CheckpointerType::InMemory => {
                Some(Arc::new(InMemoryCheckpointer::new()) as Arc<dyn Checkpointer>)
            }
            #[cfg(feature = "sqlite")]
            CheckpointerType::SQLite => {
                let db_url = std::env::var("WEAVEGRAPH_SQLITE_URL")
                    .ok()
                    .or_else(|| {
                        _sqlite_db_name
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

    fn checkpointer_type_label(checkpointer_type: &CheckpointerType) -> &'static str {
        match checkpointer_type {
            CheckpointerType::InMemory => "in-memory",
            #[cfg(feature = "sqlite")]
            CheckpointerType::SQLite => "sqlite",
            #[cfg(feature = "postgres")]
            CheckpointerType::Postgres => "postgres",
        }
    }

    async fn with_arc_and_bus(
        app: Arc<App>,
        checkpointer_type: CheckpointerType,
        checkpointer_custom: Option<Arc<dyn Checkpointer>>,
        autosave: bool,
        event_bus: EventBus,
        start_listener: bool,
        runtime_metadata: RunnerRuntimeMetadata,
    ) -> Self {
        // Precedence rule: custom checkpointer always wins when provided.
        // If custom is None, fall back to enum-based factory instantiation.
        let checkpointer = if let Some(custom) = checkpointer_custom {
            Some(custom)
        } else {
            let sqlite_db_name = app.runtime_config().sqlite_db_name.clone();
            Self::create_checkpointer(checkpointer_type, sqlite_db_name).await
        };
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
            clock: runtime_metadata.clock,
            checkpointer_descriptor: runtime_metadata.checkpointer_descriptor,
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

    /// Initialize or resume a session for repeated invocations under one durable lineage.
    ///
    /// This method behaves like [`create_session`](Self::create_session), then prepares
    /// the session to run from `entry_node`. Passing [`NodeKind::Start`] uses the
    /// graph's outgoing edges from the virtual Start node, matching normal session
    /// initialization. Passing a custom node runs directly from that registered node.
    ///
    /// The session step counter is not reset when a checkpoint is resumed, so steps
    /// remain monotonic across repeated invocations.
    #[instrument(skip(self, session_id, initial_state), err)]
    pub async fn create_iterative_session(
        &mut self,
        session_id: String,
        initial_state: VersionedState,
        entry_node: NodeKind,
    ) -> Result<SessionInit, RunnerError> {
        let frontier = self.frontier_for_iterative_entry(&entry_node)?;
        let init = self
            .create_session(session_id.clone(), initial_state)
            .await?;
        self.set_iterative_frontier(&session_id, frontier)?;
        Ok(init)
    }

    /// Apply an input patch, restart the session frontier, and run to completion.
    ///
    /// The existing session state is updated through the same deterministic barrier
    /// path used for node outputs. The frontier is then reset to `entry_node` and the
    /// scheduler's version-gating state is cleared so the entry path executes for this
    /// logical invocation even when two consecutive input patches serialize to the
    /// same state.
    ///
    /// Use [`create_iterative_session`](Self::create_iterative_session) before the
    /// first call, including after process restart, so the latest checkpoint is loaded
    /// into the runner.
    #[instrument(skip(self, input), err)]
    pub async fn invoke_next(
        &mut self,
        session_id: &str,
        input: NodePartial,
        entry_node: NodeKind,
    ) -> Result<VersionedState, RunnerError> {
        let frontier = self.frontier_for_iterative_entry(&entry_node)?;
        self.apply_iterative_input(session_id, input).await?;
        self.set_iterative_frontier(session_id, frontier)?;
        self.run_until_complete_with_policy(session_id, CompletionEventPolicy::KeepStreamOpen)
            .await
    }

    /// Emit the terminal stream marker for a completed iterative session.
    ///
    /// `invoke_next` keeps long-lived event subscriptions open between logical
    /// inputs. Call this after the final input when a subscriber should receive
    /// [`STREAM_END_SCOPE`](crate::event_bus::STREAM_END_SCOPE) and the stream
    /// should close cleanly.
    pub fn finish_iterative_session(&mut self, session_id: &str) -> Result<(), RunnerError> {
        let (_, _, final_step) = self.finalize_state_snapshot(session_id)?;
        self.emit_completion_event(
            session_id,
            StreamEndReason::Completed { step: final_step },
            CompletionEventPolicy::CloseStream,
        );
        Ok(())
    }

    fn set_iterative_frontier(
        &mut self,
        session_id: &str,
        frontier: Vec<NodeKind>,
    ) -> Result<(), RunnerError> {
        let session_state =
            self.sessions
                .get_mut(session_id)
                .ok_or_else(|| RunnerError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;

        session_state.frontier = frontier;
        session_state.scheduler_state = SchedulerState::default();
        Ok(())
    }

    fn frontier_for_iterative_entry(
        &self,
        entry_node: &NodeKind,
    ) -> Result<Vec<NodeKind>, RunnerError> {
        match entry_node {
            NodeKind::Start => {
                let frontier = self
                    .app
                    .edges()
                    .get(&NodeKind::Start)
                    .cloned()
                    .unwrap_or_default();
                if frontier.is_empty() {
                    Err(RunnerError::NoStartNodes)
                } else {
                    Ok(frontier)
                }
            }
            NodeKind::End => Err(RunnerError::InvalidIterativeEntry {
                node: entry_node.clone(),
            }),
            NodeKind::Custom(_) => {
                if self.app.nodes().contains_key(entry_node) {
                    Ok(vec![entry_node.clone()])
                } else {
                    Err(RunnerError::InvalidIterativeEntry {
                        node: entry_node.clone(),
                    })
                }
            }
        }
    }

    async fn apply_iterative_input(
        &mut self,
        session_id: &str,
        input: NodePartial,
    ) -> Result<(), RunnerError> {
        let mut updated_state = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RunnerError::SessionNotFound {
                session_id: session_id.to_string(),
            })?
            .state
            .clone();

        self.app
            .apply_barrier(&mut updated_state, &[], vec![input])
            .await
            .map_err(RunnerError::AppBarrier)?;

        let session_state =
            self.sessions
                .get_mut(session_id)
                .ok_or_else(|| RunnerError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;
        session_state.state = updated_state;
        Ok(())
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
                // SAFETY: We verified session existence above with the same session_id.
                // If this fails, we have a logic bug (e.g., concurrent mutation).
                let session_state = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| RunnerError::SessionNotFound {
                        session_id: session_id.to_string(),
                    })?
                    .clone();
                return Ok(StepResult::Paused(PausedReport {
                    session_state,
                    reason: PausedReason::BeforeNode(node.clone()),
                }));
            }
        }

        // Take ownership of session state for execution (eliminates full clone)
        // SAFETY: We verified session existence above with the same session_id.
        let mut session_state =
            self.sessions
                .remove(session_id)
                .ok_or_else(|| RunnerError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;

        // Execute one superstep; on error, emit an ErrorEvent and rethrow
        let step_report = match self.run_one_superstep(session_id, &mut session_state).await {
            Ok(rep) => rep,
            Err(e) => {
                // Build error event
                let event = match &e {
                    RunnerError::Scheduler(source) => match source {
                        crate::schedulers::SchedulerError::NodeNotFound { kind, step } => {
                            ErrorEvent {
                                when: chrono::Utc::now(),
                                scope: ErrorScope::Scheduler { step: *step },
                                error: WeaveError::msg(format!(
                                    "node {:?} not found in registry",
                                    kind
                                )),
                                tags: vec!["scheduler".into(), "node_not_found".into()],
                                context: serde_json::json!({
                                    "kind": kind.encode()
                                }),
                            }
                        }
                        crate::schedulers::SchedulerError::NodeRun { kind, step, source } => {
                            ErrorEvent {
                                when: chrono::Utc::now(),
                                scope: ErrorScope::Node {
                                    kind: kind.encode().to_string(),
                                    step: *step,
                                },
                                error: WeaveError::msg(format!("{}", source)),
                                tags: vec!["node".into()],
                                context: serde_json::json!({}),
                            }
                        }
                        crate::schedulers::SchedulerError::Join(_) => ErrorEvent {
                            when: chrono::Utc::now(),
                            scope: ErrorScope::Scheduler {
                                step: session_state.step,
                            },
                            error: WeaveError::msg(format!("{}", e)),
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
                        error: WeaveError::msg(format!("{}", e)),
                        tags: vec!["runner".into()],
                        context: serde_json::json!({
                            "frontier": session_state.frontier.iter().map(|k| k.encode()).collect::<Vec<_>>()
                        }),
                    },
                };
                // Inject via barrier mechanics by applying a synthetic NodePartial with errors field
                let mut update_state = session_state.state.clone();
                let partial = NodePartial::new().with_errors(vec![event]);
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
        session_id: &str,
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
                SchedulerRunContext {
                    event_emitter: self.event_bus.get_emitter(),
                    clock: self.clock.clone(),
                    invocation_id: Some(session_id.to_string()),
                },
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
        session_id: &str,
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
            .in_scope(|| self.schedule_step(session_id, session_state, step))
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
        self.run_until_complete_with_policy(session_id, CompletionEventPolicy::CloseStream)
            .await
    }

    async fn run_until_complete_with_policy(
        &mut self,
        session_id: &str,
        completion_policy: CompletionEventPolicy,
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
                    self.emit_completion_event(
                        session_id,
                        StreamEndReason::Error {
                            step,
                            error: reason,
                        },
                        completion_policy,
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
                    self.emit_completion_event(
                        session_id,
                        StreamEndReason::Error {
                            step,
                            error: "execution paused unexpectedly".to_string(),
                        },
                        completion_policy,
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

        self.emit_completion_event(
            session_id,
            StreamEndReason::Completed { step: final_step },
            completion_policy,
        );
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

    /// Return metadata for this runner and its compiled graph.
    #[must_use]
    pub fn run_metadata(&self) -> RunMetadata {
        RunMetadata {
            weavegraph_version: self.app.weavegraph_version().to_string(),
            graph_hash: self.app.graph_definition_hash(),
            runtime_config_hash: self.app.runtime_config().config_hash(),
            checkpointer_backend: self.checkpointer_descriptor.clone(),
            clock_mode: if self.clock.is_some() {
                "configured".to_string()
            } else {
                "unset".to_string()
            },
        }
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
        finalize_event_stream(
            &self.event_bus,
            session_id,
            reason,
            &mut self.event_stream_taken,
        );
    }

    fn emit_completion_event(
        &mut self,
        session_id: &str,
        reason: StreamEndReason,
        policy: CompletionEventPolicy,
    ) {
        match policy {
            CompletionEventPolicy::CloseStream => self.finalize_event_stream(session_id, reason),
            CompletionEventPolicy::KeepStreamOpen => {
                emit_invocation_end(&self.event_bus, session_id, reason);
            }
        }
    }
}
