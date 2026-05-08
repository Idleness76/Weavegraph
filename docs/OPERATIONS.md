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

Use `App::invoke_streaming` to run a workflow while streaming events to clients. See [STREAMING.md](STREAMING.md) for full details.

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

Weavegraph supports SQLite and PostgreSQL checkpointing, as well as in-memory state for workflows.

### SQLite Checkpointing

Automatic state persistence with configurable database location:

```rust
use weavegraph::runtimes::{AppRunner, CheckpointerType};

// Using the builder pattern (recommended)
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::SQLite)
    .build()
    .await;
```

**SQLite URL resolution order (when `CheckpointerType::SQLite` is selected):**
1. `WEAVEGRAPH_SQLITE_URL` environment variable (full URL)
2. `SQLITE_DB_NAME` environment variable (filename only; used as `sqlite://{name}`)
3. Default: `sqlite://weavegraph.db`

Tip: `RuntimeConfig` also loads `.env` automatically (via `dotenvy`) for local dev.

### PostgreSQL Checkpointing

For production deployments requiring horizontal scaling or shared state:

```rust
use weavegraph::runtimes::{AppRunner, CheckpointerType};

// Using the builder pattern
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::Postgres)
    .build()
    .await;
```

**Database URL resolution order:**
1. `WEAVEGRAPH_POSTGRES_URL` environment variable
2. `DATABASE_URL` environment variable (common convention)
3. Fallback: `postgresql://localhost/weavegraph`

**PostgreSQL vs SQLite:**

| Aspect | SQLite | PostgreSQL |
|--------|--------|------------|
| **Deployment** | Single-file, embedded | Server-based |
| **Concurrency** | Single-writer | Multi-writer |
| **Scaling** | Single instance | Horizontal scaling |
| **Best for** | Development, single-instance | Production, distributed |

**Migrations:** PostgreSQL migrations are in `migrations/postgres/`. Run with:

```bash
# Using sqlx-cli
sqlx migrate run --source migrations/postgres
```

### In-Memory Mode

For testing and ephemeral workflows:

```rust
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory)
    .build()
    .await?;
```

### Iterative Checkpointed Workflows

Use iterative sessions when one logical run should process many inputs while keeping one checkpoint lineage. This is useful for event-driven systems that repeatedly restore the latest durable state, apply the next input, run the graph, and checkpoint the result.

```rust
use weavegraph::node::NodePartial;
use weavegraph::runtimes::{AppRunner, CheckpointerType};
use weavegraph::state::VersionedState;
use weavegraph::types::NodeKind;
use weavegraph::utils::collections::new_extra_map;

# async fn example(app: weavegraph::app::App) -> Result<(), Box<dyn std::error::Error>> {
let mut runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::SQLite)
    .autosave(true)
    .build()
    .await;

let run_id = "market-run-2026-05-08".to_string();
runner
    .create_iterative_session(
        run_id.clone(),
        VersionedState::new_with_user_message("start"),
        NodeKind::Start,
    )
    .await?;

for tick in [1, 2, 3] {
    let mut extra = new_extra_map();
    extra.insert("tick".to_string(), serde_json::json!(tick));

    runner
        .invoke_next(&run_id, NodePartial::new().with_extra(extra), NodeKind::Start)
        .await?;
}
# Ok(())
# }
```

`NodeKind::Start` means the same initial frontier as a normal session: the graph's outgoing edges from the virtual Start node. A registered custom node can be used to resume from a narrower entry point. `NodeKind::End` is rejected because it is terminal.

The runner keeps `SessionState.step` monotonic across invocations. It also clears scheduler version-gating state for each `invoke_next` call, so the entry path runs for each logical input even when two consecutive input patches are identical.

If you subscribe with `AppRunner::event_stream()` before an iterative run, each `invoke_next(...)` emits `INVOCATION_END_SCOPE` and leaves the stream open for the next input. After the final input, call `finish_iterative_session(...)` to emit `STREAM_END_SCOPE` and close the stream for consumers that expect the standard terminal sentinel.

### Typed State Slots

Use `StateKey<T>` when checkpointed `extra` state needs a documented schema and compile-time payload type while staying JSON-compatible across backends.

