# Event Bus Refactor Blueprint (v2)

## Guiding Objectives
1. Make event routing configurable from `RuntimeConfig` so every `App` or `AppRunner` instance can select sinks without custom wiring.
2. Decouple node emitters from concrete channels to unlock pluggable transports (memory, channels, stdout, Redis in the future).
3. Introduce `LLMStreamingEvent` as a first-class variant with ergonomic constructors, ensuring downstream consumers can differentiate between incremental LLM output and generic node chatter.
4. Deliver a public subscriber API that works before graph invocation and supports both async `Stream` usage and blocking iteration, enabling ergonomic server and CLI integrations.
5. Keep the architecture open for distributed sinks (Redis Streams, Kafka, etc.) by separating in-process fan-out (`EventHub`) from sink workers.

## Architecture Snapshot
- **EventEmitter**: `Arc`-clonable trait object handing off events to an internal `EventHub`.
- **EventHub**: `tokio::sync::broadcast`-backed multiplexer that feeds sink workers and user subscribers; enforces bounded buffering and exposes lag metrics.

Tokio’s broadcast channel solves a couple of problems the existing flume queue can’t:

- **True fan-out**: cloning a flume receiver gives you work-sharing, not broadcast—each event is delivered to exactly one consumer. Our new API needs every sink and every user subscriber to see the full stream without multiplexing per-subscriber queues, which the broadcast channel gives us for free via `subscribe()`.
- **Async runtime ergonomics**: the broadcast receiver plugs straight into `tokio::select!`, `BroadcastStream`, and backpressure reporting (`Lagged` errors with the count of dropped messages). That makes it easy to expose lag metrics and integrate with async servers. With flume we’d need wrapper tasks and lose insight into subscriber slowness.
- **Configurable buffer with graceful overflow**: for slow subscribers we can adjust the bounded capacity and surface drop counts; flume’s behavior would require per-subscriber buffering logic to avoid blocking the main bus.

- **EventBus**: Owns the hub, manages sink workers, and is created off `RuntimeConfig::event_bus`. Produces `EmitterHandle` for nodes and `SubscriberHandle` for users.
- **Sinks**: Implement `EventSink` and subscribe to the hub. Existing sinks adapt to the new `EventStream` signature; adding Redis in the future means plugging in a worker that consumes the stream and publishes to Redis Streams.
- **Subscribers**: `App::event_stream()` (and variations) clone the hub subscription to return `EventStream` or `BlockingEvents` handles depending on call site.

```
Node.run()
   │
   ▼
NodeContext.emit()
   │ uses
   ▼
EventEmitter────────────────┐
   │                        │
   │ push(Event)            │ subscribe()
   ▼                        ▼
 EventHub (broadcast chan)  EventStreamHandle
   │             │
   │             └─→ Sink Workers (StdOut, Channel, Redis…)
   │
   └─→ App::event_stream() → user subscriber (async stream or iterator)
```

## Step-by-Step Implementation Plan

### Stage 0 – Discovery & Guard Rails
1. **Inventory Current Usage**: Trace every call to `EventBus::get_sender` and `ctx.emit` to ensure the new emitter trait does not miss a path (scheduler, tests, examples).
2. **Baseline Tests**: Capture outputs of streaming examples (`streaming_events.rs`, `convenience_streaming.rs`) so regressions are obvious after the refactor.

### Stage 1 – Data Model Enhancements
3. **Define `LLMStreamingEvent`** (`weavegraph/src/event_bus/event.rs`):
   - Fields: `session_id: Option<String>`, `node_id: Option<String>`, `stream_id: Option<String>`, `chunk: String`, `is_final: bool`, `metadata: FxHashMap<String, serde_json::Value>`, `timestamp: DateTime<Utc>`.
   - Add helper constructors: `llm_chunk`, `llm_final`, `llm_error` (auto-populate timestamp).
   - Update `Event` enum with `Event::LLM(LLMStreamingEvent)` variant and extend `fmt::Display`.
4. **Adjust Serialization & Docs**:
   - Ensure `serde` derives exist (used by sinks exporting JSON).
   - Update doc comments / READMEs describing event payloads.

