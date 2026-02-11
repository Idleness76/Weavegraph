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
//! # Graph Iteration
//!
//! The module provides petgraph-style iterators for inspecting graph structure:
//!
//! ```
//! use weavegraph::graphs::GraphBuilder;
//! use weavegraph::types::NodeKind;
//!
//! # struct MyNode;
//! # #[async_trait::async_trait]
//! # impl weavegraph::node::Node for MyNode {
//! #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
//! #         Ok(weavegraph::node::NodePartial::default())
//! #     }
//! # }
//!
//! let builder = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("A".into()), MyNode)
//!     .add_node(NodeKind::Custom("B".into()), MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
//!     .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
//!     .add_edge(NodeKind::Custom("B".into()), NodeKind::End);
//!
//! // Iterate over registered nodes
//! for node in builder.nodes() {
//!     println!("Node: {:?}", node);
//! }
//!
//! // Iterate over edges as (from, to) pairs
//! for (from, to) in builder.edges() {
//!     println!("Edge: {:?} -> {:?}", from, to);
//! }
//!
//! // Get deterministic topological ordering
//! let sorted = builder.topological_sort();
//! ```
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
//!
//! ## petgraph Integration
//!
//! With the `petgraph-compat` feature, you can convert graphs to petgraph format
//! for advanced algorithms and DOT visualization:
//!
//! ```ignore
//! // Enable with: weavegraph = { features = ["petgraph-compat"] }
//! use weavegraph::graphs::GraphBuilder;
//!
//! let builder = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("A".into()), MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
//!     .add_edge(NodeKind::Custom("A".into()), NodeKind::End);
//!
//! // Convert to petgraph for analysis
//! let pg = builder.to_petgraph();
//! assert!(!petgraph::algo::is_cyclic_directed(&pg.graph));
//!
//! // Export to DOT for visualization
//! let dot = builder.to_dot();
//! std::fs::write("workflow.dot", dot)?;
//! ```

// Internal module declarations
mod builder;
mod compilation;
mod edges;
mod iteration;

#[cfg(feature = "petgraph-compat")]
mod petgraph_compat;

// Public re-exports for backward compatibility
pub use builder::GraphBuilder;
pub use compilation::GraphCompileError;
pub use edges::{ConditionalEdge, EdgePredicate};
pub use iteration::{EdgesIter, NodesIter};

#[cfg(feature = "petgraph-compat")]
pub use petgraph_compat::{NodeIndexMap, PetgraphConversion, WeaveDiGraph, is_cyclic};
