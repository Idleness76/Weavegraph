pub mod grouper;
pub mod preprocess;

use async_trait::async_trait;
use serde_json::json;
use tracing::Instrument;

use grouper::{group_blocks, HtmlBlock};
use preprocess::sanitize_html;

use crate::semantic_chunking::assembly::{
    average_embedding, compute_stats, html_top_level_component, link_neighbors, plan_ranges,
    structural_distance,
};
use crate::semantic_chunking::breakpoints::detect_breakpoints;
use crate::semantic_chunking::cache::CacheHandle;
use crate::semantic_chunking::config::{ChunkingConfig, HtmlPreprocessConfig};
use crate::semantic_chunking::embeddings::SharedEmbeddingProvider;
use crate::semantic_chunking::segmenter;
use crate::semantic_chunking::tokenizer;
use crate::semantic_chunking::types::{
    CandidateSegment, ChunkMetadata, ChunkingError, ChunkingOutcome, ChunkingStats, ChunkingTrace,
    SegmentKind, SegmentMetadata, SemanticChunk,
};
use crate::semantic_chunking::SemanticChunker;

/// Semantic chunker for HTML documents.
pub struct HtmlSemanticChunker {
    embedder: SharedEmbeddingProvider,
    preprocess: HtmlPreprocessConfig,
    cache: CacheHandle,
}

struct AssemblyInputs<'a> {
    segments: &'a [CandidateSegment],
    embeddings: Option<&'a [Vec<f32>]>,
    cfg: &'a ChunkingConfig,
    scores: &'a [f32],
    breakpoints: &'a [usize],
    fallback_used: bool,
    document_title: Option<&'a String>,
}

impl HtmlSemanticChunker {
    pub fn new(embedder: SharedEmbeddingProvider, preprocess: HtmlPreprocessConfig) -> Self {
        Self {
            embedder,
            preprocess,
            cache: CacheHandle::new(),
        }
    }

    pub fn with_cache_capacity(self, capacity: usize) -> Self {
        self.cache.apply_capacity(Some(capacity));
        self
    }

    pub fn embedder(&self) -> &SharedEmbeddingProvider {
        &self.embedder
    }

    pub fn preprocess_config(&self) -> &HtmlPreprocessConfig {
        &self.preprocess
    }

    fn configure_cache(&self, cfg: &ChunkingConfig) {
        self.cache.apply_capacity(cfg.cache_capacity);
    }

    pub fn with_cache_handle(mut self, cache: CacheHandle) -> Self {
        self.cache = cache;
        self
    }

    pub fn cache_handle(&self) -> CacheHandle {
        self.cache.clone()
    }

    fn structural_scores(&self, segments: &[CandidateSegment]) -> Vec<f32> {
        if segments.len() < 2 {
            return Vec::new();
        }
        segments
            .windows(2)
            .map(|window| {
                let base =
                    structural_distance(&window[0].metadata, &window[1].metadata, |lhs, rhs| {
                        html_top_level_component(lhs) != html_top_level_component(rhs)
                    });
                let heading_bonus = window[0]
                    .metadata
                    .extra
                    .get("heading_chain")
                    .zip(window[1].metadata.extra.get("heading_chain"))
                    .and_then(|(a, b)| a.as_array().zip(b.as_array()))
                    .map(|(a, b)| if a != b { 0.5_f32 } else { 0.0_f32 })
                    .unwrap_or(0.0_f32);
                (base + heading_bonus).clamp(0.0, 1.0)
            })
            .collect()
    }

    async fn embed_segments(
        &self,
        texts: &[String],
        cfg: &ChunkingConfig,
    ) -> Result<Vec<Vec<f32>>, ChunkingError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let cache_handle = self.cache.clone();
        let use_cache = cache_handle.capacity().is_some();

