use crate::semantic_chunking::config::{BreakpointStrategy, ChunkingConfig};

/// Public entry point used by chunkers to turn semantic distance scores into breakpoints.
pub fn detect_breakpoints(scores: &[f32], cfg: &ChunkingConfig) -> Vec<usize> {
    StrategyBreakpointDetector::new(cfg.strategy.clone()).detect(scores, cfg)
}

/// Strategy-backed detector that applies simple statistical rules.
struct StrategyBreakpointDetector {
    strategy: BreakpointStrategy,
}

impl StrategyBreakpointDetector {
    fn new(strategy: BreakpointStrategy) -> Self {
        Self { strategy }
    }

    fn detect(&self, scores: &[f32], _cfg: &ChunkingConfig) -> Vec<usize> {
        if scores.is_empty() {
            return Vec::new();
        }

        match &self.strategy {
            BreakpointStrategy::Percentile { threshold } => {
                percentile_breakpoints(scores, *threshold)
            }
            BreakpointStrategy::StdDev { factor } => stddev_breakpoints(scores, *factor),
            BreakpointStrategy::Interquartile { factor } => iqr_breakpoints(scores, *factor),
            BreakpointStrategy::Gradient { percentile } => {
                gradient_breakpoints(scores, *percentile)
            }
        }
        .into_iter()
        .filter(|idx| *idx < scores.len() + 1)
        .collect()
    }
}

fn percentile_breakpoints(scores: &[f32], threshold: f32) -> Vec<usize> {
    if scores.is_empty() {
        return Vec::new();
    }
    let pct = threshold.clamp(0.0, 1.0);
    let mut sorted = scores.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let cutoff_idx = ((sorted.len() - 1) as f32 * pct).round() as usize;
    let cutoff = sorted
        .get(cutoff_idx)
        .copied()
        .unwrap_or(sorted[sorted.len() - 1]);
    scores
        .iter()
        .enumerate()
        .filter_map(|(idx, score)| {
            if *score >= cutoff {
                Some(idx + 1)
            } else {
                None
            }
        })
        .collect()
}

fn stddev_breakpoints(scores: &[f32], factor: f32) -> Vec<usize> {
    if scores.is_empty() {
        return Vec::new();
    }
    let mean = scores.iter().copied().sum::<f32>() / scores.len() as f32;
    let variance = scores
        .iter()
        .map(|score| {
            let delta = score - mean;
            delta * delta
        })
        .sum::<f32>()
        / scores.len() as f32;
    let std_dev = variance.sqrt();
    let cutoff = mean + std_dev * factor.max(0.0);
    scores
        .iter()
        .enumerate()
        .filter_map(|(idx, score)| {
            if *score >= cutoff {
                Some(idx + 1)
            } else {
                None
            }
        })
        .collect()
}

fn iqr_breakpoints(scores: &[f32], factor: f32) -> Vec<usize> {
    if scores.len() < 4 {
        return percentile_breakpoints(scores, 0.75);
    }
    let mut sorted = scores.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q1_idx = sorted.len() / 4;
    let q3_idx = (sorted.len() * 3) / 4;
    let q1 = sorted[q1_idx];
    let q3 = sorted[q3_idx];
    let iqr = q3 - q1;
    let cutoff = q3 + iqr * factor.max(0.0);
    scores
        .iter()
        .enumerate()
        .filter_map(|(idx, score)| {
            if *score >= cutoff {
                Some(idx + 1)
            } else {
                None
            }
        })
        .collect()
}

fn gradient_breakpoints(scores: &[f32], percentile: f32) -> Vec<usize> {
    if scores.len() < 2 {
        return Vec::new();
    }
    let gradients: Vec<f32> = scores
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .collect();
    percentile_breakpoints(&gradients, percentile)
        .into_iter()
        .map(|idx| idx + 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_breakpoints_respects_threshold() {
        let scores = vec![0.1, 0.5, 0.2, 0.9];
        let cfg = ChunkingConfig {
            strategy: BreakpointStrategy::Percentile { threshold: 0.75 },
            ..ChunkingConfig::default()
        };
        let result = detect_breakpoints(&scores, &cfg);
        assert!(!result.is_empty());
    }
}
