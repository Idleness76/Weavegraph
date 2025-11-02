use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::stream::{self, BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::{self, Receiver, error};
use tokio::time::timeout;

/// A single diagnostic entry emitted when a sink reports an error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SinkDiagnostic {
    /// Logical sink identifier. Defaults to the sink's type name unless overridden.
    pub sink: String,
    /// Human-readable error message produced by the sink.
    pub error: String,
    /// Timestamp for when the error was observed.
    pub when: DateTime<Utc>,
    /// Monotonic occurrence counter for this sink's errors.
    pub occurrence: u64,
}

/// Stream wrapper for sink diagnostics, mirroring the EventStream API surface.
#[derive(Debug)]
pub struct DiagnosticsStream {
    receiver: Receiver<SinkDiagnostic>,
}

impl DiagnosticsStream {
    pub fn new(receiver: Receiver<SinkDiagnostic>) -> Self {
        Self { receiver }
    }

    /// Receive the next diagnostic, awaiting if necessary.
    pub async fn recv(&mut self) -> Result<SinkDiagnostic, error::RecvError> {
        self.receiver.recv().await
    }

    /// Try to receive a diagnostic without awaiting.
    pub fn try_recv(&mut self) -> Result<SinkDiagnostic, broadcast::error::TryRecvError> {
        self.receiver.try_recv()
    }

    /// Consume this wrapper, returning the inner broadcast receiver.
    pub fn into_inner(self) -> Receiver<SinkDiagnostic> {
        self.receiver
    }

    /// Convert into a boxed async stream of diagnostics.
    pub fn into_async_stream(self) -> BoxStream<'static, SinkDiagnostic> {
        let receiver = self.receiver;
        stream::unfold(receiver, |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(diag) => return Some((diag, receiver)),
                    // Skip lagged notifications and keep draining
                    Err(error::RecvError::Lagged(_)) => continue,
                    Err(error::RecvError::Closed) => return None,
                }
            }
        })
        .boxed()
    }

    /// Wait up to `duration` for the next diagnostic.
    pub async fn next_timeout(&mut self, duration: Duration) -> Option<SinkDiagnostic> {
        loop {
            match timeout(duration, self.recv()).await {
                Ok(Ok(diag)) => return Some(diag),
                Ok(Err(error::RecvError::Lagged(_))) => continue,
                Ok(Err(error::RecvError::Closed)) => return None,
                Err(_) => return None,
            }
        }
    }
}