        if use_cache {
            let mut cached: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
            let mut missing: Vec<(usize, String)> = Vec::new();
            {
                let mut guard = cache_handle.lock();
                let cache = guard.as_mut().expect("cache absent despite flag");
                for (idx, text) in texts.iter().enumerate() {
                    if let Some(vector) = cache.get(text) {
                        cached[idx] = Some(vector);
                    } else {
                        missing.push((idx, text.clone()));
                    }
                }
            }

            if !missing.is_empty() {
                let batch_size = cfg.batch_size.max(1);
                let mut embeddings = Vec::with_capacity(missing.len());
                let mut buffer = Vec::new();
                for (_, text) in &missing {
                    buffer.push(text.clone());
                    if buffer.len() == batch_size {
                        let chunk_embeddings =
                            self.embedder.embed_batch(&buffer).await.map_err(|err| {
                                ChunkingError::EmbeddingFailed {
                                    reason: err.to_string(),
                                }
                            })?;
                        embeddings.extend(chunk_embeddings);
                        buffer.clear();
                    }
                }
                if !buffer.is_empty() {
                    let chunk_embeddings =
                        self.embedder.embed_batch(&buffer).await.map_err(|err| {
                            ChunkingError::EmbeddingFailed {
                                reason: err.to_string(),
                            }
                        })?;
                    embeddings.extend(chunk_embeddings);
                }

                {
                    let mut guard = cache_handle.lock();
                    let cache = guard.as_mut().expect("cache absent during update");
                    for ((idx, _), embedding) in missing.into_iter().zip(embeddings.into_iter()) {
                        cache.insert(&texts[idx], embedding.clone());
                        cached[idx] = Some(embedding);
                    }
                }
            }

            return Ok(cached
                .into_iter()
                .map(|entry| entry.expect("cached embedding"))
                .collect());
        }

        let mut embeddings = Vec::with_capacity(texts.len());
        let batch_size = cfg.batch_size.max(1);
        for batch in texts.chunks(batch_size) {
            let chunk_embeddings = self.embedder.embed_batch(batch).await.map_err(|err| {
                ChunkingError::EmbeddingFailed {
                    reason: err.to_string(),
                }
            })?;
            embeddings.extend(chunk_embeddings);
        }
        Ok(embeddings)
    }

    fn cosine_distances(&self, embeddings: &[Vec<f32>]) -> Vec<f32> {
        let mut scores = Vec::new();
        for window in embeddings.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            let score = if a.is_empty() || b.is_empty() {
                1.0
            } else {
                let mut dot = 0.0;
                let mut norm_a = 0.0;
                let mut norm_b = 0.0;
                for (lhs, rhs) in a.iter().zip(b.iter()) {
                    dot += lhs * rhs;
                    norm_a += lhs * lhs;
                    norm_b += rhs * rhs;
                }
                if norm_a == 0.0 || norm_b == 0.0 {
                    1.0
                } else {
                    1.0 - (dot / (norm_a.sqrt() * norm_b.sqrt()))
                }
            };
            scores.push(score);
        }
        scores
    }

    fn lexical_distances(&self, segments: &[CandidateSegment]) -> Vec<f32> {
        segments
            .windows(2)
            .map(|window| jaccard_distance(&window[0].text, &window[1].text))
            .collect()
    }

    fn assemble_chunks(
        &self,
        inputs: AssemblyInputs<'_>,
    ) -> (Vec<SemanticChunk>, ChunkingTrace, ChunkingStats) {
        let (ranges, trace) = plan_ranges(
            inputs.segments,
            inputs.scores,
            inputs.breakpoints,
            inputs.cfg,
            inputs.fallback_used,
        );

        let mut chunks = Vec::new();
        for (idx, (start, end)) in ranges.iter().enumerate() {
            let mut content = String::new();
            let mut dom_paths = Vec::new();
            let mut tags = Vec::new();
            let mut token_total = 0;
            for segment in &inputs.segments[*start..*end] {
                if !content.is_empty() {
                    content.push_str("\n\n");
                }
                content.push_str(segment.text.trim());
                if let Some(path) = &segment.metadata.source_path {
                    dom_paths.push(path.clone());
                }
                token_total += segment.tokens;
                tags.push(format!("{:?}", segment.metadata.kind));
            }

            let mut metadata = ChunkMetadata::default();
            metadata.source_path = dom_paths.first().cloned();
            metadata.dom_path = metadata.source_path.clone();
            if let Some(title) = inputs.document_title {
                metadata.extra.insert("document_title".into(), json!(title));
            }
            metadata.extra.insert("dom_paths".into(), json!(dom_paths));
            metadata.extra.insert("segment_tags".into(), json!(tags));

            if let Some(heading_list) = inputs.segments[*start..*end]
                .iter()
                .find_map(|segment| segment.metadata.extra_heading_chain())
            {
                metadata
                    .extra
                    .insert("heading_chain".into(), json!(heading_list));
            }

            metadata.extra.insert("chunk_index".into(), json!(idx));

            let mut chunk = SemanticChunk::new(content, token_total, metadata);
            if let Some(emb) = inputs
                .embeddings
                .and_then(|all| average_embedding(&all[*start..*end]))
            {
                chunk.embedding = Some(emb);
            }
            chunks.push(chunk);
        }

        link_neighbors(&mut chunks);

        let stats = compute_stats(&chunks, inputs.segments.len());

        (chunks, trace, stats)
    }
}

