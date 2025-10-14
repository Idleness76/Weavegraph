use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::{self, StreamExt, TryStreamExt};
use miette::{IntoDiagnostic, Result};
use reqwest::Client as HttpClient;
use rig::client::{CompletionClient, EmbeddingsClient};
use rig::completion::{AssistantContent, CompletionModel, Message as CompletionMessage};
use rig::embeddings::EmbeddingModel;
use rig::providers::ollama::Client as OllamaClient;
use rig::vector_store::request::VectorSearchRequest;
use rig::vector_store::VectorStoreIndex;
use scraping_helpers::sanitize_filename;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tokio::fs;
use tokio::time::sleep;
use tokio_rusqlite::Connection;
use url::Url;

use rag_utils::semantic_chunking::service::{ChunkDocumentRequest, ChunkSource};
use rag_utils::semantic_chunking::SemanticChunkingService;
use rag_utils::stores::sqlite::{ChunkDocument, SqliteChunkStore};
#[cfg(test)]
use weavegraph::event_bus::EventBus;
use weavegraph::graphs::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{CheckpointerType, RuntimeConfig};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;
use weavegraph::utils::collections::new_extra_map;

mod scraping_helpers {
    use url::Url;

    pub fn sanitize_filename(url: &Url, index: usize) -> String {
        let mut sanitized = url
            .path()
            .trim_matches('/')
            .replace('/', "_")
            .replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_");
        if sanitized.is_empty() {
            sanitized = "index".to_string();
        }
        format!("{:03}_{}.html", index + 1, sanitized)
    }
}

