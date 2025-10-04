use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{Client, Url};
use scraper::{Html, Selector};
use tokio::fs;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::semantic_chunking::service::{ChunkDocumentRequest, ChunkDocumentResponse, ChunkSource};
use crate::semantic_chunking::{ChunkTelemetry, SemanticChunkingService};
use crate::stores::sqlite::{ChunkDocument, SqliteChunkStore};
use crate::types::RagError;

#[derive(Clone)]
pub struct RustBookIngestOptions {
    pub base_url: Url,
    pub concurrency: usize,
    pub limit: Option<usize>,
    pub cache_dir: Option<PathBuf>,
    pub resume: bool,
    pub state_path: Option<PathBuf>,
}

impl Default for RustBookIngestOptions {
    fn default() -> Self {
        Self {
            base_url: Url::parse("https://doc.rust-lang.org/book/")
                .expect("valid default base url"),
            concurrency: 4,
            limit: None,
            cache_dir: None,
            resume: false,
            state_path: None,
        }
    }
}

pub struct RustBookIngestResult {
    pub pages_processed: usize,
    pub pages_skipped: usize,
    pub chunks_written: usize,
    pub bytes_downloaded: usize,
    pub duration: Duration,
    pub telemetry: Vec<ChunkTelemetry>,
}

pub struct RustBookIngestor<E>
where
    E: rig::embeddings::EmbeddingModel + Clone + Send + Sync + 'static,
{
    service: Arc<SemanticChunkingService>,
    store: Arc<SqliteChunkStore<E>>,
    options: RustBookIngestOptions,
    client: Client,
    state: Arc<Mutex<HashSet<String>>>,
}

