//! Workflow runtime infrastructure for session management and state persistence.
//!
//! This module provides the core runtime components for executing workflows with
//! support for checkpointing, session management, and resumable execution. The
//! runtime layer abstracts over different persistence backends while maintaining
//! a consistent API for workflow execution.
//!
//! # Architecture
//!
//! The runtime is built around several key abstractions:
//!
//! - **[`AppRunner`]** - Main orchestrator for stepwise workflow execution
//! - **[`Checkpointer`]** - Trait for pluggable state persistence
//! - **[`SessionState`]** - In-memory representation of execution state
//! - **Persistence Models** - Serde-friendly types for state serialization
//!
//! # Persistence Backends
//!
//! - **[`InMemoryCheckpointer`]** - Volatile storage for testing and development
//! - **[`SQLiteCheckpointer`]** - Durable SQLite-backed persistence
//!
//! # Usage Example
//!
//! ```rust,no_run
//! use weavegraph::runtimes::{AppRunner, CheckpointerType};
//! use weavegraph::state::VersionedState;
//! # use weavegraph::app::App;
//! # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
//!
//! let mut runner = AppRunner::builder()
//!     .app(app)
//!     .checkpointer(CheckpointerType::InMemory)
//!     .build()
//!     .await;
//! let initial_state = VersionedState::new_with_user_message("Hello");
//!
//! // Create session and run to completion
//! runner.create_session("session_1".to_string(), initial_state).await?;
//! let final_state = runner.run_until_complete("session_1").await?;
//! # Ok(())
//! # }
//! ```

pub mod checkpointer;
#[cfg(feature = "postgres")]
#[cfg_attr(docsrs, doc(cfg(feature = "postgres")))]
pub mod checkpointer_postgres;
#[cfg(feature = "postgres")]
mod checkpointer_postgres_helpers;
#[cfg(feature = "sqlite")]
#[cfg_attr(docsrs, doc(cfg(feature = "sqlite")))]
pub mod checkpointer_sqlite;
#[cfg(feature = "sqlite")]
mod checkpointer_sqlite_helpers;
pub mod execution;
#[cfg(feature = "metrics")]
#[cfg_attr(docsrs, doc(cfg(feature = "metrics")))]
pub mod metrics_observer;
pub mod observer;
pub mod persistence;
pub mod replay;
pub mod runner;
pub mod runtime_config;
pub mod session;
mod streaming;
pub mod types;

pub use checkpointer::{
    Checkpoint, Checkpointer, CheckpointerError, CheckpointerType, InMemoryCheckpointer,
    restore_session_state,
};
#[cfg(feature = "postgres")]
#[cfg_attr(docsrs, doc(cfg(feature = "postgres")))]
pub use checkpointer_postgres::{
    PageInfo as PgPageInfo, PostgresCheckpointer, StepQuery as PgStepQuery,
    StepQueryResult as PgStepQueryResult,
};
#[cfg(feature = "sqlite")]
#[cfg_attr(docsrs, doc(cfg(feature = "sqlite")))]
pub use checkpointer_sqlite::{PageInfo, SQLiteCheckpointer, StepQuery, StepQueryResult};

// Re-export execution types
pub use execution::{PausedReason, PausedReport, StepOptions, StepReport, StepResult};

// Re-export session types
pub use session::{SessionInit, SessionState, StateVersions};

// Re-export runner
pub use runner::{AppRunner, AppRunnerBuilder, RunMetadata};

pub use replay::{
    ReplayComparison, ReplayConformanceError, ReplayRun, StateNormalizeProfile,
    compare_event_sequences, compare_event_sequences_with, compare_final_state,
    compare_final_state_with, compare_replay_runs, compare_replay_runs_with,
    compare_replay_runs_with_profile, normalize_event, normalize_state, normalize_state_with,
};
pub use runtime_config::{EventBusConfig, RuntimeConfig, SinkConfig};
pub use types::{SessionId, StepNumber};

#[cfg(feature = "metrics")]
pub use metrics_observer::MetricsObserver;
pub use observer::{
    CheckpointLoadMeta, CheckpointSaveMeta, EventBusEmitMeta, InvocationFinishMeta,
    InvocationOutcome, InvocationStartMeta, NodeFinishMeta, NodeOutcome, RuntimeObserver,
};