### Stage 2 – Runtime Configuration Surface
5. **Introduce Config Types** (`weavegraph/src/runtimes/runtime_config.rs`):
   - Add `EventBusConfig { buffer_capacity: usize, sinks: Vec<SinkConfig> }` plus a simple `EventBufferConfig` alias (`type EventBufferConfig = usize`) if needed.
   - Provide builder helpers: `with_stdout_only()`, `with_memory_sink()`, `with_additional_sink(SinkConfig)`.
   - Default configuration mirrors current behavior: stdout sink only, buffer size 1024; channel-style consumers will attach through the subscriber API rather than config.
6. **Thread Config Through Builder**:
   - Extend `RuntimeConfig::new` & `Default`.
   - Update `GraphBuilder::with_runtime_config` to carry the new config.
   - Add convenience constructors on `RuntimeConfig` for common sink mixes.

### Stage 3 – Core Abstractions
7. **Define Emitter Traits** (`weavegraph/src/event_bus/mod.rs`):
   - `pub trait EventEmitter: Send + Sync { fn emit(&self, event: Event) -> Result<(), EmitterError>; async fn emit_async(&self, event: Event) -> Result<(), EmitterError>; }`
   - Provide `EmitterError` with variants for closed hub, backpressure, serialization.
8. **Implement `HubEmitter`**:
   - Wrap `EventHub` sender; `emit` uses non-async `try_send`, `emit_async` awaits `send`.
   - Export `EmitterHandle` as `Arc<dyn EventEmitter>`.
9. **Refactor `NodeContext`** (`weavegraph/src/node.rs:63`):
   - Replace `flume::Sender<Event>` with `Arc<dyn EventEmitter>`.
   - Update `emit` to call `emit` on the trait and convert errors to `NodeContextError`.
10. **Update Scheduler** (`weavegraph/src/schedulers/scheduler.rs`):
    - Accept `Arc<dyn EventEmitter>` instead of `flume::Sender<Event>`.
    - Ensure clones are taken per task to avoid `Arc` contention.

### Stage 4 – EventHub Implementation
11. **Create `EventHub` Struct** (`weavegraph/src/event_bus/hub.rs` new file):
    - Fields: `sender: tokio::sync::broadcast::Sender<Event>`, `buffer_metrics`.
    - Methods: `publish`, `publish_async`, `subscribe`, `lag`, `capacity`.
    - Provide `EventStream` wrapper around `BroadcastStream` with helpers `into_stream`, `into_blocking_iter`.
12. **Introduce `SubscriberHandle`**:
    - Holds a `broadcast::Receiver<Event>`.
    - Implements `Stream` via `Pin<Box<dyn Stream<Item = Event>>>` & blocking `Iterator`.
    - Provide `try_recv` for low-latency polls.

### Stage 5 – EventBus Refactor
13. **Rewrite `EventBus`** (`weavegraph/src/event_bus/bus.rs`):
    - Store `hub: Arc<EventHub>` and `sinks: Arc<Mutex<Vec<SinkController>>>`.
    - `listen_for_events` spawns sink workers subscribing to hub (no more internal flume queue).
    - Provide `emitter()` returning `Arc<dyn EventEmitter>`.
    - Add `subscribe()` returning `SubscriberHandle`.
14. **Sink Adaptations**:
    - `StdOutSink` and `MemorySink` implement new `EventSink` trait `fn spawn(&self, stream: EventStream) -> SinkGuard`.
    - `ChannelSink` becomes a convenience wrapper around `EventHub::subscribe()` handing back the underlying receiver to caller; document that the `EventBus` no longer pushes into user-provided channels (avoids double-buffering).
    - Prepare an interface `RedisStreamSink` prototype stub to verify API extensibility.

### Stage 6 – Public API Surface
15. **Expose Subscription on `App`** (`weavegraph/src/app.rs`):
    - Add `pub fn event_stream(&self) -> EventStream` that clones the underlying runner configuration and `EventBusConfig`.
    - Refactor `invoke`, `invoke_with_channel`, and `invoke_with_sinks` to request their subscribers through this shared pathway so there’s only one source of truth for event fan-out.
    - Provide a lightweight wrapper (`AppEventStream`) that offers both async `Stream` and blocking iterator adapters to minimize friction for CLI tooling.
