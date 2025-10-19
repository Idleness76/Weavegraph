//! Event bus utilities providing fan-out, sinks, and subscriber APIs.
//!
//! The module is organised around a broadcast-based [`EventHub`] and helpers for
//! configuring sinks (`EventBus`) and consuming the resulting [`EventStream`].

pub mod bus;
pub mod emitter;
pub mod event;
pub mod hub;
pub mod sink;

pub use bus::EventBus;
pub use emitter::{EmitterError, EventEmitter};
pub use event::{Event, LLMStreamingEvent, NodeEvent, STREAM_END_SCOPE};
pub use hub::{BlockingEventIter, EventHub, EventHubMetrics, EventStream, HubEmitter};
pub use sink::{ChannelSink, EventSink, MemorySink, StdOutSink};
