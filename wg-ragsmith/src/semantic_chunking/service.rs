use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use rig::embeddings::{embedding::EmbeddingModelDyn, EmbeddingModel};
use serde_json::Value;
use tokio::fs;
use tracing::{field, info_span};

use super::cache::{CacheHandle, CacheMetrics};
use super::config::{
    BreakpointStrategy, ChunkingConfig, HtmlPreprocessConfig, JsonPreprocessConfig,
    SemanticChunkingModuleConfig,
};
use super::embeddings::{NullEmbeddingProvider, RigEmbeddingProvider, SharedEmbeddingProvider};
use super::html::HtmlSemanticChunker;
use super::json::JsonSemanticChunker;
use super::types::{ChunkingError, ChunkingOutcome};
use super::SemanticChunker;

pub struct SemanticChunkingService {
    defaults: SemanticChunkingModuleConfig,
    base_embedder: Option<EmbedderKind>,
    null_provider: SharedEmbeddingProvider,
    json_cache: CacheHandle,
    html_cache: CacheHandle,
}

impl SemanticChunkingService {
    pub fn builder() -> SemanticChunkingServiceBuilder {
        SemanticChunkingServiceBuilder::new()
    }

    pub fn default_config(&self) -> &SemanticChunkingModuleConfig {
        &self.defaults
    }

    pub async fn chunk_document(
        &self,
        request: ChunkDocumentRequest,
    ) -> Result<ChunkDocumentResponse, ChunkingError> {
        let resolved = self.resolve_source(request.source).await?;
        let provider = self.resolve_provider(request.embedder);

        let mut chunk_cfg = request
            .chunking_config
            .unwrap_or_else(|| self.defaults.chunking.clone());
        chunk_cfg.fallback_to_lexical = !provider.configured;

        let html_cfg = request
            .html_config
            .unwrap_or_else(|| self.defaults.html.clone());
        let json_cfg = request
            .json_config
            .unwrap_or_else(|| self.defaults.json.clone());

        let span = info_span!(
            "semantic_chunking",
            source = %resolved.source_label,
            embedder = field::Empty,
            fallback = field::Empty,
            cache_hits = field::Empty,
            cache_misses = field::Empty,
            duration_ms = field::Empty,
            chunks = field::Empty,
            strategy = field::Empty,
        );

        let _entered = span.enter();

        let start = Instant::now();

        let ResolvedDocument { kind, source_label } = resolved;
        let (outcome, cache_hits, cache_misses) = match kind {
            DocumentKind::Json(value) => {
                let cache_handle = self.json_cache.clone();
                let before = cache_handle.metrics();
                let chunker = JsonSemanticChunker::new(provider.shared.clone(), json_cfg)
                    .with_cache_handle(cache_handle.clone());
                let outcome = chunker.chunk(value, &chunk_cfg).await?;
                let after = cache_handle.metrics();
                let diff = Self::metrics_diff(before, after);
                (outcome, diff.0, diff.1)
            }
            DocumentKind::Html(html) => {
                let cache_handle = self.html_cache.clone();
                let before = cache_handle.metrics();
                let chunker = HtmlSemanticChunker::new(provider.shared.clone(), html_cfg)
                    .with_cache_handle(cache_handle.clone());
                let outcome = chunker.chunk(html, &chunk_cfg).await?;
                let after = cache_handle.metrics();
                let diff = Self::metrics_diff(before, after);
                (outcome, diff.0, diff.1)
            }
        };

        let duration_ms = start.elapsed().as_millis();
        let fallback_used = outcome.trace.as_ref().is_some_and(|trace| {
            trace
                .events
                .iter()
                .any(|event| event.label == "lexical_fallback")
        });
        let embedder_label = provider
            .label
            .clone()
            .unwrap_or_else(|| provider.shared.identify().to_string());

        span.record("embedder", field::display(&embedder_label));
        span.record("fallback", field::display(fallback_used));
        span.record("cache_hits", field::display(cache_hits));
        span.record("cache_misses", field::display(cache_misses));
        span.record("duration_ms", field::display(duration_ms));
        span.record("chunks", field::display(outcome.chunks.len()));
        span.record(
            "strategy",
            field::display(strategy_label(&chunk_cfg.strategy)),
        );

        let telemetry = ChunkTelemetry {
            embedder: embedder_label,
            source: source_label,
            duration_ms,
            fallback_used,
            cache_hits,
            cache_misses,
            smoothing_window: chunk_cfg.score_smoothing_window,
            strategy: strategy_label(&chunk_cfg.strategy).to_string(),
            chunk_count: outcome.chunks.len(),
            average_tokens: outcome.stats.average_tokens,
        };

        Ok(ChunkDocumentResponse { outcome, telemetry })
    }

