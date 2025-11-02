# Streaming Events Quickstart

This guide shows you how to stream workflow events to web clients using Weavegraph's `EventStream` helpers and, when needed, the legacy channel-based sinks.

## Choose Your Pattern

| Scenario | API | Event Consumption | Notes | Example |
|----------|-----|-------------------|-------|---------|
| CLI / scripts | `App::invoke_with_channel` | flume receiver | Simplest to wire progress bars, returns `(Result, Receiver)` | `examples/convenience_streaming.rs` |
| CLI with multiple sinks | `App::invoke_with_sinks` | sinks + optional channel | Inject stdout/file sinks without touching `AppRunner` | same as above |
| Web servers / SSE/WebSocket | `App::invoke_streaming` | `EventStream` (async/iter/poll) | Preferred for live streaming; emits `STREAM_END_SCOPE` sentinel when finished | `examples/demo7_axum_sse.rs` |
| Full control | `AppRunner::with_options_and_bus` | custom `EventBus` | Use when you need per-request isolation or reuse a runner | `examples/streaming_events.rs` |

### ⭐ Simple Patterns (Convenience Methods)

For CLI tools and simple scripts, use the new convenience methods:

```rust
// Pattern 1: Single channel (simplest)
let (result, events) = app.invoke_with_channel(initial_state).await;

// Pattern 2: Multiple sinks
app.invoke_with_sinks(
    initial_state,
    vec![Box::new(StdOutSink::default()), Box::new(ChannelSink::new(tx))]
).await?;
```

**When to use:** Single-execution scenarios, CLI tools, progress monitoring

**Example:** `cargo run --example convenience_streaming`

### Production Pattern (Web Servers)

Use [`App::invoke_streaming`](../../src/app.rs) to launch the workflow and get an `EventStream` you can forward to SSE/WebSocket clients:

```rust
use std::sync::Arc;
use axum::response::sse::{Event as SseEvent, Sse};
use futures_util::StreamExt;
use tokio::{signal, sync::Mutex};
use weavegraph::event_bus::STREAM_END_SCOPE;

let (invocation, events) = app.invoke_streaming(initial_state).await;
let invocation = Arc::new(Mutex::new(Some(invocation)));

let sse_stream = async_stream::stream! {
    let mut stream = events.into_async_stream();
    while let Some(event) = stream.next().await {
        yield Ok::<SseEvent, std::convert::Infallible>(
            SseEvent::default().json_data(event.clone()).unwrap()
        );
        if event.scope_label() == Some(STREAM_END_SCOPE) {
            break;
        }
    }
};

let response = Sse::new(sse_stream);

tokio::spawn({
    let invocation = Arc::clone(&invocation);
    async move {
        tokio::select! {
            _ = async {
                if let Some(handle) = invocation.lock().await.take() {
                    if let Err(err) = handle.join().await {
                        tracing::error!("workflow failed: {err}");
                    }
                }
            } => {}
            _ = signal::ctrl_c() => {
                if let Some(handle) = invocation.lock().await.take() {
                    handle.abort();
                }
            }
        }
    }
});

response
```

**When to use:** SSE/WebSocket transports (or as a base for similar streaming adapters). The stream closes automatically when the sentinel diagnostic with scope `STREAM_END_SCOPE` arrives.

**Example:** `cargo run --example demo7_axum_sse`

---

## ⚠️ Notes on legacy patterns

`App::invoke_with_channel` and `invoke_with_sinks` remain available for scripts that prefer flume channels or multiple sinks. Under the hood they now use the same broadcast hub as `invoke_streaming`.

## Quick Start

Run the self-contained example:

```bash
cargo run --example streaming_events
```

This demonstrates the core pattern without requiring additional dependencies.

## Key Components

### 1. EventStream

`EventStream` represents the broadcast output of the EventBus. Convert it to different consumption styles:

```rust
let (invocation, events) = app.invoke_streaming(initial_state).await;

// Async iterator (SSE/WebSocket)
let mut stream = events.into_async_stream();
while let Some(event) = stream.next().await { /* ... */ }

// Blocking iterator (CLI tools)
for event in events.into_blocking_iter() { /* ... */ }

// Timed polling
if let Some(event) = events.next_timeout(Duration::from_secs(1)).await { /* ... */ }
```

