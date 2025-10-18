use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::{sync::oneshot, task};

use super::emitter::EventEmitter;
use super::hub::{EventHub, EventStream};
use super::sink::{EventSink, StdOutSink};

/// Central event broadcasting system for workflow execution events.
///
/// `EventBus` receives events from workflow nodes and broadcasts them to multiple
/// sinks (stdout, channels, files, monitoring systems, etc.). It's the backbone
/// of Weavegraph's observability and streaming capabilities.
///
/// # Architecture
///
/// The EventBus is owned by [`AppRunner`](crate::runtimes::runner::AppRunner), not
/// [`App`](crate::app::App). This design allows:
/// - Multiple runners to share the same graph with different event configurations
/// - Per-request event isolation in web servers
/// - Flexible sink composition
///
/// ```text
/// Workflow Nodes
///     │ ctx.emit()
///     ▼
/// EventBus
///     │ broadcast
///     ├─────┬─────┬─────┐
///     ▼     ▼     ▼     ▼
/// StdOut Channel File Custom
///  Sink   Sink   Sink  Sink
/// ```
///
/// # Usage Patterns
///
/// ## Default EventBus (Stdout Only)
///
/// When using [`App::invoke()`](crate::app::App::invoke), a default EventBus
/// with stdout sink is created automatically:
///
/// ```rust,no_run
/// # use weavegraph::app::App;
/// # use weavegraph::state::VersionedState;
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
/// // Events automatically go to stdout
/// let result = app.invoke(VersionedState::new_with_user_message("Hello")).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Custom EventBus (Streaming to Web Clients)
///
/// For streaming events to web clients, create a custom EventBus and pass it to
/// [`AppRunner`](crate::runtimes::runner::AppRunner):
///
/// ```rust,no_run
/// use weavegraph::event_bus::{EventBus, ChannelSink, StdOutSink};
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// use weavegraph::state::VersionedState;
/// # use weavegraph::app::App;
/// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
///
/// // Create channel for streaming
/// let (tx, rx) = flume::unbounded();
///
/// // Create EventBus with multiple sinks
/// let bus = EventBus::with_sinks(vec![
///     Box::new(StdOutSink::default()),  // Server logs
///     Box::new(ChannelSink::new(tx)),   // Client streaming
/// ]);
///
/// // Pass EventBus to AppRunner
/// let mut runner = AppRunner::with_options_and_bus(
///     app,
///     CheckpointerType::InMemory,
///     false,
///     bus,  // Custom EventBus
///     true,
/// ).await;
///
/// let session_id = "client-123".to_string();
/// runner.create_session(
///     session_id.clone(),
///     VersionedState::new_with_user_message("Process this")
/// ).await?;
///
/// // Consume events from channel
/// tokio::spawn(async move {
///     while let Ok(event) = rx.recv_async().await {
///         // Send to web client via SSE, WebSocket, etc.
///         println!("Event: {:?}", event);
///     }
/// });
///
/// runner.run_until_complete(&session_id).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Per-Request Isolation (Web Server Pattern)
///
/// Create a new EventBus for each HTTP request to isolate events:
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use weavegraph::event_bus::{EventBus, ChannelSink};
/// use weavegraph::runtimes::{AppRunner, CheckpointerType};
/// use weavegraph::state::VersionedState;
/// # use weavegraph::app::App;
/// # async fn handle_request(app: Arc<App>) -> Result<(), Box<dyn std::error::Error>> {
///
/// // Each request gets its own EventBus and channel
/// let (tx, rx) = flume::unbounded();
/// let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
///
/// // Reuse the App, create new runner with isolated EventBus
/// let mut runner = AppRunner::with_options_and_bus(
///     Arc::try_unwrap(app).unwrap_or_else(|arc| (*arc).clone()),
///     CheckpointerType::InMemory,
///     false,
///     bus,  // Isolated EventBus for this request
///     true,
/// ).await;
///
/// // Run workflow - events are isolated to this request
/// let session_id = uuid::Uuid::new_v4().to_string();
/// runner.create_session(
///     session_id.clone(),
///     VersionedState::new_with_user_message("User query")
/// ).await?;
/// runner.run_until_complete(&session_id).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Available Sinks
///
/// - [`StdOutSink`](crate::event_bus::StdOutSink) - Write to stdout (default)
/// - [`ChannelSink`](crate::event_bus::ChannelSink) - Stream to async channels
/// - [`MemorySink`](crate::event_bus::MemorySink) - Capture for testing
/// - Custom sinks implementing [`EventSink`](crate::event_bus::EventSink)
///
/// # See Also
///
/// - [`AppRunner::with_options_and_bus()`](crate::runtimes::runner::AppRunner::with_options_and_bus) - How to use custom EventBus
/// - [`ChannelSink`](crate::event_bus::ChannelSink) - For streaming events
/// - Example: `examples/streaming_events.rs` - Complete streaming demonstration
const DEFAULT_BUFFER_CAPACITY: usize = 1024;

