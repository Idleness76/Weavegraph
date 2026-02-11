//! ```text
//! Source Discovery ─┬─► ingestion::fetch_html ──► DocumentCache
//!                   └─► ingestion::resume      ──┐
//!                                                │
//! Cached HTML ──► semantic_chunking::service ──► ChunkBatch
//!                                    │
//!                                    ├─► embeddings / segmenter helpers
//!                                    └─► cache & breakpoint strategies
//!
//! ChunkBatch ──► ingestion::chunk_response_to_ingestion ──► stores::sqlite::SqliteChunkStore
//!             └─► downstream VectorStore implementations (future adapters)
//!
//! Stored vectors ──► query utilities & RAG applications
//! ```
//!
//! # Weavegraph Integration
//!
//! Enable the `weavegraph-nodes` feature for ready-to-use workflow nodes:
//!
//! ```toml
//! [dependencies]
//! wg-ragsmith = { version = "0.1", features = ["weavegraph-nodes"] }
//! ```
//!
//! See the [`nodes`] module for available node implementations.

pub mod ingestion;
pub mod semantic_chunking;
pub mod stores;
pub mod types;

// Weavegraph integration nodes (feature-gated)
#[cfg(feature = "weavegraph-nodes")]
pub mod nodes;

pub use semantic_chunking::assembly;
pub use semantic_chunking::breakpoints;
pub use semantic_chunking::cache;
pub use semantic_chunking::config;
pub use semantic_chunking::embeddings;
pub use semantic_chunking::segmenter;
pub use semantic_chunking::service;
pub use semantic_chunking::tokenizer;
pub use semantic_chunking::types as chunk_types;

// Re-export stores abstraction
pub use stores::{Backend, ChunkRecord};
