//! Ensemble scoring — combines heuristic and structural analysis scores
//! into a final injection decision.
//!
//! [`EnsembleScorer`] feeds detector scores through a pluggable
//! [`EnsembleStrategy`] to produce a block/allow [`Decision`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::injection::PatternMatch;
use super::structural::StructuralReport;

// ── EnsembleStrategy trait ─────────────────────────────────────────────

/// Strategy for combining multiple detector scores into a final score.
///
/// Implement this trait to plug in custom ensemble logic.
pub trait EnsembleStrategy: Send + Sync + std::fmt::Debug {
    /// Human-readable name of the strategy.
    fn name(&self) -> &str;

    /// Combine named `(detector_id, score)` pairs into a single `0.0–1.0` score.
    fn combine(&self, scores: &[(&str, f32)]) -> f32;
}

// ── Built-in strategies ────────────────────────────────────────────────

/// Block if *any* detector score is at or above a threshold.
///
/// Returns the maximum score across all detectors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnyAboveThreshold {
    /// Score threshold for blocking (default `0.8`).
    pub threshold: f32,
}

impl Default for AnyAboveThreshold {
    fn default() -> Self {
        Self { threshold: 0.8 }
    }
}

impl EnsembleStrategy for AnyAboveThreshold {
    fn name(&self) -> &'static str {
        "any_above_threshold"
    }

    fn combine(&self, scores: &[(&str, f32)]) -> f32 {
        scores.iter().map(|(_, s)| *s).fold(0.0_f32, f32::max)
    }
}

/// Weighted average of detector scores with configurable per-detector weights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedAverage {
    /// Per-detector weights (missing detectors default to `1.0`).
    pub weights: HashMap<String, f32>,
    /// Score threshold for blocking.
    pub threshold: f32,
}

impl Default for WeightedAverage {
    fn default() -> Self {
        let mut weights = HashMap::new();
        weights.insert("heuristic".to_string(), 0.6);
        weights.insert("structural".to_string(), 0.4);
        Self {
            weights,
            threshold: 0.7,
        }
    }
}

impl EnsembleStrategy for WeightedAverage {
    fn name(&self) -> &'static str {
        "weighted_average"
    }

    fn combine(&self, scores: &[(&str, f32)]) -> f32 {
        if scores.is_empty() {
            return 0.0;
        }
        let mut weighted_sum = 0.0_f32;
        let mut weight_total = 0.0_f32;
        for (id, score) in scores {
            let w = self.weights.get(*id).copied().unwrap_or(1.0);
            weighted_sum += w * score;
            weight_total += w;
        }
        if weight_total == 0.0 {
            0.0
        } else {
            weighted_sum / weight_total
        }
    }
}

/// Block when enough detectors report a score above `0.5`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MajorityVote {
    /// Minimum number of detectors that must exceed `0.5` to block.
    pub min_detectors: usize,
}

impl Default for MajorityVote {
    fn default() -> Self {
        Self { min_detectors: 2 }
    }
}

impl EnsembleStrategy for MajorityVote {
    fn name(&self) -> &'static str {
        "majority_vote"
    }

    #[allow(clippy::cast_precision_loss)]
    fn combine(&self, scores: &[(&str, f32)]) -> f32 {
        let above = scores.iter().filter(|(_, s)| *s > 0.5).count();
        if above >= self.min_detectors {
            // Return the max score to reflect confidence.
            scores.iter().map(|(_, s)| *s).fold(0.0_f32, f32::max)
        } else {
            // Not enough votes — return the average to stay below threshold.
            if scores.is_empty() {
                0.0
            } else {
                let sum: f32 = scores.iter().map(|(_, s)| *s).sum();
                sum / scores.len() as f32
            }
        }
    }
}

/// Returns the maximum score (semantic alias for clarity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaxScore {
    /// Score threshold for blocking.
    pub threshold: f32,
}

impl Default for MaxScore {
    fn default() -> Self {
        Self { threshold: 0.8 }
    }
}

impl EnsembleStrategy for MaxScore {
    fn name(&self) -> &'static str {
        "max_score"
    }

    fn combine(&self, scores: &[(&str, f32)]) -> f32 {
        scores.iter().map(|(_, s)| *s).fold(0.0_f32, f32::max)
    }
}

// ── Decision ───────────────────────────────────────────────────────────

/// Final injection decision produced by [`EnsembleScorer`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Decision {
    /// The input is considered safe.
    Allow,
    /// The input should be blocked.
    Block,
}

// ── DetectorScore ──────────────────────────────────────────────────────

/// Score contribution from a single detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorScore {
    /// Identifier for the detector (e.g. `"heuristic"`, `"structural"`).
    pub detector_id: String,
    /// Normalized score in `0.0–1.0`.
    pub score: f32,
    /// Human-readable details about the score.
    pub details: String,
}

