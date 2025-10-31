//! Graph definition and compilation for workflow execution.
//!
//! This module provides the core graph building functionality for creating
//! workflow graphs with nodes, edges, and conditional routing. The main
//! entry point is [`GraphBuilder`], which uses a builder pattern to
//! construct workflows that compile into executable [`App`](crate::app::App) instances.
//!
//! # Core Concepts
//!
//! - **Nodes**: Executable units of work implementing the [`Node`](crate::node::Node) trait
//! - **Edges**: Connections between nodes defining execution flow
//! - **Conditional Edges**: Dynamic routing based on state predicates
//! - **Virtual Endpoints**: `NodeKind::Start` and `NodeKind::End` for structural definition
//! - **Compilation**: Validation and conversion to executable [`App`](crate::app::App)
//!
//! # Quick Start
//!
//! ```
//! use weavegraph::graphs::GraphBuilder;
//! use weavegraph::types::NodeKind;
//! use weavegraph::node::{Node, NodeContext, NodePartial, NodeError};
//! use weavegraph::state::StateSnapshot;
//! use async_trait::async_trait;
//!
//! // Define a simple node
//! struct MyNode;
//!
//! #[async_trait]
//! impl Node for MyNode {
//!     async fn run(&self, _: StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
//!         Ok(NodePartial::default())
//!     }
//! }
//!
//! // Build a simple workflow (virtual Start/End):
//! // Start (virtual) -> process -> End (virtual)
//! let app = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("process".into()), MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
//!     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
//!     .compile();
//! ```
//!
//! # Advanced Usage
//!
//! ## Conditional Routing
//!
//! ```
//! use weavegraph::graphs::{GraphBuilder, EdgePredicate};
//! use weavegraph::types::NodeKind;
//! use std::sync::Arc;
//!
//! // Create a predicate that routes based on message count
//! let route_by_messages: EdgePredicate = Arc::new(|snapshot| {
//!     if snapshot.messages.len() > 5 {
//!         vec!["process".to_string()]
//!     } else {
//!         vec!["skip".to_string()]
//!     }
//! });
//!
//! # struct MyNode;
//! # #[async_trait::async_trait]
//! # impl weavegraph::node::Node for MyNode {
//! #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
//! #         Ok(weavegraph::node::NodePartial::default())
//! #     }
//! # }
//!
//! let app = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("process".into()), MyNode)
//!     .add_node(NodeKind::Custom("skip".into()), MyNode)
//!     // Basic structural edge from virtual Start
//!     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
//!     .add_conditional_edge(NodeKind::Start, route_by_messages)
//!     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
//!     .add_edge(NodeKind::Custom("skip".into()), NodeKind::End)
//!     .compile();
//! ```

// Internal module declarations
mod builder;
mod compilation;
mod edges;

// Public re-exports for backward compatibility
pub use builder::GraphBuilder;
pub use edges::{ConditionalEdge, EdgePredicate};
