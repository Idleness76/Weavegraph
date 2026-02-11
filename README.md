# Weavegraph

[![Crates.io](https://img.shields.io/crates/v/weavegraph.svg)](https://crates.io/crates/weavegraph)
[![Documentation](https://docs.rs/weavegraph/badge.svg)](https://docs.rs/weavegraph)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Graph-driven, concurrent agent workflow framework for Rust.**

---

> **EARLY BETA**  
> This framework is in active development (targeting v0.2.x). APIs are evolving rapidly, and **breaking changes may happen** between minor versions.  
> The core architecture is solid, but expect rough edges, API churn, and occasional surprises. Pin exact versions if stability matters.  
> Use in production at your own risk—or better yet, help us shape the future by reporting issues and suggesting improvements.

---

Weavegraph lets you build robust, concurrent, stateful workflows using a graph-based execution model. Ideal for AI agents, data pipelines, and any application needing versioned state and rich diagnostics.

## Features

- Concurrent graph execution with dependency resolution
- Type-safe, role-based message system
- Versioned state with snapshot isolation
- Structured error handling and diagnostics
- Built-in event streaming and observability
- Flexible persistence: SQLite or in-memory
- Conditional routing and dynamic edges
- Ergonomic APIs and comprehensive examples

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
weavegraph = "0.2"
```

## Documentation

- **[Developer Guide](docs/GUIDE.md)** - Messages, state, and graph building
- **[Operations Guide](docs/OPERATIONS.md)** - Event streaming, persistence, testing, and production
- **[Architecture](docs/ARCHITECTURE.md)** - Core architecture and custom reducers
- **[Documentation Index](docs/INDEX.md)** - Complete topic reference with anchor links
- **[Examples](weavegraph/examples/)** - Runnable code for all patterns

## Minimal Example

```rust
use weavegraph::{
        graphs::GraphBuilder,
        message::Message,
        node::{Node, NodeContext, NodePartial},
        state::VersionedState,
};
use async_trait::async_trait;

struct HelloNode;

#[async_trait]
impl Node for HelloNode {
        async fn run(
                &self,
                _snapshot: weavegraph::state::StateSnapshot,
                _ctx: NodeContext,
        ) -> Result<NodePartial, weavegraph::node::NodeError> {
                Ok(NodePartial::new().with_messages(vec![Message::assistant("Hello, world!")]))
        }
}

#[tokio::main]
async fn main() -> miette::Result<()> {
        use weavegraph::types::NodeKind;
        let app = GraphBuilder::new()
                .add_node(NodeKind::Custom("hello".into()), HelloNode)
                .add_edge(NodeKind::Start, NodeKind::Custom("hello".into()))
                .add_edge(NodeKind::Custom("hello".into()), NodeKind::End)
                .compile()?;
        let state = VersionedState::new_with_user_message("Hi!");
        let result = app.invoke(state).await?;
        for message in result.messages.snapshot() {
                println!("{}: {}", message.role_type(), message.content);
        }
        Ok(())
}
```
> NOTE: `NodeKind::Start` and `NodeKind::End` are virtual structural endpoints.  
> You never register them with `add_node`; attempts to do so are ignored with a warning.


## 🧪 Testing

For testing and ephemeral workflows use the InMemory checkpointer:

```rust
// After compiling the graph into an `App`:
let runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
```

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




## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).

## 🔗 Links

- [Documentation](https://docs.rs/weavegraph)
- [Crates.io](https://crates.io/crates/weavegraph)
- [Repository](https://github.com/Idleness76/weavegraph)
- [Issues](https://github.com/Idleness76/weavegraph/issues)
