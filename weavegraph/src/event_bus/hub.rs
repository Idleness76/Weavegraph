use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::time::timeout;

use super::emitter::{EmitterError, EventEmitter};
use super::event::Event;

#[derive(Debug)]
pub struct EventHub {
    sender: Sender<Event>,
    dropped_events: AtomicUsize,
    capacity: usize,
}

impl EventHub {
    pub fn new(capacity: usize) -> Arc<Self> {
        let capacity = capacity.max(1);
        let (sender, _) = broadcast::channel(capacity);
        Arc::new(Self {
            sender,
            dropped_events: AtomicUsize::new(0),
            capacity,
        })
    }

    pub fn publish(&self, event: Event) -> Result<(), EmitterError> {
        match self.sender.send(event) {
            Ok(_) => Ok(()),
            Err(broadcast::error::SendError(event)) => {
                drop(event);
                Err(EmitterError::Closed)
            }
        }
    }

    pub fn subscribe(self: &Arc<Self>) -> EventStream {
        EventStream {
            receiver: self.sender.subscribe(),
            hub: Arc::clone(self),
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

    pub fn into_async_stream(self) -> impl futures_util::stream::Stream<Item = Event> {
        stream::unfold(self, |mut stream| async move {
            loop {
                match stream.recv().await {
                    Ok(event) => return Some((event, stream)),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
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
