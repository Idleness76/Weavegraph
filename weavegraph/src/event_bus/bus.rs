use std::sync::{Arc, Mutex};
use tokio::{sync::oneshot, task};

use super::event::Event;
use super::sink::{EventSink, StdOutSink};

/// EventBus is responsible for receiving events and broadcasting to multiple sinks.
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
    /// use tokio::sync::mpsc;
    /// use weavegraph::event_bus::{EventBus, ChannelSink};
    ///
    /// let bus = EventBus::default();
    /// bus.listen_for_events();
    ///
    /// let (tx, rx) = mpsc::unbounded_channel();
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
