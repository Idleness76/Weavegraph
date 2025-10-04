use async_trait::async_trait;
use rig::embeddings::embedding::{EmbeddingModel, EmbeddingModelDyn};
use std::any::type_name;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::types::ChunkingError;

/// Abstract embedding provider used by semantic chunkers.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, ChunkingError>;

    fn identify(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn max_batch_size(&self) -> usize {
        usize::MAX
    }
}

/// Shared reference type alias for embedding providers.
pub type SharedEmbeddingProvider = Arc<dyn EmbeddingProvider>;

/// Deterministic embeddings used for tests and offline runs.
#[derive(Clone, Default)]
pub struct MockEmbeddingProvider;

impl MockEmbeddingProvider {
    pub fn new() -> Self {
        Self
    }

    fn hash_to_vector(input: &str) -> Vec<f32> {
        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        let seed = hasher.finish();
        // Produce a small deterministic vector by mixing the hash.
        (0..8)
            .map(|i| {
                let bits = seed.rotate_left(i * 8) ^ ((i as u64) << 32);
                (bits as f32) / u32::MAX as f32
            })
            .collect()
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, ChunkingError> {
        Ok(inputs
            .iter()
            .map(|text| Self::hash_to_vector(text))
            .collect())
    }
}

/// Adapter that bridges a RIG [`EmbeddingModel`] into the local [`EmbeddingProvider`] trait.
pub struct RigEmbeddingProvider {
    model: Arc<dyn EmbeddingModelDyn>,
    label: String,
}

impl RigEmbeddingProvider {
    /// Construct from a concrete RIG embedding model instance.
    pub fn from_model<M>(model: M) -> Self
    where
        M: EmbeddingModel + 'static,
    {
        let label = type_name::<M>().to_string();
        let arc = Arc::new(model);
        let dyn_arc: Arc<dyn EmbeddingModelDyn> = arc;
        Self::from_dyn(dyn_arc, Some(label))
    }

    /// Construct from a trait object handle. Optional label defaults to the trait object's type name.
    pub fn from_dyn(model: Arc<dyn EmbeddingModelDyn>, label: Option<String>) -> Self {
        let label = label.unwrap_or_else(|| "rig-embedding".to_string());
        Self { model, label }
    }

    /// Returns the model label used for telemetry.
    pub fn model_label(&self) -> &str {
        &self.label
    }
}

#[async_trait]
impl EmbeddingProvider for RigEmbeddingProvider {
    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, ChunkingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let embeddings = self
            .model
            .embed_texts(inputs.to_vec())
            .await
            .map_err(|err| ChunkingError::EmbeddingFailed {
                reason: err.to_string(),
            })?;

        Ok(embeddings
            .into_iter()
            .map(|embedding| {
                embedding
                    .vec
                    .into_iter()
                    .map(|value| value as f32)
                    .collect()
            })
            .collect())
    }

    fn identify(&self) -> &'static str {
        "rig"
    }

    fn max_batch_size(&self) -> usize {
        self.model.max_documents()
    }
}

/// Embedding provider that always fails, allowing chunkers to fall back to lexical scoring.
#[derive(Default)]
pub struct NullEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for NullEmbeddingProvider {
    async fn embed_batch(&self, _inputs: &[String]) -> Result<Vec<Vec<f32>>, ChunkingError> {
        Err(ChunkingError::EmbeddingFailed {
            reason: "embedding provider not configured".to_string(),
        })
    }

    fn identify(&self) -> &'static str {
        "null"
    }

    fn max_batch_size(&self) -> usize {
        0
    }
}
