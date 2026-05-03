#![cfg_attr(docsrs, feature(doc_cfg))]

//! ```text
//! GraphBuilder -> App::compile -> AppRunner
//!                    |              |
//!                    |              +-> Scheduler -> Nodes -> NodePartial
//!                    |                               |
//!                    |                               +-> Reducers -> VersionedState
//!                    |                               +-> EventBus (diagnostics / LLM)
//!                    |
//!                    +-> RuntimeConfig (persistence, sinks, execution knobs)
//! ```
//!
//! Weavegraph is a graph-driven workflow framework for concurrent, stateful execution.
//! You define nodes and edges with [`graphs::GraphBuilder`], compile to an [`app::App`],
//! and run with either high-level invocation helpers or the lower-level [`runtimes::AppRunner`].
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use async_trait::async_trait;
//! use weavegraph::graphs::GraphBuilder;
//! use weavegraph::message::Message;
//! use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
//! use weavegraph::state::{StateSnapshot, VersionedState};
//! use weavegraph::types::NodeKind;
//!
//! struct EchoNode;
//!
//! #[async_trait]
//! impl Node for EchoNode {
//!     async fn run(
//!         &self,
//!         snapshot: StateSnapshot,
//!         _ctx: NodeContext,
//!     ) -> Result<NodePartial, NodeError> {
//!         let reply = snapshot
//!             .messages
//!             .last()
//!             .map(|m| format!("Echo: {}", m.content))
//!             .unwrap_or_else(|| "Echo: (no input)".to_string());
//!
//!         Ok(NodePartial::new().with_messages(vec![Message::assistant(&reply)]))
//!     }
//! }
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let app = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("echo".into()), EchoNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("echo".into()))
//!     .add_edge(NodeKind::Custom("echo".into()), NodeKind::End)
//!     .compile()?;
//!
//! let initial = VersionedState::new_with_user_message("hello");
//! let final_state = app.invoke(initial).await?;
//! assert!(!final_state.snapshot().messages.is_empty());
//! # Ok(())
//! # }
//! ```
//!
//! # Feature Flags
//!
//! | Feature | Default | Purpose |
//! | ------- | ------- | ------- |
//! | `sqlite-migrations` | yes | Enables SQLite persistence support via `sqlx` and migration wiring. |
//! | `sqlite` | no | Enables SQLite checkpointer APIs and runtime backend. |
//! | `postgres-migrations` | no | Enables Postgres migration support for checkpointer setup. |
//! | `postgres` | no | Enables PostgreSQL checkpointer APIs and runtime backend. |
//! | `rig` | no | Enables Rig-based LLM interop and adapters. |
//! | `diagnostics` | no | Adds `miette` diagnostic metadata to error types. |
//! | `examples` | no | Pulls additional deps used by selected examples. |
//! | `petgraph-compat` | no | Exposes petgraph conversion helpers for graph analysis and visualization. |
//!
//! # Documentation
//!
//! - `docs/QUICKSTART.md` for API-first onboarding and composition patterns.
//! - `docs/OPERATIONS.md` for runtime operations, persistence, and deployment concerns.
//! - `docs/STREAMING.md` for event streaming patterns and production guidance.
//! - `docs/ARCHITECTURE.md` for internal architecture and execution model details.
//!
//! # Common Patterns
//!
//! ## Graph lifecycle
//!
//! ```rust,no_run
//! # use weavegraph::graphs::GraphBuilder;
//! # use weavegraph::types::NodeKind;
//! # use weavegraph::state::VersionedState;
//! # use weavegraph::runtimes::RuntimeConfig;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // 1. Build — declare nodes and edges.
//! // 2. Compile — validate topology, attach runtime config.
//! // 3. Invoke — run once or stream events to clients.
//! let app = GraphBuilder::new()
//!     /* .add_node(...).add_edge(...) */
//!     .compile()?;
//!
//! let state = VersionedState::new_with_user_message("hello");
//! let final_state = app.invoke(state).await?;
//! # Ok(())
//! # }
//! ```
//!
//! See `examples/graph_execution.rs` for a runnable graph lifecycle example.
//!
//! ## Streaming events via SSE
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use weavegraph::app::App;
//! # use weavegraph::event_bus::STREAM_END_SCOPE;
//! # use weavegraph::state::VersionedState;
//! # async fn example(app: Arc<App>) {
//! // Each call gets an isolated runner + event bus.
//! let state = VersionedState::new_with_user_message("hello");
//! let (handle, event_stream) = app.invoke_streaming(state).await;
//!
//! // Convert to an async stream and forward to your SSE layer.
//! // Terminate when STREAM_END_SCOPE is observed.
//! let _ = event_stream.into_async_stream(); // futures::Stream<Item = Event>
//! let _ = handle; // join or abort the background task
//! # }
//! ```
//!
//! See `examples/production_streaming.rs` for the full Axum + Postgres reference.
//!
//! ## Error handling in nodes
//!
//! ```rust,no_run
//! # use weavegraph::node::{NodeError, NodeResultExt};
//! // Return a domain error from any node:
//! fn validate(input: &str) -> Result<(), NodeError> {
//!     if input.is_empty() {
//!         return Err(NodeError::Other("input must not be empty".into()));
//!     }
//!     // Lift arbitrary std::error::Error with ?:
//!     std::str::from_utf8(input.as_bytes()).node_err()?;
//!     Ok(())
//! }
//! ```
//!
//! See `examples/errors_pretty.rs` for error display patterns.

#![warn(missing_docs)]

pub mod app;
pub mod channels;
pub mod control;
pub mod event_bus;
pub mod graphs;
pub mod llm;
pub mod message;
pub mod node;
pub mod reducers;
pub mod runtimes;
pub mod schedulers;
pub mod state;
pub mod telemetry;
pub mod types;
pub mod utils;

pub use control::{FrontierCommand, NodeRoute};
