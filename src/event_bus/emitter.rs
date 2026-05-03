//! [`EventEmitter`] trait and [`EmitterError`] for publishing events to the bus.
use std::fmt;
use thiserror::Error;

use super::event::Event;

/// Trait representing an abstract event emitter that workflow nodes can clone.
pub trait EventEmitter: Send + Sync + fmt::Debug {
    /// Emit an event in a synchronous, non-blocking manner.
    fn emit(&self, event: Event) -> Result<(), EmitterError>;
}

/// Errors that can occur when emitting an event.
#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum EmitterError {
    /// The event hub has been shut down and no longer accepts events.
    #[error("event hub closed")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::emitter::emitter_fail),
            help("Check event emitter configuration and downstream handler.")
        )
    )]
    Closed,
    /// Event emission failed for a reason other than hub closure.
    #[error("event emission failed: {0}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::emitter::emitter_fail),
            help("Check event emitter configuration and downstream handler.")
        )
    )]
    Other(String),
}

impl EmitterError {
    /// Construct an [`EmitterError::Other`] from any string-convertible error message.
    pub fn other(error: impl Into<String>) -> Self {
        Self::Other(error.into())
    }
}
