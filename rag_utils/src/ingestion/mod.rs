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

pub use cache::{fetch_html, DocumentCache, FetchOutcome};
pub use chunk::{chunk_response_to_ingestion, ChunkBatch, ChunkDocumentIngestion};
pub use resume::ResumeTracker;
