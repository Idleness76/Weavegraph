//! Semantic chunking primitives for JSON and HTML sources.

pub mod assembly;
pub mod breakpoints;
pub mod cache;
pub mod config;
pub mod embeddings;
pub mod html;
pub mod json;
pub mod segmenter;
pub mod service;
pub mod tokenizer;
pub mod types;

use async_trait::async_trait;

pub use config::{
    BreakpointStrategy, ChunkingConfig, EmbeddingBackend, HtmlPreprocessConfig,
    JsonPreprocessConfig, MetadataFlags, OverlapConfig, SemanticChunkingModuleConfig,
};
pub use embeddings::{
    EmbeddingProvider, MockEmbeddingProvider, NullEmbeddingProvider, RigEmbeddingProvider,
    SharedEmbeddingProvider,
};
pub use segmenter::SentenceSplitter;
pub use service::{
    ChunkDocumentRequest, ChunkDocumentResponse, ChunkSource, ChunkTelemetry, EmbedderKind,
    SemanticChunkingService, SemanticChunkingServiceBuilder,
};
pub use types::{ChunkingError, ChunkingOutcome, SemanticChunk};

/// Implemented by concrete semantic chunkers.
#[async_trait]
pub trait SemanticChunker {
    type Source;

    async fn chunk(
        &self,
        source: Self::Source,
        cfg: &config::ChunkingConfig,
    ) -> Result<types::ChunkingOutcome, types::ChunkingError>;

    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Estimate tokens using the configured tokenizer helper.
pub fn estimate_tokens(text: &str) -> usize {
    tokenizer::count(text)
}

#[cfg(test)]
mod tests;