#[derive(Clone)]
struct PipelineConfig {
    base_url: Url,
    limit: Option<usize>,
    cache_dir: PathBuf,
    db_path: PathBuf,
    concurrency: usize,
    max_retries: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct ScrapedChapterRecord {
    #[serde(default)]
    order: usize,
    url: String,
    path: String,
    bytes: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct ChunkRecord {
    id: String,
    url: String,
    heading: String,
    chunk_index: usize,
    content: String,
    embedding: Vec<f32>,
    metadata: serde_json::Value,
}

struct ScrapeNode {
    client: HttpClient,
    config: Arc<PipelineConfig>,
}

impl ScrapeNode {
    async fn fetch_chapter(
        client: HttpClient,
        config: Arc<PipelineConfig>,
        idx: usize,
        url: Url,
        ctx: NodeContext,
    ) -> std::result::Result<ScrapedChapterRecord, NodeError> {
        let max_attempts = config.max_retries.max(1);
        let mut attempt = 0usize;
        let mut backoff = Duration::from_millis(500);
        let filename = sanitize_filename(&url, idx);
        let path = config.cache_dir.join(filename);

        loop {
            attempt += 1;
            ctx.emit(
                "scrape",
                format!("Fetching {} (attempt {}/{})", url, attempt, max_attempts),
            )?;

            let send_result = client
                .get(url.clone())
                .send()
                .await
                .map_err(|err| provider_error("reqwest", err));

            match send_result.and_then(|response| {
                response
                    .error_for_status()
                    .map_err(|err| provider_error("reqwest", err))
            }) {
                Ok(response) => match response
                    .text()
                    .await
                    .map_err(|err| provider_error("reqwest", err))
                {
                    Ok(html) => {
                        if let Some(parent) = path.parent() {
                            fs::create_dir_all(parent)
                                .await
                                .map_err(|err| provider_error("fs", err))?;
                        }
                        fs::write(&path, &html)
                            .await
                            .map_err(|err| provider_error("fs", err))?;
                        ctx.emit(
                            "scrape",
                            format!("Saved {} ({} bytes)", path.display(), html.len()),
                        )?;
                        return Ok(ScrapedChapterRecord {
                            order: idx,
                            url: url.to_string(),
                            path: path.to_string_lossy().to_string(),
                            bytes: html.len(),
                        });
                    }
                    Err(err) if attempt >= max_attempts => return Err(err),
                    Err(err) => {
                        ctx.emit("scrape", format!("Retrying {} after error: {}", url, err))?;
                        sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(5));
                    }
                },
                Err(err) if attempt >= max_attempts => return Err(err),
                Err(err) => {
                    ctx.emit("scrape", format!("Retrying {} after error: {}", url, err))?;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                }
            }
        }
    }
}

struct ChunkNode {
    service: Arc<SemanticChunkingService>,
    config: Arc<PipelineConfig>,
}

struct StoreNode<E>
where
    E: rig::embeddings::EmbeddingModel + Clone + Send + Sync + 'static,
{
    store: Arc<SqliteChunkStore<E>>,
    config: Arc<PipelineConfig>,
}

struct RetrieveNode<E>
where
    E: rig::embeddings::EmbeddingModel + Clone + Send + Sync + 'static,
{
    store: Arc<SqliteChunkStore<E>>,
    embedding_model: E,
}

struct GenerateAnswerNode;

pub async fn run_demo5() -> Result<()> {
    dotenvy::dotenv().ok();

    let base_url = env::var("RUST_BOOK_BASE_URL")
        .unwrap_or_else(|_| "https://doc.rust-lang.org/book/".to_string());
    let limit = env::var("RUST_BOOK_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    let cache_dir = env::var("RUST_BOOK_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("rust_book_cache"));
    let db_path = env::var("RUST_BOOK_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("rust_book_chunks.sqlite"));
    let query =
        env::var("RUST_BOOK_QUERY").unwrap_or_else(|_| "What is ownership in Rust?".to_string());

    let concurrency = env::var("RUST_BOOK_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4)
        .max(1);

    let pipeline_config = Arc::new(PipelineConfig {
        base_url: base_url.parse().into_diagnostic()?,
        limit,
        cache_dir,
        db_path,
        concurrency,
        max_retries: 3,
    });

    if let Some(parent) = pipeline_config.cache_dir.parent() {
        fs::create_dir_all(parent).await.into_diagnostic()?;
    }
    fs::create_dir_all(&pipeline_config.cache_dir)
        .await
        .into_diagnostic()?;

    if let Some(parent) = pipeline_config.db_path.parent() {
        fs::create_dir_all(parent).await.into_diagnostic()?;
    }

    let http_client = HttpClient::builder()
        .user_agent("weavegraph-RustBook-Demo/0.1")
        .build()
        .into_diagnostic()?;

    let ollama_client = OllamaClient::new();
    let probe_model = ollama_client.embedding_model("nomic-embed-text");
    let probe = probe_model
        .clone()
        .embed_text("dimension probe")
        .await
        .into_diagnostic()?;
    let ndims = probe.vec.len();
    if ndims == 0 {
        return Err(miette::miette!(
            "embedding model 'nomic-embed-text' returned zero dimensions"
        ));
    }
    let embedding_model = ollama_client.embedding_model_with_ndims("nomic-embed-text", ndims);

    let service = Arc::new(
        SemanticChunkingService::builder()
            .with_rig_model(embedding_model.clone())
            .build(),
    );

    let store = Arc::new(
        SqliteChunkStore::open(&pipeline_config.db_path, &embedding_model)
            .await
            .into_diagnostic()?,
    );

    let scrape_node = ScrapeNode {
        client: http_client,
        config: pipeline_config.clone(),
    };
    let chunk_node = ChunkNode {
        service: service.clone(),
        config: pipeline_config.clone(),
    };
    let store_node = StoreNode {
        store: store.clone(),
        config: pipeline_config.clone(),
    };
    let retrieve_node = RetrieveNode {
        store: store.clone(),
        embedding_model: embedding_model.clone(),
    };
    let generate_node = GenerateAnswerNode;

    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("Scrape".into()), scrape_node)
        .add_node(NodeKind::Custom("Chunk".into()), chunk_node)
        .add_node(NodeKind::Custom("Store".into()), store_node)
        .add_node(NodeKind::Custom("Retrieve".into()), retrieve_node)
        .add_node(NodeKind::Custom("GenerateAnswer".into()), generate_node)
        .add_edge(NodeKind::Start, NodeKind::Custom("Scrape".into()))
        .add_edge(
            NodeKind::Custom("Scrape".into()),
            NodeKind::Custom("Chunk".into()),
        )
        .add_edge(
            NodeKind::Custom("Chunk".into()),
            NodeKind::Custom("Store".into()),
        )
        .add_edge(
            NodeKind::Custom("Store".into()),
            NodeKind::Custom("Retrieve".into()),
        )
        .add_edge(
            NodeKind::Custom("Retrieve".into()),
            NodeKind::Custom("GenerateAnswer".into()),
        )
        .add_edge(NodeKind::Custom("GenerateAnswer".into()), NodeKind::End)
        // .set_entry(NodeKind::Start)  // removed: Start is virtual; explicit entry no longer required
        .with_runtime_config(RuntimeConfig {
            session_id: Some("scrape6".to_string()),
            checkpointer: Some(CheckpointerType::SQLite),
            sqlite_db_name: None,
        })
        .compile()?;

    let initial_state = VersionedState::new_with_user_message(&query);

    let final_state = app.invoke(initial_state).await?;

    let snapshot = final_state.snapshot();
    if let Some(last) = snapshot.messages.last() {
        println!("\nAssistant response:\n{}", last.content);
    }

    println!(
        "\nArtifacts:\n  cache_dir: {}\n  db_path: {}",
        pipeline_config.cache_dir.display(),
        pipeline_config.db_path.display()
    );

    Ok(())
}

#[async_trait]
impl Node for ScrapeNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> std::result::Result<NodePartial, NodeError> {
        ctx.emit("scrape", "Fetching Rust book table of contents")?;
        let response = self
            .client
            .get(self.config.base_url.clone())
            .send()
            .await
            .map_err(|err| provider_error("reqwest", err))?
            .text()
            .await
            .map_err(|err| provider_error("reqwest", err))?;

        let mut urls = {
            let document = scraper::Html::parse_document(&response);

            let mut collected = Vec::new();
            let mut seen = std::collections::HashSet::new();

            // Primary selector: ordered lists with chapter items.
            let chapter_selector = scraper::Selector::parse("ol.chapter li.chapter-item a")
                .map_err(|err| NodeError::ValidationFailed(err.to_string()))?;
            for element in document.select(&chapter_selector) {
                if let Some(href) = element.value().attr("href") {
                    if let Ok(mut url) = self.config.base_url.join(href) {
                        url.set_fragment(None);
                        url.set_query(None);
                        if !url.path().ends_with(".html") {
                            continue;
                        }
                        if seen.insert(url.to_string()) {
                            collected.push(url);
                        }
                    }
                }
            }

            // Fallback selectors if the primary did not match.
            if collected.is_empty() {
                let fallback_patterns = [
                    "nav#TOC a",
                    "nav.toc a",
                    "nav ul li a",
                    "nav li a",
                    "ul li a",
                    "ol li a",
                    "a[href]",
                ];
                for pattern in fallback_patterns.iter() {
                    let selector = match scraper::Selector::parse(pattern) {
                        Ok(selector) => selector,
                        Err(_) => continue,
                    };

                    for element in document.select(&selector) {
                        let Some(href) = element.value().attr("href") else {
                            continue;
                        };
                        if href.starts_with('#') {
                            continue;
                        }
                        if let Ok(mut url) = self.config.base_url.join(href) {
                            url.set_fragment(None);
                            url.set_query(None);
                            if !url.path().ends_with(".html") {
                                continue;
                            }
                            if seen.insert(url.to_string()) {
                                collected.push(url);
                            }
                        }
                    }
                    if !collected.is_empty() {
                        break;
                    }
                }
            }

            collected
        };

        if urls.is_empty() {
            return Err(NodeError::ValidationFailed(
                "no chapter links found in table of contents".into(),
            ));
        }
        if let Some(limit) = self.config.limit {
            urls.truncate(limit);
        }
        ctx.emit("scrape", format!("Discovered {} chapters", urls.len()))?;

        fs::create_dir_all(&self.config.cache_dir)
            .await
            .map_err(|err| provider_error("fs", err))?;

        let concurrency = self.config.concurrency.max(1);
        let chapters_future = stream::iter(urls.into_iter().enumerate()).map(|(idx, url)| {
            let client = self.client.clone();
            let config = self.config.clone();
            let ctx = ctx.clone();
            async move { ScrapeNode::fetch_chapter(client, config, idx, url, ctx).await }
        });

        let mut chapters: Vec<ScrapedChapterRecord> = chapters_future
            .buffer_unordered(concurrency)
            .try_collect()
            .await?;

        chapters.sort_by_key(|record| record.order);
        let total_bytes: usize = chapters.iter().map(|record| record.bytes).sum();

        ctx.emit(
            "scrape",
            format!(
                "Downloaded {} chapters ({} KB)",
                chapters.len(),
                total_bytes / 1024
            ),
        )?;

        let index_path = self.config.cache_dir.join("scraped_chapters.json");
        let serialized = serde_json::to_vec_pretty(&chapters)?;
        fs::write(&index_path, serialized)
            .await
            .map_err(|err| provider_error("fs", err))?;

        ctx.emit(
            "scrape",
            format!(
                "Stored {} chapters to {}",
                chapters.len(),
                index_path.display()
            ),
        )?;

        let mut book_obj = serde_json::Map::new();
        book_obj.insert(
            "scraped_index_path".into(),
            json!(index_path.to_string_lossy()),
        );
        book_obj.insert(
            "cache_dir".into(),
            json!(self.config.cache_dir.to_string_lossy()),
        );
        book_obj.insert("chapter_count".into(), json!(chapters.len()));
        book_obj.insert("total_bytes".into(), json!(total_bytes));

        let mut extra = new_extra_map();
        extra.insert("rust_book".into(), serde_json::Value::Object(book_obj));

        Ok(NodePartial {
            messages: None,
            extra: Some(extra),
            errors: None,
        })
    }
}

#[async_trait]
impl Node for ChunkNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> std::result::Result<NodePartial, NodeError> {
        let rust_book = snapshot
            .extra
            .get("rust_book")
            .and_then(|value| value.as_object())
            .cloned()
            .ok_or(NodeError::MissingInput {
                what: "rust_book scrape metadata",
            })?;
        let index_path = rust_book
            .get("scraped_index_path")
            .and_then(|value| value.as_str())
            .ok_or(NodeError::MissingInput {
                what: "scraped_index_path",
            })?;