#[async_trait]
impl SemanticChunker for HtmlSemanticChunker {
    type Source = String;

    async fn chunk(
        &self,
        source: Self::Source,
        cfg: &ChunkingConfig,
    ) -> Result<ChunkingOutcome, ChunkingError> {
        self.configure_cache(cfg);
        let span = tracing::debug_span!("html_chunk", chunker = %self.name());
        async move {
            let sanitized = sanitize_html(&source, &self.preprocess);
            let title = sanitized.title.clone();
            let blocks = group_blocks(sanitized);

            if blocks.is_empty() {
                return Ok(ChunkingOutcome::empty());
            }

        let mut segments = Vec::new();
        let mut position = 0usize;
        for block in &blocks {
            let parts = split_block_text(block, cfg);
            let total_parts = parts.len();

            for (part_index, (text, tokens)) in parts.into_iter().enumerate() {
                let mut metadata = SegmentMetadata {
                    source_path: block.dom_paths.first().cloned(),
                    depth: block.depth,
                    kind: SegmentKind::HtmlBlock,
                    position,
                    extra: std::collections::BTreeMap::new(),
                };
                metadata
                    .extra
                    .insert("heading_chain".into(), json!(block.heading_chain.clone()));
                metadata
                    .extra
                    .insert("dom_paths".into(), json!(block.dom_paths.clone()));
                metadata
                    .extra
                    .insert("tags".into(), json!(block.tags.clone()));
                metadata
                    .extra
                    .insert("position_range".into(), json!(block.position_range));
                if total_parts > 1 {
                    metadata.extra.insert(
                        "segment_part".into(),
                        json!({
                            "index": part_index,
                            "total": total_parts,
                        }),
                    );
                }

                segments.push(CandidateSegment::new(text, tokens, metadata));
                position += 1;
            }
        }

            let texts: Vec<String> = segments
                .iter()
                .map(|segment| segment.text.clone())
                .collect();
            let mut fallback_used = false;
            let embeddings = match self.embed_segments(&texts, cfg).await {
                Ok(embeddings) => {
                    if embeddings.len() == segments.len() {
                        Some(embeddings)
                    } else {
                        fallback_used = true;
                        None
                    }
                }
                Err(err) => {
                    if cfg.fallback_to_lexical {
                        fallback_used = true;
                        tracing::debug!(error = %err, "embedding failed, falling back to lexical scoring");
                        None
                    } else {
                        return Err(err);
                    }
                }
            };


let base_scores = if let Some(embed) = embeddings.as_ref() {
    self.cosine_distances(embed)
} else {
    self.lexical_distances(&segments)
};
let structural_weight = cfg.structural_break_weight.clamp(0.0, 1.0);
let mut combined_scores = base_scores.clone();
if structural_weight > 0.0 {
    let structural = self.structural_scores(&segments);
    combined_scores = base_scores
        .iter()
        .zip(structural.iter())
        .map(|(base, structure)| {
            ((1.0 - structural_weight) * base) + (structural_weight * structure)
        })
        .collect();
}
let smoothed_scores = crate::semantic_chunking::assembly::smooth_scores(
    &combined_scores,
    cfg.score_smoothing_window,
);

let breakpoints = detect_breakpoints(&smoothed_scores, cfg);
let inputs = AssemblyInputs {
    segments: &segments,
    embeddings: embeddings.as_deref(),
    cfg,
    scores: &smoothed_scores,
    breakpoints: &breakpoints,
    fallback_used,
    document_title: title.as_ref(),
};
let (chunks, trace, stats) = self.assemble_chunks(inputs);

Ok(ChunkingOutcome {
    chunks,
    trace: Some(trace),
    stats,
})
        }
        .instrument(span)
        .await
    }
}

