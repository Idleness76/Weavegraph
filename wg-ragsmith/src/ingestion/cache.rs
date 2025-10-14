//! Helpers for fetching and caching source documents prior to chunking.

use std::path::{Path, PathBuf};

use reqwest::Client;
use tokio::fs;
use url::Url;

use crate::types::RagError;

/// Filesystem-backed cache for downloaded documents.
///
/// The cache normalizes URLs into deterministic file names so repeated runs can
/// reuse previously downloaded pages instead of hitting the network.
#[derive(Clone, Debug)]
pub struct DocumentCache {
    root: PathBuf,
}

impl DocumentCache {
    /// Creates a cache rooted at the provided path.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the cache root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Computes the cache file path for a specific URL.
    pub fn cache_path(&self, url: &Url) -> PathBuf {
        let mut components: Vec<String> = url
            .path()
            .trim_start_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(sanitize_component)
            .collect();

        if components.is_empty() {
            components.push("index".to_string());
        }

        let mut file_name = components.join("_");

        if let Some(query) = url.query() {
            file_name.push('_');
            file_name.push_str(&sanitize_component(query));
        }

        if Path::new(&file_name).extension().is_none() {
            file_name.push_str(".html");
        }

        self.root.join(file_name)
    }

    /// Default path for persisting ingestion state (e.g., resume tracking).
    pub fn state_file(&self) -> PathBuf {
        self.root.join("ingest_state.json")
    }
}

/// Result of fetching a document, indicating whether it came from the cache.
#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub url: Url,
    pub content: String,
    pub bytes: usize,
    pub cache_path: Option<PathBuf>,
    pub from_cache: bool,
}

/// Fetches the document behind `url`, optionally persisting it in `cache`.
///
/// When a cache entry already exists the contents are loaded from disk and no
/// network request is performed.
pub async fn fetch_html(
    client: &Client,
    url: &Url,
    cache: Option<&DocumentCache>,
) -> Result<FetchOutcome, RagError> {
    if let Some(cache) = cache {
        let cache_path = cache.cache_path(url);
        if cache_path.exists() {
            let content = fs::read_to_string(&cache_path).await?;
            let bytes = content.len();
            return Ok(FetchOutcome {
                url: url.clone(),
                content,
                bytes,
                cache_path: Some(cache_path),
                from_cache: true,
            });
        }

        let content = fetch_html_from_network(client, url).await?;
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&cache_path, &content).await?;

        let bytes = content.len();
        return Ok(FetchOutcome {
            url: url.clone(),
            content,
            bytes,
            cache_path: Some(cache_path),
            from_cache: false,
        });
    }

    let content = fetch_html_from_network(client, url).await?;
    let bytes = content.len();
    Ok(FetchOutcome {
        url: url.clone(),
        content,
        bytes,
        cache_path: None,
        from_cache: false,
    })
}

async fn fetch_html_from_network(client: &Client, url: &Url) -> Result<String, RagError> {
    let response = client.get(url.clone()).send().await?.error_for_status()?;
    Ok(response.text().await?)
}

fn sanitize_component(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;
    use tempfile::tempdir;

    #[tokio::test]
    async fn cache_path_sanitizes_segments() {
        let cache = DocumentCache::new("tmp");
        let url = Url::parse("https://example.com/foo/bar?chapter=1&lang=en").unwrap();
        let path = cache.cache_path(&url);
        assert!(path.ends_with("foo_bar_chapter_1_lang_en.html"));
    }

    #[tokio::test]
    async fn fetch_uses_cache_when_available() {
        let dir = tempdir().unwrap();
        let cache = DocumentCache::new(dir.path());
        let url = Url::parse("https://example.com/cache").unwrap();
        let cache_path = cache.cache_path(&url);
        if let Some(parent) = cache_path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&cache_path, "cached html").await.unwrap();

        let client = match panic::catch_unwind(|| Client::builder().use_rustls_tls().build()) {
            Ok(Ok(client)) => client,
            _ => return,
        };

        let outcome = fetch_html(&client, &url, Some(&cache)).await.unwrap();
        assert_eq!(outcome.content, "cached html");
        assert!(outcome.from_cache);
    }
}