        let data = fs::read_to_string(index_path)
            .await
            .map_err(|err| provider_error("fs", err))?;
        let mut chapters: Vec<ScrapedChapterRecord> = serde_json::from_str(&data)?;
        if chapters.is_empty() {
            return Err(NodeError::ValidationFailed(
                "scraped chapters list is empty".into(),
            ));
        }
        chapters.sort_by_key(|record| record.order);
        let total_bytes: usize = chapters.iter().map(|record| record.bytes).sum();

        let mut chunk_records = Vec::new();
        let mut total_embeddings = 0usize;
        for chapter in &chapters {
            ctx.emit("chunk", format!("Chunking {}", chapter.url))?;
            let html = fs::read_to_string(&chapter.path)
                .await
                .map_err(|err| provider_error("fs", err))?;
            let response = self
                .service
                .chunk_document(ChunkDocumentRequest::new(ChunkSource::Html(html)))
                .await
                .map_err(|err| NodeError::Provider {
                    provider: "SemanticChunkingService",
                    message: err.to_string(),
                })?;

            let telemetry = &response.telemetry;
            ctx.emit(
                "chunk",
                format!(
                    "telemetry source={} chunks={} avg_tokens={:.2} fallback={} cache_hits={} cache_misses={} strategy={}",
                    telemetry.source,
                    telemetry.chunk_count,
                    telemetry.average_tokens,
                    telemetry.fallback_used,
                    telemetry.cache_hits,
                    telemetry.cache_misses,
                    telemetry.strategy,
                ),
            )?;

            for (idx, chunk) in response.outcome.chunks.iter().enumerate() {
                let Some(embedding) = chunk.embedding.clone() else {
                    continue;
                };
                total_embeddings += 1;
                let heading = if chunk.metadata.heading_hierarchy.is_empty() {
                    String::new()
                } else {
                    chunk.metadata.heading_hierarchy.join(" > ")
                };
                let metadata = serde_json::to_value(&chunk.metadata)?;
                chunk_records.push(ChunkRecord {
                    id: chunk.id.to_string(),
                    url: chapter.url.clone(),
                    heading,
                    chunk_index: idx,
                    content: chunk.content.clone(),
                    embedding,
                    metadata,
                });
            }
        }

