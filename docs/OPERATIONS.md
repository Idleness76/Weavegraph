# Operations Guide

Runtime operations, observability, persistence, testing, and production deployment.

## Event Streaming & Observability {#event-streaming}

Weavegraph provides multiple patterns for streaming workflow events with JSON serialization support.

### Event Sinks

Built-in sinks for different use cases:
- **StdOutSink**: Human-readable console output
- **MemorySink**: In-memory capture for testing
- **ChannelSink**: Async streaming to channels
- **JsonLinesSink**: Machine-readable JSON Lines format

Events can be serialized to JSON using `event.to_json_value()`, `event.to_json_string()`, or `event.to_json_pretty()`.

### Simple Pattern (CLI Tools)

```rust
let (result, events) = app.invoke_with_channel(initial_state).await;

// Collect events while processing
tokio::spawn(async move {
    while let Ok(event) = events.recv_async().await {
        println!("Event: {:?}", event);
    }
});
```

### Multiple Sinks

```rust
use weavegraph::event_bus::{StdOutSink, ChannelSink};

app.invoke_with_sinks(
    initial_state,
    vec![
        Box::new(StdOutSink::default()),
        Box::new(ChannelSink::new(tx)),
    ]
).await?;
```

### Web Servers (SSE/WebSockets)

Use `App::invoke_streaming` to run a workflow while streaming events to clients. See `examples/demo7_axum_sse.rs` and [STREAMING_QUICKSTART.md](../weavegraph/examples/STREAMING_QUICKSTART.md) for full details.

### Sink Diagnostics

Monitor event sink health and failures:

```rust
use weavegraph::event_bus::EventBus;

let bus = EventBus::with_sinks(vec![/* your sinks */]);

// Subscribe to diagnostics (optional)
let mut diags = bus.diagnostics();
tokio::spawn(async move {
    while let Ok(diagnostic) = diags.recv().await {
        eprintln!("Sink '{}' error: {}", diagnostic.sink, diagnostic.error);
    }
});

// Query health snapshot
let health = bus.sink_health();
for entry in health {
    println!("{}: {} errors", entry.sink, entry.error_count);
}
```

### Tracing

Rich tracing integration with configurable log levels:

```bash
# Debug level for weavegraph modules
RUST_LOG=debug cargo run --example basic_nodes

# Error level globally, debug for weavegraph
RUST_LOG=error,weavegraph=debug cargo run --example advanced_patterns
```

## Persistence {#persistence}

Weavegraph supports SQLite checkpointing and in-memory state for workflows.

### SQLite Checkpointing

Automatic state persistence with configurable database location:

```rust
use weavegraph::runtimes::SQLiteCheckpointer;

let checkpointer = SQLiteCheckpointer::new("sqlite://workflow.db").await?;
let runner = AppRunner::new(app, CheckpointerType::SQLite).await;
```

**Database URL resolution order:**
1. `WEAVEGRAPH_SQLITE_URL` environment variable
2. Explicit URL in code
3. `SQLITE_DB_NAME` environment variable (filename only)
4. Default: `sqlite://weavegraph.db`

### In-Memory Mode

For testing and ephemeral workflows:

```rust
let runner = AppRunner::new(app, CheckpointerType::InMemory).await;
```

### Storage Management

**InMemoryCheckpointer** stores only the latest checkpoint per session (automatic retention). No storage management needed.

**SQLiteCheckpointer** stores complete step history for audit trails and debugging. Storage grows with `(sessions × steps_per_session × state_size)`.

For long-running applications, implement periodic cleanup:

**Option 1: Direct SQL maintenance (recommended)**

```bash
# Delete checkpoints older than 30 days
sqlite3 workflow.db "DELETE FROM steps WHERE created_at < datetime('now', '-30 days')"

# Keep only latest 100 steps per session
sqlite3 workflow.db "
  DELETE FROM steps 
  WHERE step NOT IN (
    SELECT step FROM steps 
    WHERE session_id = steps.session_id 
    ORDER BY step DESC 
    LIMIT 100
  )
"

# Vacuum to reclaim space
sqlite3 workflow.db "VACUUM"
```