    async fn resolve_source(&self, source: ChunkSource) -> Result<ResolvedDocument, ChunkingError> {
        match source {
            ChunkSource::Json(value) => Ok(ResolvedDocument {
                kind: DocumentKind::Json(value),
                source_label: "json:inline".to_string(),
            }),
            ChunkSource::Html(html) => Ok(ResolvedDocument {
                kind: DocumentKind::Html(html),
                source_label: "html:inline".to_string(),
            }),
            ChunkSource::PlainText(text) => Ok(ResolvedDocument {
                kind: DocumentKind::Html(text),
                source_label: "text:inline".to_string(),
            }),
            ChunkSource::FilePath(path) => self.load_from_path(path).await,
        }
    }

    async fn load_from_path(&self, path: PathBuf) -> Result<ResolvedDocument, ChunkingError> {
        let data = fs::read_to_string(&path)
            .await
            .map_err(|err| ChunkingError::InvalidInput {
                reason: format!("failed to read {}: {err}", path.display()),
            })?;

        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if extension == "json" {
            let value: Value =
                serde_json::from_str(&data).map_err(|err| ChunkingError::InvalidInput {
                    reason: format!("failed to parse JSON {}: {err}", path.display()),
                })?;
            return Ok(ResolvedDocument {
                kind: DocumentKind::Json(value),
                source_label: format!("json:file:{}", path.display()),
            });
        }

        let label = if extension == "html" || extension == "htm" {
            format!("html:file:{}", path.display())
        } else {
            format!("text:file:{}", path.display())
        };

        Ok(ResolvedDocument {
            kind: DocumentKind::Html(data),
            source_label: label,
        })
    }

    fn resolve_provider(&self, override_embedder: Option<EmbedderKind>) -> ProviderContext {
        let embedder = override_embedder.or_else(|| self.base_embedder.clone());
        match embedder {
            Some(EmbedderKind::Rig(handle)) => {
                let shared: SharedEmbeddingProvider = handle.clone();
                ProviderContext {
                    shared,
                    label: Some(handle.model_label().to_string()),
                    configured: true,
                }
            }
            Some(EmbedderKind::Provider(provider)) => ProviderContext {
                shared: provider.clone(),
                label: None,
                configured: true,
            },
            Some(EmbedderKind::None) | None => ProviderContext {
                shared: self.null_provider.clone(),
                label: Some("lexical-fallback".to_string()),
                configured: false,
            },
        }
    }

    fn metrics_diff(before: Option<CacheMetrics>, after: Option<CacheMetrics>) -> (usize, usize) {
        match (before, after) {
            (Some(prev), Some(next)) => (
                next.hits.saturating_sub(prev.hits),
                next.misses.saturating_sub(prev.misses),
            ),
            _ => (0, 0),
        }
    }
}

fn strategy_label(strategy: &BreakpointStrategy) -> &'static str {
    match strategy {
        BreakpointStrategy::Percentile { .. } => "percentile",
        BreakpointStrategy::StdDev { .. } => "stddev",
        BreakpointStrategy::Interquartile { .. } => "interquartile",
        BreakpointStrategy::Gradient { .. } => "gradient",
    }
}

