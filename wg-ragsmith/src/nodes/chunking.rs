//! Semantic chunking node for weavegraph workflows.

use async_trait::async_trait;
use std::sync::Arc;

use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::StateSnapshot;
use weavegraph::utils::collections::new_extra_map;

use crate::semantic_chunking::service::{
    ChunkDocumentRequest, ChunkDocumentResponse, ChunkSource, SemanticChunkingService,
};
use crate::semantic_chunking::types::ChunkingError;

/// Error type for chunking node operations.
#[derive(Debug, thiserror::Error)]
pub enum ChunkingNodeError {
    /// The input key was not found in the state's extra map.
    #[error("input key '{key}' not found in state.extra")]
    InputNotFound { key: String },

    /// The input value could not be parsed as the expected type.
    #[error("input key '{key}' has invalid type: expected {expected}")]
    InvalidInputType { key: String, expected: &'static str },

    /// The chunking operation failed.
    #[error("chunking failed: {0}")]
    ChunkingFailed(#[from] ChunkingError),
}

impl From<ChunkingNodeError> for NodeError {
    fn from(err: ChunkingNodeError) -> Self {
        NodeError::ValidationFailed(err.to_string())
    }
}

/// A weavegraph [`Node`] that performs semantic chunking on documents.
///
/// This node reads a document from the workflow state, chunks it using the
/// configured [`SemanticChunkingService`], and writes the results back to state.
///
/// # Input/Output
///
/// - **Input**: Reads from `state.extra[input_key]` - expects a string (HTML/text) or JSON
/// - **Output**: Writes to `state.extra[output_key]` - JSON array of chunks with metadata
///
/// # Event Emission
///
/// The node emits progress events via the [`NodeContext`]:
/// - `chunking:start` - When chunking begins
/// - `chunking:complete` - When chunking finishes with summary stats
///
/// # Examples
///
/// ```rust,ignore
/// use wg_ragsmith::nodes::ChunkingNode;
/// use wg_ragsmith::service::SemanticChunkingService;
///
/// let service = SemanticChunkingService::builder()
///     .with_rig_model(embedding_model)
///     .build();
///
/// let node = ChunkingNode::builder()
///     .service(service)
///     .input_key("html_content")
///     .output_key("chunks")
///     .build();
/// ```
pub struct ChunkingNode {
    service: Arc<SemanticChunkingService>,
    input_key: String,
    output_key: String,
    emit_events: bool,
}

impl ChunkingNode {
    /// Create a new builder for constructing a `ChunkingNode`.
    pub fn builder() -> ChunkingNodeBuilder {
        ChunkingNodeBuilder::default()
    }

    fn parse_input(&self, snapshot: &StateSnapshot) -> Result<ChunkSource, ChunkingNodeError> {
        let value = snapshot
            .extra
            .get(&self.input_key)
            .ok_or_else(|| ChunkingNodeError::InputNotFound {
                key: self.input_key.clone(),
            })?;

        // Try to interpret the input as different source types
        match value {
            serde_json::Value::String(s) => {
                // Heuristic: if it starts with '<', treat as HTML; otherwise plain text
                if s.trim_start().starts_with('<') {
                    Ok(ChunkSource::Html(s.clone()))
                } else {
                    Ok(ChunkSource::PlainText(s.clone()))
                }
            }
            serde_json::Value::Object(_) => Ok(ChunkSource::Json(value.clone())),
            serde_json::Value::Array(_) => Ok(ChunkSource::Json(value.clone())),
            _ => Err(ChunkingNodeError::InvalidInputType {
                key: self.input_key.clone(),
                expected: "string (HTML/text) or JSON object/array",
            }),
        }
    }
}

#[async_trait]
impl Node for ChunkingNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        // Emit start event
        if self.emit_events {
            let _ = ctx.emit("chunking", format!("Starting chunking from key '{}'", self.input_key));
        }

        // Parse input from state
        let source = self.parse_input(&snapshot)?;

        // Build the chunking request
        let request = ChunkDocumentRequest {
            source,
            chunking_config: None,
            html_config: None,
            json_config: None,
            embedder: None,
        };

        // Execute chunking
        let response: ChunkDocumentResponse = self
            .service
            .chunk_document(request)
            .await
            .map_err(ChunkingNodeError::ChunkingFailed)?;

        // Emit completion event with stats
        if self.emit_events {
            let _ = ctx.emit(
                "chunking",
                format!(
                    "Chunking complete: {} chunks, avg {} tokens, {} cache hits",
                    response.outcome.chunks.len(),
                    response.telemetry.average_tokens,
                    response.telemetry.cache_hits,
                ),
            );
        }

        // Serialize chunks to JSON for storage in state
        // Note: serde_json::Error implements Into<NodeError> via From trait
        let chunks_json = serde_json::to_value(&response.outcome.chunks)?;

        // Build output state
        let mut extra = new_extra_map();
        extra.insert(self.output_key.clone(), chunks_json);

        // Also include telemetry metadata
        let telemetry_json = serde_json::to_value(&response.telemetry)
            .unwrap_or(serde_json::Value::Null);
        extra.insert(format!("{}_telemetry", self.output_key), telemetry_json);

        Ok(NodePartial::new().with_extra(extra))
    }
}