pub struct EventBus {
    sinks: Arc<Mutex<Vec<SinkEntry>>>,
    hub: Arc<EventHub>,
    started: AtomicBool,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::with_sink(StdOutSink::default())
    }
}

impl EventBus {
    pub fn with_sink<T>(sink: T) -> Self
    where
        T: EventSink + 'static,
    {
        Self::with_sinks(vec![Box::new(sink)])
    }

    pub fn with_sinks(sinks: Vec<Box<dyn EventSink>>) -> Self {
        Self::with_capacity(sinks, DEFAULT_BUFFER_CAPACITY)
    }

    pub(crate) fn with_capacity(sinks: Vec<Box<dyn EventSink>>, buffer_capacity: usize) -> Self {
        let hub = EventHub::new(buffer_capacity);
        let entries = sinks.into_iter().map(SinkEntry::new).collect();
        Self {
            sinks: Arc::new(Mutex::new(entries)),
            hub,
            started: AtomicBool::new(false),
        }
    }

    pub fn add_sink<T: EventSink + 'static>(&self, sink: T) {
        self.add_boxed_sink(Box::new(sink));
    }

    pub fn add_boxed_sink(&self, sink: Box<dyn EventSink>) {
        let mut sinks = self.sinks.lock().unwrap();
        let mut entry = SinkEntry::new(sink);
        if self.started.load(Ordering::SeqCst) {
            entry.spawn_worker(self.hub.clone());
        }
        sinks.push(entry);
    }

    pub fn get_emitter(&self) -> Arc<dyn EventEmitter> {
        Arc::new(self.hub.emitter())
    }

    pub fn subscribe(&self) -> EventStream {
        self.hub.subscribe()
    }

    pub fn listen_for_events(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut sinks = self.sinks.lock().unwrap();
        for entry in sinks.iter_mut() {
            entry.spawn_worker(self.hub.clone());
        }
    }

    pub async fn stop_listener(&self) {
        if !self.started.swap(false, Ordering::SeqCst) {
            return;
        }
        let mut sinks = self.sinks.lock().unwrap();
        for entry in sinks.iter_mut() {
            entry.stop_worker().await;
        }
    }
}

impl Drop for EventBus {
    fn drop(&mut self) {
        if self.started.load(Ordering::SeqCst) {
            if let Ok(mut sinks) = self.sinks.lock() {
                for entry in sinks.iter_mut() {
                    entry.abort_worker();
                }
            }
        }
    }
}

struct SinkEntry {
    sink: Arc<Mutex<Box<dyn EventSink>>>,
    worker: Option<SinkWorker>,
}

impl SinkEntry {
    fn new(sink: Box<dyn EventSink>) -> Self {
        Self {
            sink: Arc::new(Mutex::new(sink)),
            worker: None,
        }
    }

    fn spawn_worker(&mut self, hub: Arc<EventHub>) {
        if self.worker.is_some() {
            return;
        }
        let sink = Arc::clone(&self.sink);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let mut stream = hub.subscribe();
        let handle = task::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    event = stream.recv() => match event {
                        Ok(event) => {
                            if let Ok(mut guard) = sink.lock() {
                                if let Err(err) = guard.handle(&event) {
                                    eprintln!("EventBus sink error: {err}");
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
            }
        });
        self.worker = Some(SinkWorker {
            shutdown: shutdown_tx,
            handle,
        });
    }

    async fn stop_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.shutdown.send(());
            let _ = worker.handle.await;
        }
    }

    fn abort_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.shutdown.send(());
            worker.handle.abort();
        }
    }
}

struct SinkWorker {
    shutdown: oneshot::Sender<()>,
    handle: task::JoinHandle<()>,
}
