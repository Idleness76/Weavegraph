//! Outcome types returned by guardrail stages.
//!
//! Every [`GuardrailStage`](super::stage::GuardrailStage) returns a
//! [`StageOutcome`] describing what should happen to the content, plus a
//! [`Severity`] level for audit and alerting.
//!
//! The outcome enum is **non-exhaustive** — future stages may introduce
//! new actions (e.g. `Quarantine`, `RateLimit`).

use super::content::Content;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

// ── Severity ───────────────────────────────────────────────────────────

/// Severity level for security events.
///
/// Ordered from lowest to highest — `Ord` is derived so that comparisons
/// like `severity >= Severity::High` work naturally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational — no action required.
    Info,
    /// Low risk — may warrant logging.
    Low,
    /// Medium risk — warrants investigation.
    Medium,
    /// High risk — should block in most policies.
    High,
    /// Critical — immediate block and incident trigger.
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

// ── StageOutcome ───────────────────────────────────────────────────────

/// The decision a guardrail stage makes about a piece of [`Content`].
///
/// ```rust
/// use wg_bastion::pipeline::outcome::{StageOutcome, Severity};
///
/// // A stage that always allows content:
/// let outcome = StageOutcome::Allow { confidence: 0.99 };
/// assert!(outcome.is_allow());
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StageOutcome {
    /// Allow the content to proceed unchanged.
    ///
    /// `confidence` ∈ \[0.0, 1.0\] indicates how certain the stage is that
    /// the content is safe.  Used for audit trails and ensemble scoring.
    Allow {
        /// Confidence that the content is safe (0.0–1.0).
        confidence: f32,
    },

    /// Block the content entirely — it must not reach the LLM or user.
    Block {
        /// Human-readable reason for blocking.
        reason: String,
        /// Severity of the detected threat.
        severity: Severity,
    },

    /// Transform the content (e.g. PII masking, sanitisation) and let the
    /// modified version proceed.
    Transform {
        /// The transformed content that should replace the original.
        content: Content,
        /// Short description of what was changed.
        description: String,
    },

    /// Escalate for human review — the stage cannot decide on its own.
    ///
    /// The pipeline will pause (up to `timeout`) waiting for an external
    /// approval signal.
    Escalate {
        /// Reason for escalation.
        reason: String,
        /// Maximum time to wait for a decision before falling back.
        timeout: Duration,
    },

    /// The stage has nothing to say — this content is outside its scope.
    ///
    /// Example: an injection detector returning `Skip` on a `ToolResult`
    /// because it only inspects user-facing text.
    Skip {
        /// Why the stage skipped evaluation.
        reason: String,
    },
}

impl StageOutcome {
    /// Returns a short label for this outcome variant.
    ///
    /// Useful for metrics, logging, and audit without exposing payload.
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Allow { .. } => "allow",
            Self::Block { .. } => "block",
            Self::Transform { .. } => "transform",
            Self::Escalate { .. } => "escalate",
            Self::Skip { .. } => "skip",
        }
    }

    /// Returns `true` if the outcome is [`Allow`](Self::Allow).
    #[must_use]
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    /// Returns `true` if the outcome is [`Block`](Self::Block).
    #[must_use]
    pub fn is_block(&self) -> bool {
        matches!(self, Self::Block { .. })
    }

    /// Returns `true` if the outcome is [`Transform`](Self::Transform).
    #[must_use]
    pub fn is_transform(&self) -> bool {
        matches!(self, Self::Transform { .. })
    }

    /// Returns `true` if the outcome is [`Escalate`](Self::Escalate).
    #[must_use]
    pub fn is_escalate(&self) -> bool {
        matches!(self, Self::Escalate { .. })
    }

    /// Returns `true` if the outcome is [`Skip`](Self::Skip).
    #[must_use]
    pub fn is_skip(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }

    /// Convenience constructor for a confident allow.
    ///
    /// # Panics (debug only)
    ///
    /// Debug-asserts that `confidence` is in \[0.0, 1.0\].
    #[must_use]
    pub fn allow(confidence: f32) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&confidence),
            "confidence must be in [0.0, 1.0], got {confidence}",
        );
        Self::Allow { confidence }
    }

    /// Convenience constructor for a block.
    #[must_use]
    pub fn block(reason: impl Into<String>, severity: Severity) -> Self {
        Self::Block {
            reason: reason.into(),
            severity,
        }
    }

    /// Convenience constructor for a skip.
    #[must_use]
    pub fn skip(reason: impl Into<String>) -> Self {
        Self::Skip {
            reason: reason.into(),
        }
    }

    /// Convenience constructor for a transform.
    #[must_use]
    pub fn transform(content: Content, description: impl Into<String>) -> Self {
        Self::Transform {
            content,
            description: description.into(),
        }
    }

    /// Convenience constructor for an escalation.
    #[must_use]
    pub fn escalate(reason: impl Into<String>, timeout: Duration) -> Self {
        Self::Escalate {
            reason: reason.into(),
            timeout,
        }
    }
}

// ── StageError ─────────────────────────────────────────────────────────

/// An error encountered during guardrail stage evaluation.
///
/// This is distinct from a "threat detected" block — it means the stage
/// *could not complete its analysis*.  The pipeline uses the stage's
/// [`degradable()`](super::stage::GuardrailStage::degradable) flag to
/// decide whether to fail-closed or continue with degraded coverage.
#[derive(Debug, Error)]
pub enum StageError {
    /// The stage's backing model or service is unavailable.
    #[error("backend unavailable for stage '{stage}': {reason}")]
    BackendUnavailable {
        /// Stage identifier.
        stage: String,
        /// Human-readable reason.
        reason: String,
    },

    /// The content could not be processed (format mismatch, too large, etc.).
    #[error("invalid content for stage '{stage}': {reason}")]
    InvalidContent {
        /// Stage identifier.
        stage: String,
        /// What went wrong.
        reason: String,
    },

    /// Catch-all for unexpected failures.
    #[error("internal error in stage '{stage}': {source}")]
    Internal {
        /// Stage identifier.
        stage: String,
        /// Underlying error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(Severity::Info < Severity::Low);
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Critical.to_string(), "critical");
    }

    #[test]
    fn outcome_is_methods() {
        assert!(StageOutcome::allow(0.99).is_allow());
        assert!(StageOutcome::block("bad", Severity::High).is_block());
        assert!(StageOutcome::skip("n/a").is_skip());
    }

    #[test]
    fn stage_error_display() {
        let err = StageError::BackendUnavailable {
            stage: "moderation".into(),
            reason: "timeout".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("moderation"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn severity_round_trips_json() {
        let json = serde_json::to_string(&Severity::High).unwrap();
        assert_eq!(json, r#""high""#);
        let parsed: Severity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Severity::High);
    }
}
