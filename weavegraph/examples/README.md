# Weavegraph Examples

This directory contains examples demonstrating various Weavegraph features and patterns.

## Quick Reference

| Example | Purpose | Key Features |
|---------|---------|--------------|
| `basic_nodes.rs` | Simple node implementation | Node trait, basic graphs |
| `demo1.rs` | Basic workflow | Message passing, state management |
| `demo2.rs` | State channels | Extra data, channel updates |
| `demo3.rs` | Conditional routing | Edge predicates, dynamic graphs |
| `demo4.rs` | Advanced patterns | Complex workflows |
| `demo5_rag.rs` | RAG pipeline | Document processing, retrieval |
| `demo6_agent_mcp.rs` | LLM agent with MCP | Tool calling, streaming responses |
| `streaming_events.rs` | **Event streaming** | **ChannelSink, AppRunner, web integration** |
| `cap_demo.rs` | CAP framework | Structured outputs |
| `advanced_patterns.rs` | Advanced techniques | Multiple patterns |
| `errors_pretty.rs` | Error handling | Error formatting, diagnostics |

## Event Streaming (New!)

### streaming_events.rs - Stream Events to Web Clients

**Purpose:** Demonstrates how to stream workflow events in real-time to web clients, monitoring systems, or async consumers.

**Run it:**
```bash
cargo run --example streaming_events
```

**Key Concepts:**
- ✅ Using `AppRunner::with_options_and_bus()` instead of `App.invoke()`
- ✅ Creating custom `EventBus` with `ChannelSink`
- ✅ Per-request event isolation in web servers
- ✅ SSE/WebSocket integration patterns

**Documentation:**
- See `STREAMING_QUICKSTART.md` for a quick guide
- See API docs: `AppRunner::with_options_and_bus()`
- See API docs: `ChannelSink`

### ⚠️ Important: Event Streaming Pattern

**Don't do this** (it won't work):
```rust
let bus = EventBus::default();
bus.add_sink(ChannelSink::new(tx));
app.invoke(state).await;  // ❌ Creates its own EventBus!
```

**Do this instead**:
```rust
let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
let mut runner = AppRunner::with_options_and_bus(app, ..., bus, true).await;
runner.run_until_complete(&session_id).await;  // ✅ Uses your EventBus
```

**Why?** `App.invoke()` internally creates an `AppRunner` with a default EventBus (stdout only). To use custom event sinks, you must create the `AppRunner` yourself and pass your custom EventBus.

## Getting Started

### 1. Start with Basic Nodes

```bash
cargo run --example basic_nodes
```

Learn the fundamentals of creating nodes and building simple graphs.

### 2. Explore Message Passing

```bash
cargo run --example demo1
```

See how state flows through workflow nodes with message passing.

### 3. Try Conditional Routing

```bash
cargo run --example demo3
```

Learn how to create dynamic workflows that route based on state.

### 4. Stream Events to Clients

```bash
cargo run --example streaming_events
```

Learn how to stream workflow events to web clients in real-time.

### 5. Build an LLM Agent

```bash
cargo run --example demo6_agent_mcp
```

See a complete LLM agent with tool calling and streaming responses.

## Common Patterns

### Simple Workflow Execution

When stdout logging is sufficient:

```rust
let app = GraphBuilder::new()
    .add_node(...)
    .compile()?;

let result = app.invoke(initial_state).await?;
```

### Event Streaming to Web Clients

When you need real-time event streaming:

```rust
// Create channel
let (tx, rx) = flume::unbounded();

// Create EventBus with ChannelSink
let bus = EventBus::with_sinks(vec![
    Box::new(StdOutSink::default()),
    Box::new(ChannelSink::new(tx)),
]);

// Use AppRunner with custom EventBus
let mut runner = AppRunner::with_options_and_bus(
    app,
    CheckpointerType::InMemory,
    false,
    bus,
    true,
).await;

runner.create_session(session_id.clone(), initial_state).await?;
runner.run_until_complete(&session_id).await?;
```

### Per-Request Isolation (Web Server)

```rust
async fn handle_request(app: Arc<App>) -> Result<Stream> {
    // Each request gets its own EventBus and channel
    let (tx, rx) = flume::unbounded();
    let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
    
    let mut runner = AppRunner::with_options_and_bus(
        Arc::try_unwrap(app).unwrap_or_else(|arc| (*arc).clone()),
        CheckpointerType::InMemory,
        false,
        bus,
        true,
    ).await;
    
    // Run workflow - events isolated to this request
    tokio::spawn(async move {
        runner.create_session(session_id, state).await.ok();
        runner.run_until_complete(&session_id).await.ok();
    });
    
    // Return stream immediately
    Ok(rx)
}
```

## Documentation

### In-Code Documentation

Most examples have extensive doc comments explaining the patterns. Read the source!

### API Documentation

Generate and view the full API docs:

```bash
cargo doc --no-deps --open
```

Key modules to explore:
- `weavegraph::app::App` - Workflow execution
- `weavegraph::runtimes::runner::AppRunner` - Runtime with event streaming
- `weavegraph::event_bus` - Event broadcasting system
- `weavegraph::node::Node` - Node trait and execution
- `weavegraph::graphs::GraphBuilder` - Graph construction

### Additional Resources

- `STREAMING_QUICKSTART.md` - Event streaming guide
- `NodePartial_examples.md` - Node output patterns

## Tips

### Running Examples

All examples can be run with:

```bash
cargo run --example <name>
```

### Debugging

Enable verbose logging:

```bash
RUST_LOG=debug cargo run --example <name>
```

### Testing Patterns

Many patterns from examples can be found in the test suite:

```bash
cargo test --test <test_name>
```

## Contributing

When adding new examples:

1. Add a descriptive doc comment at the top
2. Include inline comments explaining non-obvious patterns
3. Update this README with the example
4. Consider adding a section to the quickstart guides

## Need Help?

- Check the API docs: `cargo doc --no-deps --open`
- Read the examples source code
- See the test suite for more patterns
- Review the streaming guides for event handling

Happy coding! 🚀
