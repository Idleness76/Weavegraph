# Streaming Events Quickstart

This guide shows you how to stream workflow events to web clients using Weavegraph's EventBus and ChannelSink.

## Choose Your Pattern

### ⭐ NEW: Simple Patterns (Convenience Methods)

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

For web servers with per-request isolation, use `AppRunner` directly:

```rust
let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
let mut runner = AppRunner::with_options_and_bus(app, ..., bus, true).await;
runner.run_until_complete(&session_id).await;
```

**When to use:** SSE, WebSocket, per-client event streams

**Example:** `cargo run --example streaming_events`

**This guide focuses on the production pattern.** For simple cases, see `convenience_streaming.rs`.

---

## ⚠️ Important: Why You Need AppRunner

**You cannot stream events using `App.invoke()` alone!**

```rust
// ❌ WRONG - This will NOT stream events to your channel!
let bus = EventBus::default();
bus.add_sink(ChannelSink::new(tx));
app.invoke(state).await;  // Creates its OWN EventBus internally!

// ✅ CORRECT - Use AppRunner with custom EventBus
let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
let mut runner = AppRunner::with_options_and_bus(app, ..., bus, true).await;
runner.run_until_complete(&session_id).await;
```

**Why?**
- `App.invoke()` internally creates `AppRunner::new()` which creates a **default EventBus** (stdout only)
- Your custom EventBus is ignored
- Use `AppRunner::with_options_and_bus()` to inject your custom EventBus

## Quick Start

Run the self-contained example:

```bash
cargo run --example streaming_events
```

This demonstrates the core pattern without requiring additional dependencies.

## Key Components

### 1. ChannelSink

Forwards events from the EventBus to a flume channel:

```rust
use weavegraph::event_bus::ChannelSink;

let (tx, rx) = flume::unbounded();
let channel_sink = ChannelSink::new(tx);
```

### 2. EventBus with Multiple Sinks

Create an EventBus that broadcasts to multiple destinations:

```rust
use weavegraph::event_bus::{EventBus, StdOutSink};

let bus = EventBus::with_sinks(vec![
    Box::new(StdOutSink::default()),  // For debugging
    Box::new(ChannelSink::new(tx)),   // For streaming
]);
```

### 3. AppRunner Integration

Pass the custom EventBus to the AppRunner:

```rust
use weavegraph::runtimes::{runner::AppRunner, CheckpointerType};

let mut runner = AppRunner::with_options_and_bus(
    app,                          // Your compiled graph
    CheckpointerType::InMemory,   // Checkpointing strategy
    false,                        // Autosave disabled
    bus,                          // Custom EventBus
    true,                         // Start event listener
).await;

// Create and run session
let session_id = "my-session".to_string();
runner.create_session(session_id.clone(), initial_state).await?;
runner.run_until_complete(&session_id).await?;
```

### 4. Consuming Events

Process events from the channel:

```rust
while let Ok(event) = rx.recv_async().await {
    println!("Event: {}", serde_json::to_string_pretty(&event)?);
}
```

## Web Framework Integration

### Pattern for HTTP Streaming (Axum Example)

```rust
use axum::{
    extract::State,
    response::sse::{Event as SseEvent, Sse},
};
use futures_util::stream::Stream;

async fn stream_workflow(
    State(graph): State<Arc<App>>
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    // 1. Create channel for this client
    let (tx, rx) = flume::unbounded();
    
    // 2. Create EventBus with ChannelSink
    let bus = EventBus::with_sinks(vec![
        Box::new(ChannelSink::new(tx))
    ]);
    
    // 3. Run workflow in background with custom EventBus
    tokio::spawn(async move {
        let mut runner = AppRunner::with_options_and_bus(
            Arc::try_unwrap(graph).unwrap_or_else(|arc| (*arc).clone()),
            CheckpointerType::InMemory,
            false,
            bus,
            true,
        ).await;
        
        let session_id = format!("client-{}", uuid::Uuid::new_v4());
        let initial_state = VersionedState::new_with_user_message("Process this");
        
        runner.create_session(session_id.clone(), initial_state).await.ok();
        runner.run_until_complete(&session_id).await.ok();
    });
    
    // 4. Stream events as SSE (flume has built-in stream support)
    let stream = rx.into_stream().map(|event| {
        Ok(SseEvent::default()
            .event("workflow-event")
            .json_data(event)
            .unwrap())
    });
    
    Sse::new(stream)
}
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
