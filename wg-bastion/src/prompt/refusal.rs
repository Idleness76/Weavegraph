//! [`RefusalPolicy`] — configurable response modes for blocking outcomes
//! with per-severity mapping.
//!
//! When a guardrail stage produces a blocking [`StageOutcome`], the refusal
//! policy determines *how* the block is communicated back to the caller:
//! hard block, content redaction, a canned safe response, or escalation to
//! human review.

use crate::pipeline::outcome::{Severity, StageOutcome};
use crate::pipeline::stage::SecurityContext;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::SystemTime;

// ── RefusalMode ────────────────────────────────────────────────────────

/// How a blocking outcome should be communicated to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum RefusalMode {
    /// Hard block with a status message.
    Block {
        /// Message returned to the caller.
        status_message: String,
    },
    /// Replace sensitive content with a redaction marker.
    Redact {
        /// Text that replaces the sensitive content.
        replacement_text: String,
        /// Marker indicating redaction occurred.
        redaction_marker: String,
    },
    /// Return a canned safe response from a template.
    SafeResponse {
        /// Template with `{{reason_category}}` placeholder.
        template: String,
    },
    /// Escalate to human review.
    Escalate {
        /// Optional channel/queue for the escalation notification.
        notify_channel: Option<String>,
    },
}

impl fmt::Display for RefusalMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Block { .. } => write!(f, "block"),
            Self::Redact { .. } => write!(f, "redact"),
            Self::SafeResponse { .. } => write!(f, "safe_response"),
            Self::Escalate { .. } => write!(f, "escalate"),
        }
    }
}

// ── RefusalConfig ──────────────────────────────────────────────────────

/// Configuration for a [`RefusalPolicy`].
///
/// Use the builder pattern via [`RefusalConfig::builder()`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RefusalConfig {
    /// Mode used when no severity-specific override exists.
    pub default_mode: RefusalMode,
    /// Per-severity overrides.
    #[serde(default)]
    pub severity_overrides: HashMap<Severity, RefusalMode>,
    /// Whether to generate audit entries for refusals.
    #[serde(default = "default_audit_enabled")]
    pub audit_enabled: bool,
}

fn default_audit_enabled() -> bool {
    true
}

impl Default for RefusalConfig {
    fn default() -> Self {
        Self {
            default_mode: RefusalMode::Block {
                status_message: "Request blocked for security reasons".into(),
            },
            severity_overrides: HashMap::new(),
            audit_enabled: true,
        }
    }
}

impl RefusalConfig {
    /// Start building a [`RefusalConfig`].
    #[must_use]
    pub fn builder() -> RefusalConfigBuilder {
        RefusalConfigBuilder::default()
    }
}

// ── RefusalConfigBuilder ───────────────────────────────────────────────

/// Builder for [`RefusalConfig`].
#[derive(Debug, Default)]
pub struct RefusalConfigBuilder {
    config: RefusalConfig,
}

impl RefusalConfigBuilder {
    /// Set the default refusal mode.
    #[must_use]
    pub fn default_mode(mut self, mode: RefusalMode) -> Self {
        self.config.default_mode = mode;
        self
    }

    /// Add a severity-specific override.
    #[must_use]
    pub fn severity_override(mut self, severity: Severity, mode: RefusalMode) -> Self {
        self.config.severity_overrides.insert(severity, mode);
        self
    }

    /// Enable or disable audit entry generation.
    #[must_use]
    pub fn audit_enabled(mut self, enabled: bool) -> Self {
        self.config.audit_enabled = enabled;
        self
    }

    /// Build the config.
    #[must_use]
    pub fn build(self) -> RefusalConfig {
        self.config
    }
}

// ── AuditEntry ─────────────────────────────────────────────────────────

/// Audit record for a refusal action.
///
/// The `reason_hash` field contains a deterministic hash of the original
/// reason text — never the raw content — to support correlation without
/// logging potentially sensitive data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuditEntry {
    /// When the refusal occurred.
    pub timestamp: SystemTime,
    /// Session that triggered the refusal.
    pub session_id: Option<String>,
    /// User that triggered the refusal.
    pub user_id: Option<String>,
    /// Stage that produced the blocking outcome.
    pub stage_id: String,
    /// Severity of the detected threat.
    pub severity: Severity,
    /// Variant name of the refusal mode applied.
    pub refusal_mode: String,
    /// Deterministic hash of the reason (NOT raw content).
    pub reason_hash: String,
}

