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

/// Channel-based sink for streaming events to async consumers.
///
/// `ChannelSink` forwards events to a tokio mpsc channel, enabling real-time
/// event streaming to web clients, monitoring systems, or any async consumer.
///
/// # Use Cases
///
/// - **Server-Sent Events (SSE)**: Stream workflow progress to web browsers
/// - **WebSocket**: Real-time bidirectional communication
/// - **Live Dashboards**: Monitor workflow execution in real-time
/// - **Logging Services**: Forward events to centralized logging
/// - **Monitoring**: Send metrics to observability platforms
///
/// # Integration Pattern
///
/// ⚠️ **Important**: `ChannelSink` must be passed to `AppRunner`, not used with `App.invoke()`:
///
/// ```text
/// ❌ WRONG:
/// let bus = EventBus::default();
/// bus.add_sink(ChannelSink::new(tx));
/// graph.invoke(state).await;  // Uses its OWN EventBus!
///
/// ✅ CORRECT:
/// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
/// let runner = AppRunner::with_options_and_bus(app, ..., bus, true).await;
/// runner.run_until_complete(&session_id).await;
/// ```
///
/// # Examples
///
/// ## Basic Streaming
///
/// ```rust,no_run
/// use weavegraph::event_bus::{EventBus, ChannelSink};
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// use weavegraph::state::VersionedState;
/// use tokio::sync::mpsc;
/// # use weavegraph::app::App;
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
///
/// // Create channel
/// let (tx, mut rx) = mpsc::unbounded_channel();
///
/// // Create EventBus with ChannelSink
/// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
///
/// // Use AppRunner with custom EventBus
/// let mut runner = AppRunner::with_options_and_bus(
///     app,
///     CheckpointerType::InMemory,
///     false,
///     bus,
///     true,
/// ).await;
///
/// let session_id = "my-session".to_string();
/// runner.create_session(
///     session_id.clone(),
///     VersionedState::new_with_user_message("Process this")
/// ).await?;
///
/// // Consume events in parallel
/// tokio::spawn(async move {
///     while let Some(event) = rx.recv().await {
///         println!("Event: {:?}", event);
///     }
/// });
///
/// runner.run_until_complete(&session_id).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Web Server Pattern (Per-Request Isolation)
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use weavegraph::event_bus::{EventBus, ChannelSink};
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// use weavegraph::state::VersionedState;
/// use tokio::sync::mpsc;
/// # use weavegraph::app::App;
/// # async fn handle_request(app: Arc<App>, request_id: String) -> Result<(), Box<dyn std::error::Error>> {
///
/// // Each request gets its own channel and EventBus
/// let (tx, rx) = mpsc::unbounded_channel();
/// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
///
/// // Create isolated runner for this request
/// let mut runner = AppRunner::with_options_and_bus(
///     Arc::try_unwrap(app).unwrap_or_else(|arc| (*arc).clone()),
///     CheckpointerType::InMemory,
///     false,
///     bus,
///     true,
/// ).await;
///
/// let session_id = format!("request-{}", request_id);
/// runner.create_session(
///     session_id.clone(),
///     VersionedState::new_with_user_message("User query")
/// ).await?;
///
/// // Events are isolated to this request's channel
/// runner.run_until_complete(&session_id).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Server-Sent Events (SSE) with Axum
///
/// ```rust,ignore
/// use axum::response::sse::{Event as SseEvent, Sse};
/// use tokio_stream::wrappers::UnboundedReceiverStream;
/// use futures_util::stream::Stream;
///
/// async fn stream_workflow(
///     State(app): State<Arc<App>>
/// ) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
///     let (tx, rx) = mpsc::unbounded_channel();
///     let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
///     
///     tokio::spawn(async move {
///         let mut runner = AppRunner::with_options_and_bus(
///             Arc::try_unwrap(app).unwrap_or_else(|arc| (*arc).clone()),
///             CheckpointerType::InMemory,
///             false,
///             bus,
///             true,
///         ).await;
///         
///         let session_id = uuid::Uuid::new_v4().to_string();
///         runner.create_session(
///             session_id.clone(),
///             VersionedState::new_with_user_message("Query")
///         ).await.ok();
///         runner.run_until_complete(&session_id).await.ok();
///     });
///     
///     let stream = UnboundedReceiverStream::new(rx).map(|event| {
///         Ok(SseEvent::default().json_data(event).unwrap())
///     });
///     
///     Sse::new(stream)
/// }
/// ```
///
/// # Error Handling
///
/// If the receiver is dropped, `handle()` returns an error which is logged by the EventBus
/// but doesn't stop event broadcasting to other sinks.
///
/// # See Also
///
/// - [`AppRunner::with_options_and_bus()`](crate::runtimes::runner::AppRunner::with_options_and_bus) - How to inject custom EventBus
/// - [`EventBus::with_sinks()`](crate::event_bus::EventBus::with_sinks) - Create EventBus with sinks
/// - Example: `examples/streaming_events.rs` - Complete working example
pub struct ChannelSink {
    tx: mpsc::UnboundedSender<Event>,
}

impl ChannelSink {
    /// Create a new ChannelSink that forwards events to the given channel.
    ///
    /// # Parameters
    ///
    /// * `tx` - The sender side of an unbounded mpsc channel
    ///
    /// # Returns
    ///
    /// A ChannelSink ready to be added to an EventBus.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::{EventBus, ChannelSink};
    /// use tokio::sync::mpsc;
    ///
    /// let (tx, mut rx) = mpsc::unbounded_channel();
    /// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
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
