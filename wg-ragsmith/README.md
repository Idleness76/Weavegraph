# wg-ragsmith

[![Crates.io](https://img.shields.io/crates/v/wg-ragsmith.svg)](https://crates.io/crates/wg-ragsmith)
[![Documentation](https://docs.rs/wg-ragsmith/badge.svg)](https://docs.rs/wg-ragsmith)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Semantic chunking and RAG utilities for document processing and retrieval-augmented generation.**

`wg-ragsmith` provides high-performance semantic chunking algorithms and vector storage utilities designed for building RAG (Retrieval-Augmented Generation) applications. It supports multiple document formats (HTML, JSON, plain text) and integrates with popular embedding providers.

---

> **⚠️ EARLY BETA WARNING**  
> This crate is in early development (v0.1.x). APIs are unstable and **will** change between minor versions.  
> Breaking changes may arrive without fanfare. Pin exact versions in production, and check release notes carefully before upgrading.  
> That said, the core algorithms work—just expect some assembly required.

---

## ✨ Key Features

- **🔍 Semantic Chunking**: Intelligent document segmentation using embeddings and structural analysis
- **📄 Multi-format Support**: Process HTML, JSON, and plain text documents
- **🧠 Embedding Integration**: Built-in support for Rig-based embedding providers
- **💾 Vector Storage**: SQLite-based vector store with efficient similarity search
- **🔄 Async Processing**: Full async/await support with tokio runtime
- **📊 Rich Metadata**: Preserve document structure and provenance information
- **🎛️ Configurable**: Extensive tuning options for different use cases

## 🚀 Quick Start

Add `wg-ragsmith` to your `Cargo.toml`:

```toml
[dependencies]
wg-ragsmith = "0.1"
```

### Basic Document Chunking

```rust
use wg_ragsmith::semantic_chunking::service::{SemanticChunkingService, ChunkDocumentRequest, ChunkSource};
use wg_ragsmith::semantic_chunking::embeddings::MockEmbeddingProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a chunking service with mock embeddings
    let service = SemanticChunkingService::builder()
        .with_embedding_provider(MockEmbeddingProvider)
        .build()?;

    // Chunk an HTML document
    let html_content = r#"
        <html><body>
            <h1>Introduction</h1>
            <p>This is a sample document for chunking.</p>
            <h2>Section 1</h2>
            <p>More content here with detailed information.</p>
        </body></html>
    "#;

    let request = ChunkDocumentRequest::new(ChunkSource::Html(html_content.to_string()));
    let response = service.chunk_document(request).await?;

    println!("Created {} chunks", response.outcome.chunks.len());
    for chunk in &response.outcome.chunks {
        println!("Chunk: {} ({} tokens)", chunk.content.chars().take(50).collect::<String>(), chunk.tokens);
    }

    Ok(())
}
```

### Vector Storage and Retrieval

```rust
use wg_ragsmith::stores::sqlite::SqliteChunkStore;
use wg_ragsmith::ingestion::chunk_response_to_ingestion;
use std::sync::Arc;

// Set up vector store
let store = Arc::new(SqliteChunkStore::new("chunks.db").await?);

// Store chunks (from previous example)
let ingestion = chunk_response_to_ingestion(&url, response)?;
store.store_batch(&ingestion.batch).await?;

// Search for similar content
let query_embedding = vec![0.1, 0.2, 0.3]; // Your query embedding
let results = store.search_similar(&query_embedding, 5).await?;

for result in results {
    println!("Found: {} (score: {})", result.content, result.score);
}
```

## 📋 Feature Flags

- `semantic-chunking-tiktoken` (default): Enable OpenAI tiktoken-based tokenization
- `semantic-chunking-rust-bert`: Enable Rust BERT integration for advanced NLP
- `semantic-chunking-segtok`: Enable segtok sentence segmentation

## 🏗️ Architecture

### Core Components

- **`SemanticChunkingService`**: Main entry point for document processing
- **`HtmlSemanticChunker`**: HTML-specific chunking with DOM awareness
- **`JsonSemanticChunker`**: JSON document processing with structural preservation
- **`SqliteChunkStore`**: Vector storage with SQLite backend
- **Embedding Providers**: Pluggable embedding generation (Rig, custom implementations)

### Chunking Strategies

- **Percentile**: Breakpoints based on embedding similarity percentiles
- **Standard Deviation**: Statistical outlier detection for breakpoints
- **Interquartile**: Robust statistical breakpoint detection
- **Gradient**: Similarity gradient analysis

## 🤝 Integration Examples

### With Rig (Recommended)

```rust
use rig::providers::openai::Client as OpenAIClient;
use wg_ragsmith::semantic_chunking::service::SemanticChunkingService;

let openai_client = OpenAIClient::new("your-api-key");
let service = SemanticChunkingService::builder()
    .with_embedding_provider(openai_client.embedding_model("text-embedding-ada-002"))
    .build()?;
```

### Custom Embedding Provider

```rust
use wg_ragsmith::semantic_chunking::embeddings::{EmbeddingProvider, SharedEmbeddingProvider};

struct MyEmbeddingProvider;

#[async_trait::async_trait]
impl EmbeddingProvider for MyEmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        // Your custom embedding logic
        todo!()
    }

    fn identify(&self) -> &'static str {
        "my-provider"
    }
}
```

## 📊 Performance

- **Memory Efficient**: Streaming processing for large documents
- **Concurrent**: Parallel embedding generation with configurable batching
- **Cached**: Built-in embedding caching to reduce API calls
- **Scalable**: SQLite backend supports large document collections

## 🔧 Configuration

Extensive configuration options for tuning chunking behavior:

```rust
use wg_ragsmith::semantic_chunking::config::{ChunkingConfig, BreakpointStrategy};

let config = ChunkingConfig {
    strategy: BreakpointStrategy::Percentile { threshold: 0.9 },
    max_tokens: 512,
    min_tokens: 32,
    batch_size: 16,
    // ... more options
};
```

## 📚 Documentation

- [API Documentation](https://docs.rs/wg-ragsmith)
- [Examples](./examples/)
- [Semantic Chunking Guide](./doc/senantic_chunking_plan.md)

## 🤝 Contributing

Contributions welcome! Please see the main [Weavegraph repository](https://github.com/Idleness76/weavegraph) for contribution guidelines.

## 📄 License

Licensed under the MIT License. See [LICENSE](../LICENSE) for details.