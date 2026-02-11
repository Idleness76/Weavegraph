//! Weavegraph node implementations for RAG pipelines.
//!
//! This module provides ready-to-use [`weavegraph::node::Node`] implementations that integrate
//! wg-ragsmith's chunking and embedding capabilities into weavegraph workflows.
//!
//! # Feature Flag
//!
//! This module requires the `weavegraph-nodes` feature:
//!
//! ```toml
//! [dependencies]
//! wg-ragsmith = { version = "0.1", features = ["weavegraph-nodes"] }
//! ```
//!
//! # Available Nodes
//!
//! - [`ChunkingNode`] - Semantic chunking of documents into retrievable segments
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use weavegraph::graphs::GraphBuilder;
//! use weavegraph::types::NodeKind;
//! use wg_ragsmith::nodes::ChunkingNode;
//! use wg_ragsmith::service::ChunkSource;
//!
//! let chunking_node = ChunkingNode::builder()
//!     .service(chunking_service)
//!     .input_key("document_html")
//!     .output_key("chunks")
//!     .build();
//!
//! let builder = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("chunker".into()), chunking_node)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("chunker".into()))
//!     .add_edge(NodeKind::Custom("chunker".into()), NodeKind::End);
//! ```

mod chunking;

pub use chunking::{ChunkingNode, ChunkingNodeBuilder, ChunkingNodeError};
