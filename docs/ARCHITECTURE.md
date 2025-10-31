# Architecture Overview

The `graft` workspace houses two Rust crates that work together to deliver graph-driven
workflow orchestration and retrieval-augmented generation (RAG) tooling:

| Crate | Purpose | Highlights |
| ----- | ------- | ---------- |
| `weavegraph` | Executes concurrent, stateful graphs with structured observability. | Graph builder + runtime, event bus, checkpointing, reducers, scheduler. |
| `wg-ragsmith` | Provides ingestion, semantic chunking, and storage utilities for RAG workloads. | HTML/JSON parsers, semantic chunkers, SQLite vector store helpers. |

The workspace-level `Cargo.toml` ties both crates together, while shared tooling (`Makefile`,
GitHub Actions workflows) enforces consistent governance across the repository.

---

## Workspace Topology

```
docs/                     → Architectural plans, production hardening roadmap.
weavegraph/               → Core orchestration crate (library + examples + tests).
wg-ragsmith/              → RAG utilities crate (library + examples + tests).
data/                     → Local development databases (ignored in version control).
external/                 → Vendor snapshots (RAGatouille, raptor) kept outside the workspace.
.github/workflows/        → Continuous integration pipelines.
Makefile                  → Developer/CI task runner (fmt, clippy, test, doc, deny, machete, migrations).
ARCHITECTURE.md           → This document.
```

The workspace targets Rust 1.89 as the minimum supported version and enables 2021 edition
features across both crates.

---

## `weavegraph` Crate

`weavegraph` implements the runtime that powers concurrent, graph-based workflows. The library
is organised around a handful of core modules:

| Module | Highlights |
| ------ | ---------- |
| `graphs::{builder, edges, compilation}` | `GraphBuilder` DSL for wiring nodes, unconditional and conditional edges, and compiling into a runnable `App`. |
| `app` | High-level façade that owns compiled nodes/edges, reducer registry, and runtime config. Provides `invoke`, `invoke_streaming`, and event stream APIs. |
| `runtimes::{runner, checkpointer_*, runtime_config}` | `AppRunner` drives supersteps, coordinates the scheduler, applies barriers, and persists to SQLite (via `sqlx::migrate!`). |
| `schedulers` | Dependency-aware scheduler that fans out runnable nodes and enforces bounded concurrency. |
| `node` | `Node` trait, `NodeContext`, `NodePartial`, and error types used by application code. |
| `state`, `channels`, `reducers` | Versioned state model split across message/extra/error channels with deterministic merge reducers. |
| `event_bus` | Broadcast-based event hub with sinks (stdout, memory, channel) and streaming helpers for web servers or CLIs. |
| `telemetry`, `utils` | Tracing helpers, deterministic RNG, clocks, ID generators, and collection utilities. |

### Authoring Nodes & State

Weavegraph applications revolve around three building blocks:

```rust
use weavegraph::{
    graphs::GraphBuilder,
    message::Message,
    node::{Node, NodeContext, NodePartial},
    state::VersionedState,
    types::NodeKind,
};
use async_trait::async_trait;

struct GreetingNode;

#[async_trait]
impl Node for GreetingNode {
    async fn run(
        &self,
        _snapshot: weavegraph::state::StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, weavegraph::node::NodeError> {
        ctx.emit("greeting", "Saying hi!")?;
        Ok(NodePartial::new().with_messages(vec![Message::assistant("Hello!")]))
    }
}

let app = GraphBuilder::new()
    .add_node(NodeKind::Custom("greet".into()), GreetingNode)
    .add_edge(NodeKind::Start, NodeKind::Custom("greet".into()))
    .add_edge(NodeKind::Custom("greet".into()), NodeKind::End)
    .compile()?;

let initial = VersionedState::new_with_user_message("Hi?");
let result = app.invoke(initial).await?;
```

Key practices:

- Prefer the convenience constructors on `Message` (`Message::user`, `Message::assistant`, etc.) or the builder helpers when crafting payloads.
- Build state with `VersionedState::new_with_user_message` or the builder pattern (`VersionedState::builder()...build()`).
- Use `NodeContext::emit*` helpers for telemetry instead of writing directly to stdout.
- Return structured errors (`NodeError::MissingInput`, `NodeError::Provider`) or populate `NodePartial::with_errors` for recoverable issues.

### Execution Flow

1. **Authoring** – Build a graph with `GraphBuilder`, registering nodes (implementations of `Node`)
   and the edges that connect them. Conditional edges can inspect `StateSnapshot` at runtime.
2. **Compilation** – `GraphBuilder::compile()` validates topology and produces an `App`.
3. **Invocation** – `App::invoke()` (or streaming variants) constructs an `AppRunner` with the
   chosen `RuntimeConfig`, checkpointer (`InMemory` or SQLite), and event bus sinks.
