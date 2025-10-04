use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use tracing::Instrument;

use super::assembly::{
    average_embedding, compute_stats, json_top_level_component, link_neighbors, plan_ranges,
    structural_distance,
};
use super::breakpoints::detect_breakpoints;
use super::cache::CacheHandle;
use super::config::{ChunkingConfig, JsonPreprocessConfig};
use super::embeddings::SharedEmbeddingProvider;
use super::segmenter;
use super::tokenizer;
use super::types::{
    CandidateSegment, ChunkMetadata, ChunkingError, ChunkingOutcome, SegmentKind, SegmentMetadata,
    SemanticChunk,
};
use super::SemanticChunker;

const SEGMENT_TARGET_TOKENS: usize = 160;

struct SegmentContext<'cfg, 'segments> {
    cfg: &'cfg ChunkingConfig,
    position: &'segments mut usize,
    segments: &'segments mut Vec<CandidateSegment>,
}

impl<'cfg, 'segments> SegmentContext<'cfg, 'segments> {
    fn new(
        cfg: &'cfg ChunkingConfig,
        position: &'segments mut usize,
        segments: &'segments mut Vec<CandidateSegment>,
    ) -> Self {
        Self {
            cfg,
            position,
            segments,
        }
    }
}

/// Semantic chunker for JSON inputs.
pub struct JsonSemanticChunker {
    embedder: SharedEmbeddingProvider,
    preprocess: JsonPreprocessConfig,
    cache: CacheHandle,
}

