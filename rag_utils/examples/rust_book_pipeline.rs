use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rag_utils::ingestion::rust_book::{RustBookIngestOptions, RustBookIngestor};
use rag_utils::semantic_chunking::embeddings::MockEmbeddingProvider;
use rag_utils::semantic_chunking::service::SemanticChunkingService;
use rag_utils::stores::sqlite::SqliteChunkStore;
use rig::embeddings::embedding::{Embedding, EmbeddingError, EmbeddingModel};
use tokio::fs;
use tracing_subscriber::FmtSubscriber;
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let base_url = env::var("RUST_BOOK_BASE_URL")
        .unwrap_or_else(|_| "https://doc.rust-lang.org/book/".to_string());
    let base_url = Url::parse(&base_url)?;

    let cache_dir = env::var("RUST_BOOK_CACHE").unwrap_or_else(|_| "./rust_book_cache".to_string());
    let cache_dir = PathBuf::from(cache_dir);
    fs::create_dir_all(&cache_dir).await?;

    let db_path =
        env::var("RUST_BOOK_DB").unwrap_or_else(|_| "./rust_book_chunks.sqlite".to_string());
    let db_path = PathBuf::from(db_path);
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }

    let limit = env::var("RUST_BOOK_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    let resume = env::var("RUST_BOOK_RESUME")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let concurrency = env::var("RUST_BOOK_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4)
        .max(1);

    let options = RustBookIngestOptions {
        base_url,
        concurrency,
        limit,
        cache_dir: Some(cache_dir.clone()),
        resume,
        state_path: None,
    };

    let service = Arc::new(
        SemanticChunkingService::builder()
            .with_embedding_provider(Arc::new(MockEmbeddingProvider::new()))
            .build(),
    );

    let embedding_model = DemoEmbeddingModel;
    let store = Arc::new(SqliteChunkStore::open(&db_path, &embedding_model).await?);

    let ingestor = RustBookIngestor::new(service, store, options)?;

    println!("🚀 Starting Rust Book ingestion...");
    let result = ingestor.ingest().await?;

    println!("\n✅ Ingestion complete!");
    println!("  pages processed : {}", result.pages_processed);
    println!("  pages skipped   : {}", result.pages_skipped);
    println!("  chunks written  : {}", result.chunks_written);
    println!(
        "  bytes downloaded: {:.2} MB",
        result.bytes_downloaded as f64 / (1024.0 * 1024.0)
    );
    println!("  duration        : {:?}", format_duration(result.duration));
    println!("  cache directory : {}", cache_dir.display());
    println!("  sqlite database : {}", db_path.display());

    Ok(())
}

fn init_tracing() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let subscriber = FmtSubscriber::builder().with_env_filter("info").finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let minutes = secs / 60;
    let seconds = secs % 60;
    format!("{}m {}.{:03}s", minutes, seconds, millis)
}

#[derive(Clone)]
struct DemoEmbeddingModel;

impl EmbeddingModel for DemoEmbeddingModel {
    const MAX_DOCUMENTS: usize = 64;

    fn ndims(&self) -> usize {
        8
    }

    fn embed_texts(
        &self,
        texts: impl IntoIterator<Item = String> + Send,
    ) -> impl std::future::Future<Output = Result<Vec<Embedding>, EmbeddingError>> + Send {
        let docs: Vec<String> = texts.into_iter().collect();
        async move {
            Ok(docs
                .into_iter()
                .map(|document| Embedding {
                    vec: hash_to_vec(&document),
                    document,
                })
                .collect())
        }
    }
}

fn hash_to_vec(text: &str) -> Vec<f64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let seed = hasher.finish();
    (0..8)
        .map(|i| {
            let bits = seed.rotate_left((i * 8) as u32) ^ ((i as u64) << 24);
            (bits as f64) / u32::MAX as f64
        })
        .collect()
}