`next_timeout` skips over lag notifications automatically—if the stream logs a warning about dropped events, consider increasing the configured buffer (see below).

### 2. Legacy ChannelSink (Optional)

If you still prefer channel-based forwarding, the convenience helpers continue to work:

```rust
let (result, events) = app.invoke_with_channel(initial_state).await;
```

- The `Event` enum now includes `Node`, `Diagnostic`, **and** `LLM` variants—remember to handle the streaming case (`Event::LLM`).

Every `invoke_streaming` run ends with a diagnostic whose scope equals `STREAM_END_SCOPE`. Use it to notify clients that the workflow has finished and the event stream is about to close.

## Tuning Buffer Capacity

- Default capacity is `1024` events per broadcast channel.
- Increase the buffer with `RuntimeConfig::default().with_event_bus(EventBusConfig::new(capacity, sinks))`.
- Slow consumers trigger a `weavegraph::event_bus` warning (`event stream lagged; dropped events`) and increment `EventStream::dropped()`.
- Benchmark with `cargo bench --bench event_bus_throughput` to validate settings for your workload.

## Web Framework Integration

### Pattern for HTTP Streaming (Axum Example)

```rust
let (invocation, events) = app.invoke_streaming(initial_state).await;

tokio::spawn(async move {
    if let Err(err) = invocation.join().await {
        tracing::error!("workflow failed: {err}");
    }
});

let sse_stream = events
    .into_async_stream()
    .map(|event| SseEvent::default().json_data(event).unwrap());

Sse::new(sse_stream)
```

### Required Dependencies (for Axum)

Add to your `Cargo.toml`:

```toml
[dependencies]
axum = "0.7"
futures-util = "0.3"
# flume is already a dependency of weavegraph
```

## Architecture Flow

```text
┌──────────────┐
│ HTTP Request │
└──────┬───────┘
       │
       ▼
┌──────────────────────────────────────┐
│ HTTP Handler                         │
│ 1. Create mpsc channel               │
│ 2. Create EventBus + ChannelSink     │
│ 3. Spawn workflow task               │
│ 4. Return SSE stream immediately     │
└──────────────────────────────────────┘
       │                           │
       │ spawn                     │ return SSE
       ▼                           ▼
┌──────────────────┐      ┌────────────────┐
│ Background Task  │      │ Client Stream  │
│ ┌──────────────┐ │      │ ┌────────────┐ │
│ │  AppRunner   │ │      │ │ SSE Stream │ │
│ │  + EventBus  │ │      │ │ ← channel  │ │
│ └──────┬───────┘ │      │ └────────────┘ │
│        │         │      └────────────────┘
│        ▼         │
│  ┌──────────┐   │
│  │ Workflow │   │
│  │  Nodes   │   │
│  └────┬─────┘   │
│       │         │
│   ctx.emit()    │
│       │         │
│       ▼         │
│  ┌──────────┐   │
│  │ EventBus │───┼──→ ChannelSink ──→ mpsc ──→ Client
│  └──────────┘   │
└──────────────────┘
```

## Key Patterns

### 1. One ChannelSink Per Client

Each HTTP connection should get its own channel:

```rust
// GOOD: New channel per request
async fn handler() -> Sse<impl Stream> {
    let (tx, rx) = flume::unbounded();  // ✓ Per-request channel
    let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
    // ...
}

// BAD: Shared channel across requests
static SHARED_CHANNEL: OnceCell<Sender<Event>> = OnceCell::new();  // ✗ Don't do this
```

### 2. Background Task for Long-Running Workflows

Spawn the workflow execution so the handler can return immediately:

```rust
// GOOD: Non-blocking handler
tokio::spawn(async move {
    runner.run_until_complete(&session_id).await
});
return Sse::new(stream);  // Returns immediately

// BAD: Blocking handler
runner.run_until_complete(&session_id).await?;  // ✗ Blocks until completion
return Sse::new(stream);
```

### 3. Event Filtering

Filter events by scope or type before sending to clients:

```rust
let stream = UnboundedReceiverStream::new(rx)
    .filter(|event| {
        matches!(event.event_type(), EventType::Node | EventType::Diagnostic)
    })
    .map(|event| Ok(SseEvent::default().json_data(event).unwrap()));
```

