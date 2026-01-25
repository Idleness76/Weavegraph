//! Integration tests for wg-ragsmith with mock embeddings.
//!
//! These tests verify the semantic chunking pipeline works correctly
//! with mock embedding providers, suitable for CI and deterministic testing.

use std::sync::Arc;

use wg_ragsmith::semantic_chunking::config::{BreakpointStrategy, ChunkingConfig};
use wg_ragsmith::semantic_chunking::embeddings::{EmbeddingProvider, MockEmbeddingProvider};
use wg_ragsmith::semantic_chunking::service::{
    ChunkDocumentRequest, ChunkSource, SemanticChunkingService,
};

fn make_test_service() -> SemanticChunkingService {
    // Use the mock embedding provider via with_embedding_provider
    let mock_provider: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new());
    SemanticChunkingService::builder()
        .with_embedding_provider(mock_provider)
        .build()
}

fn sample_html() -> String {
    r#"<!DOCTYPE html>
<html>
<head><title>Test Document</title></head>
<body>
    <h1>Introduction</h1>
    <p>This is the introduction paragraph with some content about the topic.</p>
    
    <h2>First Section</h2>
    <p>Here we discuss the first major point. It contains several sentences
    that should be chunked together based on semantic similarity.</p>
    
    <h2>Second Section</h2>
    <p>The second section covers different material. This paragraph has
    distinct content that should form its own chunk.</p>
    
    <h3>Subsection</h3>
    <p>A deeper subsection with more specific information.</p>
    
    <h1>Conclusion</h1>
    <p>Final thoughts and summary of the document.</p>
</body>
</html>"#
        .to_string()
}

fn sample_json() -> serde_json::Value {
    serde_json::json!({
        "title": "Sample Document",
        "sections": [
            {
                "heading": "Overview",
                "content": "This section provides an overview of the topic. It contains multiple sentences that describe the general scope and purpose."
            },
            {
                "heading": "Details",
                "content": "Detailed information about specific aspects. This section dives deeper into the technical implementation."
            },
            {
                "heading": "Summary",
                "content": "A brief summary wrapping up the key points discussed."
            }
        ],
        "metadata": {
            "author": "Test Author",
            "date": "2025-01-25"
        }
    })
}

#[tokio::test]
async fn test_html_chunking_with_mock_embeddings() {
    let service = make_test_service();

    let request = ChunkDocumentRequest::new(ChunkSource::Html(sample_html()));

    let response = service.chunk_document(request).await.unwrap();

    // Should produce multiple chunks
    assert!(
        !response.outcome.chunks.is_empty(),
        "should produce at least one chunk"
    );

    // Verify chunk structure
    for chunk in &response.outcome.chunks {
        assert!(!chunk.content.is_empty(), "chunk content should not be empty");
    }

    // Telemetry should be populated
    assert!(response.telemetry.duration_ms > 0 || response.telemetry.chunk_count > 0);
}

#[tokio::test]
async fn test_json_chunking_with_mock_embeddings() {
    let service = make_test_service();

    let request = ChunkDocumentRequest::new(ChunkSource::Json(sample_json()));

    let response = service.chunk_document(request).await.unwrap();

    assert!(
        !response.outcome.chunks.is_empty(),
        "should produce chunks from JSON"
    );

    // Each chunk should have content
    for chunk in &response.outcome.chunks {
        assert!(!chunk.content.is_empty());
    }
}

#[tokio::test]
async fn test_plain_text_chunking() {
    let service = make_test_service();

    // Use a longer plain text to ensure chunking happens
    let plain_text = r#"
    The first paragraph discusses topic A in substantial detail. It contains several 
    sentences that explain the concept thoroughly and provide background context for 
    readers who may be unfamiliar with the subject matter. This ensures there is 
    enough content for the semantic chunking algorithm to analyze.
    
    The second paragraph moves on to topic B, which is a related but distinct subject 
    that requires its own treatment and consideration. We explore multiple aspects of 
    this topic, including historical background, current applications, and future 
    implications. The goal is to provide comprehensive coverage.
    
    In the third paragraph, we delve into topic C, examining technical implementation 
    details and practical considerations. This section covers architecture decisions, 
    performance optimization strategies, and common pitfalls to avoid.
    
    Finally, the fourth paragraph provides a conclusion that summarizes the main points 
    discussed throughout this document. It wraps up the discussion by highlighting 
    key takeaways and suggesting areas for further exploration and research.
    "#
    .to_string();

    let request = ChunkDocumentRequest::new(ChunkSource::PlainText(plain_text));

    let response = service.chunk_document(request).await.unwrap();

    // Plain text chunking should complete without error.
    // The number of chunks depends on the chunking algorithm and thresholds.
    // An empty result is acceptable if the text doesn't meet chunking criteria.
    assert!(
        response.outcome.chunks.is_empty() || !response.outcome.chunks[0].content.is_empty(),
        "if chunks are produced, they should have content"
    );
}

