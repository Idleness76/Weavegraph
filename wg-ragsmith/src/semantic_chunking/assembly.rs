use crate::semantic_chunking::config::ChunkingConfig;
use crate::semantic_chunking::tokenizer;
use crate::semantic_chunking::types::{
    CandidateSegment, ChunkingStats, ChunkingTrace, SegmentMetadata, SemanticChunk, TraceEvent,
};

/// Prepare chunk ranges and trace data based on breakpoint detection results.
pub fn plan_ranges(
    segments: &[CandidateSegment],
    scores: &[f32],
    breakpoints: &[usize],
    cfg: &ChunkingConfig,
    fallback_used: bool,
) -> (Vec<(usize, usize)>, ChunkingTrace) {
    let mut trace_events = Vec::new();
    for (idx, score) in scores.iter().enumerate() {
        trace_events.push(TraceEvent::new("distance", Some(*score), Some(idx)));
    }
    for idx in breakpoints {
        trace_events.push(TraceEvent::new("breakpoint", None, Some(*idx)));
    }
    if fallback_used {
        trace_events.push(TraceEvent::new("lexical_fallback", None, None));
    }

    let mut sorted_breakpoints = breakpoints.to_vec();
    sorted_breakpoints.sort_unstable();
    sorted_breakpoints.dedup();

    let mut ranges = Vec::new();
    let mut start = 0;
    for &point in &sorted_breakpoints {
        if point <= start || point >= segments.len() {
            continue;
        }
        ranges.push((start, point));
        start = point;
    }
    if start < segments.len() {
        ranges.push((start, segments.len()));
    }
    if ranges.is_empty() {
        ranges.push((0, segments.len()));
    }

    let mut adjusted: Vec<(usize, usize)> = Vec::new();
    for range in ranges {
        let tokens = range_tokens(segments, range);
        if tokens < cfg.min_tokens {
            if let Some(last) = adjusted.last_mut() {
                last.1 = range.1;
                continue;
            }
        }
        adjusted.push(range);
    }

    let adjusted_len = adjusted.len();
    if adjusted_len > 1 {
        if let Some(last_range) = adjusted.last().cloned() {
            if range_tokens(segments, last_range) < cfg.min_tokens {
                let prev_index = adjusted_len - 2;
                if let Some(prev) = adjusted.get_mut(prev_index) {
                    prev.1 = last_range.1;
                }
                adjusted.pop();
            }
        }
    }

    let mut bounded: Vec<(usize, usize)> = Vec::new();
    let mut max_split_indices = Vec::new();
    let max_tokens = cfg.max_tokens.max(1);

    for range in adjusted.into_iter() {
        let mut cursor = range.0;
        let mut acc = 0usize;
        for (offset, segment) in segments[range.0..range.1].iter().enumerate() {
            let idx = range.0 + offset;
            let segment_tokens = segment.tokens;
            if acc > 0 && acc + segment_tokens > max_tokens && cursor < idx {
                bounded.push((cursor, idx));
                max_split_indices.push(idx);
                cursor = idx;
                acc = 0;
            }

            acc += segment_tokens;

            if acc >= max_tokens {
                let split = idx + 1;
                bounded.push((cursor, split));
                max_split_indices.push(split);
                cursor = split;
                acc = 0;
            }
        }

        if cursor < range.1 {
            bounded.push((cursor, range.1));
        }
    }

    if bounded.is_empty() {
        bounded.push((0, segments.len()));
    }

    let mut final_ranges: Vec<(usize, usize)> = Vec::new();
    for range in bounded.into_iter() {
        if let Some(last) = final_ranges.last_mut() {
            let current_tokens = range_tokens(segments, range);
            if current_tokens < cfg.min_tokens {
                let previous_tokens = range_tokens(segments, *last);
                if previous_tokens + current_tokens <= cfg.max_tokens {
                    last.1 = range.1;
                    continue;
                }
            }
        }
        final_ranges.push(range);
    }

    if final_ranges.is_empty() {
        final_ranges.push((0, segments.len()));
    }

    for index in max_split_indices {
        trace_events.push(TraceEvent::new("max_token_split", None, Some(index)));
    }

    (
        final_ranges,
        ChunkingTrace {
            events: trace_events,
        },
    )
}

