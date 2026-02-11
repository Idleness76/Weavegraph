//! Session state management for workflow execution.
//!
//! This module defines the core types for managing session state during workflow
//! execution, including state persistence across steps and session initialization.

use crate::schedulers::{Scheduler, SchedulerState};
use crate::state::VersionedState;
use crate::types::NodeKind;

/// Session state that needs to be persisted across steps.
///
/// Contains all the information needed to resume a workflow from a checkpoint,
/// including the versioned state, current step number, execution frontier,
/// and scheduler state.
///
/// # Examples
///
/// ```rust
/// use weavegraph::runtimes::SessionState;
/// use weavegraph::state::VersionedState;
/// use weavegraph::types::NodeKind;
/// use weavegraph::schedulers::{Scheduler, SchedulerState};
///
/// let session = SessionState {
///     state: VersionedState::new_with_user_message("Hello"),
///     step: 0,
///     frontier: vec![NodeKind::Custom("start".into())],
///     scheduler: Scheduler::new(4),
///     scheduler_state: SchedulerState::default(),
/// };
///
/// assert_eq!(session.step, 0);
/// ```
#[derive(Debug, Clone)]
pub struct SessionState {
    /// The versioned state containing messages and extra data.
    pub state: VersionedState,
    /// The current step number in the workflow execution.
    pub step: u64,
    /// The current execution frontier - nodes to be processed next.
    pub frontier: Vec<NodeKind>,
    /// The scheduler managing concurrent node execution.
    pub scheduler: Scheduler,
    /// Internal scheduler tracking state.
    pub scheduler_state: SchedulerState,
}

/// Indicates how a session was initialized.
///
/// Used to inform callers whether they're working with a fresh session
/// or one that was resumed from a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionInit {
    /// A brand new session was created.
    Fresh,
    /// An existing session was resumed from a checkpoint.
    Resumed {
        /// The step number at which the session was checkpointed.
        checkpoint_step: u64,
    },
}

/// Snapshot of channel versions for tracking state evolution.
///
/// Used to detect state changes between steps and enable version-based
/// optimizations in the scheduler.
#[derive(Debug, Clone)]
pub struct StateVersions {
    /// Version counter for the messages channel.
    pub messages_version: u32,
    /// Version counter for the extra data channel.
    pub extra_version: u32,
}
