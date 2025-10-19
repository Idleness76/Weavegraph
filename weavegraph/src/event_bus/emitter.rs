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
pub enum EmitterError {
    #[error("event hub closed")]
    Closed,
    #[error("event lag exceeded buffer; dropped {0} messages")]
    Lagged(usize),
    #[error("event emission failed: {0}")]
    Other(String),
}

impl EmitterError {
    pub fn other(error: impl Into<String>) -> Self {
        Self::Other(error.into())
    }
}
