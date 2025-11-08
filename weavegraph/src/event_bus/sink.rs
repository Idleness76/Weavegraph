use flume;
use std::any::type_name;
use std::fs::File;
use std::io::{self, Result as IoResult, Stdout, Write};
use std::path::Path;
use std::sync::Arc;
use parking_lot::Mutex as ParkingMutex;

use super::event::Event;
use crate::telemetry::{PlainFormatter, TelemetryFormatter};

/// Abstraction over an output target that consumes full Event objects.
pub trait EventSink: Sync + Send {
    /// Handle a structured event. Sink decides how to serialize/format it.
    ///
    /// Implementations are allowed to perform blocking I/O; the event bus will
    /// hand the call off to `spawn_blocking` to keep the async runtime responsive.
    fn handle(&mut self, event: &Event) -> IoResult<()>;

    /// A stable, human-friendly identifier for this sink instance.
    ///
    /// Defaults to the concrete type name; implementors may override to provide
    /// shorter names or include configuration context.
    fn name(&self) -> String {
        type_name::<Self>().to_string()
    }
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
            formatter: PlainFormatter::new(),
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
    entries: Arc<ParkingMutex<Vec<Event>>>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a snapshot of all captured events. Clones the internal buffer so callers
    /// can inspect state without holding the mutex.
    pub fn snapshot(&self) -> Vec<Event> {
        self.entries.lock().clone()
    }

    /// Clear all captured events.
    pub fn clear(&self) {
        self.entries.lock().clear();
    }
}

impl EventSink for MemorySink {
    fn handle(&mut self, event: &Event) -> IoResult<()> {
        self.entries.lock().push(event.clone());
        Ok(())
    }
}

/// JSON Lines (JSONL) sink for machine-readable structured logging.
///
/// Outputs one JSON object per line, ideal for:
/// - Log aggregation systems (ELK, Splunk, DataDog)
/// - Stream processing pipelines
/// - Automated testing with structured assertions
/// - Integration with monitoring tools
///
/// # Format
///
/// Each event is serialized to a single line of JSON using the normalized schema:
/// ```json
/// {"type":"node","scope":"routing","message":"Processing","timestamp":"2025-11-03T12:34:56Z","metadata":{"node_id":"router","step":5}}
/// {"type":"diagnostic","scope":"system","message":"Ready","timestamp":"2025-11-03T12:34:57Z","metadata":{}}
/// ```
///
/// # Examples
///
/// ## Write to stdout
///
/// ```rust,no_run
/// use weavegraph::event_bus::{EventBus, JsonLinesSink};
///
/// let sink = JsonLinesSink::to_stdout();
/// let bus = EventBus::with_sinks(vec![Box::new(sink)]);
/// // Events will be written as JSON lines to stdout
/// ```
///
/// ## Write to file
///
/// ```rust,no_run
/// use weavegraph::event_bus::{EventBus, JsonLinesSink};
///
/// let sink = JsonLinesSink::to_file("events.jsonl").unwrap();
/// let bus = EventBus::with_sinks(vec![Box::new(sink)]);
/// // Events will be written to events.jsonl
/// ```
///
/// ## Pretty-printed output
///
/// ```rust,no_run
/// use weavegraph::event_bus::JsonLinesSink;
/// use std::io;
///
/// let sink = JsonLinesSink::with_pretty_print(Box::new(io::stdout()));
/// // Events will be pretty-printed (not valid JSONL, but human-readable)
/// ```
pub struct JsonLinesSink {
    handle: Box<dyn Write + Send + Sync>,
    pretty: bool,
}

impl JsonLinesSink {
    /// Create a new JsonLinesSink with a custom writer.
    ///
    /// # Parameters
    ///
    /// * `handle` - Any writer implementing Write + Send
    ///
    /// # Example
    ///
    /// ```rust
    /// use weavegraph::event_bus::JsonLinesSink;
    /// use std::io::Cursor;
    ///
    /// let buffer = Cursor::new(Vec::new());
    /// let sink = JsonLinesSink::new(Box::new(buffer));
    /// ```
    pub fn new(handle: Box<dyn Write + Send + Sync>) -> Self {
        Self {
            handle,
            pretty: false,
        }
    }

    /// Create a JsonLinesSink with pretty-printing enabled.
    ///
    /// Note: Pretty-printed output is NOT valid JSON Lines format
    /// (which requires one JSON object per line). Use this for debugging
    /// and human-readable logs only.
    ///
    /// # Example
    ///
    /// ```rust
    /// use weavegraph::event_bus::JsonLinesSink;
    /// use std::io::Cursor;
    ///
    /// let buffer = Cursor::new(Vec::new());
    /// let sink = JsonLinesSink::with_pretty_print(Box::new(buffer));
    /// ```
    pub fn with_pretty_print(handle: Box<dyn Write + Send + Sync>) -> Self {
        Self {
            handle,
            pretty: true,
        }
    }

    /// Create a JsonLinesSink writing to stdout.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::JsonLinesSink;
    ///
    /// let sink = JsonLinesSink::to_stdout();
    /// ```
    pub fn to_stdout() -> Self {
        Self::new(Box::new(io::stdout()))
    }

    /// Create a JsonLinesSink writing to a file.
    ///
    /// # Parameters
    ///
    /// * `path` - Path to the output file (will be created or truncated)
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or opened.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::JsonLinesSink;
    ///
    /// let sink = JsonLinesSink::to_file("events.jsonl").unwrap();
    /// ```
    pub fn to_file(path: impl AsRef<Path>) -> IoResult<Self> {
        let file = File::create(path)?;
        Ok(Self::new(Box::new(file)))
    }
}

impl EventSink for JsonLinesSink {
    fn handle(&mut self, event: &Event) -> IoResult<()> {
        let json = if self.pretty {
            event.to_json_pretty()
        } else {
            event.to_json_string()
        }
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        writeln!(self.handle, "{}", json)?;
        self.handle.flush()
    }

    fn name(&self) -> String {
        if self.pretty {
            "JsonLinesSink(pretty)".to_string()
        } else {
            "JsonLinesSink".to_string()
        }
    }
}

/// Channel-based sink for streaming events to async consumers.
///
/// `ChannelSink` forwards events to a flume channel, enabling real-time
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
/// # use weavegraph::app::App;
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
///
/// // Create channel
/// let (tx, rx) = flume::unbounded();
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
///     while let Ok(event) = rx.recv_async().await {
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
/// # use weavegraph::app::App;
/// # async fn handle_request(app: Arc<App>, request_id: String) -> Result<(), Box<dyn std::error::Error>> {
///
/// // Each request gets its own channel and EventBus
/// let (tx, rx) = flume::unbounded();
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
/// use futures_util::stream::Stream;
///
/// async fn stream_workflow(
///     State(app): State<Arc<App>>
/// ) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
///     let (tx, rx) = flume::unbounded();
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
///     // Convert flume receiver to stream
///     let stream = rx.into_stream().map(|event| {
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
    tx: flume::Sender<Event>,
}

impl ChannelSink {
    /// Create a new ChannelSink that forwards events to the given channel.
    ///
    /// # Parameters
    ///
    /// * `tx` - The sender side of an unbounded flume channel
    ///
    /// # Returns
    ///
    /// A ChannelSink ready to be added to an EventBus.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::{EventBus, ChannelSink};
    ///
    /// let (tx, rx) = flume::unbounded();
    /// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
    /// ```
    pub fn new(tx: flume::Sender<Event>) -> Self {
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
