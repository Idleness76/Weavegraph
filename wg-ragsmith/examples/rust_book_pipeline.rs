use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use rig::embeddings::embedding::{Embedding, EmbeddingError, EmbeddingModel};
use scraper::{Html, Selector};
use tokio::fs;
use tracing_subscriber::FmtSubscriber;
use url::Url;
use wg_ragsmith::ingestion::{
    DocumentCache, ResumeTracker, chunk_response_to_ingestion, fetch_html,
};
use wg_ragsmith::semantic_chunking::embeddings::MockEmbeddingProvider;
use wg_ragsmith::semantic_chunking::service::{
    ChunkDocumentRequest, ChunkSource, SemanticChunkingService,
};
use wg_ragsmith::stores::sqlite::SqliteChunkStore;
use wg_ragsmith::types::RagError;

#[tokio::main]
async fn main() -> Result<(), RagError> {
    init_tracing();

    let base_url = env::var("RUST_BOOK_BASE_URL")
        .unwrap_or_else(|_| "https://doc.rust-lang.org/book/".to_string());
    let base_url =
        Url::parse(&base_url).map_err(|err| RagError::InvalidDocument(err.to_string()))?;

    let cache_dir = env::var("RUST_BOOK_CACHE").unwrap_or_else(|_| "./rust_book_cache".to_string());
    let cache_dir = PathBuf::from(cache_dir);
    if let Some(parent) = cache_dir.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }
    fs::create_dir_all(&cache_dir).await?;
    let cache = DocumentCache::new(cache_dir.clone());

    let state_path = env::var("RUST_BOOK_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| cache.state_file());

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

    let client = Client::builder()
        .user_agent("weavegraph-RustBook-Ingestor/0.2")
        .use_rustls_tls()
        .build()?;

    let service = Arc::new(
        SemanticChunkingService::builder()
            .with_embedding_provider(Arc::new(MockEmbeddingProvider::new()))
            .build(),
    );

    let embedding_model = DemoEmbeddingModel;
    let store = Arc::new(SqliteChunkStore::open(&db_path, &embedding_model).await?);

    let mut toc_urls = fetch_rust_book_toc(&client, &base_url).await?;
    if let Some(limit) = limit {
        toc_urls.truncate(limit);
    }

    let resume_tracker = if resume {
        let tracker = ResumeTracker::new(state_path);
        tracker.load().await?;
        Some(tracker)
    } else {
        None
    };

    println!("Found {} chapters to process", toc_urls.len());

    let start = Instant::now();
    let mut pages_processed = 0usize;
    let mut pages_skipped = 0usize;
    let mut chunks_written = 0usize;
    let mut bytes_downloaded = 0usize;
    let mut telemetry = Vec::new();

    for url in toc_urls {
        if let Some(tracker) = &resume_tracker {
            if tracker.contains(&url).await {
                pages_skipped += 1;
                println!("⏭︎ Skipping {} (already recorded)", url);
                continue;
            }
        }

        println!("→ Fetching {}", url);
        let fetch = fetch_html(&client, &url, Some(&cache)).await?;
        if fetch.from_cache {
            println!("   using cache ({:.2} KB)", fetch.bytes as f64 / 1024.0);
        } else {
            println!("   downloaded {:.2} KB", fetch.bytes as f64 / 1024.0);
        }
        bytes_downloaded += fetch.bytes;

        let response = service
            .chunk_document(ChunkDocumentRequest::new(ChunkSource::Html(fetch.content)))
            .await
            .map_err(|err| RagError::Chunking(err.to_string()))?;

        let ingestion = chunk_response_to_ingestion(&url, response)?;
        let chunk_count = ingestion.chunk_count();
        let skipped_chunks = ingestion.skipped_chunks();
        let (batch, _outcome, telemetry_item) = ingestion.into_parts();

        store.add_chunks(batch.into_documents()).await?;
        telemetry.push(telemetry_item);

        pages_processed += 1;
        chunks_written += chunk_count;

        println!(
            "   stored {} chunks (skipped {} without embeddings)",
            chunk_count, skipped_chunks
        );

        if let Some(tracker) = &resume_tracker {
            tracker.mark_processed(&url).await?;
        }
    }

    let duration = start.elapsed();

    println!("\n✅ Ingestion complete!");
    println!("  pages processed : {}", pages_processed);
    println!("  pages skipped   : {}", pages_skipped);
    println!("  chunks written  : {}", chunks_written);
    println!(
        "  bytes downloaded: {:.2} MB",
        bytes_downloaded as f64 / (1024.0 * 1024.0)
    );
    println!("  duration        : {}", format_duration(duration));
    println!("  cache directory : {}", cache_dir.display());
    println!("  sqlite database : {}", db_path.display());

    if !telemetry.is_empty() {
        let avg_tokens: f32 =
            telemetry.iter().map(|t| t.average_tokens).sum::<f32>() / telemetry.len() as f32;
        let avg_chunks: f32 =
            telemetry.iter().map(|t| t.chunk_count as f32).sum::<f32>() / telemetry.len() as f32;
        println!(
            "  avg chunks/run : {:.1} (avg tokens {:.1})",
            avg_chunks, avg_tokens
        );
    }

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

async fn fetch_rust_book_toc(client: &Client, base_url: &Url) -> Result<Vec<Url>, RagError> {
    let response = client
        .get(base_url.clone())
        .send()
        .await?
        .error_for_status()?;
    let body = response.text().await?;
    let document = Html::parse_document(&body);
    let selector =
        Selector::parse("nav ul li a").map_err(|err| RagError::InvalidDocument(err.to_string()))?;

    let mut urls = Vec::new();
    for element in document.select(&selector) {
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        if href.starts_with('#') {
            continue;
        }
        if let Ok(mut url) = base_url.join(href) {
            url.set_fragment(None);
            if !urls.iter().any(|existing| existing == &url) {
                urls.push(url);
            }
        }
    }

    if urls.is_empty() {
        return Err(RagError::InvalidDocument(
            "no chapter links found".to_string(),
        ));
    }

    Ok(urls)
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