16. **Extend `AppRunner`** (`weavegraph/src/runtimes/runner.rs`):
    - Accept external subscriptions: if caller requests `get_event_subscriber()` before `create_session`, return `SubscriberHandle`.
    - Prevent listener duplication: start sink workers once even if subscriber exists, and document the call ordering expectations.

    *Implementation notes*: `AppRunner::event_stream()` already exists; after the App API lands, ensure the runner caches the hub subscription until `listen_for_events()` is invoked, so repeated calls don’t accidentally fast-forward the consumer.
17. **Async + Blocking Helpers**:
    - On `EventStream` expose `into_stream()` for async contexts, `into_blocking_iter()` for CLI loops, and a `next_timeout(Duration)` helper that returns `Option<Event>` to simplify backpressure handling.

### Stage 7 – Backward Compatibility & Migration
18. **Shim Old APIs** *(complete)*:
    - Maintained `invoke_with_channel` and `invoke_with_sinks` by layering channel sinks on top of the broadcast hub.
    - Examples now rely on the configuration-based bus while legacy helpers remain available.

### Stage 8 – Testing & Validation
19. **Unit Tests** *(complete)*:
    - Event hub adapters (`into_async_stream`, `into_blocking_iter`, `next_timeout`) covered via new tests.
    - NodeContext tests updated to exercise the new emitter behaviour.
20. **Integration Tests** *(complete)*:
    - `cargo test -p weavegraph` executes updated `AppRunner` guard tests and streaming helpers.
    - Examples compile with the new configuration surface.
21. **Concurrency / Stress Tests** *(deferred)*:
    - Existing event_bus concurrency test retained; deeper stress harness postponed until Redis integration.

### Stage 9 – Documentation & Communication
22. **Docs Refresh** *(in-progress)*:
    - Replaced `docs/event_bus_refactor.md` with the new architecture summary and quick reference.
    - README/primary docs still need an `App::event_stream()` snippet.
23. **Migration Guide** *(todo)*:
    - Migration appendix outstanding (breaking changes + SSE recipe to be documented).

### Stage 10 – Future-Proof Hooks
24. **Redis Sink Skeleton**:
    - Implement trait stub `RedisStreamSink` behind `#[cfg(feature = "redis")]` to validate composer design.
25. **Metrics Hook**:
    - Expose `EventHubMetrics` so ops can monitor dropped events or subscriber lag.

## ASCII Flowchart
```
+-----------------------+
|   Node::run()        |
+----------+------------+
           |
           v
+-----------------------+
|   NodeContext::emit   |
+----------+------------+
           |
           v
+-----------------------+
|   EventEmitter        |
+----------+------------+
           |
           v
+-----------------------+
|   EventHub (broadcast)|
+-----+------------+----+
      |            |
      |            v
      |     +-------------+
      |     | Event Sinks |
      |     +-------------+
      v
+-----------------------+
| User Subscriber (API) |
+-----------------------+
```

## Execution Log

- **Stage 0 – Step 1** *(complete)*: Enumerated all `EventBus::get_sender` callsites (runner, scheduler, tests, examples). Every consumer clones the flume sender directly, so swapping in the emitter trait will touch those locations only.
- **Stage 1 – Step 3** *(complete)*:
  - Design: Confirmed `LLMStreamingEvent` fields and helper constructors (`chunk_event`, `final_event`, `error_event`) with automatic timestamps.
  - Implementation: Added `Event::LLM` variant plus struct definition and accessors in `weavegraph/src/event_bus/event.rs`. Display, message, and scope helpers now surface LLM stream context while defaulting to `llm_stream` when metadata is missing. Updated documentation snippets and streaming example to account for the new variant.
- **Stage 1 – Step 3 (verification)**: `cargo check -p weavegraph` passes, validating the new enum variant and helpers integrate cleanly with existing call sites.
- **Stage 1 – Step 4** *(ongoing)*: Updated `STREAMING_QUICKSTART.md` to call out the new `Event::LLM` variant so downstream integrations pattern-match all cases; broader doc refresh and serialization helpers still pending.
- **Stage 2 – Step 5** *(complete)*:
  - Design: Settled on `EventBusConfig { buffer_capacity: usize, sinks: Vec<SinkConfig> }` covering built-in, self-contained sinks (`StdOut`, `Memory`), leaving channel-style consumers to the subscriber API so config stays declarative.
  - Implementation: Added `event_bus` field on `RuntimeConfig` with defaults plus helper constructors (`with_event_bus`, `with_memory_sink`, `add_sink`) in `weavegraph/src/runtimes/runtime_config.rs`.
