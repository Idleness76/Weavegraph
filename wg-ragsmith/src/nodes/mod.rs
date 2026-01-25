//! Weavegraph node implementations for RAG pipelines.
//!
//! This module provides ready-to-use [`Node`] implementations that integrate
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
//! use weavegraph::app::GraphBuilder;
//! use wg_ragsmith::nodes::ChunkingNode;
//! use wg_ragsmith::service::ChunkSource;
//!
//! let chunking_node = ChunkingNode::builder()
//!     .service(chunking_service)
//!     .input_key("document_html")
//!     .output_key("chunks")
//!     .build();
//!
//! let mut builder = GraphBuilder::new();
//! builder.add_node("chunker", chunking_node);
//! builder.add_edge("Start", "chunker");
//! builder.add_edge("chunker", "End");
//! ```

mod chunking;

pub use chunking::{ChunkingNode, ChunkingNodeBuilder, ChunkingNodeError};