**Option 2: Application-level session management**

Delete entire sessions when workflows complete:

```rust
// Using sqlx directly
sqlx::query("DELETE FROM sessions WHERE id = ?")
    .bind(&session_id)
    .execute(&pool)
    .await?;
// Cascading delete removes all associated steps
```

**Storage monitoring:**

```bash
# Check database size
ls -lh workflow.db

# Count checkpoints per session
sqlite3 workflow.db "
  SELECT session_id, COUNT(*) as checkpoint_count 
  FROM steps 
  GROUP BY session_id
"
```

## Testing {#testing}

Weavegraph supports comprehensive testing, including property-based tests and event capture.

### Running Tests

```bash
# All tests with output
cargo test --all -- --nocapture

# Specific test categories
cargo test schedulers:: -- --nocapture
cargo test channels:: -- --nocapture
cargo test integration:: -- --nocapture
```

### Event Capture in Tests

Use `MemorySink` for synchronous event capture:

```rust
use weavegraph::event_bus::{EventBus, MemorySink};

let sink = MemorySink::new();
let event_bus = EventBus::with_sink(sink.clone());
let runner = AppRunner::with_bus(graph, event_bus);

// After execution
let events = sink.snapshot();
assert_eq!(events.len(), 5);
```

### Property-Based Testing

Weavegraph uses `proptest` to ensure correctness across edge cases. See the test suite for examples of property-based validation of schedulers, channels, and state management.

## Error Handling {#errors}

Weavegraph provides structured error propagation and beautiful diagnostics via `miette` and `thiserror`.

### Basic Usage

```rust
fn main() -> miette::Result<()> {
    // Your workflow code here
    Ok(())
}
```

### Handling Graph Compilation Errors

```rust
use weavegraph::graphs::{GraphBuilder, GraphCompileError};
use weavegraph::types::NodeKind;

fn build_app() -> Result<weavegraph::app::App, miette::Report> {
    match GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
    {
        Ok(app) => Ok(app),
        Err(GraphCompileError::MissingEntry) => {
            Err(miette::miette!("graph has no Start entry"))
        }
        Err(GraphCompileError::UnknownNode(nk)) => {
            Err(miette::miette!("unknown node referenced: {nk}"))
        }
        Err(e) => Err(miette::miette!("graph validation failed: {e}")),
    }
}
```

**Features:**
- Automatic error context and pretty printing
- Match on error variants for custom handling
- Rich diagnostic output with source code context

See `examples/errors_pretty.rs` for comprehensive error handling patterns.

## Production Considerations {#production}

### Performance

- **Bounded concurrency** prevents resource exhaustion
- **Snapshot isolation** eliminates state races
- **Channel-based architecture** enables efficient partial updates
- **SQLite checkpointing** supports failure recovery

### Monitoring

- **Structured event streaming** for observability platforms
- **Rich tracing spans** for distributed tracing
- **Error aggregation** and pretty diagnostics
- **Custom event sinks** for metrics collection

### Deployment

- **Docker-ready** with provided `docker-compose.yml`
- **Environment-based configuration** for flexible deployment
- **Graceful shutdown handling** for clean termination
- **Migration support** for schema evolution

### Production Patterns

For web servers with per-request isolation:

```rust
use weavegraph::event_bus::{EventBus, ChannelSink};
use weavegraph::runtimes::{AppRunner, CheckpointerType};

// Per-request EventBus with isolated channel
let (tx, rx) = flume::unbounded();
let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);

let mut runner = AppRunner::with_options_and_bus(
    app.clone(),
    CheckpointerType::InMemory,
    true,  // autosave
    bus,
    true   // start event listener
).await;
```

See also: [Developer Guide](GUIDE.md), [Architecture](ARCHITECTURE.md)