- **Stage 2 – Step 6** *(complete)*: Re-exported `EventBusConfig`/`SinkConfig`, added builder helpers on `RuntimeConfig`, exposed `GraphBuilder::with_event_bus_config`, deduped sink additions, and wired `AppRunner` to materialize event buses from config values.
- **Stage 2 – Steps 5–6 (verification)**: `cargo check -p weavegraph` confirms runtime configuration updates build cleanly.
- **Stage 3 – Step 7** *(in-progress)*:
  - Design: Prefer a lean `EventEmitter` trait with a single synchronous `emit` method (async transports can fan out via background tasks), plus `EmitterError` capturing closed hub, lag/backpressure, and generic failures.
  - Implementation: Added `event_bus/emitter.rs` with minimal trait + structured error type; re-exported from `event_bus::mod`.
- **Stage 3 – Step 8** *(in-progress)*:
  - Design: `HubEmitter` wraps the broadcast sender, mapping `SendError` into `EmitterError::Closed` while lag accounting is captured on the subscriber side when receivers report missed messages.
  - Implementation: Added `event_bus/hub.rs` with `EventHub`, `HubEmitter`, and `EventStream` built on `tokio::sync::broadcast`, including lag metrics via receiver bookkeeping.
- **Stage 3 – Step 8 (verification)**: `cargo check -p weavegraph` passes with the new hub abstractions.
- **Stage 3 – Step 9** *(in-progress)*:
  - Updated `NodeContext` to store `Arc<dyn EventEmitter>` and refactored scheduler, runner, tests, and examples to consume `EventBus::get_emitter()`.
  - Swapped all producers to the trait-based emitter; `FlumeEmitter` is now obsolete after the hub integration.
- **Stage 3 – Step 9 (verification)**: `cargo check -p weavegraph` passes after the NodeContext + emitter migration.
- **Stage 3 – Step 10** *(complete)*: Scheduler instrumentation now clones `Arc<dyn EventEmitter>`; no further adjustments required post hub migration.
- **Stage 5 – Step 13** *(complete)*:
  - Rebuilt `EventBus` on top of the new `EventHub` (`weavegraph/src/event_bus/bus.rs`), replacing the flume queue with broadcast-based sink workers and atomic start/stop management.
  - Updated event bus tests (`weavegraph/tests/event_bus.rs`) to emit via the new emitter API and verified ChannelSink/MemSink fan-out still works.
- **Stage 5 – Step 13 (verification)**: `cargo check -p weavegraph` passes after the EventBus overhaul.
- **Stage 5 – Step 14** *(complete)*: Sink workers now subscribe directly to the hub with per-sink tasks and shutdown handles, preserving existing `EventSink` trait semantics without API changes.
- **Stage 5 – Step 15** *(in-progress)*: Added `AppRunner::event_stream()` to surface hub subscriptions ahead of execution; next step is plumbing `App::event_stream()` and tightening lifetime semantics around shared subscriptions.
- **Stage 5 – Step 15 (verification)**: `cargo check -p weavegraph` passes with the new subscription API.
- **Stage 6 – Step 15** *(in-progress)*: Implemented `AppEventStream` and `App::event_stream()`; core App helpers now reuse the config-driven bus path, pending ergonomic adapters and coverage.
- **Stage 6 – Step 15 (verification)**: `cargo check -p weavegraph` passes after wiring App helpers to the shared event bus flow.
- **Stage 6 – Step 16** *(in-progress)*: `AppRunner::event_stream()` now requires a mutable runner, panics on repeated access, and `EventBus::subscribe()` ensures sink workers are active before handing out handles; doc updates still pending.
- **Stage 6 – Step 16 (verification)**: Added runner panic test and ensured new adapters are covered; `cargo test -p weavegraph` passes.
- **Stage 7 – Step 18** *(complete)*: Legacy convenience helpers reuse the new broadcast hub while examples adopt the config-centric flow.
- **Stage 8 – Step 19–20** *(complete)*: Added targeted unit tests for adapters, runner guard, and confirmed full `cargo test -p weavegraph` success; stress harness remains future work.
- **Stage 9 – Step 22** *(in-progress)*: Authored new architecture summary; README + migration snippets still pending.
- **Stage 6 – Step 17** *(complete)*: Added blocking iterator, timeout helper, and async stream adapter on `EventStream`/`AppEventStream`; examples still pending coverage updates.
- **Stage 6 – Step 15 (start)**: Baseline `cargo check` clean; begin sketching `AppEventStream` wrapper and `App::event_stream()` signature.
- **Stage 6 – Step 17 (verification)**: `cargo check -p weavegraph` passes with the new iterator/timeout helpers.
- **Stage 9 – Step 23** *(todo)*: Migration appendix remaining (document API changes + SSE recipe).