        if chunk_records.is_empty() {
            return Err(NodeError::ValidationFailed(
                "chunking produced no embeddings".into(),
            ));
        }

        let chunks_path = self.config.cache_dir.join("chunks.json");
        fs::write(&chunks_path, serde_json::to_vec_pretty(&chunk_records)?)
            .await
            .map_err(|err| provider_error("fs", err))?;

        ctx.emit(
            "chunk",
            format!(
                "Wrote {} chunks with embeddings to {}",
                chunk_records.len(),
                chunks_path.display()
            ),
        )?;

        let mut book_obj = rust_book.clone();
        book_obj.insert(
            "chunks_index_path".into(),
            json!(chunks_path.to_string_lossy()),
        );
        book_obj.insert("total_chunks".into(), json!(chunk_records.len()));
        book_obj.insert("total_embeddings".into(), json!(total_embeddings));
        book_obj.insert("total_bytes".into(), json!(total_bytes));

        let mut extra = new_extra_map();
        extra.insert("rust_book".into(), serde_json::Value::Object(book_obj));

        Ok(NodePartial {
            messages: None,
            extra: Some(extra),
            errors: None,
        })
    }
}

#[async_trait]
impl<E> Node for StoreNode<E>
where
    E: rig::embeddings::EmbeddingModel + Clone + Send + Sync + 'static,
{
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> std::result::Result<NodePartial, NodeError> {
        let rust_book = snapshot
            .extra
            .get("rust_book")
            .and_then(|value| value.as_object())
            .cloned()
            .ok_or(NodeError::MissingInput {
                what: "rust_book chunk metadata",
            })?;
        let chunks_path = rust_book
            .get("chunks_index_path")
            .and_then(|value| value.as_str())
            .ok_or(NodeError::MissingInput {
                what: "chunks_index_path",
            })?;

        ctx.emit("store", format!("Loading chunks from {chunks_path}"))?;
        let data = fs::read_to_string(chunks_path)
            .await
            .map_err(|err| provider_error("fs", err))?;
        let chunk_records: Vec<ChunkRecord> = serde_json::from_str(&data)?;
        if chunk_records.is_empty() {
            return Err(NodeError::ValidationFailed(
                "chunk records list is empty".into(),
            ));
        }

        let existing_map = load_existing_chunk_ids(self.config.db_path.as_path())
            .await
            .map_err(|err| provider_error("sqlite", err))?;

        let mut seen_keys: HashSet<(String, usize)> = HashSet::new();
        let mut replaced = 0usize;
        let mut skipped_duplicates = 0usize;
        let mut documents: Vec<(ChunkDocument, Vec<f32>)> = Vec::new();

        for record in chunk_records.into_iter() {
            let key = (record.url.clone(), record.chunk_index);
            if !seen_keys.insert(key.clone()) {
                skipped_duplicates += 1;
                continue;
            }

            let mut id = record.id;
            if let Some(existing_id) = existing_map.get(&key) {
                if id != *existing_id {
                    id = existing_id.clone();
                }
                replaced += 1;
            }

            let ChunkRecord {
                url,
                heading,
                chunk_index,
                content,
                embedding,
                metadata,
                ..
            } = record;

            let document = ChunkDocument {
                id,
                url,
                heading,
                chunk_index,
                content,
                metadata,
            };
            documents.push((document, embedding));
        }

        if skipped_duplicates > 0 {
            ctx.emit(
                "store",
                format!("Skipped {skipped_duplicates} duplicate chunk records in batch"),
            )?;
        }
        if replaced > 0 {
            ctx.emit(
                "store",
                format!("Updating {replaced} existing chunks with fresh content"),
            )?;
        }

        let stored_count = documents.len();
        ctx.emit(
            "store",
            format!(
                "Persisting {} chunks into {}",
                stored_count,
                self.config.db_path.display()
            ),
        )?;
        self.store
            .add_chunks(documents)
            .await
            .map_err(|err| NodeError::Provider {
                provider: "SqliteChunkStore",
                message: err.to_string(),
            })?;

        let mut book_obj = rust_book.clone();
        book_obj.insert(
            "db_path".into(),
            json!(self.config.db_path.to_string_lossy()),
        );
        book_obj.insert("stored_chunks".into(), json!(stored_count));
        book_obj.insert("replaced_chunks".into(), json!(replaced));
        book_obj.insert("skipped_chunk_duplicates".into(), json!(skipped_duplicates));

        let mut extra = new_extra_map();
        extra.insert("rust_book".into(), serde_json::Value::Object(book_obj));

        Ok(NodePartial {
            messages: None,
            extra: Some(extra),
            errors: None,
        })
    }
}

