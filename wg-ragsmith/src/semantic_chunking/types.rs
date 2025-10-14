use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// A fully assembled semantic chunk ready for downstream retrieval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticChunk {
    pub id: Uuid,
    pub content: String,
    pub tokens: usize,
    pub metadata: ChunkMetadata,
    pub embedding: Option<Vec<f32>>,
    pub prev_ids: Vec<Uuid>,
    pub next_ids: Vec<Uuid>,
}

impl SemanticChunk {
    pub fn new(content: String, tokens: usize, metadata: ChunkMetadata) -> Self {
        Self {
            id: Uuid::new_v4(),
            content,
            tokens,
            metadata,
            embedding: None,
            prev_ids: Vec::new(),
            next_ids: Vec::new(),
        }
    }
}

/// Lightweight metadata that carries provenance and structural hints.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub source_path: Option<String>,
    pub dom_path: Option<String>,
    pub heading_hierarchy: Vec<String>,
    pub attributes: BTreeMap<String, String>,
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Aggregate result returned by a chunker, including optional trace data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkingOutcome {
    pub chunks: Vec<SemanticChunk>,
    pub trace: Option<ChunkingTrace>,
    pub stats: ChunkingStats,
}

impl ChunkingOutcome {
    pub fn empty() -> Self {
        Self {
            chunks: Vec::new(),
            trace: None,
            stats: ChunkingStats::default(),
        }
    }
}

/// Basic runtime stats for diagnostics.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkingStats {
    pub total_segments: usize,
    pub total_chunks: usize,
    pub average_tokens: f32,
}

/// Trace data is useful for debugging breakpoint placement.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkingTrace {
    pub events: Vec<TraceEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceEvent {
    pub label: String,
    pub score: Option<f32>,
    pub index: Option<usize>,
}

impl TraceEvent {
    pub fn new(label: impl Into<String>, score: Option<f32>, index: Option<usize>) -> Self {
        Self {
            label: label.into(),
            score,
            index,
        }
    }
}

/// Candidate segments feed the breakpoint detector before chunk assembly.
#[derive(Clone, Debug)]
pub struct CandidateSegment {
    pub text: String,
    pub tokens: usize,
    pub metadata: SegmentMetadata,
}

impl CandidateSegment {
    pub fn new(text: String, tokens: usize, metadata: SegmentMetadata) -> Self {
        Self {
            text,
            tokens,
            metadata,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SegmentMetadata {
    pub source_path: Option<String>,
    pub depth: usize,
    pub kind: SegmentKind,
    pub position: usize,
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SegmentKind {
    JsonValue,
    JsonObject,
    JsonArray,
    HtmlBlock,
    HtmlInline,
    Unknown,
}

impl Default for SegmentKind {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Errors that a semantic chunker can surface to callers.
#[derive(thiserror::Error, Debug)]
pub enum ChunkingError {
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("embedding failed: {reason}")]
    EmbeddingFailed { reason: String },
    #[error("internal error: {0}")]
    Internal(String),
}
