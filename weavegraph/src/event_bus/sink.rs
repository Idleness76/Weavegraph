use std::io::{self, Result as IoResult, Stdout, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::event::Event;
use crate::telemetry::{PlainFormatter, TelemetryFormatter};

/// Abstraction over an output target that consumes full Event objects.
pub trait EventSink: Sync + Send {
    /// Handle a structured event. Sink decides how to serialize/format it.
    fn handle(&mut self, event: &Event) -> IoResult<()>;
}

/// Stdout sink with optional formatting.
pub struct StdOutSink<F: TelemetryFormatter = PlainFormatter> {
    handle: Stdout,
    formatter: F,
}

impl Default for StdOutSink {
    fn default() -> Self {
        Self {
            handle: io::stdout(),
            formatter: PlainFormatter,
        }
    }
}

impl<F: TelemetryFormatter> StdOutSink<F> {
    pub fn with_formatter(formatter: F) -> Self {
        Self {
            handle: io::stdout(),
            formatter,
        }
    }
}

impl<F: TelemetryFormatter> EventSink for StdOutSink<F> {
    fn handle(&mut self, event: &Event) -> IoResult<()> {
        let rendered = self.formatter.render_event(event).join_lines();
        self.handle.write_all(rendered.as_bytes())?;
        self.handle.flush()
    }
}

/// In-memory sink for testing and snapshots.
#[derive(Clone, Default)]
pub struct MemorySink {
    entries: Arc<Mutex<Vec<Event>>>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a snapshot of all captured events.
    pub fn snapshot(&self) -> Vec<Event> {
        self.entries.lock().unwrap().clone()
    }

    /// Clear all captured events.
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

impl EventSink for MemorySink {
    fn handle(&mut self, event: &Event) -> IoResult<()> {
        self.entries.lock().unwrap().push(event.clone());
        Ok(())
    }
}

/// Channel-based sink for streaming to async consumers (e.g., web clients).
///
/// Events are forwarded to a tokio mpsc channel without blocking.
/// Useful for real-time dashboards, SSE endpoints, or live logging.
pub struct ChannelSink {
    tx: mpsc::UnboundedSender<Event>,
}

impl ChannelSink {
    /// Create a new channel sink.
    ///
    /// # Example
    /// ```no_run
    /// use tokio::sync::mpsc;
    /// use weavegraph::event_bus::{EventBus, ChannelSink};
    ///
    /// let (tx, mut rx) = mpsc::unbounded_channel();
    /// let bus = EventBus::default();
    /// bus.add_sink(ChannelSink::new(tx));
    ///
    /// // In another task, consume events:
    /// tokio::spawn(async move {
    ///     while let Some(event) = rx.recv().await {
    ///         println!("Received: {}", event);
    ///     }
    /// });
    /// ```
    pub fn new(tx: mpsc::UnboundedSender<Event>) -> Self {
        Self { tx }
    }
}

impl EventSink for ChannelSink {
    fn handle(&mut self, event: &Event) -> IoResult<()> {
        self.tx
            .send(event.clone())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "channel receiver dropped"))
    }
}
