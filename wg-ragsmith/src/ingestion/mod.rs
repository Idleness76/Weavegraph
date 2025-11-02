//! Ingestion utilities for turning external documents into chunked datasets.
//!
//! The helpers in this module provide three core capabilities:
//!
//! * [`cache`] — disk-backed caching for downloaded documents.
//! * [`resume`] — state tracking to support resumable ingestion jobs.
//! * [`chunk`] — conversion utilities that transform chunking output into
//!   vector-store ready batches.

pub mod cache;
pub mod chunk;
pub mod resume;

pub use cache::{DocumentCache, FetchOutcome, fetch_html};
pub use chunk::{ChunkBatch, ChunkDocumentIngestion, chunk_response_to_ingestion};
pub use resume::ResumeTracker;