impl<E> RustBookIngestor<E>
where
    E: rig::embeddings::EmbeddingModel + Clone + Send + Sync + 'static,
{
    pub fn new(
        service: Arc<SemanticChunkingService>,
        store: Arc<SqliteChunkStore<E>>,
        options: RustBookIngestOptions,
    ) -> Result<Self, RagError> {
        let client = Client::builder()
            .user_agent("weavegraph-RustBook-Ingestor/0.1")
            .build()
            .map_err(|err| RagError::Network(err.to_string()))?;

        let state = Arc::new(Mutex::new(HashSet::new()));

        Ok(Self {
            service,
            store,
            options,
            client,
            state,
        })
    }

    pub async fn ingest(&self) -> Result<RustBookIngestResult, RagError> {
        self.ensure_state_loaded().await?;
        let mut urls = self.fetch_toc().await?;
        if let Some(limit) = self.options.limit {
            urls.truncate(limit);
        }

        let start = Instant::now();
        let mut pages_processed = 0usize;
        let mut pages_skipped = 0usize;
        let mut chunks_written = 0usize;
        let mut bytes_downloaded = 0usize;
        let mut telemetry = Vec::new();

        for url in urls {
            if self.should_skip(&url).await {
                info!("url" = %url, "Skipping already ingested chapter");
                pages_skipped += 1;
                continue;
            }

            match self.process_chapter(url.clone()).await {
                Ok(page_result) => {
                    pages_processed += 1;
                    chunks_written += page_result.chunk_count;
                    bytes_downloaded += page_result.bytes_downloaded;
                    telemetry.push(page_result.telemetry);
                    self.mark_processed(&url).await?;
                }
                Err(err) => {
                    return Err(err);
                }
            }
        }

        Ok(RustBookIngestResult {
            pages_processed,
            pages_skipped,
            chunks_written,
            bytes_downloaded,
            duration: start.elapsed(),
            telemetry,
        })
    }

    async fn ensure_state_loaded(&self) -> Result<(), RagError> {
        if !self.options.resume {
            return Ok(());
        }
        let path = match self.state_file_path() {
            Some(path) => path,
            None => return Ok(()),
        };
        if !path.exists() {
            return Ok(());
        }
        let data = fs::read_to_string(&path).await?;
        let urls: Vec<String> =
            serde_json::from_str(&data).map_err(|err| RagError::Io(err.to_string()))?;
        let mut guard = self.state.lock().await;
        guard.extend(urls);
        Ok(())
    }

    async fn mark_processed(&self, url: &Url) -> Result<(), RagError> {
        if !self.options.resume {
            return Ok(());
        }
        let Some(path) = self.state_file_path() else {
            return Ok(());
        };
        let mut guard = self.state.lock().await;
        guard.insert(url.to_string());
        let serialized = serde_json::to_string(&guard.iter().collect::<Vec<_>>())
            .map_err(|err| RagError::Io(err.to_string()))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).await?;
        }
        fs::write(path, serialized).await?;
        Ok(())
    }

    fn state_file_path(&self) -> Option<PathBuf> {
        self.options.state_path.clone().or_else(|| {
            self.options
                .cache_dir
                .as_ref()
                .map(|dir| dir.join("ingest_state.json"))
        })
    }

    async fn should_skip(&self, url: &Url) -> bool {
        if !self.options.resume {
            return false;
        }
        let guard = self.state.lock().await;
        guard.contains(url.as_str())
    }

    async fn fetch_toc(&self) -> Result<Vec<Url>, RagError> {
        let response = self
            .client
            .get(self.options.base_url.clone())
            .send()
            .await?
            .error_for_status()
            .map_err(|err| RagError::Network(err.to_string()))?
            .text()
            .await?;

        let document = Html::parse_document(&response);
        let selector = Selector::parse("nav ul li a")
            .map_err(|err| RagError::InvalidDocument(err.to_string()))?;
        let mut urls = Vec::new();
        for element in document.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                if let Ok(url) = self.options.base_url.join(href) {
                    if !urls.iter().any(|existing| existing == &url) {
                        urls.push(url);
                    }
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

    async fn process_chapter(&self, url: Url) -> Result<PageResult, RagError> {
        let html = self.load_or_fetch(&url).await?;
        let bytes_downloaded = html.len();
        let request = ChunkDocumentRequest::new(ChunkSource::Html(html));
        let ChunkDocumentResponse { outcome, telemetry } = self
            .service
            .chunk_document(request)
            .await
            .map_err(|err| RagError::Chunking(err.to_string()))?;

        let mut documents = Vec::new();
        for (idx, chunk) in outcome.chunks.iter().enumerate() {
            let Some(embedding) = chunk.embedding.as_ref() else {
                debug!("chunk_without_embedding" = %chunk.id, "Skipping chunk without embedding");
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

        let chunk_count = documents.len();
        self.store.add_chunks(documents).await?;

        Ok(PageResult {
            chunk_count,
            bytes_downloaded,
            telemetry,
        })
    }

    async fn load_or_fetch(&self, url: &Url) -> Result<String, RagError> {
        if let Some(cache_dir) = &self.options.cache_dir {
            let cache_path = self.cache_path(cache_dir, url);
            if cache_path.exists() {
                return Ok(fs::read_to_string(cache_path).await?);
            }
            let html = self.fetch_html(url).await?;
            if let Some(parent) = cache_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::write(&cache_path, &html).await?;
            return Ok(html);
        }
        self.fetch_html(url).await
    }

    async fn fetch_html(&self, url: &Url) -> Result<String, RagError> {
        info!("url" = %url, "Fetching chapter");
        let response = self
            .client
            .get(url.clone())
            .send()
            .await?
            .error_for_status()
            .map_err(|err| RagError::Network(err.to_string()))?
            .text()
            .await?;
        Ok(response)
    }

    fn cache_path(&self, base: &Path, url: &Url) -> PathBuf {
        let mut file = url.path().trim_start_matches('/').replace('/', "_");
        if file.is_empty() {
            file = "index".to_string();
        }
        if let Some(query) = url.query() {
            file.push('_');
            file.push_str(&query.replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
        }
        base.join(format!("{}.html", file))
    }
}

struct PageResult {
    chunk_count: usize,
    bytes_downloaded: usize,
    telemetry: ChunkTelemetry,
}