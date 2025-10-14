use serde::{Deserialize, Serialize};

use crate::semantic_chunking::segmenter::SentenceSplitter;

/// High-level tuning knobs shared by all semantic chunkers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkingConfig {
    pub strategy: BreakpointStrategy,
    pub max_tokens: usize,
    pub min_tokens: usize,
    pub batch_size: usize,
    pub overlap: Option<OverlapConfig>,
    pub metadata_flags: MetadataFlags,
    pub context_window: usize,
    pub fallback_to_lexical: bool,
    pub cache_capacity: Option<usize>,
    pub sentence_splitter: SentenceSplitter,
    pub structural_break_weight: f32,
    pub score_smoothing_window: Option<usize>,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            strategy: BreakpointStrategy::default(),
            max_tokens: 512,
            min_tokens: 32,
            batch_size: 16,
            overlap: None,
            metadata_flags: MetadataFlags::default(),
            context_window: 1,
            fallback_to_lexical: true,
            cache_capacity: None,
            sentence_splitter: SentenceSplitter::default(),
            structural_break_weight: 0.2,
            score_smoothing_window: None,
        }
    }
}

/// Sliding overlap settings for cross-chunk context.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OverlapConfig {
    pub tokens: usize,
    pub capture_neighbors: bool,
}

impl Default for OverlapConfig {
    fn default() -> Self {
        Self {
            tokens: 32,
            capture_neighbors: true,
        }
    }
}

/// Controls which metadata adornments should be populated.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetadataFlags {
    pub include_source_path: bool,
    pub include_dom_path: bool,
    pub include_heading_hierarchy: bool,
    pub include_extra_debug: bool,
}

impl Default for MetadataFlags {
    fn default() -> Self {
        Self {
            include_source_path: true,
            include_dom_path: true,
            include_heading_hierarchy: true,
            include_extra_debug: false,
        }
    }
}

/// Breakpoint selection strategy tuned by configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BreakpointStrategy {
    Percentile { threshold: f32 },
    StdDev { factor: f32 },
    Interquartile { factor: f32 },
    Gradient { percentile: f32 },
}

impl Default for BreakpointStrategy {
    fn default() -> Self {
        Self::Percentile { threshold: 0.9 }
    }
}

/// Available embedding backends. Concrete clients are wired elsewhere.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EmbeddingBackend {
    Mock,
    OpenAI { model: String },
    Cohere { model: String },
    LocalModel { identifier: String },
}

impl Default for EmbeddingBackend {
    fn default() -> Self {
        Self::Mock
    }
}

/// HTML-specific preprocessing knobs to strip boilerplate before scoring.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HtmlPreprocessConfig {
    pub drop_tags: Vec<String>,
    pub keep_attributes: bool,
    pub preserve_whitespace: bool,
}

impl Default for HtmlPreprocessConfig {
    fn default() -> Self {
        Self {
            drop_tags: vec![
                "script".into(),
                "style".into(),
                "noscript".into(),
                "iframe".into(),
                "nav".into(),
                "footer".into(),
                "aside".into(),
            ],
            keep_attributes: false,
            preserve_whitespace: false,
        }
    }
}

/// JSON-specific preprocessing knobs for flattening and traversal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonPreprocessConfig {
    pub max_depth: Option<usize>,
    pub flatten_large_arrays: bool,
    pub array_sample_size: Option<usize>,
}

impl Default for JsonPreprocessConfig {
    fn default() -> Self {
        Self {
            max_depth: None,
            flatten_large_arrays: true,
            array_sample_size: Some(32),
        }
    }
}

/// Shared module bootstrap configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SemanticChunkingModuleConfig {
    pub embedding_backend: EmbeddingBackend,
    pub chunking: ChunkingConfig,
    pub html: HtmlPreprocessConfig,
    pub json: JsonPreprocessConfig,
}