## Production Hardening Plan

The refactor is feature-complete and covered by tests, but a few clarity, documentation, and performance polish tasks remain before we can declare the event bus architecture production-grade. The checklist below evaluates each changed area and details the next actions.

### A. API Clarity & Ergonomics

1. **`App::invoke_streaming` handle** — *in-progress*  
   - ✅ Implemented `InvocationHandle::join`, `abort`, and `is_finished`; `join` surfaces `RunnerError::Join` instead of panicking.  
   - ⬜ Document cancellation semantics (dropping the handle vs. dropping the stream) and add rustdoc examples.  
   - ⬜ Consider exposing a scoped cancellation token instead of `abort` for finer control.  
2. **`EventStream::into_async_stream` signature** — *complete*  
   - Returns `BoxStream<'static, Event>`; callers get a `Send` stream without manual boxing/pinning.  
   - Examples updated to rely directly on the boxed stream.
3. **`AppRunner::event_stream` guard** — *production ready (stub)*  
   - Panic on double subscription already covered by tests; no action required.

### B. Code Cleanliness & Idiomatic Usage

1. **Examples (`streaming_events.rs`, `demo7_axum_sse.rs`)** — *in-progress*  
   - ✅ Updated to call `invoke_streaming` + `InvocationHandle::join`; JSON logging simplified.  
   - ⬜ Demonstrate optional cancellation using `tokio::select!` (e.g., stop streaming when client disconnects).  
   - ⬜ Highlight `Event::LLM` handling in comments for consumers.
2. **App rustdoc** — *needs documentation*  
   - Expand rustdoc for `AppEventStream` and `invoke_streaming` with async + blocking usage snippets.  
   - Link rustdoc examples to the Axum demo and CLI streaming example.
3. **Legacy convenience APIs** — *production ready (stub)*  
   - Internals already use the broadcast hub; no further action.

### C. Documentation & Migration Strategy

1. **README & quickstart polish** — *needs polish*  
   - Finalize README sample to include imports and demonstrate mapping to SSE frames.  
   - In `STREAMING_QUICKSTART.md`, add a comparison table (`invoke_with_channel` vs `invoke_streaming` vs `AppRunner`) and document `next_timeout`.
2. **Migration guide (Stage 9 Step 23)** — *todo*  
   - Author `docs/event_bus_migration.md` covering: removal of raw `flume::Sender`, new `RuntimeConfig::event_bus`, and SSE recipe.  
   - Link the migration doc from README and this plan.

### D. Performance & Observability

1. **Broadcast buffer sizing** — *needs evaluation*  
   - Benchmark high-throughput workflows to validate default capacity (1024).  
   - Expose `EventHub::dropped()` metrics via tracing warning when lag occurs.  
   - Document guidance for tuning `EventBusConfig::buffer_capacity`.
2. **Lag handling helpers** — *production ready (stub)*  
   - `next_timeout` already updates metrics; no additional work required.

### E. Testing & Tooling

1. **SSE integration test** — *needs addition*  
   - Add a `trycmd`/integration test gated behind `--ignored` that launches the Axum example and validates SSE output shape.
2. **Fuzz/property tests for event serialization** — *optional improvement*  
   - Consider QuickCheck/proptest ensuring serialization never panics with complex metadata payloads.

### F. Summary of Readiness

- **Production-ready**: `LLMStreamingEvent`, runtime configuration plumbing, EventHub broadcast model, legacy convenience adapters, lag metrics.  
- **Follow-up required**: `invoke_streaming` ergonomics, async stream signature, documentation/migration notes, perf benchmarking, optional integration tests.  
- Completing the tasks above will position the new event bus architecture for a production flag.
