//! Resume helpers for long-running ingestion jobs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::fs;
use tokio::sync::Mutex;
use url::Url;

use crate::types::RagError;

/// Tracks which URLs have already been processed so ingest jobs can resume.
#[derive(Clone, Debug)]
pub struct ResumeTracker {
    path: PathBuf,
    state: Arc<Mutex<HashSet<String>>>,
}

impl ResumeTracker {
    /// Creates a new tracker that persists state to the provided path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            state: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Path where the tracker will persist processed URLs.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads previously persisted state, if any.
    pub async fn load(&self) -> Result<(), RagError> {
        if !self.path.exists() {
            return Ok(());
        }
        let data = fs::read_to_string(&self.path).await?;
        let urls: Vec<String> =
            serde_json::from_str(&data).map_err(|err| RagError::Io(err.to_string()))?;
        let mut guard = self.state.lock().await;
        guard.clear();
        guard.extend(urls);
        Ok(())
    }

    /// Returns `true` if the given URL has already been processed.
    pub async fn contains(&self, url: &Url) -> bool {
        let guard = self.state.lock().await;
        guard.contains(url.as_str())
    }

    /// Marks a URL as processed and persists the updated state.
    pub async fn mark_processed(&self, url: &Url) -> Result<(), RagError> {
        let mut guard = self.state.lock().await;
        let inserted = guard.insert(url.as_str().to_string());
        if !inserted && self.path.exists() {
            return Ok(());
        }
        let urls: Vec<String> = guard.iter().cloned().collect();
        drop(guard);

        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }
        let serialized =
            serde_json::to_string(&urls).map_err(|err| RagError::Io(err.to_string()))?;
        fs::write(&self.path, serialized).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn tracker_persists_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let tracker = ResumeTracker::new(&path);
        tracker.load().await.unwrap();

        let url = Url::parse("https://example.com/chapter").unwrap();
        assert!(!tracker.contains(&url).await);

        tracker.mark_processed(&url).await.unwrap();
        assert!(tracker.contains(&url).await);

        let tracker_two = ResumeTracker::new(&path);
        tracker_two.load().await.unwrap();
        assert!(tracker_two.contains(&url).await);
    }
}