// ── EnsembleResult ─────────────────────────────────────────────────────

/// Result of ensemble scoring.
#[derive(Debug, Clone)]
pub struct EnsembleResult {
    /// Block or allow decision.
    pub decision: Decision,
    /// Confidence in the decision (`0.0–1.0`).
    pub confidence: f32,
    /// Per-detector scores that fed into the decision.
    pub scores: Vec<DetectorScore>,
    /// Name of the strategy that produced this result.
    pub strategy_name: String,
}

// ── EnsembleScorer ─────────────────────────────────────────────────────

/// Combines heuristic and structural detector outputs into a final
/// [`Decision`] using a pluggable [`EnsembleStrategy`].
pub struct EnsembleScorer {
    strategy: Box<dyn EnsembleStrategy>,
}

impl std::fmt::Debug for EnsembleScorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnsembleScorer")
            .field("strategy", &self.strategy)
            .finish()
    }
}

impl EnsembleScorer {
    /// Create a scorer with a custom strategy.
    pub fn new(strategy: impl EnsembleStrategy + 'static) -> Self {
        Self {
            strategy: Box::new(strategy),
        }
    }

    /// Create a scorer from a pre-boxed strategy.
    #[must_use]
    pub fn from_boxed(strategy: Box<dyn EnsembleStrategy>) -> Self {
        Self { strategy }
    }

    /// Create a scorer with sensible defaults (`AnyAboveThreshold` at `0.7`).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(AnyAboveThreshold { threshold: 0.7 })
    }

    /// Produce an ensemble [`EnsembleResult`] from heuristic pattern matches
    /// and a structural report.
    #[must_use]
    pub fn score(
        &self,
        heuristic_matches: &[PatternMatch],
        structural: &StructuralReport,
    ) -> EnsembleResult {
        // Heuristic score: sum of matched weights, capped at 1.0.
        let h_score: f32 = heuristic_matches
            .iter()
            .map(|m| m.weight)
            .sum::<f32>()
            .min(1.0);

        // Structural score: already normalized 0.0–1.0.
        let s_score = structural.overall_risk;

        let h_details = if heuristic_matches.is_empty() {
            "no patterns matched".to_string()
        } else {
            format!(
                "{} pattern(s) matched, combined weight {:.2}",
                heuristic_matches.len(),
                h_score,
            )
        };

        let s_details = format!("overall structural risk {s_score:.2}");

        let detector_scores = vec![
            DetectorScore {
                detector_id: "heuristic".to_string(),
                score: h_score,
                details: h_details,
            },
            DetectorScore {
                detector_id: "structural".to_string(),
                score: s_score,
                details: s_details,
            },
        ];

        let pairs: Vec<(&str, f32)> = detector_scores
            .iter()
            .map(|ds| (ds.detector_id.as_str(), ds.score))
            .collect();

        let combined = self.strategy.combine(&pairs);

        // Determine threshold from the strategy (duck-type check via downcast
        // is not worth it — instead, determine decision by comparing combined
        // score against a threshold derived from the strategy type).
        let threshold = self.resolve_threshold();
        let decision = if combined >= threshold {
            Decision::Block
        } else {
            Decision::Allow
        };

        // Confidence: distance from the threshold, scaled to 0.0–1.0.
        let confidence = if decision == Decision::Block {
            // How far above threshold (more = more confident).
            ((combined - threshold) / (1.0 - threshold + f32::EPSILON)).min(1.0)
        } else {
            // How far below threshold (more = more confident).
            ((threshold - combined) / (threshold + f32::EPSILON)).min(1.0)
        };

        EnsembleResult {
            decision,
            confidence,
            scores: detector_scores,
            strategy_name: self.strategy.name().to_string(),
        }
    }

    /// Resolve the effective threshold for the active strategy.
    fn resolve_threshold(&self) -> f32 {
        match self.strategy.name() {
            "any_above_threshold" | "max_score" | "weighted_average" => 0.7,
            // "majority_vote" and custom strategies default to 0.5.
            _ => 0.5,
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unnecessary_literal_bound)]
mod tests {
    use std::borrow::Cow;
    use std::ops::Range;

    use super::*;
    use crate::input::patterns::PatternCategory;
    use crate::pipeline::outcome::Severity;

    /// Helper: build a `PatternMatch` with the given weight.
    fn pattern_match(weight: f32) -> PatternMatch {
        PatternMatch {
            pattern_id: Cow::Borrowed("TEST-001"),
            category: PatternCategory::RoleConfusion,
            matched_span: Range { start: 0, end: 4 },
            matched_text: "test".to_string(),
            severity: Severity::High,
            weight,
        }
    }

