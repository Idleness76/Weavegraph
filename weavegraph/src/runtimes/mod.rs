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
//! let mut runner = AppRunner::new(app, CheckpointerType::SQLite).await;
//! let initial_state = VersionedState::new_with_user_message("Hello");
//!
//! // Create session and run to completion
//! runner.create_session("session_1".to_string(), initial_state).await?;
//! let final_state = runner.run_until_complete("session_1").await?;
//! # Ok(())
//! # }
//! ```

pub mod checkpointer;
pub mod checkpointer_sqlite;
mod checkpointer_sqlite_helpers;
pub mod persistence;
pub mod runner;
pub mod runtime_config;

#[cfg(test)]
mod error_persistence_tests;
pub mod types;

pub use checkpointer::{
    restore_session_state, Checkpoint, Checkpointer, CheckpointerError, CheckpointerType,
    InMemoryCheckpointer,
};
pub use checkpointer_sqlite::{PageInfo, SQLiteCheckpointer, StepQuery, StepQueryResult};
pub use persistence::*;
pub use runner::{
    AppRunner, PausedReason, PausedReport, SessionInit, SessionState, StateVersions, StepOptions,
    StepReport, StepResult,
};

pub use runtime_config::RuntimeConfig;
pub use types::{SessionId, StepNumber};