/// Builder for constructing [`ChunkingNode`] instances.
#[derive(Default)]
pub struct ChunkingNodeBuilder {
    service: Option<Arc<SemanticChunkingService>>,
    input_key: Option<String>,
    output_key: Option<String>,
    emit_events: bool,
}

impl ChunkingNodeBuilder {
    /// Set the chunking service to use.
    ///
    /// This is required before calling [`build()`](Self::build).
    #[must_use]
    pub fn service(mut self, service: SemanticChunkingService) -> Self {
        self.service = Some(Arc::new(service));
        self
    }

    /// Set the chunking service from an existing Arc.
    ///
    /// Use this to share a service across multiple nodes.
    #[must_use]
    pub fn service_arc(mut self, service: Arc<SemanticChunkingService>) -> Self {
        self.service = Some(service);
        self
    }

    /// Set the key to read input from in `state.extra`.
    ///
    /// Defaults to `"document"`.
    #[must_use]
    pub fn input_key(mut self, key: impl Into<String>) -> Self {
        self.input_key = Some(key.into());
        self
    }

    /// Set the key to write output to in `state.extra`.
    ///
    /// Defaults to `"chunks"`.
    #[must_use]
    pub fn output_key(mut self, key: impl Into<String>) -> Self {
        self.output_key = Some(key.into());
        self
    }

    /// Enable or disable event emission during chunking.
    ///
    /// Defaults to `true`.
    #[must_use]
    pub fn emit_events(mut self, emit: bool) -> Self {
        self.emit_events = emit;
        self
    }

    /// Build the [`ChunkingNode`].
    ///
    /// # Panics
    ///
    /// Panics if [`service()`](Self::service) was not called.
    pub fn build(self) -> ChunkingNode {
        ChunkingNode {
            service: self.service.expect("ChunkingNodeBuilder requires a service"),
            input_key: self.input_key.unwrap_or_else(|| "document".to_string()),
            output_key: self.output_key.unwrap_or_else(|| "chunks".to_string()),
            emit_events: self.emit_events,
        }
    }

    /// Build the [`ChunkingNode`], returning `None` if service is not set.
    pub fn try_build(self) -> Option<ChunkingNode> {
        Some(ChunkingNode {
            service: self.service?,
            input_key: self.input_key.unwrap_or_else(|| "document".to_string()),
            output_key: self.output_key.unwrap_or_else(|| "chunks".to_string()),
            emit_events: self.emit_events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full integration tests require embedding models.
    // These are basic structural tests.

    #[test]
    fn builder_defaults() {
        // Can't build without service
        let builder = ChunkingNodeBuilder::default();
        assert!(builder.try_build().is_none());
    }
}
