//! Event stream management for workflow execution.
//!
//! This module handles the lifecycle of event streams during workflow
//! execution, including finalization and cleanup.

use crate::event_bus::{Event, EventBus, STREAM_END_SCOPE};

/// Internal reason for ending an event stream.
pub(crate) enum StreamEndReason {
    /// The workflow completed successfully.
    Completed {
        /// The final step number.
        step: u64,
    },
    /// The workflow ended due to an error.
    Error {
        /// The step at which the error occurred (if known).
        step: Option<u64>,
        /// Description of the error.
        error: String,
    },
}

impl StreamEndReason {
    /// Format the stream end reason as a diagnostic message.
    pub fn format_message(&self, session_id: &str) -> String {
        match self {
            StreamEndReason::Completed { step } => {
                format!("session={session_id} status=completed step={step}")
            }
            StreamEndReason::Error { step, error } => step
                .map(|s| format!("session={session_id} status=error step={s} error={error}"))
                .unwrap_or_else(|| format!("session={session_id} status=error error={error}")),
        }
    }
}

/// Handles event stream finalization for a workflow session.
///
/// This function emits a stream termination event and optionally closes
/// the event channel. Called when a workflow completes or errors.
pub(crate) fn finalize_event_stream(
    event_bus: &EventBus,
    session_id: &str,
    reason: StreamEndReason,
    event_stream_taken: &mut bool,
) {
    let message = reason.format_message(session_id);

    if let Err(err) = event_bus
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

    if *event_stream_taken {
        event_bus.close_channel();
        *event_stream_taken = false;
    }
}
