//! Storage backends for vector embeddings and chunk documents.
//!
//! This module provides a unified [`Backend`] trait that abstracts over different
//! storage implementations, allowing code to work with any supported backend
//! without being tied to a specific database.
//!
//! # Architecture (Vector Storage for RAG)
//!
//! ```text
//!                     ┌─────────────────┐
//!                     │  Backend Trait  │
//!                     │  (async CRUD)   │
//!                     └────────┬────────┘
//!                              │
//!           ┌──────────────────┼──────────────────┐
//!           │                  │                  │
//!           ▼                  ▼                  ▼
//!    ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
//!    │   SQLite    │   │  (future)   │   │  (future)   │
//!    │ sqlite-vec  │   │  pgvector   │   │   Redis     │
//!    └─────────────┘   └─────────────┘   └─────────────┘
//! ```
//!
//! Note: This is separate from Weavegraph's workflow checkpointing backends
//! (`SQLiteCheckpointer`, `PostgresCheckpointer`) which handle state persistence.
//!
//! # Usage
//!
//! The [`Backend`] trait provides a common interface for storing and retrieving
//! chunk documents with their embeddings:
//!
//! ```rust,ignore
//! use wg_ragsmith::stores::{Backend, ChunkRecord};
//!
//! async fn store_chunks<B: Backend>(backend: &B, chunks: Vec<ChunkRecord>) {
//!     backend.insert_chunks(chunks).await.expect("failed to store");
//! }
//! ```
//!
//! # Supported Backends
//!
//! - [`sqlite::SqliteChunkStore`] - SQLite with vector search via `sqlite-vec`
//!
//! # Note on Weavegraph Persistence
//!
//! For workflow state checkpointing, Weavegraph itself provides both SQLite and
//! Postgres backends via `sqlx` (see `weavegraph::runtimes::SQLiteCheckpointer`
//! and `weavegraph::runtimes::PostgresCheckpointer`). This module focuses on
//! **vector storage for RAG** rather than workflow state persistence.

pub mod sqlite;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::future::Future;

use crate::types::RagError;

// Re-exports for convenience
pub use sqlite::{ChunkDocument, SqliteChunkStore};

/// A record representing a chunk with its embedding, ready for storage.
///
/// This is a backend-agnostic representation that can be converted to/from
/// the specific document types used by each backend implementation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkRecord {
    /// Unique identifier for this chunk
    pub id: String,
    /// Source URL or path
    pub url: String,
    /// Heading context for the chunk
    pub heading: String,
    /// Zero-based index of this chunk within the source
    pub chunk_index: usize,
    /// The actual text content
    pub content: String,
    /// Additional metadata as JSON
    pub metadata: serde_json::Value,
    /// The embedding vector (if computed)
    pub embedding: Option<Vec<f32>>,
}

impl ChunkRecord {
    /// Create a new chunk record.
    pub fn new(
        id: impl Into<String>,
        url: impl Into<String>,
        heading: impl Into<String>,
        chunk_index: usize,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
            heading: heading.into(),
            chunk_index,
            content: content.into(),
            metadata: serde_json::Value::Object(Default::default()),
            embedding: None,
        }
    }

    /// Set additional metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set the embedding vector.
    #[must_use]
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }
}

impl From<ChunkRecord> for ChunkDocument {
    fn from(record: ChunkRecord) -> Self {
        ChunkDocument {
            id: record.id,
            url: record.url,
            heading: record.heading,
            chunk_index: record.chunk_index,
            content: record.content,
            metadata: record.metadata,
        }
    }
}

impl From<ChunkDocument> for ChunkRecord {
    fn from(doc: ChunkDocument) -> Self {
        ChunkRecord {
            id: doc.id,
            url: doc.url,
            heading: doc.heading,
            chunk_index: doc.chunk_index,
            content: doc.content,
            metadata: doc.metadata,
            embedding: None,
        }
    }
}

/// Unified trait for chunk storage backends.
///
/// This trait provides a database-agnostic interface for storing and retrieving
/// chunk documents with their embeddings. Implementations handle the details
/// of each specific storage system.
///
/// # Examples
///
/// Using the trait with any backend:
///
/// ```rust,ignore
/// use wg_ragsmith::stores::Backend;
///
/// async fn example<B: Backend>(backend: &B) -> Result<(), wg_ragsmith::types::RagError> {
///     // Insert chunks
///     let chunks = vec![
///         ChunkRecord::new("id1", "http://example.com", "Title", 0, "Content here")
///             .with_embedding(vec![0.1, 0.2, 0.3]),
///     ];
///     backend.insert_chunks(chunks).await?;
///
///     // Query by URL
///     let found = backend.get_chunks_by_url("http://example.com").await?;
///     println!("Found {} chunks", found.len());
///
///     Ok(())
/// }
/// ```
///
/// # Implementors
///
/// - [`SqliteChunkStore`] - SQLite backend with vector search
#[async_trait]
pub trait Backend: Send + Sync {
    /// Insert multiple chunk records into the store.
    ///
    /// Records with embeddings will be stored with their vectors for similarity search.
    /// Records without embeddings will be stored but may not be searchable via vector queries.
    async fn insert_chunks(&self, chunks: Vec<ChunkRecord>) -> Result<(), RagError>;

    /// Retrieve all chunks associated with a given URL/source.
    async fn get_chunks_by_url(&self, url: &str) -> Result<Vec<ChunkRecord>, RagError>;

    /// Retrieve a specific chunk by its ID.
    async fn get_chunk_by_id(&self, id: &str) -> Result<Option<ChunkRecord>, RagError>;

    /// Delete all chunks associated with a given URL/source.
    async fn delete_chunks_by_url(&self, url: &str) -> Result<usize, RagError>;

    /// Perform a similarity search using a query embedding.
    ///
    /// Returns chunks ordered by similarity (most similar first), limited to `top_k` results.
    async fn search_similar(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(ChunkRecord, f32)>, RagError>;

    /// Return the total number of chunks in the store.
    async fn count(&self) -> Result<usize, RagError>;
}

/// Marker trait for backends that support transactional operations.
///
/// Backends implementing this trait guarantee that operations within
/// a transaction are atomic.
pub trait TransactionalBackend: Backend {
    /// The transaction handle type.
    type Transaction<'a>: Send
    where
        Self: 'a;

    /// Begin a new transaction.
    fn begin_transaction(
        &self,
    ) -> impl Future<Output = Result<Self::Transaction<'_>, RagError>> + Send;

    /// Commit a transaction.
    fn commit_transaction(
        &self,
        tx: Self::Transaction<'_>,
    ) -> impl Future<Output = Result<(), RagError>> + Send;

    /// Rollback a transaction.
    fn rollback_transaction(
        &self,
        tx: Self::Transaction<'_>,
    ) -> impl Future<Output = Result<(), RagError>> + Send;
}

// Future backend implementations:
// - Postgres vector store (pgvector) - would complement weavegraph's PostgresCheckpointer
// - Redis vector store - for high-throughput caching scenarios
