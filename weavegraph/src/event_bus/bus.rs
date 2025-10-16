use std::sync::{Arc, Mutex};
use tokio::{sync::oneshot, task};

use super::event::Event;
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
pub struct EventBus {
    sinks: Arc<Mutex<Vec<Box<dyn EventSink>>>>,
    event_channel: (flume::Sender<Event>, flume::Receiver<Event>),
    listener: Arc<Mutex<Option<ListenerState>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::with_sink(StdOutSink::default())
    }
}

impl EventBus {
    /// Create an EventBus with a single sink.
    pub fn with_sink<T>(sink: T) -> Self
    where
        T: EventSink + 'static,
    {
        Self {
            sinks: Arc::new(Mutex::new(vec![Box::new(sink)])),
            event_channel: flume::unbounded(),
            listener: Arc::new(Mutex::new(None)),
        }
    }

    /// Create an EventBus with multiple sinks.
    pub fn with_sinks(sinks: Vec<Box<dyn EventSink>>) -> Self {
        Self {
            sinks: Arc::new(Mutex::new(sinks)),
            event_channel: flume::unbounded(),
            listener: Arc::new(Mutex::new(None)),
        }
    }

    /// Dynamically add a sink (useful for per-request streaming).
    ///
    /// # Example
    /// ```no_run
    /// use weavegraph::event_bus::{EventBus, ChannelSink};
    ///
    /// let bus = EventBus::default();
    /// bus.listen_for_events();
    ///
    /// let (tx, rx) = flume::unbounded();
    /// bus.add_sink(ChannelSink::new(tx));
    /// // Now events go to both stdout and the channel
    /// ```
    pub fn add_sink<T: EventSink + 'static>(&self, sink: T) {
        self.sinks.lock().unwrap().push(Box::new(sink));
    }

    /// Get a clone of the sender side so producers can emit events.
    pub fn get_sender(&self) -> flume::Sender<Event> {
        self.event_channel.0.clone()
    }

    /// Spawn a background task that listens for events and broadcasts to all sinks.
    /// Idempotent: calling multiple times has no effect.
    pub fn listen_for_events(&self) {
        let mut guard = self.listener.lock().expect("listener poisoned");
        if guard.is_some() {
            return; // Already listening
        }

        let receiver_clone = self.event_channel.1.clone();
        let sinks = self.sinks.clone();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let handle = task::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    recv = receiver_clone.recv_async() => match recv {
                        Err(e) => {
                            eprintln!("EventBus receiver error: {e}");
                            break;
                        }
                        Ok(event) => {
                            // Broadcast to all sinks
                            let mut sinks_guard = sinks.lock().unwrap();
                            for sink in sinks_guard.iter_mut() {
                                if let Err(e) = sink.handle(&event) {
                                    eprintln!("EventBus sink error: {e}");
                                }
                            }
                        }
                    }
                }
            }
        });

        *guard = Some(ListenerState {
            shutdown_tx,
            handle,
        });
    }

    /// Stop the background listener task.
    pub async fn stop_listener(&self) {
        let state = {
            let mut guard = self.listener.lock().expect("listener poisoned");
            guard.take()
        };
        if let Some(state) = state {
            let _ = state.shutdown_tx.send(());
            let _ = state.handle.await;
        }
    }
}

impl Drop for EventBus {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.listener.lock() {
            if let Some(state) = guard.take() {
                let _ = state.shutdown_tx.send(());
                state.handle.abort();
            }
        }
    }
}

struct ListenerState {
    shutdown_tx: oneshot::Sender<()>,
    handle: task::JoinHandle<()>,
}
