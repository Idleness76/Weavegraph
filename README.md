# Weavegraph

[![Crates.io](https://img.shields.io/crates/v/weavegraph.svg)](https://crates.io/crates/weavegraph)
[![Documentation](https://docs.rs/weavegraph/badge.svg)](https://docs.rs/weavegraph)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Graph-driven, concurrent agent workflow framework for Rust.**

---

> **Pre-1.0 Status**  
> Weavegraph core APIs are stable and production-ready. However, as we approach 1.0, minor versions may introduce targeted refinements to APIs or behaviors. See [MIGRATION.md](docs/MIGRATION.md) for upgrade guidance between releases.  
> Your feedback shapes the future—please report issues and suggest improvements as you use the framework.

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
weavegraph = "0.3"
```

> **Note:** Examples and instructions in this README are current as of 0.3.x. For upgrading from 0.2.x, see [MIGRATION.md](docs/MIGRATION.md).

## Dependency Compatibility

Weavegraph targets **Rust 1.90+** (MSRV). The following table shows compatibility with key dependencies:

| Rust Version | Tokio | Serde | SQLx |
|--------------|-------|-------|------|
| 1.90 (MSRV)  | 1.40+ | 1.0+  | 0.8+ |
| Stable       | 1.40+ | 1.0+  | 0.8+ |
| Nightly      | 1.40+ | 1.0+  | 0.8+ |

**Optional dependencies:** SQLx 0.8 (postgres/sqlite features), miette 7.x (diagnostics feature), rig-core 0.30 (rig feature).

See [Cargo.toml](Cargo.toml) for complete dependency versions and feature configuration.

## Documentation

- **[Quickstart](docs/QUICKSTART.md)** - Fast path to building and running workflows
- **[Operations Guide](docs/OPERATIONS.md)** - Event streaming, persistence, testing, and production
- **[Architecture](docs/ARCHITECTURE.md)** - Core architecture and custom reducers
- **[Documentation Index](docs/INDEX.md)** - Complete topic reference with anchor links
- **[Examples](examples/)** - Runnable code for all patterns

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
        use weavegraph::types::NodeKind;
        let app = GraphBuilder::new()
                .add_node(NodeKind::Custom("hello".into()), HelloNode)
                .add_edge(NodeKind::Start, NodeKind::Custom("hello".into()))
                .add_edge(NodeKind::Custom("hello".into()), NodeKind::End)
                .compile()?;
        let state = VersionedState::new_with_user_message("Hi!");
        let result = app.invoke(state).await?;
        for message in result.messages.snapshot() {
                println!("{}: {}", message.role, message.content);
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

## CI Parity

To minimize local/CI drift, this repository pins Rust with `rust-toolchain.toml` to `1.90.0` and runs required CI checks on that version.

Before opening a PR, run:

```bash
./scripts/ci-quick.sh
```

Before merging or cutting a release, run full local parity checks:

```bash
./scripts/ci-local.sh
```

`ci-local.sh` intentionally fails if required tools are missing (`cargo-semver-checks`, `cargo-deny`) so a local pass is a meaningful signal for CI.

## Resources

- **[Migration Guide](docs/MIGRATION.md)** - Upgrade paths between releases (0.2.x → 0.3.x and beyond)
- **[Architecture Guide](docs/ARCHITECTURE.md)** - Deep dive into core design and internals
- **[Examples Directory](examples/)** - Runnable patterns: graph execution, scheduling, streaming, persistence, and more

## Related Crates

- **[wg-ragsmith](https://github.com/Idleness76/wg-ragsmith)** - Semantic chunking and RAG utilities for Weavegraph nodes

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).

## 🔗 Links

- [Documentation](https://docs.rs/weavegraph)
- [Crates.io](https://crates.io/crates/weavegraph)
- [Repository](https://github.com/Idleness76/weavegraph)
- [Issues](https://github.com/Idleness76/weavegraph/issues)