4. **Scheduling** – The scheduler selects runnable nodes, issues `NodeContext`s, and executes
   nodes concurrently. Each node returns a `NodePartial` with channel deltas and optional
   control-flow directives.
5. **Barrier & Reduction** – Reducers merge channel updates deterministically, update the
   versioned state, and hand control back to the scheduler for the next superstep.
6. **Persistence & Observability** – Checkpointer snapshots state into SQLite (when enabled),
   the event bus broadcasts diagnostics / LLM chunk streams, and telemetry surfaces to sinks.

### Optional Features

* `llm` – Enables Rig-based LLM support (Ollama/MCP integrations).
* `sqlite-migrations` – Turns on SQLite-backed persistence (default).
* `examples` – Pulls in `wg-ragsmith`, `reqwest`, and `scraper` for richer demos.

### Tests & Examples

* `weavegraph/tests/` – Covers state channels, reducers, scheduler semantics, checkpointer, and event bus.
* `weavegraph/examples/` – Progressive walkthroughs:
  * `basic_nodes.rs`, `demo1.rs`, `demo2.rs` show core messaging and state channels.
  * `demo3.rs`, `demo4.rs`, `demo6_agent_mcp.rs` integrate LLM providers (Ollama/MCP),
    leveraging the `llm` feature.
  * `streaming_events.rs`, `convenience_streaming.rs`, `demo7_axum_sse.rs` demonstrate the
    broadcast event bus and web-friendly streaming patterns.
  * `demo5_rag.rs` ties into `wg-ragsmith` to orchestrate a RAG pipeline end-to-end.

---

## `wg-ragsmith` Crate

`wg-ragsmith` contains the ingestion and vector-store tooling used by RAG pipelines. It can be
used standalone or pulled into `weavegraph` via the `examples` feature.

| Module | Highlights |
| ------ | ---------- |
| `ingestion::{cache, chunk, resume}` | Disk-backed document cache, chunk-to-ingestion conversion, and resumable pipeline tracking. |
| `semantic_chunking::{html, json, segmenter, embeddings, service}` | HTML/JSON preprocessors, statistical breakpoint strategies, mock/real embedding providers, and the async chunking service. |
| `stores::sqlite` | `SqliteChunkStore` built on `rig-sqlite` + `sqlite-vec`, including schema, vec3 registration, and helper methods to upsert/search chunks. |
| `types` | `RagError` and supporting data structures for ingestion/persistence. |

### Examples

* `examples/rust_book_pipeline.rs` – Async ingestion pipeline that scrapes the Rust book,
  chunks and embeds sections, and writes them into SQLite.
* `examples/query_chunks.rs` & `query_db.sh` – Smoke tests showing how to query stored chunks.

These examples share environment variables with the weavegraph RAG demo (see `.env.example`).

### Feature Flags

* `semantic-chunking-tiktoken` (default) – OpenAI tiktoken tokeniser.
* `semantic-chunking-rust-bert` – Enables Rust-BERT based embedding pipeline.
* `semantic-chunking-segtok` – Alternative segmentation strategy.

---

## Shared Operational Pieces

* **Tooling** – The top-level `Makefile` standardises `cargo fmt`, `cargo clippy`,
  `cargo test`, `cargo doc`, `cargo deny`, `cargo machete`, and `sqlx` migrations so that
  local developers and CI run identical commands.
* **CI/CD** – `.github/workflows/ci.yml` runs the Makefile/`cargo` commands across three
  toolchains (`1.89.0`, current stable, nightly) and per workspace member to guard API evolution.
* **Migrations** – `weavegraph/migrations` houses the `sqlx` migration set for the SQLite
  checkpointer. The Makefile’s `migrate*` targets wrap `sqlx` CLI calls.
* **Docs** – `docs/` captures forward-looking design documents (event bus refactor,
  control-flow commands, hybrid RAG pipeline) and the production readiness plan. Use
  this architecture document as the entry point.

---

## From Examples to Production

1. **Local exploration** – Start with `cargo run --example basic_nodes` to learn the node API.
2. **Observability** – Switch to `convenience_streaming.rs` or `streaming_events.rs` to wire
   custom sinks or web streams.
3. **Persistence** – Enable SQLite checkpointing by setting `WEAVEGRAPH_SQLITE_URL`
   (defaults provided in `.env.example`) and run `make migrate` to initialise the database.
4. **RAG integration** – Flip on the `examples` feature and execute `demo5_rag.rs` or the
   `wg-ragsmith` pipelines to ingest data. Migrate to Qdrant by replacing the SQLite store
   with future adapters described in `docs/hybrid_rag_pipeline_plan.md`.
5. **Hardening** – Follow `docs/production_readiness_plan.md` for remaining governance,
   API audits, and release engineering tasks.

By keeping orchestration (`weavegraph`) and content pipelines (`wg-ragsmith`) modular, the
workspace supports both lightweight agent workflows and full production RAG systems using
the same building blocks.