#[derive(Clone)]
pub enum EmbedderKind {
    Rig(Arc<RigEmbeddingProvider>),
    Provider(SharedEmbeddingProvider),
    None,
}

pub struct SemanticChunkingServiceBuilder {
    defaults: SemanticChunkingModuleConfig,
    embedder: Option<EmbedderKind>,
}

impl SemanticChunkingServiceBuilder {
    fn new() -> Self {
        Self {
            defaults: SemanticChunkingModuleConfig::default(),
            embedder: None,
        }
    }

    pub fn with_module_config(mut self, config: SemanticChunkingModuleConfig) -> Self {
        self.defaults = config;
        self
    }

    pub fn with_rig_model<M>(mut self, model: M) -> Self
    where
        M: EmbeddingModel + 'static,
    {
        let provider = Arc::new(RigEmbeddingProvider::from_model(model));
        self.embedder = Some(EmbedderKind::Rig(provider));
        self
    }

    pub fn with_rig_model_dyn(
        mut self,
        model: Arc<dyn EmbeddingModelDyn>,
        label: Option<String>,
    ) -> Self {
        let provider = Arc::new(RigEmbeddingProvider::from_dyn(model, label));
        self.embedder = Some(EmbedderKind::Rig(provider));
        self
    }

    pub fn with_embedding_provider(mut self, provider: SharedEmbeddingProvider) -> Self {
        self.embedder = Some(EmbedderKind::Provider(provider));
        self
    }

    pub fn with_cache_capacity(mut self, capacity: usize) -> Self {
        self.defaults.chunking.cache_capacity = Some(capacity);
        self
    }

    pub fn without_cache(mut self) -> Self {
        self.defaults.chunking.cache_capacity = Some(0);
        self
    }

    pub fn build(self) -> SemanticChunkingService {
        let null_provider: SharedEmbeddingProvider = Arc::new(NullEmbeddingProvider);
        let json_cache = CacheHandle::from_capacity(self.defaults.chunking.cache_capacity);
        let html_cache = CacheHandle::from_capacity(self.defaults.chunking.cache_capacity);
        SemanticChunkingService {
            defaults: self.defaults,
            base_embedder: self.embedder,
            null_provider,
            json_cache,
            html_cache,
        }
    }
}

#[derive(Clone)]
pub enum ChunkSource {
    Json(Value),
    Html(String),
    PlainText(String),
    FilePath(PathBuf),
}

#[derive(Clone)]
pub struct ChunkDocumentRequest {
    pub source: ChunkSource,
    pub chunking_config: Option<ChunkingConfig>,
    pub html_config: Option<HtmlPreprocessConfig>,
    pub json_config: Option<JsonPreprocessConfig>,
    pub embedder: Option<EmbedderKind>,
}

impl ChunkDocumentRequest {
    pub fn new(source: ChunkSource) -> Self {
        Self {
            source,
            chunking_config: None,
            html_config: None,
            json_config: None,
            embedder: None,
        }
    }

    pub fn with_chunking_config(mut self, config: ChunkingConfig) -> Self {
        self.chunking_config = Some(config);
        self
    }

    pub fn update_chunking_config<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut ChunkingConfig),
    {
        let mut cfg = self.chunking_config.take().unwrap_or_default();
        f(&mut cfg);
        self.chunking_config = Some(cfg);
        self
    }

    pub fn with_html_config(mut self, config: HtmlPreprocessConfig) -> Self {
        self.html_config = Some(config);
        self
    }

    pub fn with_json_config(mut self, config: JsonPreprocessConfig) -> Self {
        self.json_config = Some(config);
        self
    }

    pub fn with_embedder(mut self, embedder: EmbedderKind) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_rig_model<M>(self, model: M) -> Self
    where
        M: EmbeddingModel + 'static,
    {
        self.with_embedder(EmbedderKind::Rig(Arc::new(
            RigEmbeddingProvider::from_model(model),
        )))
    }
}