```rust
use serde::{Deserialize, Serialize};
use weavegraph::node::NodePartial;
use weavegraph::state::{StateKey, StateSnapshot};

#[derive(Serialize, Deserialize)]
struct PortfolioState {
    cash_cents: i64,
}

const PORTFOLIO: StateKey<PortfolioState> = StateKey::new("wq", "portfolio", 1);

fn load(snapshot: &StateSnapshot) -> Result<PortfolioState, weavegraph::state::StateSlotError> {
    snapshot.require_typed(PORTFOLIO)
}

fn store(value: PortfolioState) -> Result<NodePartial, weavegraph::state::StateSlotError> {
    NodePartial::new().with_typed_extra(PORTFOLIO, value)
}
```

The generated storage key is `namespace:name:v{schema_version}`, so old and new schemas can coexist during migrations.

### Deterministic Clock And Run Metadata

Inject a clock when simulations, replay, or tests need logical time to be independent of wall-clock time. The same clock is available from `NodeContext::now_unix_ms()` and is attached to node event metadata when present.

```rust
use std::sync::Arc;
use weavegraph::runtimes::{AppRunner, CheckpointerType};
use weavegraph::utils::clock::MockClock;

let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory)
    .clock(Arc::new(MockClock::new(1_700_000_000)))
    .build()
    .await;

let metadata = runner.run_metadata();
println!("graph={} runtime={} clock={}", metadata.graph_hash, metadata.runtime_config_hash, metadata.clock_mode);
```

`App::graph_metadata()` and `App::graph_definition_hash()` are useful for replay manifests and checkpoint labels. The graph hash covers the graph definition surface, including node kinds, edges, conditional edge registrations, and reducer definition labels. Custom reducers can override `Reducer::definition_label(...)` when a stable domain label is more appropriate than the default Rust type path.

### Replay Conformance Checks

Replay helpers compare normalized events and final state snapshots for uninterrupted/resumed run parity.

```rust
use weavegraph::runtimes::{ReplayRun, compare_replay_runs};

let expected = ReplayRun::new(expected_state, expected_events);
let actual = ReplayRun::new(actual_state, actual_events);

compare_replay_runs(&expected, &actual).assert_matches()?;
```

Use `compare_event_sequences_with(...)` or `compare_replay_runs_with(...)` when domain events need custom normalization before comparison.

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
use weavegraph::runtimes::{AppRunner, CheckpointerType};
use weavegraph::state::VersionedState;

# async fn example(app: weavegraph::app::App) -> Result<(), Box<dyn std::error::Error>> {
let sink = MemorySink::new();
let event_bus = EventBus::with_sink(sink.clone());

let mut runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory)
    .event_bus(event_bus)
    .autosave(false)
    .start_listener(true)
    .build()
    .await;

let session_id = "test-session".to_string();
runner
    .create_session(session_id.clone(), VersionedState::new_with_user_message("Hi"))
    .await?;
runner.run_until_complete(&session_id).await?;

let events = sink.snapshot();
assert!(!events.is_empty());
# Ok(())
# }
```

### Property-Based Testing

Weavegraph uses `proptest` to ensure correctness across edge cases. See the test suite for examples of property-based validation of schedulers, channels, and state management.

## Error Handling {#errors}

Weavegraph provides structured, matchable error enums via `thiserror`.
Rich diagnostic metadata is available behind the optional `diagnostics` feature.

### Basic Usage

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Your workflow code here
    Ok(())
}
```

### Handling Graph Compilation Errors

```rust
use weavegraph::graphs::{GraphBuilder, GraphCompileError};
use weavegraph::types::NodeKind;

fn build_app() -> Result<weavegraph::app::App, weavegraph::graphs::GraphCompileError> {
    match GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
    {
        Ok(app) => Ok(app),
        Err(GraphCompileError::MissingEntry) => Err(GraphCompileError::MissingEntry),
        Err(GraphCompileError::UnknownNode(nk)) => Err(GraphCompileError::UnknownNode(nk)),
        Err(e) => Err(e),
    }
}
```

**Features:**
- Match on error variants for custom handling
- Lightweight core error model for library consumers
- Optional diagnostic metadata (`--features diagnostics`) for richer terminal reporting

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

let mut runner = AppRunner::builder()
    .app(app.clone())
    .checkpointer(CheckpointerType::InMemory)
    .event_bus(bus)
    .autosave(true)
    .start_listener(true)
    .build()
    .await;
```

See also: [Quickstart](QUICKSTART.md), [Architecture](ARCHITECTURE.md)
