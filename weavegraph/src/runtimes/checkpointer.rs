//! Checkpointer infrastructure
//!
//! This initial implementation introduces a `Checkpointer` trait and an
//! in‑memory implementation (`InMemoryCheckpointer`). It is intentionally
//! minimal: it stores only the latest checkpoint per session (no history)
//! and performs no serialization (pure in‑process persistence). Later
//! extensions (Week 2+) can add:
//!   * Persistent backends (e.g. Postgres)
//!   * Incremental history / lineage
//!   * Compaction & retention policies
//!   * Structured metadata & tracing correlation IDs
//!

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rustc_hash::FxHashMap;
use std::sync::RwLock;

use crate::{
    runtimes::runner::SessionState, schedulers::SchedulerState, state::VersionedState,
    types::NodeKind,
};

/// A durable snapshot of session execution state at a barrier boundary.
///
/// This structure captures both the current state and execution history
/// to enable full session resumption and audit trails.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub session_id: String,
    pub step: u64,
    pub state: VersionedState,
    pub frontier: Vec<NodeKind>,
    pub versions_seen: FxHashMap<String, FxHashMap<String, u64>>, // scheduler gating
    pub concurrency_limit: usize,
    pub created_at: DateTime<Utc>,
    /// Nodes that executed in this step (empty for step 0)
    pub ran_nodes: Vec<NodeKind>,
    /// Nodes that were skipped in this step (empty for step 0)
    pub skipped_nodes: Vec<NodeKind>,
    /// Channels that were updated in this step (empty for step 0)
    pub updated_channels: Vec<String>,
}

impl Checkpoint {
    /// Create a checkpoint from the current session state.
    ///
    /// This captures a snapshot of the session's execution state that can be
    /// persisted and later restored to resume execution from this point.
    ///
    /// # Parameters
    ///
    /// * `session_id` - Unique identifier for the session
    /// * `session` - Current session state to checkpoint
    ///
    /// # Returns
    ///
    /// A `Checkpoint` containing all necessary state for resumption
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use weavegraph::runtimes::{Checkpoint, SessionState};
    /// # fn example(session_state: SessionState) {
    /// let checkpoint = Checkpoint::from_session("my_session", &session_state);
    /// // checkpoint can now be saved via a Checkpointer
    /// # }
    /// ```
    #[must_use]
    pub fn from_session(session_id: &str, session: &SessionState) -> Self {
        Self {
            session_id: session_id.to_string(),
            step: session.step,
            state: session.state.clone(),
            frontier: session.frontier.clone(),
            versions_seen: session.scheduler_state.versions_seen.clone(),
            concurrency_limit: session.scheduler.concurrency_limit,
            created_at: Utc::now(),
            ran_nodes: vec![], // No execution history for raw session state
            skipped_nodes: vec![],
            updated_channels: vec![],
        }
    }

    /// Create a checkpoint from a completed step report.
    ///
    /// This captures the full execution context including what nodes ran,
    /// were skipped, and which channels were updated during the step.
    ///
    /// # Parameters
    ///
    /// * `session_id` - Unique identifier for the session
    /// * `session_state` - Current session state after step execution  
    /// * `step_report` - Details of what happened during step execution
    ///
    /// # Returns
    ///
    /// A `Checkpoint` with complete step execution metadata
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use weavegraph::runtimes::{Checkpoint, SessionState, StepReport};
    /// # fn example(session_state: SessionState, step_report: StepReport) {
    /// let checkpoint = Checkpoint::from_step_report(
    ///     "my_session",
    ///     &session_state,
    ///     &step_report
    /// );
    /// # }
    /// ```
    #[must_use]
    pub fn from_step_report(
        session_id: &str,
        session_state: &SessionState,
        step_report: &crate::runtimes::runner::StepReport,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            step: session_state.step,
            state: session_state.state.clone(),
            frontier: session_state.frontier.clone(),
            versions_seen: session_state.scheduler_state.versions_seen.clone(),
            concurrency_limit: session_state.scheduler.concurrency_limit,
            created_at: Utc::now(),
            ran_nodes: step_report.ran_nodes.clone(),
            skipped_nodes: step_report.skipped_nodes.clone(),
            updated_channels: step_report
                .updated_channels
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }
}

/// Errors from checkpointer operations.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum CheckpointerError {
    /// Session was not found in the checkpointer.
    #[error("session not found: {session_id}")]
    #[diagnostic(
        code(weavegraph::checkpointer::not_found),
        help("Ensure the session ID is correct and the session has been created.")
    )]
    NotFound { session_id: String },

    /// Backend storage error (database, filesystem, etc.).
    #[error("backend error: {message}")]
    #[diagnostic(
        code(weavegraph::checkpointer::backend),
        help("Check backend connectivity and permissions.")
    )]
    Backend { message: String },

    /// Other checkpointer errors.
    #[error("checkpointer error: {message}")]
    #[diagnostic(code(weavegraph::checkpointer::other))]
    Other { message: String },
}