#[async_trait]
impl<E> Node for RetrieveNode<E>
where
    E: rig::embeddings::EmbeddingModel + Clone + Send + Sync + 'static,
{
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> std::result::Result<NodePartial, NodeError> {
        let query = snapshot
            .messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone())
            .ok_or(NodeError::MissingInput { what: "user query" })?;

        ctx.emit("retrieve", format!("Running vector search for '{query}'"))?;
        let retriever = self.store.index(self.embedding_model.clone());
        let search_request = VectorSearchRequest::builder()
            .query(query.clone())
            .samples(5)
            .build()
            .map_err(|err| NodeError::Provider {
                provider: "VectorStore",
                message: err.to_string(),
            })?;
        let matches = retriever
            .top_n::<ChunkDocument>(search_request)
            .await
            .map_err(|err| NodeError::Provider {
                provider: "VectorStore",
                message: err.to_string(),
            })?;

        let mut matches_summary = Vec::new();
        let mut response = String::new();
        let mut extra = new_extra_map();

        if let Some(rust_book) = snapshot.extra.get("rust_book") {
            extra.insert("rust_book".into(), rust_book.clone());
        }

        if matches.is_empty() {
            response.push_str("Vector search returned no results; showing cached chapters:\n");
            let fallback_records = snapshot
                .extra
                .get("rust_book")
                .and_then(|value| value.as_object())
                .and_then(|map| map.get("chunks_index_path"))
                .and_then(|value| value.as_str())
                .ok_or(NodeError::ValidationFailed(
                    "retriever returned no results".into(),
                ))?;
            let data = fs::read_to_string(fallback_records)
                .await
                .map_err(|err| provider_error("fs", err))?;
            let chunk_records: Vec<ChunkRecord> = serde_json::from_str(&data)?;
            if chunk_records.is_empty() {
                return Err(NodeError::ValidationFailed(
                    "retriever returned no results".into(),
                ));
            }
            for (rank, record) in chunk_records.iter().take(3).enumerate() {
                ctx.emit(
                    "retrieve",
                    format!("Fallback match {} from {}", rank + 1, record.url),
                )?;
                let snippet = record.content.chars().take(200).collect::<String>();
                response.push_str(&format!(
                    "{}. [{}] {}\n{}\n\n",
                    rank + 1,
                    record.heading,
                    record.url,
                    snippet
                ));
                matches_summary.push(json!({
                    "source": "fallback",
                    "url": record.url,
                    "heading": record.heading,
                    "metadata": record.metadata,
                    "preview": snippet,
                }));
            }
        } else {
            response.push_str("Top relevant passages:\n");
            for (rank, (distance, _id, doc)) in matches.iter().take(3).enumerate() {
                ctx.emit(
                    "retrieve",
                    format!("Match {} score {:.4} from {}", rank + 1, distance, doc.url),
                )?;
                response.push_str(&format!(
                    "{}. [{} | {:.4}] {}\n{}\n\n",
                    rank + 1,
                    doc.heading,
                    distance,
                    doc.url,
                    doc.content
                ));
                matches_summary.push(json!({
                    "source": "vector",
                    "score": distance,
                    "url": doc.url,
                    "heading": doc.heading,
                    "metadata": doc.metadata,
                    "preview": doc.content.chars().take(200).collect::<String>(),
                }));
            }
        }

        extra.insert(
            "retrieval".into(),
            json!({
                "query": query,
                "matches": matches_summary,
            }),
        );

        Ok(NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: response,
            }]),
            extra: Some(extra),
            errors: None,
        })
    }
}

