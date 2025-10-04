//! Helpers for turning chunking results into vector-store documents.

use url::Url;

use crate::semantic_chunking::types::ChunkingOutcome;
use crate::semantic_chunking::ChunkTelemetry;
use crate::stores::sqlite::ChunkDocument;
use crate::types::RagError;

/// Collection of chunk documents paired with embeddings ready for persistence.
#[derive(Debug, Clone)]
pub struct ChunkBatch {
    documents: Vec<(ChunkDocument, Vec<f32>)>,
    skipped_chunks: usize,
}

impl ChunkBatch {
    /// Number of documents that will be persisted.
    pub fn chunk_count(&self) -> usize {
        self.documents.len()
    }

    /// Number of chunks skipped because they were missing embeddings.
    pub fn skipped_chunks(&self) -> usize {
        self.skipped_chunks
    }

    /// Returns `true` when the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Provides read-only access to the stored documents.
    pub fn documents(&self) -> &[(ChunkDocument, Vec<f32>)] {
        &self.documents
    }

    /// Consumes the batch and yields the underlying documents.
    pub fn into_documents(self) -> Vec<(ChunkDocument, Vec<f32>)> {
        self.documents
    }
}

/// Container holding the raw chunking outcome, telemetry, and persistence batch.
#[derive(Debug, Clone)]
pub struct ChunkDocumentIngestion {
    pub outcome: ChunkingOutcome,
    pub telemetry: ChunkTelemetry,
    pub batch: ChunkBatch,
}

impl ChunkDocumentIngestion {
    /// Number of chunks that produced embeddings and will be persisted.
    pub fn chunk_count(&self) -> usize {
        self.batch.chunk_count()
    }

    /// Number of chunks skipped because they lacked embeddings.
    pub fn skipped_chunks(&self) -> usize {
        self.batch.skipped_chunks()
    }

    /// Borrow the documents slated for insertion into the vector store.
    pub fn documents(&self) -> &[(ChunkDocument, Vec<f32>)] {
        self.batch.documents()
    }

    /// Consumes the ingestion result and returns its components.
    pub fn into_parts(self) -> (ChunkBatch, ChunkingOutcome, ChunkTelemetry) {
        (self.batch, self.outcome, self.telemetry)
    }
}

/// Converts a `ChunkDocumentResponse` into a persistence-friendly batch.
pub fn chunk_response_to_ingestion(
    url: &Url,
    response: crate::semantic_chunking::service::ChunkDocumentResponse,
) -> Result<ChunkDocumentIngestion, RagError> {
    let crate::semantic_chunking::service::ChunkDocumentResponse { outcome, telemetry } = response;
    let batch = outcome_to_batch(url, &outcome)?;
    Ok(ChunkDocumentIngestion {
        outcome,
        telemetry,
        batch,
    })
}

/// Builds a [`ChunkBatch`] from raw chunking output.
pub fn outcome_to_batch(url: &Url, outcome: &ChunkingOutcome) -> Result<ChunkBatch, RagError> {
    let mut documents = Vec::new();
    let mut skipped = 0usize;

    for (idx, chunk) in outcome.chunks.iter().enumerate() {
        let Some(embedding) = chunk.embedding.as_ref() else {
            skipped += 1;
            continue;
        };

        let heading = if chunk.metadata.heading_hierarchy.is_empty() {
            String::new()
        } else {
            chunk.metadata.heading_hierarchy.join(" > ")
        };

        let metadata = serde_json::to_value(&chunk.metadata)
            .map_err(|err| RagError::Chunking(err.to_string()))?;

        let document = ChunkDocument {
            id: chunk.id.to_string(),
            url: url.to_string(),
            heading,
            chunk_index: idx,
            content: chunk.content.clone(),
            metadata,
        };

        documents.push((document, embedding.clone()));
    }

    Ok(ChunkBatch {
        documents,
        skipped_chunks: skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_chunking::types::{ChunkMetadata, ChunkingStats, SemanticChunk};

    #[test]
    fn chunk_response_conversion_skips_missing_embeddings() {
        let url = Url::parse("https://example.com/doc").unwrap();

        let mut chunk_a = SemanticChunk::new(
            "Chunk A".to_string(),
            12,
            ChunkMetadata {
                heading_hierarchy: vec!["Heading".to_string()],
                ..Default::default()
            },
        );
        chunk_a.embedding = Some(vec![0.1, 0.2]);

        let chunk_b = SemanticChunk::new("Chunk B".to_string(), 8, ChunkMetadata::default());

        let outcome = ChunkingOutcome {
            chunks: vec![chunk_a.clone(), chunk_b],
            trace: None,
            stats: ChunkingStats {
                total_segments: 2,
                total_chunks: 2,
                average_tokens: 10.0,
            },
        };

        let response = crate::semantic_chunking::service::ChunkDocumentResponse {
            outcome: outcome.clone(),
            telemetry: ChunkTelemetry {
                embedder: "mock".into(),
                source: "html".into(),
                duration_ms: 10,
                fallback_used: false,
                cache_hits: 0,
                cache_misses: 1,
                smoothing_window: None,
                strategy: "percentile".into(),
                chunk_count: 2,
                average_tokens: 10.0,
            },
        };

        let ingestion = chunk_response_to_ingestion(&url, response).unwrap();
        assert_eq!(ingestion.chunk_count(), 1);
        assert_eq!(ingestion.skipped_chunks(), 1);
        let (batch, _, _) = ingestion.into_parts();
        assert_eq!(batch.chunk_count(), 1);
        let (doc, embedding) = batch.documents().first().unwrap();
        assert_eq!(doc.url, url.to_string());
        assert_eq!(doc.heading, "Heading");
        assert_eq!(embedding, chunk_a.embedding.as_ref().unwrap());
    }
}