/// Selects the backing implementation of the `Checkpointer` trait.
///
/// Variants:
/// * `InMemory` – Volatile process‑local storage. Fast, non‑durable; suitable for
///   tests and ephemeral runs.
/// * `SQLite` – Durable, file (or memory) backed storage using `SQLiteCheckpointer`
///   (see `runtimes::checkpointer_sqlite`). Persists step history and the latest
///   snapshot for session resumption.
///
/// Note:
/// The runtime previously had an unreachable wildcard match when exhaustively
/// enumerating these variants. If additional variants are added in the future,
/// they should be explicitly matched (or a deliberate catch‑all retained).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointerType {
    /// In‑memory (non‑durable) checkpointing.
    InMemory,
    /// SQLite‑backed durable checkpointing (see `SQLiteCheckpointer`).
    SQLite,
}

pub type Result<T> = std::result::Result<T, CheckpointerError>;

/// Trait for persistent storage and retrieval of workflow execution state.
///
/// Checkpointers provide durable storage for workflow execution state, enabling
/// session resumption across process restarts. Implementations must ensure that
/// checkpoints are atomic and consistent.
///
/// # Design Principles
///
/// - **Atomicity**: Checkpoint saves should be all-or-nothing operations
/// - **Consistency**: The stored state should always be in a valid, resumable state  
/// - **Idempotency**: Saving the same checkpoint multiple times should be safe
/// - **Isolation**: Concurrent access to different sessions should not interfere
///
/// # Implementation Notes
///
/// - All operations should be idempotent where possible
/// - Concurrent access to the same session should be handled gracefully
/// - Backend errors should be mapped to appropriate `CheckpointerError` variants
/// - The `save` operation replaces any existing checkpoint for the session
/// - The `load_latest` operation returns `None` for non-existent sessions
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` to allow usage across async tasks
/// and thread boundaries. Interior mutability should use appropriate synchronization
/// primitives (e.g., `RwLock`, `Mutex`).
///
/// # Error Handling
///
/// Methods should return specific `CheckpointerError` variants:
/// - `NotFound`: When a session doesn't exist (only for operations that require it)
/// - `Backend`: For storage-related errors (database, filesystem, network)
/// - `Other`: For serialization errors or other unexpected conditions
///
/// # Examples
///
/// ```rust,no_run
/// use weavegraph::runtimes::{Checkpointer, Checkpoint, InMemoryCheckpointer};
/// use weavegraph::state::VersionedState;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let checkpointer = InMemoryCheckpointer::new();
///
/// // Save a checkpoint
/// let state = VersionedState::new_with_user_message("Hello");
/// // ... create checkpoint from session state
/// # let checkpoint = todo!(); // placeholder
/// checkpointer.save(checkpoint).await?;
///
/// // Load the latest checkpoint
/// if let Some(checkpoint) = checkpointer.load_latest("session_id").await? {
///     // Resume execution from checkpoint
///     println!("Resuming from step {}", checkpoint.step);
/// }
///
/// // List all sessions
/// let sessions = checkpointer.list_sessions().await?;
/// println!("Found {} sessions", sessions.len());
/// # Ok(())
/// # }
/// ```
#[async_trait]
pub trait Checkpointer: Send + Sync {
    /// Persist the latest checkpoint for a session.
    ///
    /// This operation should be atomic and idempotent. If a checkpoint already
    /// exists for the session, it will be replaced. The implementation should
    /// ensure that concurrent saves to the same session are handled safely.
    ///
    /// # Parameters
    ///
    /// * `checkpoint` - The checkpoint data to persist
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Checkpoint was successfully saved
    /// * `Err(CheckpointerError)` - Save operation failed
    ///
    /// # Errors
    ///
    /// * `Backend` - Storage backend error (database, filesystem, etc.)
    /// * `Other` - Serialization error or other unexpected condition
    async fn save(&self, checkpoint: Checkpoint) -> Result<()>;

    /// Load the most recent checkpoint for a session.
    ///
    /// Returns `None` if no checkpoint exists for the given session ID.
    /// This operation should be consistent with the latest `save` operation.
    ///
    /// # Parameters
    ///
    /// * `session_id` - Unique identifier for the session
    ///
    /// # Returns
    ///
    /// * `Ok(Some(checkpoint))` - Latest checkpoint was found and loaded
    /// * `Ok(None)` - No checkpoint exists for this session
    /// * `Err(CheckpointerError)` - Load operation failed
    ///
    /// # Errors
    ///
    /// * `Backend` - Storage backend error
    /// * `Other` - Deserialization error or corruption
    async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>>;

    /// List all session IDs known to this checkpointer.
    ///
    /// Returns a vector of session IDs that have at least one checkpoint
    /// stored. The order is implementation-defined but should be consistent.
    ///
    /// # Returns
    ///
    /// * `Ok(session_ids)` - List of all known session IDs
    /// * `Err(CheckpointerError)` - List operation failed
    ///
    /// # Errors
    ///
    /// * `Backend` - Storage backend error
    async fn list_sessions(&self) -> Result<Vec<String>>;
}