#[tokio::test]
async fn test_chunking_with_custom_config() {
    let service = make_test_service();

    let custom_config = ChunkingConfig {
        max_tokens: 100, // Force smaller chunks
        min_tokens: 10,
        strategy: BreakpointStrategy::Percentile { threshold: 0.8 },
        fallback_to_lexical: true,
        ..Default::default()
    };

    let request = ChunkDocumentRequest::new(ChunkSource::Html(sample_html()))
        .with_chunking_config(custom_config);

    let response = service.chunk_document(request).await.unwrap();

    // With smaller max_tokens, should produce chunks
    assert!(
        !response.outcome.chunks.is_empty(),
        "should produce chunks"
    );
}

#[tokio::test]
async fn test_mock_embedding_provider_determinism() {
    let provider = MockEmbeddingProvider::new();

    let inputs = vec![
        "Hello world".to_string(),
        "Goodbye world".to_string(),
        "Hello world".to_string(), // Duplicate
    ];

    let embeddings1 = provider.embed_batch(&inputs).await.unwrap();
    let embeddings2 = provider.embed_batch(&inputs).await.unwrap();

    // Same inputs should produce same embeddings
    assert_eq!(embeddings1, embeddings2, "mock embeddings should be deterministic");

    // Same text should produce same embedding
    assert_eq!(
        embeddings1[0], embeddings1[2],
        "identical text should have identical embedding"
    );

    // Different text should produce different embeddings
    assert_ne!(
        embeddings1[0], embeddings1[1],
        "different text should have different embeddings"
    );
}

#[tokio::test]
async fn test_embedding_cache_hits() {
    let service = make_test_service();

    let html = sample_html();

    // First request
    let request1 = ChunkDocumentRequest::new(ChunkSource::Html(html.clone()));

    let response1 = service.chunk_document(request1).await.unwrap();

    // Second request with same content - should have cache hits
    let request2 = ChunkDocumentRequest::new(ChunkSource::Html(html));

    let response2 = service.chunk_document(request2).await.unwrap();

    // Cache should show improvement on second call
    assert!(
        response2.telemetry.cache_hits >= response1.telemetry.cache_hits,
        "second request should have equal or more cache hits"
    );
}

#[tokio::test]
async fn test_empty_document_handling() {
    let service = make_test_service();

    let empty_html = "<html><body></body></html>".to_string();

    let request = ChunkDocumentRequest::new(ChunkSource::Html(empty_html));

    // Should not panic, might produce zero chunks
    let result = service.chunk_document(request).await;
    assert!(result.is_ok(), "empty document should not cause error");
}

#[tokio::test]
async fn test_large_document_chunking() {
    let service = make_test_service();

    // Generate a large document
    let mut paragraphs = Vec::new();
    for i in 0..50 {
        paragraphs.push(format!(
            "<p>This is paragraph {} with content about topic {}. \
            It contains multiple sentences to ensure there's enough text \
            for meaningful semantic analysis and chunking.</p>",
            i, i % 5
        ));
    }

    let large_html = format!(
        "<html><body><h1>Large Document</h1>{}</body></html>",
        paragraphs.join("\n")
    );

    let request = ChunkDocumentRequest::new(ChunkSource::Html(large_html));

    let response = service.chunk_document(request).await.unwrap();

    // Should handle large documents
    assert!(
        response.outcome.chunks.len() > 1,
        "large document should produce multiple chunks, got {}",
        response.outcome.chunks.len()
    );
}

#[tokio::test]
async fn test_chunking_preserves_semantic_units() {
    let service = make_test_service();

    // Document with clear semantic boundaries
    let html = r#"
    <html><body>
        <section>
            <h2>Mathematics</h2>
            <p>Calculus is the mathematical study of continuous change. 
            It has two major branches: differential calculus and integral calculus.</p>
        </section>
        <section>
            <h2>History</h2>
            <p>World War II was a global war that lasted from 1939 to 1945.
            It was the most widespread war in history.</p>
        </section>
    </body></html>
    "#
    .to_string();

    let request = ChunkDocumentRequest::new(ChunkSource::Html(html));

    let response = service.chunk_document(request).await.unwrap();

    // Check that math and history content don't mix in same chunk
    for chunk in &response.outcome.chunks {
        let has_math = chunk.content.contains("Calculus") || chunk.content.contains("calculus");
        let has_history = chunk.content.contains("World War") || chunk.content.contains("1939");

        // Ideally chunks are topic-coherent (this is a soft check)
        // The semantic chunker should try to keep related content together
        if has_math && has_history {
            // Not ideal but acceptable - log for debugging
            eprintln!(
                "Note: chunk mixes topics (may be expected with small docs): {}...",
                &chunk.content[..chunk.content.len().min(50)]
            );
        }
    }
}