pub struct ChunkDocumentResponse {
    pub outcome: ChunkingOutcome,
    pub telemetry: ChunkTelemetry,
}

#[derive(Clone, Debug)]
pub struct ChunkTelemetry {
    pub embedder: String,
    pub source: String,
    pub duration_ms: u128,
    pub fallback_used: bool,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub smoothing_window: Option<usize>,
    pub strategy: String,
    pub chunk_count: usize,
    pub average_tokens: f32,
}

struct ProviderContext {
    shared: SharedEmbeddingProvider,
    label: Option<String>,
    configured: bool,
}

struct ResolvedDocument {
    kind: DocumentKind,
    source_label: String,
}

enum DocumentKind {
    Json(Value),
    Html(String),
}

impl SemanticChunkingService {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_chunking::MockEmbeddingProvider;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::fs::write;

    #[tokio::test]
    async fn chunks_json_without_embedder() {
        let service = SemanticChunkingService::builder().build();
        let value = json!({
            "title": "Example",
            "body": "Hello world."
        });
        let request = ChunkDocumentRequest::new(ChunkSource::Json(value));
        let response = service.chunk_document(request).await.unwrap();
        assert!(!response.outcome.chunks.is_empty());
        assert!(response.telemetry.fallback_used);
        assert_eq!(response.telemetry.embedder, "lexical-fallback");
        assert!(response
            .outcome
            .chunks
            .iter()
            .all(|chunk| chunk.embedding.is_none()));
    }

    #[tokio::test]
    async fn produces_embeddings_with_mock_provider() {
        let provider: SharedEmbeddingProvider = Arc::new(MockEmbeddingProvider::new());
        let service = SemanticChunkingService::builder()
            .with_embedding_provider(provider)
            .build();

        let mut chunk_cfg = service.default_config().chunking.clone();
        chunk_cfg.cache_capacity = Some(64);

        let request = ChunkDocumentRequest::new(ChunkSource::Html(
            "<article><h1>Title</h1><p>Body text.</p></article>".to_string(),
        ))
        .with_chunking_config(chunk_cfg.clone());

        let response = service.chunk_document(request).await.unwrap();
        assert!(!response.telemetry.fallback_used);
        assert!(response
            .outcome
            .chunks
            .iter()
            .any(|chunk| chunk.embedding.is_some()));
    }

    #[tokio::test]
    async fn reuses_cache_across_requests() {
        let provider: SharedEmbeddingProvider = Arc::new(MockEmbeddingProvider::new());
        let service = SemanticChunkingService::builder()
            .with_embedding_provider(provider)
            .build();

        let mut chunk_cfg = service.default_config().chunking.clone();
        chunk_cfg.cache_capacity = Some(128);

        let request = ChunkDocumentRequest::new(ChunkSource::Json(json!({
            "alpha": "The quick brown fox jumps over the lazy dog.",
            "beta": "Another sentence to encourage multiple segments."
        })))
        .with_chunking_config(chunk_cfg.clone());

        let response_one = service.chunk_document(request.clone()).await.unwrap();
        let response_two = service.chunk_document(request).await.unwrap();

        assert!(response_one.telemetry.cache_hits == 0);
        assert!(response_two.telemetry.cache_hits > 0);
    }

    #[tokio::test]
    async fn chunks_from_file_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("doc.json");
        write(
            &path,
            "{\"title\":\"Doc\",\"body\":\"File content for chunking.\"}",
        )
        .await
        .unwrap();

        let service = SemanticChunkingService::builder().build();
        let request = ChunkDocumentRequest::new(ChunkSource::FilePath(path.clone()));
        let response = service.chunk_document(request).await.unwrap();

        assert!(response.telemetry.source.starts_with("json:file"));
        assert!(!response.outcome.chunks.is_empty());
    }
}
