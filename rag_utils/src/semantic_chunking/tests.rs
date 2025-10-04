use std::sync::Arc;

use super::config::{ChunkingConfig, HtmlPreprocessConfig, JsonPreprocessConfig};
use super::embeddings::{EmbeddingProvider, MockEmbeddingProvider, SharedEmbeddingProvider};
use super::html::HtmlSemanticChunker;
use super::json::JsonSemanticChunker;
use super::types::ChunkingOutcome;
use super::SemanticChunker;
use super::SentenceSplitter;

use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

const JSON_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/semantic_chunking/json/example_nested.json"
));
const HTML_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/semantic_chunking/html/example_article.html"
));
const HTML_TABLE_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/semantic_chunking/html/example_table.html"
));
const JSON_METRICS_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/semantic_chunking/json/example_metrics.json"
));

fn mock_embedder() -> SharedEmbeddingProvider {
    Arc::new(MockEmbeddingProvider::new())
}

struct CountingProvider {
    calls: std::sync::Arc<AtomicUsize>,
}

impl CountingProvider {
    fn new_with_counter() -> (SharedEmbeddingProvider, std::sync::Arc<AtomicUsize>) {
        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider {
            calls: counter.clone(),
        };
        (std::sync::Arc::new(provider), counter)
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for CountingProvider {
    async fn embed_batch(
        &self,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>, super::types::ChunkingError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(inputs
            .iter()
            .map(|text| super::tokenizer::count(text) as f32)
            .map(|seed| vec![seed, seed / 2.0])
            .collect())
    }
}

struct FailingProvider;

#[async_trait::async_trait]
impl EmbeddingProvider for FailingProvider {
    async fn embed_batch(
        &self,
        _inputs: &[String],
    ) -> Result<Vec<Vec<f32>>, super::types::ChunkingError> {
        Err(super::types::ChunkingError::EmbeddingFailed {
            reason: "forced failure".to_string(),
        })
    }
}
#[tokio::test]
async fn json_chunker_produces_chunks() {
    let chunker = JsonSemanticChunker::new(mock_embedder(), JsonPreprocessConfig::default());
    let cfg = ChunkingConfig::default();
    let payload: Value = serde_json::from_str(JSON_FIXTURE).unwrap();

    let outcome = chunker.chunk(payload, &cfg).await.unwrap();
    assert!(!outcome.chunks.is_empty());
    assert!(outcome.stats.total_segments >= outcome.stats.total_chunks);
    assert!(outcome.trace.is_some());
    let first = &outcome.chunks[0];
    assert!(first.metadata.extra.contains_key("segment_paths"));
}

#[tokio::test]
async fn json_chunker_breakpoints_stable() {
    let chunker = JsonSemanticChunker::new(mock_embedder(), JsonPreprocessConfig::default());
    let cfg = ChunkingConfig::default();
    let payload: Value = serde_json::from_str(JSON_FIXTURE).unwrap();

    let outcome = chunker.chunk(payload, &cfg).await.unwrap();
    let signatures: Vec<String> = outcome
        .chunks
        .iter()
        .map(|chunk| chunk.content.lines().next().unwrap_or_default().to_string())
        .collect();
    assert!(!signatures.is_empty());
    assert_eq!(
        signatures,
        vec![
            "owner: Ariel".to_string(),
            "description: Gather requirements and examples.".to_string(),
        ]
    );
}

#[tokio::test]
async fn html_chunker_breakpoints_stable() {
    let chunker = HtmlSemanticChunker::new(mock_embedder(), HtmlPreprocessConfig::default());
    let cfg = ChunkingConfig::default();

    let outcome = chunker.chunk(HTML_FIXTURE.to_string(), &cfg).await.unwrap();
    let signatures: Vec<String> = outcome
        .chunks
        .iter()
        .map(|chunk| chunk.content.lines().next().unwrap_or_default().to_string())
        .collect();
    assert!(!signatures.is_empty());
    assert_eq!(
        signatures,
        vec![
            "Semantic Chunking 101 Semantic chunking divides documents into meaningful passages that can be embedded effectively. Why It Matters Embedding models work best when each chunk contains a single, cohesive idea. Improves retrieval accuracy Reduces hallucination risk Implementation Tips Combine embeddings with lexical heuristics to place breakpoints. Preserve structural metadata like headings and DOM paths.".to_string(),
            "Why It Matters".to_string(),
        ]
    );
}

#[tokio::test]
async fn json_chunker_handles_metrics_fixture() {
    let chunker = JsonSemanticChunker::new(mock_embedder(), JsonPreprocessConfig::default());
    let cfg = ChunkingConfig::default();
    let payload: Value = serde_json::from_str(JSON_METRICS_FIXTURE).unwrap();

    let outcome = chunker.chunk(payload, &cfg).await.unwrap();
    assert!(!outcome.chunks.is_empty());
    assert!(outcome
        .chunks
        .iter()
        .any(|chunk| chunk.content.contains("accuracy")));
}

#[tokio::test]
async fn html_chunker_handles_table_fixture() {
    let chunker = HtmlSemanticChunker::new(mock_embedder(), HtmlPreprocessConfig::default());
    let cfg = ChunkingConfig::default();

    let outcome = chunker
        .chunk(HTML_TABLE_FIXTURE.to_string(), &cfg)
        .await
        .unwrap();
    assert!(!outcome.chunks.is_empty());
    assert!(outcome
        .chunks
        .iter()
        .any(|chunk| chunk.content.contains("Bucket")));
}

#[cfg(feature = "semantic-chunking-segtok")]
#[tokio::test]
async fn json_chunker_uses_segtok_splitter() {
    let chunker = JsonSemanticChunker::new(mock_embedder(), JsonPreprocessConfig::default());
    let cfg = ChunkingConfig {
        sentence_splitter: SentenceSplitter::Segtok,
        ..ChunkingConfig::default()
    };
    let payload: Value = serde_json::from_str(JSON_FIXTURE).unwrap();

    let outcome = chunker.chunk(payload, &cfg).await.unwrap();
    assert!(!outcome.chunks.is_empty());
}

#[tokio::test]
async fn json_chunker_handles_fixture_multiple_runs_with_cache() {
    let (embedder, calls) = CountingProvider::new_with_counter();
    let chunker =
        JsonSemanticChunker::new(embedder, JsonPreprocessConfig::default()).with_cache_capacity(32);
    let cfg = ChunkingConfig::default();
    let payload: Value = serde_json::from_str(JSON_FIXTURE).unwrap();

    let first = chunker.chunk(payload.clone(), &cfg).await.unwrap();
    assert!(!first.chunks.is_empty());
    let call_count_after_first = calls.load(Ordering::SeqCst);

    let second = chunker.chunk(payload, &cfg).await.unwrap();
    assert_eq!(first.chunks.len(), second.chunks.len());
    assert_eq!(call_count_after_first, calls.load(Ordering::SeqCst));
}

#[tokio::test]
async fn html_chunker_captures_heading_metadata() {
    let chunker = HtmlSemanticChunker::new(mock_embedder(), HtmlPreprocessConfig::default());
    let cfg = ChunkingConfig::default();
    let html = r#"
        <html>
            <head><title>Chunk Doc</title></head>
            <body>
                <h1>Main Title</h1>
                <p>Intro paragraph with some descriptive content.</p>
                <h2>Details Section</h2>
                <p>More information lives here so we can test grouping behavior.</p>
            </body>
        </html>
    "#;

    let outcome = chunker.chunk(html.to_string(), &cfg).await.unwrap();
    assert!(!outcome.chunks.is_empty());

    let heading_chunk = outcome
        .chunks
        .iter()
        .find(|chunk| {
            chunk
                .metadata
                .extra
                .get("heading_chain")
                .and_then(|value| value.as_array())
                .map(|array| {
                    array
                        .iter()
                        .any(|value| value.as_str() == Some("Main Title"))
                })
                .unwrap_or(false)
        })
        .expect("expected chunk capturing heading metadata");

    let title_value = heading_chunk
        .metadata
        .extra
        .get("document_title")
        .and_then(|value| value.as_str())
        .expect("document title metadata present");
    assert_eq!(title_value, "Chunk Doc");

    assert!(heading_chunk.tokens > 0);
}

#[tokio::test]
async fn html_chunker_returns_chunks() {
    let chunker = HtmlSemanticChunker::new(mock_embedder(), HtmlPreprocessConfig::default());
    let cfg = ChunkingConfig::default();

    let outcome = chunker.chunk(HTML_FIXTURE.to_string(), &cfg).await.unwrap();
    match outcome {
        ChunkingOutcome { chunks, .. } if !chunks.is_empty() => {}
        _ => panic!("expected at least one chunk"),
    }
}

#[tokio::test]
async fn json_chunker_falls_back_to_lexical() {
    let chunker = JsonSemanticChunker::new(
        std::sync::Arc::new(FailingProvider),
        JsonPreprocessConfig::default(),
    );
    let cfg = ChunkingConfig {
        fallback_to_lexical: true,
        cache_capacity: Some(0),
        sentence_splitter: SentenceSplitter::Regex,
        ..ChunkingConfig::default()
    };
    let payload: Value = serde_json::from_str(JSON_METRICS_FIXTURE).unwrap();

    let outcome = chunker.chunk(payload, &cfg).await.unwrap();
    assert!(!outcome.chunks.is_empty());
    let trace = outcome.trace.expect("trace expected");
    assert!(trace
        .events
        .iter()
        .any(|event| event.label == "lexical_fallback"));
}

#[tokio::test]
async fn html_chunker_falls_back_to_lexical() {
    let chunker = HtmlSemanticChunker::new(
        std::sync::Arc::new(FailingProvider),
        HtmlPreprocessConfig::default(),
    );
    let cfg = ChunkingConfig {
        fallback_to_lexical: true,
        cache_capacity: Some(0),
        sentence_splitter: SentenceSplitter::Regex,
        ..ChunkingConfig::default()
    };

    let outcome = chunker
        .chunk(HTML_TABLE_FIXTURE.to_string(), &cfg)
        .await
        .unwrap();
    assert!(!outcome.chunks.is_empty());
    let trace = outcome.trace.expect("trace expected");
    assert!(trace
        .events
        .iter()
        .any(|event| event.label == "lexical_fallback"));
}
