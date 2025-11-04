//! Event bus utilities providing fan-out, sinks, and subscriber APIs.
//!
//! The module is organised around a broadcast-based [`EventHub`] and helpers for
//! configuring sinks (`EventBus`) and consuming the resulting [`EventStream`].
//!
//! # JSON Serialization
//!
//! Events can be serialized to JSON using:
//! - [`Event::to_json_value()`] - Structured JSON value with normalized schema
//! - [`Event::to_json_string()`] - Compact JSON string
//! - [`Event::to_json_pretty()`] - Pretty-printed JSON for debugging
//!
//! The [`JsonLinesSink`] provides machine-readable JSON Lines output for log
//! aggregation systems and monitoring tools.

pub mod bus;
pub mod diagnostics;
pub mod emitter;
pub mod event;
pub mod hub;
pub mod sink;

pub use bus::EventBus;
pub use diagnostics::{DiagnosticsStream, SinkDiagnostic};
pub use emitter::{EmitterError, EventEmitter};
pub use event::{Event, LLMStreamingEvent, NodeEvent, STREAM_END_SCOPE};
pub use hub::{BlockingEventIter, EventHub, EventHubMetrics, EventStream, HubEmitter};
pub use sink::{ChannelSink, EventSink, JsonLinesSink, MemorySink, StdOutSink};
