use std::sync::{Arc, Mutex};

use tokio::{sync::oneshot, task};

use super::event::Event;
use super::sink::{EventSink, StdOutSink};
use crate::telemetry::{PlainFormatter, TelemetryFormatter, CONTEXT_COLOR, RESET_COLOR};

/// EventBus is responsible for receiving events and forwarding them to an output.
pub struct EventBus {
    // Wrapped in Arc<Mutex<...>> so we can mutate the writer inside the async task.
    output_sink: Arc<Mutex<dyn EventSink>>,
    formatter: Arc<dyn TelemetryFormatter>,
    event_channel: (flume::Sender<Event>, flume::Receiver<Event>),
    listener: Arc<Mutex<Option<ListenerState>>>,
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
        Self::with_sink_and_formatter(sink, PlainFormatter)
    }

    pub fn with_sink_and_formatter<T, F>(sink: T, formatter: F) -> Self
    where
        T: EventSink + 'static,
        F: TelemetryFormatter + 'static,
    {
        Self {
            output_sink: Arc::new(Mutex::new(sink)),
            formatter: Arc::new(formatter),
            event_channel: flume::unbounded(),
            listener: Arc::new(Mutex::new(None)),
        }
    }

    /// Get a clone of the sender side so producers can emit events.
    pub fn get_sender(&self) -> flume::Sender<Event> {
        self.event_channel.0.clone()
    }

    /// Spawn a background task that listens for events and writes them out.
    /// Idempotent: calling multiple times spawns multiple listeners (call once).
    pub fn listen_for_events(&self) {
        let mut guard = self.listener.lock().expect("listener poisoned");
        if guard.is_some() {
            return;
        }
        let receiver_clone = self.event_channel.1.clone();
        let output = self.output_sink.clone();
        let formatter = self.formatter.clone();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let handle = task::spawn(async move {
            let mut current_scope: Option<String> = None;
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    recv = receiver_clone.recv_async() => match recv {
                        Err(e) => {
                            eprintln!("EventBus receiver error: {e}");
                            break;
                        }
                        Ok(event) => {
                            let render = formatter.render_event(&event);
                            let context = render.context.clone();
                            let body = render.join_lines();
                            let message = match context {
                                Some(scope) => {
                                    let colored_scope = format!("{CONTEXT_COLOR}{}{RESET_COLOR}", scope);
                                    if current_scope.as_deref() != Some(scope.as_str()) {
                                        current_scope = Some(scope.clone());
                                        format!("{colored_scope}: {body}")
                                    } else {
                                        body
                                    }
                                }
                                None => {
                                    current_scope = None;
                                    body
                                }
                            };

                            if let Err(e) = output
                                .lock()
                                .map_err(|poisoned| {
                                    std::io::Error::other(
                                        format!("poisoned mutex: {poisoned}"),
                                    )
                                })
                                .and_then(|mut guard| guard.write(&message))
                            {
                                eprintln!("EventBus write error: {e}");
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

    pub async fn stop_listener(&self) {
        let state = {
            let mut guard = self.listener.lock().expect("listener poisoned");
            guard.take()
        };
        if let Some(state) = state {
            let _ = state.shutdown_tx.send(());
            if state.handle.await.is_err() {
                // If the task was already aborted or panicked, nothing else to do.
            }
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