// ── RefusalAction ──────────────────────────────────────────────────────

/// The concrete action produced by applying a [`RefusalPolicy`] to a
/// blocking [`StageOutcome`].
#[derive(Debug, Clone)]
pub struct RefusalAction {
    /// The refusal mode that was applied.
    pub mode: RefusalMode,
    /// Severity from the original blocking outcome.
    pub original_severity: Severity,
    /// The actual response text to send to the user.
    pub response_text: String,
    /// Audit record, if auditing is enabled.
    pub audit_entry: Option<AuditEntry>,
}

// ── Reason categories ──────────────────────────────────────────────────

/// Predefined reason categories for safe-response template rendering.
///
/// Only these categories are interpolated — no user content is ever
/// inserted into templates.
const CATEGORY_SECURITY_VIOLATION: &str = "security_violation";
const CATEGORY_CONTENT_POLICY: &str = "content_policy";
const CATEGORY_INJECTION_DETECTED: &str = "injection_detected";
const CATEGORY_PROMPT_LEAKAGE: &str = "prompt_leakage";
const CATEGORY_UNKNOWN: &str = "unknown";

/// Map a blocking reason string to a predefined category.
fn classify_reason(reason: &str) -> &'static str {
    let lower = reason.to_lowercase();
    if lower.contains("inject") {
        CATEGORY_INJECTION_DETECTED
    } else if lower.contains("leak") || lower.contains("prompt") {
        CATEGORY_PROMPT_LEAKAGE
    } else if lower.contains("policy") || lower.contains("content") {
        CATEGORY_CONTENT_POLICY
    } else if lower.contains("security") || lower.contains("secret") || lower.contains("threat") {
        CATEGORY_SECURITY_VIOLATION
    } else {
        CATEGORY_UNKNOWN
    }
}

/// Render a template by replacing `{{reason_category}}` with the category.
fn render_template(template: &str, category: &str) -> String {
    template.replace("{{reason_category}}", category)
}