/// Attach forward/backward pointers between adjacent chunks.
pub fn link_neighbors(chunks: &mut [SemanticChunk]) {
    if chunks.len() > 1 {
        for i in 0..chunks.len() - 1 {
            let next_id = chunks[i + 1].id;
            let current_id = chunks[i].id;
            chunks[i].next_ids.push(next_id);
            chunks[i + 1].prev_ids.push(current_id);
        }
    }
}

/// Compute aggregate statistics for assembled chunks.
pub fn compute_stats(chunks: &[SemanticChunk], total_segments: usize) -> ChunkingStats {
    let total_chunks = chunks.len();
    let token_sum: usize = chunks.iter().map(|chunk| chunk.tokens).sum();
    let average_tokens = if total_chunks == 0 {
        0.0
    } else {
        token_sum as f32 / total_chunks as f32
    };

    ChunkingStats {
        total_segments,
        total_chunks,
        average_tokens,
    }
}

/// Sum tokenizer counts across a slice of segments.
pub fn smooth_scores(scores: &[f32], window: Option<usize>) -> Vec<f32> {
    let w = window.unwrap_or(1).max(1);
    if w <= 1 || scores.is_empty() {
        return scores.to_vec();
    }
    let mut smoothed = Vec::with_capacity(scores.len());
    let half = w / 2;
    for i in 0..scores.len() {
        let start = i.saturating_sub(half);
        let end = (i + half + 1).min(scores.len());
        let slice = &scores[start..end];
        let sum: f32 = slice.iter().copied().sum();
        smoothed.push(sum / slice.len() as f32);
    }
    smoothed
}

pub fn range_tokens(segments: &[CandidateSegment], range: (usize, usize)) -> usize {
    segments[range.0..range.1]
        .iter()
        .map(|segment| segment.tokens)
        .sum()
}

/// Rough token estimator for combined strings (utility for callers assembling custom text).
pub fn combine_text_with_tokens(parts: &[&str]) -> (String, usize) {
    let mut text = String::new();
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            text.push_str("\n\n");
        }
        text.push_str(part.trim());
    }
    let tokens = tokenizer::count(&text);
    (text, tokens)
}

/// Compute structural break contribution for adjacent segments based on path/depth/kind differences.
pub fn structural_distance<F>(
    prev: &SegmentMetadata,
    next: &SegmentMetadata,
    mut path_differs: F,
) -> f32
where
    F: FnMut(Option<&str>, Option<&str>) -> bool,
{
    let mut score: f32 = 0.0;
    if path_differs(prev.source_path.as_deref(), next.source_path.as_deref()) {
        score += 1.0_f32;
    }
    if prev.depth != next.depth {
        score += 0.5_f32;
    }
    if prev.kind != next.kind {
        score += 0.25_f32;
    }
    score.clamp(0.0, 1.0)
}

/// Extract the top-level JSON path component used for structural comparisons.
pub fn json_top_level_component(path: Option<&str>) -> Option<&str> {
    let path = path?;
    path.split('/').find(|part| !part.is_empty())
}

/// Extract the top-level HTML path component used for structural comparisons.
pub fn html_top_level_component(path: Option<&str>) -> Option<&str> {
    let path = path?;
    let segment = path.split('>').next()?.trim();
    if segment.is_empty() {
        return None;
    }
    segment
        .split('/')
        .find_map(|part| {
            let trimmed = part.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .or_else(|| {
            segment
                .split_whitespace()
                .find(|component| !component.is_empty())
        })
}

/// Average a slice of embedding vectors, returning None on mismatch.
pub fn average_embedding(slice: &[Vec<f32>]) -> Option<Vec<f32>> {
    let dims = slice.first()?.len();
    if dims == 0 {
        return None;
    }
    let mut sum = vec![0.0; dims];
    for emb in slice {
        if emb.len() != dims {
            return None;
        }
        for (idx, value) in emb.iter().enumerate() {
            sum[idx] += value;
        }
    }
    let len = slice.len() as f32;
    if len == 0.0 {
        return None;
    }
    for value in &mut sum {
        *value /= len;
    }
    Some(sum)
}