fn jaccard_distance(a: &str, b: &str) -> f32 {
    let tokens_a: std::collections::HashSet<String> = a
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let tokens_b: std::collections::HashSet<String> = b
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let intersection = tokens_a.intersection(&tokens_b).count() as f32;
    let union = tokens_a.union(&tokens_b).count() as f32;
    if union == 0.0 {
        1.0
    } else {
        1.0 - (intersection / union)
    }
}

trait HeadingMetadataExt {
    fn extra_heading_chain(&self) -> Option<Vec<String>>;
}

impl HeadingMetadataExt for SegmentMetadata {
    fn extra_heading_chain(&self) -> Option<Vec<String>> {
        self.extra
            .get("heading_chain")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }
}

fn split_block_text(block: &HtmlBlock, cfg: &ChunkingConfig) -> Vec<(String, usize)> {
    let max_tokens = cfg.max_tokens.max(1);
    let text = block.text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    if block.is_heading {
        return chunk_text_by_tokens(text, max_tokens)
            .into_iter()
            .filter_map(|chunk| {
                let tokens = tokenizer::count(&chunk);
                (tokens > 0).then_some((chunk, tokens))
            })
            .collect();
    }

    let mut segments = Vec::new();
    let mut buffer = String::new();

    let sentences = segmenter::split_sentences(text, cfg.sentence_splitter.clone());
    let sources: Vec<String> = if sentences.is_empty() {
        vec![text.to_string()]
    } else {
        sentences
    };

    for sentence in sources {
        for piece in chunk_sentence(&sentence, max_tokens) {
            push_piece(&piece, max_tokens, &mut buffer, &mut segments);
        }
    }

    flush_segment_buffer(&mut buffer, &mut segments);

    if segments.is_empty() {
        let tokens = tokenizer::count(text);
        if tokens > 0 {
            segments.push((text.to_string(), tokens));
        }
    }

    segments
}

fn chunk_sentence(sentence: &str, max_tokens: usize) -> Vec<String> {
    let trimmed = sentence.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let tokens = tokenizer::count(trimmed);
    if tokens == 0 {
        return Vec::new();
    }
    if tokens > max_tokens {
        chunk_text_by_tokens(trimmed, max_tokens)
    } else {
        vec![trimmed.to_string()]
    }
}

fn chunk_text_by_tokens(text: &str, max_tokens: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current, word)
        };
        let tokens = tokenizer::count(&candidate);

        if !current.is_empty() && tokens > max_tokens {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                chunks.push(trimmed.to_string());
            }
            current.clear();
            current.push_str(word);
            if tokenizer::count(&current) > max_tokens {
                chunks.push(current.trim().to_string());
                current.clear();
            }
        } else {
            current = candidate;
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        chunks.push(trimmed.to_string());
    }

    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks
    }
}

fn push_piece(
    piece: &str,
    max_tokens: usize,
    buffer: &mut String,
    segments: &mut Vec<(String, usize)>,
) {
    let trimmed = piece.trim();
    if trimmed.is_empty() {
        return;
    }

    let candidate = if buffer.is_empty() {
        trimmed.to_string()
    } else {
        format!("{} {}", buffer, trimmed)
    };
    let candidate_tokens = tokenizer::count(&candidate);

    if !buffer.is_empty() && candidate_tokens > max_tokens {
        flush_segment_buffer(buffer, segments);
        let piece_tokens = tokenizer::count(trimmed);
        if piece_tokens > max_tokens {
            segments.push((trimmed.to_string(), piece_tokens));
        } else {
            buffer.push_str(trimmed);
            let tokens = tokenizer::count(buffer);
            if tokens >= max_tokens {
                flush_segment_buffer(buffer, segments);
            }
        }
    } else {
        *buffer = candidate;
        if candidate_tokens >= max_tokens {
            flush_segment_buffer(buffer, segments);
        }
    }
}

fn flush_segment_buffer(buffer: &mut String, segments: &mut Vec<(String, usize)>) {
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        buffer.clear();
        return;
    }
    let tokens = tokenizer::count(trimmed);
    if tokens > 0 {
        segments.push((trimmed.to_string(), tokens));
    }
    buffer.clear();
}