/// Deterministic hash of a reason string (for audit, not cryptographic).
fn hash_reason(reason: &str) -> String {
    let mut hasher = DefaultHasher::new();
    reason.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ── RefusalPolicy ──────────────────────────────────────────────────────

/// Maps blocking [`StageOutcome`]s to concrete [`RefusalAction`]s based
/// on a [`RefusalConfig`].
#[derive(Debug, Clone)]
pub struct RefusalPolicy {
    config: RefusalConfig,
}

impl RefusalPolicy {
    /// Create a policy from the given config.
    #[must_use]
    pub fn new(config: RefusalConfig) -> Self {
        Self { config }
    }

    /// Create a policy with sensible defaults:
    ///
    /// - `Low` → Redact
    /// - `Medium` → `SafeResponse`
    /// - `High` / `Critical` → Block
    #[must_use]
    pub fn with_defaults() -> Self {
        let config = RefusalConfig::builder()
            .severity_override(
                Severity::Low,
                RefusalMode::Redact {
                    replacement_text: "[REDACTED]".into(),
                    redaction_marker: "content_redacted".into(),
                },
            )
            .severity_override(
                Severity::Medium,
                RefusalMode::SafeResponse {
                    template: "I'm unable to process this request due to {{reason_category}}."
                        .into(),
                },
            )
            .severity_override(
                Severity::High,
                RefusalMode::Block {
                    status_message: "Request blocked: high severity threat detected".into(),
                },
            )
            .severity_override(
                Severity::Critical,
                RefusalMode::Block {
                    status_message: "Request blocked: critical severity threat detected".into(),
                },
            )
            .build();
        Self { config }
    }

    /// Resolve the [`RefusalMode`] for a given severity.
    ///
    /// Returns the severity-specific override if one exists, otherwise the
    /// default mode.
    #[must_use]
    pub fn resolve_mode(&self, severity: &Severity) -> &RefusalMode {
        self.config
            .severity_overrides
            .get(severity)
            .unwrap_or(&self.config.default_mode)
    }

    /// Apply this policy to a [`StageOutcome`].
    ///
    /// Returns `None` for non-blocking outcomes (Allow, Transform, Skip).
    /// For Block outcomes, returns a [`RefusalAction`] with the appropriate
    /// mode, response text, and optional audit entry.
    #[must_use]
    pub fn apply(&self, outcome: &StageOutcome, ctx: &SecurityContext) -> Option<RefusalAction> {
        let (reason, severity) = match outcome {
            StageOutcome::Block { reason, severity } => (reason.as_str(), *severity),
            _ => return None,
        };

        let mode = self.resolve_mode(&severity).clone();
        let category = classify_reason(reason);

        let response_text = match &mode {
            RefusalMode::Block { status_message } => status_message.clone(),
            RefusalMode::Redact {
                replacement_text, ..
            } => replacement_text.clone(),
            RefusalMode::SafeResponse { template } => render_template(template, category),
            RefusalMode::Escalate { .. } => {
                "Your request has been escalated for human review.".into()
            }
        };

        let audit_entry = if self.config.audit_enabled {
            Some(AuditEntry {
                timestamp: SystemTime::now(),
                session_id: Some(ctx.session_id().to_owned()),
                user_id: ctx.user_id().map(str::to_owned),
                stage_id: "refusal_policy".into(),
                severity,
                refusal_mode: mode.to_string(),
                reason_hash: hash_reason(reason),
            })
        } else {
            None
        };

        if self.config.audit_enabled {
            tracing::warn!(
                severity = %severity,
                refusal_mode = %mode,
                reason_hash = %hash_reason(reason),
                "refusal policy applied"
            );
        }

        Some(RefusalAction {
            mode,
            original_severity: severity,
            response_text,
            audit_entry,
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SecurityContext {
        SecurityContext::builder()
            .session_id("test-session")
            .user_id("user-42")
            .risk_score(0.0)
            .build()
    }

    fn block_outcome(reason: &str, severity: Severity) -> StageOutcome {
        StageOutcome::block(reason, severity)
    }

    // 1. Block mode returns correct status message
    #[test]
    fn block_mode_returns_status_message() {
        let config = RefusalConfig::builder()
            .default_mode(RefusalMode::Block {
                status_message: "Nope".into(),
            })
            .build();
        let policy = RefusalPolicy::new(config);
        let outcome = block_outcome("threat detected", Severity::High);
        let action = policy.apply(&outcome, &ctx()).unwrap();
        assert_eq!(action.response_text, "Nope");
        assert!(matches!(action.mode, RefusalMode::Block { .. }));
    }

    // 2. Redact mode returns replacement text
    #[test]
    fn redact_mode_returns_replacement_text() {
        let config = RefusalConfig::builder()
            .default_mode(RefusalMode::Redact {
                replacement_text: "[REMOVED]".into(),
                redaction_marker: "redacted".into(),
            })
            .build();
        let policy = RefusalPolicy::new(config);
        let outcome = block_outcome("secret found", Severity::Low);
        let action = policy.apply(&outcome, &ctx()).unwrap();
        assert_eq!(action.response_text, "[REMOVED]");
        assert!(matches!(action.mode, RefusalMode::Redact { .. }));
    }

    // 3. SafeResponse mode renders template with reason_category
    #[test]
    fn safe_response_renders_template() {
        let config = RefusalConfig::builder()
            .default_mode(RefusalMode::SafeResponse {
                template: "Blocked: {{reason_category}}".into(),
            })
            .build();
        let policy = RefusalPolicy::new(config);
        let outcome = block_outcome("injection attempt", Severity::Medium);
        let action = policy.apply(&outcome, &ctx()).unwrap();
        assert_eq!(action.response_text, "Blocked: injection_detected");
    }

    // 4. Escalate mode creates proper action
    #[test]
    fn escalate_mode_creates_action() {
        let config = RefusalConfig::builder()
            .default_mode(RefusalMode::Escalate {
                notify_channel: Some("security-team".into()),
            })
            .build();
        let policy = RefusalPolicy::new(config);
        let outcome = block_outcome("suspicious activity", Severity::High);
        let action = policy.apply(&outcome, &ctx()).unwrap();
        assert_eq!(
            action.response_text,
            "Your request has been escalated for human review."
        );
        assert!(matches!(
            action.mode,
            RefusalMode::Escalate {
                notify_channel: Some(ref ch),
            } if ch == "security-team"
        ));
    }

    // 5. Per-severity override: Low→Redact, High→Block
    #[test]
    fn severity_overrides_applied() {
        let config = RefusalConfig::builder()
            .default_mode(RefusalMode::SafeResponse {
                template: "default".into(),
            })
            .severity_override(
                Severity::Low,
                RefusalMode::Redact {
                    replacement_text: "[LOW-REDACT]".into(),
                    redaction_marker: "low".into(),
                },
            )
            .severity_override(
                Severity::High,
                RefusalMode::Block {
                    status_message: "HIGH-BLOCK".into(),
                },
            )
            .build();
        let policy = RefusalPolicy::new(config);

        let low_action = policy
            .apply(&block_outcome("minor leak", Severity::Low), &ctx())
            .unwrap();
        assert_eq!(low_action.response_text, "[LOW-REDACT]");
        assert!(matches!(low_action.mode, RefusalMode::Redact { .. }));

        let high_action = policy
            .apply(&block_outcome("threat", Severity::High), &ctx())
            .unwrap();
        assert_eq!(high_action.response_text, "HIGH-BLOCK");
        assert!(matches!(high_action.mode, RefusalMode::Block { .. }));
    }

    // 6. Allow outcome returns None
    #[test]
    fn allow_outcome_returns_none() {
        let policy = RefusalPolicy::with_defaults();
        let outcome = StageOutcome::allow(0.99);
        assert!(policy.apply(&outcome, &ctx()).is_none());
    }

    // 7. Audit entry created with hashed reason (not raw)
    #[test]
    fn audit_entry_has_hashed_reason() {
        let policy = RefusalPolicy::new(RefusalConfig::default());
        let reason = "secret API key detected in template";
        let outcome = block_outcome(reason, Severity::Critical);
        let action = policy.apply(&outcome, &ctx()).unwrap();

        let audit = action
            .audit_entry
            .as_ref()
            .expect("audit should be present");
        // Reason hash must NOT contain the raw reason text
        assert!(!audit.reason_hash.contains("secret"));
        assert!(!audit.reason_hash.contains("API"));
        // Hash should be a 16-char hex string
        assert_eq!(audit.reason_hash.len(), 16);
        assert!(audit.reason_hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(audit.severity, Severity::Critical);
        assert_eq!(audit.session_id.as_deref(), Some("test-session"));
        assert_eq!(audit.user_id.as_deref(), Some("user-42"));
    }

    // 8. Default policy maps severities correctly
    #[test]
    fn default_policy_severity_mapping() {
        let policy = RefusalPolicy::with_defaults();

        assert!(matches!(
            policy.resolve_mode(&Severity::Low),
            RefusalMode::Redact { .. }
        ));
        assert!(matches!(
            policy.resolve_mode(&Severity::Medium),
            RefusalMode::SafeResponse { .. }
        ));
        assert!(matches!(
            policy.resolve_mode(&Severity::High),
            RefusalMode::Block { .. }
        ));
        assert!(matches!(
            policy.resolve_mode(&Severity::Critical),
            RefusalMode::Block { .. }
        ));
    }

    // 9. Skip outcome returns None
    #[test]
    fn skip_outcome_returns_none() {
        let policy = RefusalPolicy::with_defaults();
        let outcome = StageOutcome::skip("not applicable");
        assert!(policy.apply(&outcome, &ctx()).is_none());
    }

    // 10. Reason classification covers categories
    #[test]
    fn reason_classification() {
        assert_eq!(classify_reason("injection attempt"), "injection_detected");
        assert_eq!(classify_reason("prompt leakage"), "prompt_leakage");
        assert_eq!(classify_reason("content policy"), "content_policy");
        assert_eq!(
            classify_reason("security violation found"),
            "security_violation"
        );
        assert_eq!(classify_reason("something else"), "unknown");
    }

    // 11. Audit disabled produces no entry
    #[test]
    fn audit_disabled_no_entry() {
        let config = RefusalConfig::builder().audit_enabled(false).build();
        let policy = RefusalPolicy::new(config);
        let outcome = block_outcome("threat", Severity::High);
        let action = policy.apply(&outcome, &ctx()).unwrap();
        assert!(action.audit_entry.is_none());
    }
}
