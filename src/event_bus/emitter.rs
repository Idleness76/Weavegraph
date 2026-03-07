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
    #[error("event hub closed")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::emitter::emitter_fail),
            help("Check event emitter configuration and downstream handler.")
        )
    )]
    Closed,
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
    pub fn other(error: impl Into<String>) -> Self {
        Self::Other(error.into())
    }
}
