# Event Bus Refactor Summary

This document tracks the refactor that delivers configurable event routing, public streaming APIs, and richer event payloads for Weavegraph.

## Goals Recap

1. **Runtime-configurable EventBus** – Users select sinks and buffer sizing via `RuntimeConfig::event_bus`.
2. **Pluggable Emitters** – Nodes emit through a trait object (`EventEmitter`) decoupled from channel specifics.
3. **LLM Streaming Events** – Add `Event::LLM(LLMStreamingEvent)` for structured streaming output.
4. **Public Subscriber API** – Expose ergonomic async + blocking event consumers on `App`/`AppRunner`.
5. **Future Sinks** – Broadcast hub architecture keeps adding sinks (Redis, Kafka) straightforward.

## What Shipped

### Configurable EventBus

- `RuntimeConfig` now includes an `EventBusConfig` (`buffer_capacity`, `Vec<SinkConfig>`).
- Helper constructors (`with_stdout_only`, `with_memory_sink`, `add_sink`) ease setup.
- `build_event_bus()` instantiates the configured sinks; `AppRunner` and `App` call this transparently.

### Event Hub + Emitters

- `EventEmitter` trait (sync `emit`) replaces direct `flume::Sender` usage.
- `EventHub` wraps `tokio::sync::broadcast` with lag metrics and spawnable subscribers.
- `EventBus` manages sink workers; `add_sink`/`add_boxed_sink` hooks allow runtime additions.
- `AppRunner::event_stream()` hands out a single-use subscription, preventing duplicate drains.

### LLM Streaming Support

- `LLMStreamingEvent` carries `session_id`, `node_id`, `stream_id`, `chunk`, `metadata`, `is_final`, `timestamp`.
- `Event::LLM` is surfaced in examples and docs so downstream consumers can differentiate streaming messages.

### Subscriber Ergonomics

- `EventStream` exposes:
  - `into_async_stream()` – async iterator (`Stream<Item = Event>`).
  - `into_blocking_iter()` – blocking iterator for CLI use.
  - `next_timeout(Duration)` – await with timeout and lag handling.
- `AppEventStream` mirrors the adapters and publishes `event_bus()` accessor.
- `App::event_stream()` lets users subscribe before invocation; `invoke_with_*` helpers reuse the same pathway.

### Tests & Examples

- New tests cover async stream adapter, blocking iterator, timeout helper, and the runner panic when subscribing twice.
- Examples (`demo5_rag`, `demo6_agent_mcp`) updated to populate the new `event_bus` config field.

## Remaining Follow-ups

- Document the public API in the main README / crate docs with samples.
- Extend examples to show `App::event_stream()` usage in both blocking + async contexts.
- Add guardrails or better DX messaging if users request the event stream after `listen_for_events` starts (currently panics).
- Stage 10 (Redis sink prototype + metrics) – tracked separately.

## Quick Reference

```rust
use weavegraph::app::App;
use weavegraph::runtimes::{EventBusConfig, RuntimeConfig};

let app = graph_builder
    .with_runtime_config(
        RuntimeConfig::default()
            .with_event_bus(EventBusConfig::with_memory_sink())
    )
    .compile()
    .unwrap();

let mut stream_handle = app.event_stream();
let mut events = stream_handle.into_async_stream();

tokio::spawn(async move {
    while let Some(event) = events.next().await {
        println!("scope={:?} message={}", event.scope_label(), event.message());
    }
});
```

This refactor provides a solid base for future sinks (Redis Streams, Kafka) and more advanced observability tooling without breaking existing convenience APIs.