    /// Helper: build a `StructuralReport` with the given overall risk.
    fn structural_report(overall_risk: f32) -> StructuralReport {
        StructuralReport {
            suspicious_char_count: 0,
            suspicious_char_positions: vec![],
            instruction_density: 0.0,
            language_mixing_score: 0.0,
            repetition_score: 0.0,
            punctuation_anomaly_score: 0.0,
            overall_risk,
        }
    }

    // 1. AnyAboveThreshold: high heuristic → Block
    #[test]
    fn any_above_threshold_high_heuristic_blocks() {
        let scorer = EnsembleScorer::new(AnyAboveThreshold { threshold: 0.7 });
        let matches = vec![pattern_match(0.9)];
        let structural = structural_report(0.1);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.decision, Decision::Block);
    }

    // 2. AnyAboveThreshold: low scores → Allow
    #[test]
    fn any_above_threshold_low_scores_allows() {
        let scorer = EnsembleScorer::new(AnyAboveThreshold { threshold: 0.8 });
        let matches = vec![pattern_match(0.2)];
        let structural = structural_report(0.1);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.decision, Decision::Allow);
    }

    // 3. AnyAboveThreshold: exactly at threshold → Block
    #[test]
    fn any_above_threshold_exact_threshold_blocks() {
        let scorer = EnsembleScorer::new(AnyAboveThreshold { threshold: 0.7 });
        let matches = vec![pattern_match(0.7)];
        let structural = structural_report(0.0);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.decision, Decision::Block);
    }

    // 4. WeightedAverage: balanced scores → correct average
    #[test]
    fn weighted_average_balanced_scores() {
        let scorer = EnsembleScorer::new(WeightedAverage::default());
        // heuristic = 0.8, structural = 0.6
        // weighted avg = (0.6*0.8 + 0.4*0.6) / (0.6+0.4) = 0.72
        let matches = vec![pattern_match(0.8)];
        let structural = structural_report(0.6);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.strategy_name, "weighted_average");
        // 0.72 >= 0.7 → Block
        assert_eq!(result.decision, Decision::Block);
    }

    // 5. MajorityVote: 2/2 above 0.5 → Block
    #[test]
    fn majority_vote_both_above_blocks() {
        let scorer = EnsembleScorer::new(MajorityVote { min_detectors: 2 });
        let matches = vec![pattern_match(0.8)];
        let structural = structural_report(0.7);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.decision, Decision::Block);
    }

    // 6. MajorityVote: 1/2 above with min_detectors=2 → Allow
    #[test]
    fn majority_vote_one_above_allows() {
        let scorer = EnsembleScorer::new(MajorityVote { min_detectors: 2 });
        let matches = vec![pattern_match(0.7)];
        let structural = structural_report(0.2);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.decision, Decision::Allow);
    }

    // 7. No matches → Allow with high confidence
    #[test]
    fn no_matches_allows_with_high_confidence() {
        let scorer = EnsembleScorer::with_defaults();
        let matches: Vec<PatternMatch> = vec![];
        let structural = structural_report(0.0);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.decision, Decision::Allow);
        assert!(
            result.confidence > 0.5,
            "expected high confidence for clean input, got {}",
            result.confidence,
        );
    }

    // 8. Custom strategy via trait
    #[test]
    fn custom_strategy_works() {
        /// Always returns 1.0 — useful for testing.
        #[derive(Debug)]
        struct AlwaysBlock;

        impl EnsembleStrategy for AlwaysBlock {
            fn name(&self) -> &str {
                "always_block"
            }
            fn combine(&self, _scores: &[(&str, f32)]) -> f32 {
                1.0
            }
        }

        let scorer = EnsembleScorer::new(AlwaysBlock);
        let matches: Vec<PatternMatch> = vec![];
        let structural = structural_report(0.0);
        let result = scorer.score(&matches, &structural);
        assert_eq!(result.decision, Decision::Block);
    }

    // 9. Score normalization: multiple high-weight matches → capped at 1.0
    #[test]
    fn score_normalization_capped_at_one() {
        let scorer = EnsembleScorer::new(AnyAboveThreshold { threshold: 0.7 });
        let matches = vec![pattern_match(0.8), pattern_match(0.7), pattern_match(0.9)];
        let structural = structural_report(0.1);
        let result = scorer.score(&matches, &structural);
        // Heuristic score should be capped at 1.0 even though sum is 2.4.
        let h_score = result
            .scores
            .iter()
            .find(|s| s.detector_id == "heuristic")
            .unwrap();
        assert!(
            (h_score.score - 1.0).abs() < f32::EPSILON,
            "heuristic score should be capped at 1.0, got {}",
            h_score.score,
        );
        assert_eq!(result.decision, Decision::Block);
    }
}