## Testing

Test your streaming setup with `curl`:

```bash
# Start your server
cargo run

# In another terminal, stream events
curl -N http://localhost:3000/stream

# You should see SSE events:
# event: workflow-event
# data: {"type":"node","message":"Processing...","scope":"worker","timestamp":"..."}
```

## Further Reading

- **`streaming_events.rs`** - Self-contained example (no web framework)
- **`demo6_agent_mcp.rs`** - Real-world LLM streaming example
- **EventBus source**: `weavegraph/src/event_bus/`
- **AppRunner source**: `weavegraph/src/runtimes/runner.rs`

## Common Issues

### Events Not Appearing in Stream

**Problem**: Workflow runs but no events in channel.

**Solution**: Ensure you're using `AppRunner::with_options_and_bus()` to inject your custom EventBus:

```rust
// ✓ CORRECT: Custom EventBus
let mut runner = AppRunner::with_options_and_bus(app, ..., bus, true).await;

// ✗ WRONG: Default EventBus (events go nowhere)
let mut runner = AppRunner::new(app).await;
```

### Stream Ends Immediately

**Problem**: SSE connection closes right away.

**Solution**: Make sure the workflow task is spawned, not awaited:

```rust
// ✓ CORRECT: Spawned task
tokio::spawn(async move { runner.run_until_complete(&id).await });
return Sse::new(stream);  // Returns immediately, stream stays open

// ✗ WRONG: Awaited task
runner.run_until_complete(&id).await?;
return Sse::new(stream);  // Stream already finished
```

### Missing Events at Start

**Problem**: First few events are dropped.

**Solution**: Create the channel and EventBus *before* starting the workflow:

```rust
// ✓ CORRECT: Channel exists before workflow starts
let (tx, rx) = flume::unbounded();
let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
tokio::spawn(async move { /* run with bus */ });
Sse::new(stream)  // Events captured from the start

// ✗ WRONG: Channel created after workflow starts
tokio::spawn(async move { /* already running */ });
let (tx, rx) = flume::unbounded();  // Too late!
```

---

## Sink Diagnostics: Monitoring Failures

Weavegraph provides **opt-in diagnostics** for monitoring event sink health without disrupting your main event stream. This is useful for production observability and debugging sink-specific issues.

### Quick Start: No Changes Needed

Existing code works unchanged—diagnostics are isolated and optional:

```rust
// ✓ Constructing and using EventBus exactly as before
let bus = EventBus::with_sinks(vec![Box::new(StdOutSink::default())]);
app_runner.run_until_complete(&session).await?;

// ✓ No changes needed for EventStream consumers
let mut events = bus.subscribe();
while let Ok(event) = events.recv().await { /* ... */ }

// ✓ No changes needed for sinks like StdOutSink, MemorySink, or ChannelSink
```

### Opt-In: Subscribe to Diagnostics

To monitor sink failures, subscribe to the diagnostics stream:

```rust
use weavegraph::event_bus::EventBus;

let bus = EventBus::with_sinks(vec![
    Box::new(StdOutSink::default()),
    Box::new(ChannelSink::new(tx)),
]);

// Subscribe to diagnostics (doesn't affect main event stream)
let mut diags = bus.diagnostics();

tokio::spawn(async move {
    while let Ok(diagnostic) = diags.recv().await {
        eprintln!(
            "[{}] Sink '{}' error #{}: {}",
            diagnostic.when.format("%H:%M:%S"),
            diagnostic.sink,
            diagnostic.occurrence,
            diagnostic.error
        );
    }
});

// Main event stream continues independently
let mut events = bus.subscribe();
```

**DiagnosticsStream API** mirrors `EventStream`:
- `recv()` → blocking receive
- `try_recv()` → non-blocking poll (returns `Empty`, `Closed`, or `Ok(diagnostic)`)
- `into_async_stream()` → convert to `futures::Stream`
- `next_timeout(duration)` → receive with timeout

### Health Snapshots

Query aggregated sink health at any time without subscribing:

