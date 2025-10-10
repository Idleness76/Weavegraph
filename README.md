# Weavegraph

[![Crates.io](https://img.shields.io/crates/v/weavegraph.svg)](https://crates.io/crates/weavegraph)
[![Documentation](https://docs.rs/weavegraph/badge.svg)](https://docs.rs/weavegraph)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![CI/CD](https://github.com/Idleness76/weavegraph/workflows/CI/CD/badge.svg)](https://github.com/Idleness76/weavegraph/actions)
[![Security Audit](https://github.com/Idleness76/weavegraph/workflows/Security%20Audit/badge.svg)](https://github.com/Idleness76/weavegraph/actions)

**Graph-driven, concurrent agent workflow framework with versioned state, deterministic barrier merges, and rich diagnostics.**

Weavegraph is a modern Rust framework for building complex, stateful workflows using graph-based execution. It's designed for AI agents, data processing pipelines, and any application requiring sophisticated state management with concurrent node execution.

## ✨ Key Features

- **🔄 Concurrent Graph Execution**: Bounded-concurrency scheduler with dependency resolution and versioned barrier synchronization
- **📝 Rich Message System**: Type-safe message construction with role-based messaging for AI workflows
- **🎯 Versioned State Management**: Channel-based state with snapshot isolation and deterministic merges
- **🚨 Comprehensive Error Handling**: Structured error propagation with beautiful diagnostics via `miette` and `thiserror`
- **📊 Built-in Observability**: Rich tracing spans and event streaming for monitoring and debugging
- **💾 Flexible Persistence**: SQLite checkpointing with automatic schema management, plus in-memory options
- **🎭 Conditional Workflows**: Dynamic routing and edge conditions based on runtime state
- **🔧 Developer Experience**: Extensive examples, comprehensive docs, and ergonomic APIs

## 🚀 Quick Start

Add Weavegraph to your `Cargo.toml`:

```toml
[dependencies]
weavegraph = "0.1"
```

### Basic Example

```rust
use weavegraph::{
    graph::GraphBuilder,
    message::Message,
    node::{Node, NodeContext, NodePartial},
    state::VersionedState,
};
use async_trait::async_trait;

// Define a simple greeting node
struct GreetingNode;

#[async_trait]
impl Node for GreetingNode {
    async fn run(
        &self,
        _snapshot: weavegraph::state::StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, weavegraph::node::NodeError> {
        ctx.emit("greeting", "Generating welcome message")?;

        let greeting = Message::assistant("Hello! How can I help you today?");

        Ok(NodePartial {
            messages: Some(vec![greeting]),
            extra: None,
            errors: None,
        })
    }
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    // Build a simple graph
    let graph = GraphBuilder::new()
        .add_node("greet", GreetingNode)
        .set_entry_point("greet")
        .build()?;

    // Create initial state and run
    let state = VersionedState::new_with_user_message("Hello, system!");
    let result = graph.invoke(state).await?;

    // Access results
    for message in result.messages {
        println!("{}: {}", message.role, message.content);
    }

    Ok(())
}
```

## 📋 Core Concepts

### Messages

Messages are the primary communication primitive with convenient constructors:

```rust
use weavegraph::message::Message;

// Use convenience constructors (recommended)
let user_msg = Message::user("What's the weather like?");
let assistant_msg = Message::assistant("It's sunny and 75°F!");
let system_msg = Message::system("You are a helpful assistant.");

// For custom roles
let function_msg = Message::new("function", "Processing complete");

// Complex cases with builder pattern
let complex_msg = Message::builder()
    .role("custom_agent")
    .content("Task completed successfully")
    .build();
```

### State Management

Versioned state with channel isolation and snapshot consistency:

```rust
use weavegraph::state::VersionedState;

// Simple initialization
let state = VersionedState::new_with_user_message("Hello!");

// Rich initialization with builder
let state = VersionedState::builder()
    .with_user_message("What's the weather?")
    .with_system_message("You are a weather assistant")
    .with_extra("location", serde_json::json!("San Francisco"))
    .build();
```

### Graph Building

Declarative workflow definition with conditional routing:

```rust
use weavegraph::graph::GraphBuilder;

let graph = GraphBuilder::new()
    .add_node("input", InputProcessorNode)
    .add_node("analyze", AnalyzerNode)
    .add_node("respond", ResponseNode)
    .add_edge("input", "analyze")
    .add_conditional_edge("analyze", |state| {
        if state.extra.contains_key("needs_escalation") {
            "escalate"
        } else {
            "respond"
        }
    })
    .set_entry_point("input")
    .build()?;
```

## 🔧 Examples

The repository includes comprehensive examples demonstrating different patterns:

```bash
# Basic node patterns and message handling
cargo run --example basic_nodes

# Advanced patterns: error handling, conditional routing, data transformation
cargo run --example advanced_patterns

# Error handling and pretty diagnostics
cargo run --example errors_pretty
```

### Demo Applications

Historical demo applications showcase evolution of capabilities:

```bash
# Basic graph execution patterns (examples/demo1.rs)
cargo run --example demo1

# Direct scheduler usage and barrier synchronization (examples/demo2.rs)
cargo run --example demo2

# LLM workflows with Ollama integration (examples/demo3.rs)
cargo run --example demo3

# Advanced multi-step workflows (examples/demo4.rs)
cargo run --example demo4
```

**Note**: Demo3 requires Ollama running at `http://localhost:11434` with models like `gemma3`. Use the provided `docker-compose.yml` to set up Ollama:

```bash
docker-compose up -d ollama
```

## 🏗️ Architecture

Weavegraph is built around several core modules:

- **[`message`]** - Type-safe message construction and role-based messaging
- **[`state`]** - Versioned state management with channel isolation
- **[`node`]** - Node execution primitives and async trait definitions
- **[`graph`]** - Workflow graph definition and conditional routing
- **[`schedulers`]** - Concurrent execution with dependency resolution
- **[`runtimes`]** - High-level execution runtime and checkpointing
- **[`channels`]** - Channel-based state storage and versioning
- **[`reducers`]** - State merge strategies and conflict resolution
- **[`event_bus`]** - Event streaming and observability infrastructure

## 🔍 Observability & Debugging

### Tracing

Rich tracing integration with configurable log levels:

```bash
# Debug level for weavegraph modules
RUST_LOG=debug cargo run --example basic_nodes

# Error level globally, debug for weavegraph
RUST_LOG=error,weavegraph=debug cargo run --example advanced_patterns
```

### Event Streaming

Built-in event bus for monitoring workflow execution:

```rust
use weavegraph::event_bus::{EventBus, MemorySink};

// Custom event handling for testing
let event_bus = EventBus::with_sink(MemorySink::new());
let runner = AppRunner::with_bus(graph, event_bus);

// Events include node starts/completions, state changes, errors
```

### Error Diagnostics

Beautiful error reporting with context and suggestions:

```rust
// Automatic error context and pretty printing
fn main() -> miette::Result<()> {
    // Your workflow code here
    Ok(())
}
```

## 💾 Persistence

### SQLite Checkpointing

Automatic state persistence with configurable database location:

```rust
use weavegraph::runtimes::SqliteCheckpointer;

let checkpointer = SqliteCheckpointer::new("sqlite://workflow.db").await?;
let runner = AppRunner::with_checkpointer(graph, checkpointer);
```

Database URL resolution order:
1. `WEAVEGRAPH_SQLITE_URL` environment variable
2. Explicit URL in code
3. `SQLITE_DB_NAME` environment variable (filename only)
4. Default: `sqlite://weavegraph.db`

### In-Memory Mode

For testing and ephemeral workflows:

```rust
let runner = AppRunner::new(graph); // Uses in-memory state
```

## 🧪 Testing

Run the comprehensive test suite:

```bash
# All tests with output
cargo test --all -- --nocapture

# Specific test categories
cargo test schedulers:: -- --nocapture
cargo test channels:: -- --nocapture
cargo test integration:: -- --nocapture
```

Property-based testing with `proptest` ensures correctness across edge cases.


Overview mermain flowchart of the app

```mermaid

flowchart TB

subgraph Client
  user[Client App or UI]
end

subgraph Build
  gb[GraphBuilder]
end

subgraph Graph
  cg[CompiledGraph]
end

subgraph Runtime
  app[App]
  sched[Scheduler]
  router[Router Edges and Commands]
  barrier[Barrier Applier]
end

subgraph Nodes
  usernode[User Nodes]
  llmnode[LLM Node]
  toolnode[Tool Node]
end

subgraph State
  vstate[Versioned State]
  snap[State Snapshot]
end

subgraph Reducers
  redreg[Reducer Registry]
end

subgraph Checkpoint
  cpif[Checkpointer]
end

subgraph Rig
  rigad[Rig Adapter]
  llmprov[LLM Provider]
end

subgraph Tools
  toolreg[Tool Registry]
  exttools[External Tools]
end

subgraph Stream
  stream[Stream Controller]
end

subgraph Viz
  viz[Visualizer]
end


user --> gb
gb --> cg

user --> app
cg --> app

app --> sched
sched --> snap
vstate --> snap

sched --> usernode
sched --> llmnode
sched --> toolnode

usernode --> barrier
llmnode --> barrier
toolnode --> barrier
redreg --> barrier
barrier --> vstate

snap --> router
app --> router
router --> sched

llmnode --> rigad
rigad --> llmprov
llmprov --> rigad
rigad --> llmnode

toolnode --> toolreg
toolnode --> exttools
exttools --> toolnode

barrier --> cpif

app --> stream
stream --> user

cg --> viz

```

## 🚀 Production Considerations

### Performance

- Bounded concurrency prevents resource exhaustion
- Snapshot isolation eliminates state races
- Channel-based architecture enables efficient partial updates
- SQLite checkpointing supports failure recovery

### Monitoring

- Structured event streaming for observability platforms
- Rich tracing spans for distributed tracing
- Error aggregation and pretty diagnostics
- Custom event sinks for metrics collection

### Deployment

- Docker-ready with provided compose configuration
- Environment-based configuration
- Graceful shutdown handling
- Migration support for schema evolution

## 🎓 Project Background

Weavegraph originated as a capstone project for a Rust online course, developed by contributors with Python backgrounds and experience with LangGraph and LangChain. The goal was to bring similar graph-based workflow capabilities to Rust while leveraging its performance, safety, and concurrency advantages.

While rooted in educational exploration, Weavegraph has evolved into a production-ready framework with continued active development well beyond the classroom setting.

## 🤝 Contributing

We welcome contributions! Please see our [contributing guidelines](CONTRIBUTING.md) for details.

Areas we're particularly interested in:
- Additional persistence backends (PostgreSQL, Redis)
- Enhanced AI/LLM integration patterns
- Performance optimizations
- Documentation improvements
- Example applications

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🔗 Links

- [Documentation](https://docs.rs/weavegraph)
- [Crates.io](https://crates.io/crates/weavegraph)
- [Repository](https://github.com/Idleness76/weavegraph)
- [Issues](https://github.com/Idleness76/weavegraph/issues)