#[async_trait]
impl Node for GenerateAnswerNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> std::result::Result<NodePartial, NodeError> {
        let retrieved_content = snapshot.messages.last().ok_or(NodeError::MissingInput {
            what: "retrieval summary message",
        })?;

        let user_query = snapshot
            .messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone())
            .ok_or(NodeError::MissingInput { what: "user query" })?;

        let prompt = format!(
            "Using the following context answer the user query. If you don't know the answer, say so. Answer using the provided context only.
            \n\nContext:\n{}\n\nUser Query: {}\n\nAnswer:",
            retrieved_content.content,
            user_query
        );
        ctx.emit("generate", "Generating final answer with Ollama")?;

        let client = OllamaClient::new();
        let completion_model = client.completion_model("gemma3");
        let completion_request = completion_model
            .completion_request(CompletionMessage::user(prompt))
            .preamble(
                "You are a helpful Rust assistant. Use the provided context to answer the user query."
                    .to_owned(),
            )
            .temperature(0.2)
            .build();

        let response = completion_model
            .completion(completion_request)
            .await
            .map_err(|err| NodeError::Provider {
                provider: "ollama",
                message: err.to_string(),
            })?;

        let mut combined = String::new();
        for content in response.choice.into_iter() {
            if let AssistantContent::Text(text) = content {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(text.text());
            }
        }

        if combined.trim().is_empty() {
            return Err(NodeError::Provider {
                provider: "ollama",
                message: "completion returned no text".into(),
            });
        }

        Ok(NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: combined,
            }]),
            extra: None,
            errors: None,
        })
    }
}

