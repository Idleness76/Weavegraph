use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures_util::stream::{self, BoxStream, StreamExt};
use tokio::sync::{
    broadcast::{self, Receiver, Sender},
    watch,
};
use tokio::time::timeout;

use super::emitter::{EmitterError, EventEmitter};
use super::event::Event;

#[derive(Debug)]
pub struct EventHub {
    sender: RwLock<Option<Sender<Event>>>,
    dropped_events: AtomicUsize,
    capacity: usize,
}

impl EventHub {
    pub fn new(capacity: usize) -> Arc<Self> {
        let capacity = capacity.max(1);
        let (sender, _) = broadcast::channel(capacity);
        Arc::new(Self {
            sender: RwLock::new(Some(sender)),
            dropped_events: AtomicUsize::new(0),
            capacity,
        })
    }

    pub fn publish(&self, event: Event) -> Result<(), EmitterError> {
        let maybe_sender = {
            let guard = self.sender.read().unwrap();
            guard.as_ref().cloned()
        };
        match maybe_sender {
            Some(sender) => match sender.send(event) {
                Ok(_) => Ok(()),
                Err(broadcast::error::SendError(event)) => {
                    drop(event);
                    Err(EmitterError::Closed)
                }
            },
            None => Err(EmitterError::Closed),
        }
    }

    pub fn subscribe(self: &Arc<Self>) -> EventStream {
        let maybe_receiver = {
            let guard = self.sender.read().unwrap();
            guard.as_ref().cloned().map(|sender| sender.subscribe())
        };
        let receiver = maybe_receiver.unwrap_or_else(|| {
            let (sender, receiver) = broadcast::channel(self.capacity.max(1));
            drop(sender);
            receiver
        });
        EventStream {
            receiver,
            hub: Arc::clone(self),
            shutdown: None,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn dropped(&self) -> usize {
        self.dropped_events.load(Ordering::Relaxed)
    }

    pub fn emitter(self: &Arc<Self>) -> HubEmitter {
        HubEmitter {
            hub: Arc::clone(self),
        }
    }

    pub fn close(&self) {
        let _ = self.sender.write().unwrap().take();
    }
}

#[derive(Clone, Debug)]
pub struct HubEmitter {
    hub: Arc<EventHub>,
}

impl EventEmitter for HubEmitter {
    fn emit(&self, event: Event) -> Result<(), EmitterError> {
        self.hub.publish(event)
    }
}

#[derive(Debug)]
pub struct EventStream {
    receiver: Receiver<Event>,
    hub: Arc<EventHub>,
    shutdown: Option<watch::Receiver<bool>>,
}

impl EventStream {
    pub async fn recv(&mut self) -> Result<Event, broadcast::error::RecvError> {
        match self.receiver.recv().await {
            Ok(event) => Ok(event),
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                self.hub
                    .dropped_events
                    .fetch_add(missed as usize, Ordering::Relaxed);
                Err(broadcast::error::RecvError::Lagged(missed))
            }
            Err(err) => Err(err),
        }
    }

    pub fn try_recv(&mut self) -> Result<Event, broadcast::error::TryRecvError> {
        match self.receiver.try_recv() {
            Ok(event) => Ok(event),
            Err(broadcast::error::TryRecvError::Lagged(missed)) => {
                self.hub
                    .dropped_events
                    .fetch_add(missed as usize, Ordering::Relaxed);
                Err(broadcast::error::TryRecvError::Lagged(missed))
            }
            Err(err) => Err(err),
        }
    }

    pub fn into_inner(self) -> Receiver<Event> {
        self.receiver
    }

    pub fn into_blocking_iter(self) -> BlockingEventIter {
        BlockingEventIter {
            receiver: self.receiver,
            hub: self.hub,
        }
    }

    pub fn with_shutdown(mut self, shutdown: watch::Receiver<bool>) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    pub fn into_async_stream(self) -> BoxStream<'static, Event> {
        let EventStream {
            receiver,
            hub,
            shutdown,
        } = self;
        stream::unfold((receiver, hub, shutdown), |(mut receiver, hub, mut shutdown)| async move {
            loop {
                if let Some(ref mut shutdown_rx) = shutdown {
                    tokio::select! {
                        biased;
                        changed = shutdown_rx.changed() => {
                            if changed.is_ok() && *shutdown_rx.borrow() {
                                return None;
                            }
                            continue;
                        }
                        recv = receiver.recv() => {
                            match recv {
                                Ok(event) => return Some((event, (receiver, hub.clone(), shutdown))),
                                Err(broadcast::error::RecvError::Lagged(missed)) => {
                                    hub.dropped_events.fetch_add(missed as usize, Ordering::Relaxed);
                                    continue;
                                }
                                Err(broadcast::error::RecvError::Closed) => return None,
                            }
                        }
                    }
                } else {
                    match receiver.recv().await {
                        Ok(event) => return Some((event, (receiver, hub.clone(), shutdown))),
                        Err(broadcast::error::RecvError::Lagged(missed)) => {
                            hub.dropped_events.fetch_add(missed as usize, Ordering::Relaxed);
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => return None,
                    }
                }
            }
        })
        .boxed()
    }

    pub async fn next_timeout(&mut self, duration: Duration) -> Option<Event> {
        loop {
            match timeout(duration, self.recv()).await {
                Ok(Ok(event)) => return Some(event),
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                Err(_) => return None,
            }
        }
    }
}

pub struct BlockingEventIter {
    receiver: Receiver<Event>,
    hub: Arc<EventHub>,
}

impl Iterator for BlockingEventIter {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.receiver.blocking_recv() {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    self.hub
                        .dropped_events
                        .fetch_add(missed as usize, Ordering::Relaxed);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}