/// Simple in‑memory checkpointer. Stores only the *latest* checkpoint per session.
#[derive(Default)]
pub struct InMemoryCheckpointer {
    inner: RwLock<FxHashMap<String, Checkpoint>>,
}

impl InMemoryCheckpointer {
    /// Create a new in-memory checkpointer.
    ///
    /// # Returns
    ///
    /// A new `InMemoryCheckpointer` instance
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(FxHashMap::default()),
        }
    }
}

#[async_trait]
impl Checkpointer for InMemoryCheckpointer {
    async fn save(&self, checkpoint: Checkpoint) -> Result<()> {
        let mut map = self.inner.write().map_err(|e| CheckpointerError::Backend {
            message: format!("lock poisoned: {e}"),
        })?;
        map.insert(checkpoint.session_id.clone(), checkpoint);
        Ok(())
    }

    async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let map = self.inner.read().map_err(|e| CheckpointerError::Backend {
            message: format!("lock poisoned: {e}"),
        })?;
        Ok(map.get(session_id).cloned())
    }

    async fn list_sessions(&self) -> Result<Vec<String>> {
        let map = self.inner.read().map_err(|e| CheckpointerError::Backend {
            message: format!("lock poisoned: {e}"),
        })?;
        Ok(map.keys().cloned().collect())
    }
}

/// Restore a `SessionState` from a persisted `Checkpoint`.
///
/// This utility function reconstructs the in-memory session state from a
/// checkpoint, allowing execution to resume from the checkpointed step.
/// The restored state maintains all version information and scheduler state
/// for seamless continuation.
///
/// # Parameters
///
/// * `cp` - The checkpoint to restore from
///
/// # Returns
///
/// A `SessionState` ready for continued execution with:
/// - Restored versioned state channels (messages, extra)
/// - Correct step counter and frontier nodes
/// - Reconstructed scheduler with original concurrency limits
/// - Preserved version tracking for proper barrier coordination
///
/// # Examples
///
/// ```rust,no_run
/// # use weavegraph::runtimes::{restore_session_state, Checkpoint};
/// # async fn example(checkpoint: Checkpoint) {
/// let session_state = restore_session_state(&checkpoint);
/// // session_state can now be used to continue execution
/// assert_eq!(session_state.step, checkpoint.step);
/// assert_eq!(session_state.frontier, checkpoint.frontier);
/// # }
/// ```
#[must_use = "restored session state should be used to continue execution"]
pub fn restore_session_state(cp: &Checkpoint) -> SessionState {
    use crate::schedulers::Scheduler;
    SessionState {
        state: cp.state.clone(),
        step: cp.step,
        frontier: cp.frontier.clone(),
        scheduler: Scheduler::new(cp.concurrency_limit),
        scheduler_state: SchedulerState {
            versions_seen: cp.versions_seen.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        channels::Channel, schedulers::SchedulerState, state::VersionedState, types::NodeKind,
    };

    #[tokio::test]
    async fn test_save_and_load_roundtrip() {
        let cp_store = InMemoryCheckpointer::new();
        let mut session = SessionState {
            state: VersionedState::new_with_user_message("hi"),
            step: 3,
            frontier: vec![NodeKind::Start],
            scheduler: crate::schedulers::Scheduler::new(4),
            scheduler_state: SchedulerState::default(),
        };
        session.scheduler_state.versions_seen.insert(
            "Start".into(),
            FxHashMap::from_iter([("messages".into(), 1_u64), ("extra".into(), 1_u64)]),
        );

        let cp = Checkpoint::from_session("sess1", &session);
        cp_store.save(cp.clone()).await.unwrap();

        let loaded = cp_store.load_latest("sess1").await.unwrap().unwrap();
        assert_eq!(loaded.step, 3);
        assert_eq!(loaded.frontier, vec![NodeKind::Start]);
        assert_eq!(
            loaded.versions_seen.get("Start").unwrap().get("messages"),
            Some(&1)
        );
        assert_eq!(
            loaded.state.messages.snapshot().len(),
            session.state.messages.snapshot().len()
        );
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let cp_store = InMemoryCheckpointer::new();
        let session = SessionState {
            state: VersionedState::new_with_user_message("x"),
            step: 0,
            frontier: vec![NodeKind::Start],
            scheduler: crate::schedulers::Scheduler::new(1),
            scheduler_state: SchedulerState::default(),
        };
        cp_store
            .save(Checkpoint::from_session("alpha", &session))
            .await
            .unwrap();
        cp_store
            .save(Checkpoint::from_session("beta", &session))
            .await
            .unwrap();
        let mut ids = cp_store.list_sessions().await.unwrap();
        ids.sort();
        assert_eq!(ids, vec!["alpha", "beta"]);
    }
}