impl JsonSemanticChunker {
    pub fn new(embedder: SharedEmbeddingProvider, preprocess: JsonPreprocessConfig) -> Self {
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

    pub fn without_cache(self) -> Self {
        self.cache.disable();
        self
    }

    pub fn with_cache_handle(mut self, cache: CacheHandle) -> Self {
        self.cache = cache;
        self
    }

    pub fn cache_handle(&self) -> CacheHandle {
        self.cache.clone()
    }

    pub fn embedder(&self) -> &SharedEmbeddingProvider {
        &self.embedder
    }

    pub fn preprocess_config(&self) -> &JsonPreprocessConfig {
        &self.preprocess
    }

    fn configure_cache(&self, cfg: &ChunkingConfig) {
        self.cache.apply_capacity(cfg.cache_capacity);
    }

    fn enforce_depth(&self, value: &Value, depth: usize) -> Result<(), ChunkingError> {
        if let Some(max_depth) = self.preprocess.max_depth {
            if depth > max_depth {
                return Err(ChunkingError::InvalidInput {
                    reason: format!("JSON payload exceeds max depth of {}", max_depth),
                });
            }
        }

        match value {
            Value::Array(items) => {
                for item in items {
                    self.enforce_depth(item, depth + 1)?;
                }
            }
            Value::Object(map) => {
                for value in map.values() {
                    self.enforce_depth(value, depth + 1)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn collect_segments(
        &self,
        key: Option<&str>,
        value: &Value,
        path: &str,
        depth: usize,
        ctx: &mut SegmentContext<'_, '_>,
    ) {
        match value {
            Value::Object(map) => {
                self.process_object(key, map, path, depth, ctx);
            }
            Value::Array(items) => {
                self.process_array(key, items, path, depth, ctx);
            }
            Value::String(text) => {
                self.emit_text_segments(key, text, path, depth, ctx);
            }
            Value::Number(_) | Value::Bool(_) | Value::Null => {
                self.emit_scalar_segment(key, value, path, depth, ctx.position, ctx.segments);
            }
        }
    }

    fn process_object(
        &self,
        key: Option<&str>,
        map: &serde_json::Map<String, Value>,
        path: &str,
        depth: usize,
        ctx: &mut SegmentContext<'_, '_>,
    ) {
        let mut scalar_lines = Vec::new();
        for (child_key, child_value) in map {
            let child_path = format!("{}/{}", path, child_key);
            match child_value {
                Value::String(text) => {
                    self.emit_text_segments(Some(child_key), text, &child_path, depth + 1, ctx);
                }
                Value::Object(_) | Value::Array(_) => {
                    self.collect_segments(
                        Some(child_key),
                        child_value,
                        &child_path,
                        depth + 1,
                        ctx,
                    );
                }
                _ => {
                    scalar_lines.push(format!(
                        "{}: {}",
                        child_key,
                        self.format_scalar(child_value)
                    ));
                }
            }
        }

        if !scalar_lines.is_empty() {
            let mut text = scalar_lines.join("\n");
            if let Some(label) = key {
                text = format!("{}\n{}", label, text);
            }
            self.push_segment(
                text,
                SegmentKind::JsonObject,
                Some(path.to_string()),
                depth,
                ctx.position,
                ctx.segments,
            );
        }
    }

    fn process_array(
        &self,
        key: Option<&str>,
        items: &[Value],
        path: &str,
        depth: usize,
        ctx: &mut SegmentContext<'_, '_>,
    ) {
        let iter: Box<dyn Iterator<Item = (usize, &Value)>> =
            if self.preprocess.flatten_large_arrays {
                if let Some(limit) = self.preprocess.array_sample_size {
                    if items.len() > limit {
                        Box::new(items.iter().take(limit).enumerate())
                    } else {
                        Box::new(items.iter().enumerate())
                    }
                } else {
                    Box::new(items.iter().enumerate())
                }
            } else {
                Box::new(items.iter().enumerate())
            };

        let mut scalar_lines = Vec::new();
        for (idx, item) in iter {
            let child_path = format!("{}/{}", path, idx);
            match item {
                Value::String(text) => {
                    let label = key.map(|k| format!("{}[{}]", k, idx));
                    self.emit_text_segments(label.as_deref(), text, &child_path, depth + 1, ctx);
                }
                Value::Object(_) | Value::Array(_) => {
                    self.collect_segments(None, item, &child_path, depth + 1, ctx);
                }
                _ => {
                    scalar_lines.push(format!("[{}]: {}", idx, self.format_scalar(item)));
                }
            }
        }

        if !scalar_lines.is_empty() {
            let header = key
                .map(|k| k.to_string())
                .unwrap_or_else(|| path.to_string());
            let mut text = format!("{}\n{}", header, scalar_lines.join("\n"));
            if items.len() > scalar_lines.len() {
                text.push_str("\n…");
            }
            self.push_segment(
                text,
                SegmentKind::JsonArray,
                Some(path.to_string()),
                depth,
                ctx.position,
                ctx.segments,
            );
        }
    }

    fn emit_scalar_segment(
        &self,
        key: Option<&str>,
        value: &Value,
        path: &str,
        depth: usize,
        position: &mut usize,
        segments: &mut Vec<CandidateSegment>,
    ) {
        let text = match key {
            Some(label) => format!("{}: {}", label, self.format_scalar(value)),
            None => self.format_scalar(value),
        };
        self.push_segment(
            text,
            SegmentKind::JsonValue,
            Some(path.to_string()),
            depth,
            position,
            segments,
        );
    }

    fn emit_text_segments(
        &self,
        key: Option<&str>,
        text: &str,
        path: &str,
        depth: usize,
        ctx: &mut SegmentContext<'_, '_>,
    ) {
        let cfg = ctx.cfg;
        let target = SEGMENT_TARGET_TOKENS.min(cfg.max_tokens.saturating_sub(cfg.min_tokens));
        let target = target.max(cfg.min_tokens.max(64));
        let parts = self.split_text(text, target, cfg);
        for part in parts {
            let segment_text = match key {
                Some(label) => format!("{}: {}", label, part.trim()),
                None => part.trim().to_string(),
            };
            self.push_segment(
                segment_text,
                SegmentKind::JsonValue,
                Some(path.to_string()),
                depth,
                ctx.position,
                ctx.segments,
            );
        }
    }

    fn structural_scores(&self, segments: &[CandidateSegment]) -> Vec<f32> {
        if segments.len() < 2 {
            return Vec::new();
        }
        segments
            .windows(2)
            .map(|window| {
                structural_distance(&window[0].metadata, &window[1].metadata, |lhs, rhs| {
                    json_top_level_component(lhs) != json_top_level_component(rhs)
                })
            })
            .collect()
    }

    fn push_segment(
        &self,
        text: String,
        kind: SegmentKind,
        path: Option<String>,
        depth: usize,
        position: &mut usize,
        segments: &mut Vec<CandidateSegment>,
    ) {
        if text.trim().is_empty() {
            return;
        }
        let tokens = tokenizer::count(&text);
        let metadata = SegmentMetadata {
            source_path: path,
            depth,
            kind,
            position: *position,
            extra: std::collections::BTreeMap::new(),
        };
        *position += 1;
        segments.push(CandidateSegment::new(text, tokens, metadata));
    }

    fn format_scalar(&self, value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            Value::Number(num) => num.to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Null => "null".to_string(),
            Value::Array(items) => format!("array(len={})", items.len()),
            Value::Object(map) => format!("object(len={})", map.len()),
        }
    }

    fn split_text(&self, text: &str, target_tokens: usize, cfg: &ChunkingConfig) -> Vec<String> {
        let sentences = segmenter::split_sentences(text, cfg.sentence_splitter.clone());
        if sentences.is_empty() {
            return vec![text.to_string()];
        }

        let mut segments = Vec::new();
        let mut buffer = String::new();
        for sentence in sentences {
            let trimmed = sentence.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate = if buffer.is_empty() {
                trimmed.to_string()
            } else {
                format!("{} {}", buffer, trimmed)
            };

            if tokenizer::count(&candidate) > target_tokens && !buffer.is_empty() {
                segments.push(buffer.trim().to_string());
                buffer = trimmed.to_string();
            } else {
                buffer = candidate;
            }
        }

        if !buffer.trim().is_empty() {
            segments.push(buffer.trim().to_string());
        }

        if segments.is_empty() {
            vec![text.to_string()]
        } else {
            segments
        }
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
        let use_cache = {
            let guard = cache_handle.lock();
            guard.is_some()
        };

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
            .map(|window| {
                let tokens_a: HashSet<String> = window[0]
                    .text
                    .split_whitespace()
                    .map(|w| w.to_ascii_lowercase())
                    .collect();
                let tokens_b: HashSet<String> = window[1]
                    .text
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
            })
            .collect()
    }

    fn strategy_name(&self, cfg: &ChunkingConfig) -> &'static str {
        match cfg.strategy {
            super::config::BreakpointStrategy::Percentile { .. } => "percentile",
            super::config::BreakpointStrategy::StdDev { .. } => "stddev",
            super::config::BreakpointStrategy::Interquartile { .. } => "interquartile",
            super::config::BreakpointStrategy::Gradient { .. } => "gradient",
        }
    }
}

#[async_trait]
impl SemanticChunker for JsonSemanticChunker {
    type Source = Value;

    async fn chunk(
        &self,
        source: Self::Source,
        cfg: &ChunkingConfig,
    ) -> Result<ChunkingOutcome, ChunkingError> {
        self.enforce_depth(&source, 0)?;
        self.configure_cache(cfg);
        let span = tracing::debug_span!("json_chunk", chunker = %self.name());
        async move {
            let mut segments = Vec::new();
            let mut position = 0usize;
            let mut context = SegmentContext::new(cfg, &mut position, &mut segments);
            self.collect_segments(None, &source, "/", 0, &mut context);

            if segments.is_empty() {
                return Ok(ChunkingOutcome::empty());
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
let (ranges, trace) = plan_ranges(
    &segments,
    &smoothed_scores,
    &breakpoints,
    cfg,
    fallback_used,
);

            let mut chunks = Vec::new();
            for (idx, (start, end)) in ranges.iter().enumerate() {
                let mut content = String::new();
                let mut paths = Vec::new();
                let mut kinds = Vec::new();
                let mut positions = Vec::new();
                let mut token_total = 0;
                for segment in &segments[*start..*end] {
                    if !content.is_empty() {
                        content.push_str("\n\n");
                    }
                    content.push_str(segment.text.trim());
                    if let Some(path) = &segment.metadata.source_path {
                        paths.push(path.clone());
                    }
                    kinds.push(format!("{:?}", segment.metadata.kind));
                    positions.push(segment.metadata.position);
                    token_total += segment.tokens;
                }

                let primary_path = paths.first().cloned();
                let mut metadata = ChunkMetadata {
                    source_path: primary_path.clone(),
                    dom_path: primary_path,
                    ..Default::default()
                };
                metadata
                    .extra
                    .insert("segment_paths".into(), serde_json::json!(paths));
                metadata
                    .extra
                    .insert("segment_kinds".into(), serde_json::json!(kinds));
                metadata
                    .extra
                    .insert("segment_positions".into(), serde_json::json!(positions));
                metadata
                    .extra
                    .insert("strategy".into(), serde_json::json!(self.strategy_name(cfg)));

                let mut chunk = SemanticChunk::new(content, token_total, metadata);
                if let Some(emb) = embeddings
                    .as_ref()
                    .and_then(|all| average_embedding(&all[*start..*end]))
                {
                    chunk.embedding = Some(emb);
                }
                chunk
                    .metadata
                    .extra
                    .insert("chunk_index".into(), serde_json::json!(idx));
                chunks.push(chunk);
            }

            link_neighbors(&mut chunks);
            let stats = compute_stats(&chunks, segments.len());

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