fn provider_error<E: std::fmt::Display>(provider: &'static str, err: E) -> NodeError {
    NodeError::Provider {
        provider,
        message: err.to_string(),
    }
}

async fn load_existing_chunk_ids(
    db_path: &std::path::Path,
) -> Result<HashMap<(String, usize), String>, tokio_rusqlite::Error> {
    let conn = Connection::open(db_path).await?;
    conn.call(|conn| {
        let mut stmt = conn.prepare("SELECT id, url, chunk_index FROM chunks")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let url: String = row.get(1)?;
            let chunk_index: String = row.get(2)?;
            Ok((id, url, chunk_index))
        })?;

        let mut map = HashMap::new();
        for row in rows {
            let (id, url, chunk_index) = row?;
            if let Ok(index) = chunk_index.parse::<usize>() {
                map.insert((url, index), id);
            }
        }
        Ok(map)
    })
    .await
}

#[tokio::main]
async fn main() -> Result<()> {
    run_demo5().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use miette::IntoDiagnostic;
    use rig::embeddings::embedding::{Embedding, EmbeddingError, EmbeddingModel};
    use tempfile::tempdir;

    #[derive(Clone)]
    struct TestEmbeddingModel;

    impl EmbeddingModel for TestEmbeddingModel {
        const MAX_DOCUMENTS: usize = 32;

        fn ndims(&self) -> usize {
            8
        }

        fn embed_texts(
            &self,
            texts: impl IntoIterator<Item = String> + Send,
        ) -> impl std::future::Future<Output = Result<Vec<Embedding>, EmbeddingError>> + Send
        {
            let docs: Vec<String> = texts.into_iter().collect();
            async move {
                let embeddings = docs
                    .into_iter()
                    .map(|document| Embedding {
                        vec: hash_to_vec(&document),
                        document,
                    })
                    .collect();
                Ok(embeddings)
            }
        }
    }

    fn hash_to_vec(_text: &str) -> Vec<f64> {
        vec![0.0; 8]
    }

    #[tokio::test]
    async fn pipeline_runs_against_mock_server() -> Result<()> {
        let server = MockServer::start_async().await;

        let toc_html = r#"
            <html><body>
            <nav><ul>
              <li><a href="chapter1.html">Chapter 1</a></li>
              <li><a href="chapter2.html">Chapter 2</a></li>
            </ul></nav>
            </body></html>
        "#;
        server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200).body(toc_html);
        });

        let chapter1_html = r#"
            <html><body>
            <h1>Ownership</h1>
            <p>Ownership is Rust's model for managing memory safely.</p>
            </body></html>
        "#;
        server.mock(|when, then| {
            when.method(GET).path("/chapter1.html");
            then.status(200).body(chapter1_html);
        });

        let chapter2_html = r#"
            <html><body>
            <h1>Borrowing</h1>
            <p>Borrowing enables references without taking ownership.</p>
            </body></html>
        "#;
        server.mock(|when, then| {
            when.method(GET).path("/chapter2.html");
            then.status(200).body(chapter2_html);
        });

        let cache_dir = tempdir().into_diagnostic()?;
        let db_dir = tempdir().into_diagnostic()?;
        let db_path = db_dir.path().join("chunks.sqlite");

        let pipeline_config = Arc::new(PipelineConfig {
            base_url: Url::parse(&(server.base_url() + "/")).unwrap(),
            limit: None,
            cache_dir: cache_dir.path().to_path_buf(),
            db_path: db_path.clone(),
            concurrency: 2,
            max_retries: 3,
        });

        let http_client = HttpClient::builder().build().unwrap();
        let embedding_model = TestEmbeddingModel;
        let event_bus = EventBus::default();

        let service = Arc::new(
            SemanticChunkingService::builder()
                .with_rig_model(embedding_model.clone())
                .build(),
        );

        let store = Arc::new(
            SqliteChunkStore::open(&pipeline_config.db_path, &embedding_model)
                .await
                .into_diagnostic()?,
        );

        let scrape_node = ScrapeNode {
            client: http_client,
            config: pipeline_config.clone(),
        };
        let chunk_node = ChunkNode {
            service: service.clone(),
            config: pipeline_config.clone(),
        };
        let store_node = StoreNode {
            store: store.clone(),
            config: pipeline_config.clone(),
        };
        let retrieve_node = RetrieveNode {
            store: store.clone(),
            embedding_model: embedding_model.clone(),
        };

        let mut state = VersionedState::new_with_user_message("Explain ownership");
        let sender = event_bus.get_sender();

        let mut step = 1u64;
        let partial = scrape_node
            .run(
                state.snapshot(),
                NodeContext {
                    node_id: "Scrape".into(),
                    step,
                    event_bus_sender: sender.clone(),
                },
            )
            .await
            .into_diagnostic()?;
        apply_partial(&mut state, partial);
        step += 1;

        let partial = chunk_node
            .run(
                state.snapshot(),
                NodeContext {
                    node_id: "Chunk".into(),
                    step,
                    event_bus_sender: sender.clone(),
                },
            )
            .await
            .into_diagnostic()?;
        apply_partial(&mut state, partial);
        step += 1;

        let partial = store_node
            .run(
                state.snapshot(),
                NodeContext {
                    node_id: "Store".into(),
                    step,
                    event_bus_sender: sender.clone(),
                },
            )
            .await
            .into_diagnostic()?;
        apply_partial(&mut state, partial);
        step += 1;

        let partial = retrieve_node
            .run(
                state.snapshot(),
                NodeContext {
                    node_id: "Retrieve".into(),
                    step,
                    event_bus_sender: sender,
                },
            )
            .await
            .into_diagnostic()?;
        apply_partial(&mut state, partial);

        let snapshot = state.snapshot();
        let assistant_reply = snapshot
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .expect("assistant message present");
        assert!(
            assistant_reply.content.contains("Top relevant passages")
                || assistant_reply
                    .content
                    .contains("Vector search returned no results")
        );

        let retrieval = snapshot
            .extra
            .get("retrieval")
            .and_then(|value| value.as_object())
            .expect("retrieval extra present");
        assert!(retrieval
            .get("matches")
            .and_then(|value| value.as_array())
            .map(|matches| !matches.is_empty())
            .unwrap_or(false));

        let cache_index = pipeline_config.cache_dir.join("scraped_chapters.json");
        assert!(cache_index.exists());
        assert!(pipeline_config.cache_dir.join("chunks.json").exists());

        let metadata = fs::metadata(&db_path).await.into_diagnostic()?;
        assert!(metadata.len() > 0);

        Ok(())
    }

    use weavegraph::channels::Channel;

    fn apply_partial(state: &mut VersionedState, partial: NodePartial) {
        if let Some(mut messages) = partial.messages {
            state.messages.get_mut().append(&mut messages);
            let version = state.messages.version().saturating_add(1);
            state.messages.set_version(version);
        }

        if let Some(extra) = partial.extra {
            let map = state.extra.get_mut();
            for (key, value) in extra {
                map.insert(key, value);
            }
            let version = state.extra.version().saturating_add(1);
            state.extra.set_version(version);
        }

        if let Some(mut errors) = partial.errors {
            state.errors.get_mut().append(&mut errors);
            let version = state.errors.version().saturating_add(1);
            state.errors.set_version(version);
        }
    }
}
