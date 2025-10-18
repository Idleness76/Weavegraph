# Streaming Events Quickstart

This guide shows you how to stream workflow events to web clients using Weavegraph's `EventStream` helpers and, when needed, the legacy channel-based sinks.

## Choose Your Pattern

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
let initial = VersionedState::new_with_user_message("Stream me");
let (workflow, events) = app.invoke_streaming(initial).await;

tokio::spawn(async move {
    if let Err(err) = workflow.await.and_then(|res| res) {
        tracing::error!("workflow failed: {err}");
    }
});

let sse_stream = events
    .into_async_stream()
    .map(|event| SseEvent::default().json_data(event).unwrap());

Sse::new(sse_stream)
```

**When to use:** SSE, WebSocket, per-client event streams

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
let (workflow, events) = app.invoke_streaming(initial_state).await;

// Async iterator (SSE/WebSocket)
let mut stream = events.into_async_stream();
while let Some(event) = stream.next().await { /* ... */ }

// Blocking iterator (CLI tools)
for event in events.into_blocking_iter() { /* ... */ }

// Timed polling
if let Some(event) = events.next_timeout(Duration::from_secs(1)).await { /* ... */ }
```

### 2. Legacy ChannelSink (Optional)

If you still prefer channel-based forwarding, the convenience helpers continue to work:

```rust
let (result, events) = app.invoke_with_channel(initial_state).await;
```

- The `Event` enum now includes `Node`, `Diagnostic`, **and** `LLM` variants—remember to handle the streaming case (`Event::LLM`).

## Web Framework Integration

### Pattern for HTTP Streaming (Axum Example)

```rust
let (workflow, events) = app.invoke_streaming(initial_state).await;

tokio::spawn(async move {
    if let Err(err) = workflow.await.and_then(|res| res) {
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