```rust
// Get current health for all sinks
let health = bus.sink_health();

for entry in health {
    println!(
        "Sink '{}': {} errors, last: {:?}",
        entry.sink,
        entry.error_count,
        entry.last_error.as_deref().unwrap_or("none")
    );
}
```

**Use cases:**
- Health check endpoints in web servers
- Periodic alerting without continuous monitoring
- Post-mortem analysis after workflow completion

### Configuration Options

Control diagnostics behavior via `EventBusConfig`:

```rust
use weavegraph::runtimes::{RuntimeConfig, EventBusConfig, DiagnosticsConfig};

// Disable diagnostics entirely (saves memory)
let config = RuntimeConfig::default()
    .with_event_bus(
        EventBusConfig::with_stdout_only()
            .with_diagnostics(DiagnosticsConfig {
                enabled: false,
                buffer_capacity: None,
                emit_to_events: false,
            })
    );

// Enable diagnostics with custom buffer
let config = RuntimeConfig::default()
    .with_event_bus(
        EventBusConfig::with_stdout_only()
            .with_diagnostics(DiagnosticsConfig {
                enabled: true,
                buffer_capacity: Some(512),  // Default: same as event bus capacity
                emit_to_events: false,
            })
    );

// Emit diagnostics to BOTH the diagnostics stream AND main event stream
// ⚠️ Caution: Only use when sinks cannot create feedback loops
let config = RuntimeConfig::default()
    .with_event_bus(
        EventBusConfig::with_stdout_only()
            .with_diagnostics(DiagnosticsConfig {
                enabled: true,
                buffer_capacity: None,
                emit_to_events: true,  // Also emit Event::Diagnostic to main stream
            })
    );
```

**When to use `emit_to_events: true`:**
- You have a single monitoring sink that won't fail on diagnostic events
- You want diagnostics visible in existing event consumers (logs, metrics, etc.)
- You understand the risk of cascading failures (e.g., a sink that fails on all events will emit diagnostics that trigger more failures)

**Default behavior (`emit_to_events: false`):**
- Diagnostics are isolated to the dedicated diagnostics stream
- Main event stream is unaffected by sink failures
- Safer for most production use cases

### Custom Sink Naming

Override the default sink name for clearer diagnostics:

```rust
use std::borrow::Cow;
use weavegraph::event_bus::EventSink;

struct DatabaseSink { /* ... */ }

impl EventSink for DatabaseSink {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("postgres_events_sink")
    }

    fn handle(&mut self, event: &Event) -> std::io::Result<()> {
        // ... write to database
        Ok(())
    }
}

// Diagnostics will show "postgres_events_sink" instead of generic type name
let bus = EventBus::with_sink(DatabaseSink { /* ... */ });
let health = bus.sink_health();
assert_eq!(health[0].sink, "postgres_events_sink");
```

### Example: Health Monitoring in Axum

```rust
use axum::{Json, routing::get, Router};
use serde_json::json;
use std::sync::Arc;

async fn health_check(bus: Arc<EventBus>) -> Json<serde_json::Value> {
    let health = bus.sink_health();
    let any_errors = health.iter().any(|h| h.error_count > 0);

    Json(json!({
        "status": if any_errors { "degraded" } else { "healthy" },
        "sinks": health.iter().map(|h| json!({
            "name": h.sink,
            "errors": h.error_count,
            "last_error": h.last_error,
            "last_error_at": h.last_error_at,
        })).collect::<Vec<_>>()
    }))
}

let app = Router::new()
    .route("/health", get(health_check))
    .with_state(Arc::new(event_bus));
```

### Troubleshooting

**Q: Diagnostics stream returns `Closed` immediately**

A: Diagnostics may be disabled in config. Check `EventBusConfig::diagnostics.enabled`.

**Q: I'm not seeing diagnostics for sink failures I know are happening**

A: Ensure you're subscribing *before* the workflow starts, or check if the diagnostics buffer is lagging (broadcast receivers drop messages when full).

**Q: Health snapshot shows zero errors but I see tracing logs**

A: Diagnostics tracking is disabled. Set `diagnostics.enabled: true` in config.

**Q: Can I get diagnostics for a specific sink only?**

A: Filter by `diagnostic.sink` name after receiving. The stream contains all sink diagnostics.

