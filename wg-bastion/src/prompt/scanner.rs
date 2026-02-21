//! [`TemplateScanner`] — regex + Shannon entropy secret detection for system prompts.
//!
//! Scans prompt templates for accidentally embedded secrets (API keys, JWTs,
//! private key headers, passwords in URLs, etc.) using a compiled [`RegexSet`]
//! plus per-match Shannon entropy filtering.

use crate::pipeline::content::Content;
use crate::pipeline::outcome::{Severity, StageError, StageOutcome};
use crate::pipeline::stage::{GuardrailStage, SecurityContext};
use async_trait::async_trait;
use regex::{Regex, RegexSet};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Range;

// ── PatternCategory ────────────────────────────────────────────────────

/// Category of secret pattern detected by the scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PatternCategory {
    /// AWS access key ID.
    AwsKey,
    /// Google Cloud Platform API key.
    GcpKey,
    /// OpenAI API key.
    OpenAiKey,
    /// Anthropic API key.
    AnthropicKey,
    /// JSON Web Token.
    Jwt,
    /// PEM-encoded private key header.
    PrivateKey,
    /// Password embedded in a URL.
    PasswordUrl,
    /// Generic high-entropy API key.
    GenericApiKey,
    /// GitHub personal access or service token.
    GithubToken,
    /// Slack bot/user/app token.
    SlackToken,
    /// User-supplied custom pattern.
    Custom,
}

// ── SecretPattern ──────────────────────────────────────────────────────

/// Metadata for a single secret-detection pattern.
///
/// The compiled regex is stored separately in the [`TemplateScanner`]'s
/// `RegexSet` and individual `Vec<Regex>`.
#[derive(Debug, Clone)]
pub struct SecretPattern {
    /// Unique identifier for this pattern (e.g. `"aws-key"`).
    pub id: Cow<'static, str>,
    /// Human-readable description.
    pub description: Cow<'static, str>,
    /// Category of secret.
    pub category: PatternCategory,
    /// Severity assigned when this pattern matches.
    pub severity: Severity,
}

// ── SecretFinding ──────────────────────────────────────────────────────

/// A single secret found during a template scan.
#[derive(Debug, Clone)]
pub struct SecretFinding {
    /// Pattern that triggered the finding.
    pub pattern_id: Cow<'static, str>,
    /// Redacted representation (first 4 chars + `***` + last 2 chars).
    pub matched_text_redacted: String,
    /// Byte range within the scanned text.
    pub position: Range<usize>,
    /// Severity inherited from the pattern.
    pub severity: Severity,
    /// Shannon entropy of the matched region, if entropy-based detection was used.
    pub entropy: Option<f64>,
}

// ── CustomPattern ──────────────────────────────────────────────────────

/// A user-defined pattern added via [`ScannerConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CustomPattern {
    /// Unique identifier.
    pub id: String,
    /// Regex pattern string.
    pub regex: String,
    /// Human-readable description.
    pub description: String,
    /// Severity when matched.
    pub severity: Severity,
}

// ── ScanError ──────────────────────────────────────────────────────────

/// Errors that can occur during scanner construction or scanning.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ScanError {
    /// A regex pattern failed to compile.
    #[error("regex compilation failed for pattern '{pattern_id}': {reason}")]
    RegexCompilation {
        /// Pattern that failed.
        pattern_id: String,
        /// Underlying error message.
        reason: String,
    },

    /// Input content exceeds the maximum allowed size.
    #[error("content too large: {size} bytes (max {max})")]
    ContentTooLarge {
        /// Actual size in bytes.
        size: usize,
        /// Configured maximum.
        max: usize,
    },
}

// ── ScannerConfig ──────────────────────────────────────────────────────

/// Configuration for the [`TemplateScanner`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScannerConfig {
    /// Additional user-defined patterns.
    #[serde(default)]
    pub custom_patterns: Vec<CustomPattern>,
    /// Minimum Shannon entropy (bits/byte) for generic-key detection.
    #[serde(default = "default_entropy_threshold")]
    pub entropy_threshold: f64,
    /// Sliding window size (bytes) for entropy calculation.
    #[serde(default = "default_entropy_window")]
    pub entropy_window: usize,
    /// Which built-in categories to enable (empty = all).
    #[serde(default)]
    pub enabled_categories: HashSet<PatternCategory>,
    /// Maximum content size in bytes (0 = unlimited).
    #[serde(default)]
    pub max_content_size: usize,
}

fn default_entropy_threshold() -> f64 {
    4.5
}
fn default_entropy_window() -> usize {
    20
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            custom_patterns: Vec::new(),
            entropy_threshold: default_entropy_threshold(),
            entropy_window: default_entropy_window(),
            enabled_categories: HashSet::new(), // empty = all enabled
            max_content_size: 0,
        }
    }
}

impl ScannerConfig {
    /// Start building a [`ScannerConfig`].
    #[must_use]
    pub fn builder() -> ScannerConfigBuilder {
        ScannerConfigBuilder::default()
    }
}

// ── ScannerConfigBuilder ───────────────────────────────────────────────

/// Builder for [`ScannerConfig`].
#[derive(Debug, Default)]
pub struct ScannerConfigBuilder {
    config: ScannerConfig,
}

impl ScannerConfigBuilder {
    /// Add a custom pattern.
    #[must_use]
    pub fn custom_pattern(mut self, pattern: CustomPattern) -> Self {
        self.config.custom_patterns.push(pattern);
        self
    }

    /// Set the Shannon entropy threshold for generic-key detection.
    #[must_use]
    pub fn entropy_threshold(mut self, threshold: f64) -> Self {
        self.config.entropy_threshold = threshold;
        self
    }

    /// Set the sliding window size for entropy calculation.
    #[must_use]
    pub fn entropy_window(mut self, window: usize) -> Self {
        self.config.entropy_window = window;
        self
    }

    /// Enable only the specified categories (empty set = all enabled).
    #[must_use]
    pub fn enabled_categories(mut self, categories: HashSet<PatternCategory>) -> Self {
        self.config.enabled_categories = categories;
        self
    }

    /// Set maximum content size in bytes.
    #[must_use]
    pub fn max_content_size(mut self, max: usize) -> Self {
        self.config.max_content_size = max;
        self
    }

    /// Build the config.
    #[must_use]
    pub fn build(self) -> ScannerConfig {
        self.config
    }
}

// ── Built-in patterns ──────────────────────────────────────────────────

struct BuiltinPattern {
    id: &'static str,
    description: &'static str,
    regex: &'static str,
    category: PatternCategory,
    severity: Severity,
    /// If true, require entropy > threshold on the matched region.
    needs_entropy: bool,
}

const BUILTIN_PATTERNS: &[BuiltinPattern] = &[
    BuiltinPattern {
        id: "aws-key",
        description: "AWS access key ID",
        regex: r"AKIA[0-9A-Z]{16}",
        category: PatternCategory::AwsKey,
        severity: Severity::Critical,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "gcp-key",
        description: "Google Cloud Platform API key",
        regex: r"AIza[0-9A-Za-z\-_]{35}",
        category: PatternCategory::GcpKey,
        severity: Severity::Critical,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "openai-key",
        description: "OpenAI API key",
        regex: r"sk-[a-zA-Z0-9]{20,}",
        category: PatternCategory::OpenAiKey,
        severity: Severity::Critical,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "anthropic-key",
        description: "Anthropic API key",
        regex: r"sk-ant-[a-zA-Z0-9]{20,}",
        category: PatternCategory::AnthropicKey,
        severity: Severity::Critical,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "jwt",
        description: "JSON Web Token",
        regex: r"eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+",
        category: PatternCategory::Jwt,
        severity: Severity::High,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "private-key",
        description: "PEM-encoded private key header",
        regex: r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        category: PatternCategory::PrivateKey,
        severity: Severity::Critical,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "password-url",
        description: "Password embedded in URL",
        regex: r"://[^:]+:[^@]+@",
        category: PatternCategory::PasswordUrl,
        severity: Severity::High,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "generic-api-key",
        description: "High-entropy generic API key",
        regex: r"[a-zA-Z0-9]{32,}",
        category: PatternCategory::GenericApiKey,
        severity: Severity::Medium,
        needs_entropy: true,
    },
    BuiltinPattern {
        id: "github-token",
        description: "GitHub personal access or service token",
        regex: r"gh[ps]_[A-Za-z0-9_]{36,}",
        category: PatternCategory::GithubToken,
        severity: Severity::Critical,
        needs_entropy: false,
    },
    BuiltinPattern {
        id: "slack-token",
        description: "Slack bot/user/app token",
        regex: r"xox[bpras]-[0-9A-Za-z\-]+",
        category: PatternCategory::SlackToken,
        severity: Severity::High,
        needs_entropy: false,
    },
];

// ── Shannon entropy ────────────────────────────────────────────────────

/// Calculate byte-level Shannon entropy of the given data.
///
/// Returns bits per byte in \[0.0, 8.0\].  An empty slice returns 0.0.
fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let mut counts = [0u32; 256];
    for &b in data {
        counts[b as usize] += 1;
    }

    let len = data.len() as f64;
    let mut entropy = 0.0_f64;
    for &c in &counts {
        if c > 0 {
            let p = f64::from(c) / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

// ── TemplateScanner ────────────────────────────────────────────────────

/// Regex + Shannon entropy secret detector for system-prompt templates.
///
/// Compiles all patterns (built-in + custom) into a [`RegexSet`] at
/// construction time for efficient multi-pattern scanning.
#[derive(Debug)]
pub struct TemplateScanner {
    regex_set: RegexSet,
    regexes: Vec<Regex>,
    patterns: Vec<SecretPattern>,
    needs_entropy: Vec<bool>,
    entropy_threshold: f64,
    #[allow(dead_code)]
    entropy_window: usize,
    max_content_size: usize,
}

impl TemplateScanner {
    /// Construct a scanner from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::RegexCompilation`] if any pattern regex is invalid.
    pub fn new(config: ScannerConfig) -> Result<Self, ScanError> {
        let mut regex_strings = Vec::new();
        let mut patterns = Vec::new();
        let mut needs_entropy = Vec::new();

        let all_enabled = config.enabled_categories.is_empty();

        // Built-in patterns
        for bp in BUILTIN_PATTERNS {
            if !all_enabled && !config.enabled_categories.contains(&bp.category) {
                continue;
            }
            regex_strings.push(bp.regex.to_owned());
            patterns.push(SecretPattern {
                id: Cow::Borrowed(bp.id),
                description: Cow::Borrowed(bp.description),
                category: bp.category,
                severity: bp.severity,
            });
            needs_entropy.push(bp.needs_entropy);
        }

        // Custom patterns
        for cp in &config.custom_patterns {
            regex_strings.push(cp.regex.clone());
            patterns.push(SecretPattern {
                id: Cow::Owned(cp.id.clone()),
                description: Cow::Owned(cp.description.clone()),
                category: PatternCategory::Custom,
                severity: cp.severity,
            });
            needs_entropy.push(false);
        }

        // Compile individual regexes
        let mut regexes = Vec::with_capacity(regex_strings.len());
        for (i, raw) in regex_strings.iter().enumerate() {
            let re = Regex::new(raw).map_err(|e| ScanError::RegexCompilation {
                pattern_id: patterns[i].id.to_string(),
                reason: e.to_string(),
            })?;
            regexes.push(re);
        }

        // Compile RegexSet
        let regex_set =
            RegexSet::new(&regex_strings).map_err(|e| ScanError::RegexCompilation {
                pattern_id: "regex_set".into(),
                reason: e.to_string(),
            })?;

        Ok(Self {
            regex_set,
            regexes,
            patterns,
            needs_entropy,
            entropy_threshold: config.entropy_threshold,
            entropy_window: config.entropy_window,
            max_content_size: config.max_content_size,
        })
    }

    /// Convenience constructor with all built-in patterns and default settings.
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::RegexCompilation`] if a built-in pattern is invalid
    /// (should never happen).
    pub fn with_defaults() -> Result<Self, ScanError> {
        Self::new(ScannerConfig::default())
    }

    /// Scan the given template text for embedded secrets.
    ///
    /// # Errors
    ///
    /// Returns [`ScanError::ContentTooLarge`] if the text exceeds the
    /// configured maximum size.
    pub fn scan(&self, template: &str) -> Result<Vec<SecretFinding>, ScanError> {
        if self.max_content_size > 0 && template.len() > self.max_content_size {
            return Err(ScanError::ContentTooLarge {
                size: template.len(),
                max: self.max_content_size,
            });
        }

        let mut findings = Vec::new();
        let matched_indices: Vec<usize> = self.regex_set.matches(template).into_iter().collect();

        for &idx in &matched_indices {
            let re = &self.regexes[idx];
            let pattern = &self.patterns[idx];
            let check_entropy = self.needs_entropy[idx];

            for m in re.find_iter(template) {
                let matched_bytes = m.as_str().as_bytes();
                let ent = shannon_entropy(matched_bytes);

                if check_entropy && ent < self.entropy_threshold {
                    continue;
                }

                findings.push(SecretFinding {
                    pattern_id: pattern.id.clone(),
                    matched_text_redacted: redact(m.as_str()),
                    position: m.start()..m.end(),
                    severity: pattern.severity,
                    entropy: if check_entropy { Some(ent) } else { None },
                });
            }
        }

        Ok(findings)
    }
}

/// Redact a matched string: first 4 chars + `***` + last 2 chars.
fn redact(s: &str) -> String {
    if s.len() <= 6 {
        return "*".repeat(s.len());
    }
    let first: String = s.chars().take(4).collect();
    let last: String = s.chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{first}***{last}")
}

// ── GuardrailStage impl ────────────────────────────────────────────────

#[async_trait]
impl GuardrailStage for TemplateScanner {
    fn id(&self) -> &str {
        "template_scanner"
    }

    fn priority(&self) -> u32 {
        5
    }

    fn degradable(&self) -> bool {
        false
    }

    async fn evaluate(
        &self,
        content: &Content,
        _ctx: &SecurityContext,
    ) -> Result<StageOutcome, StageError> {
        let text = content.as_text();

        let findings = self.scan(&text).map_err(|e| StageError::InvalidContent {
            stage: self.id().to_owned(),
            reason: e.to_string(),
        })?;

        if findings.is_empty() {
            return Ok(StageOutcome::allow(1.0));
        }

        let max_severity = findings
            .iter()
            .map(|f| f.severity)
            .max()
            .unwrap_or(Severity::Info);

        let pattern_ids: Vec<&str> = findings.iter().map(|f| f.pattern_id.as_ref()).collect();
        let reason = format!(
            "secret(s) detected in template: [{}] (highest severity: {max_severity})",
            pattern_ids.join(", ")
        );

        Ok(StageOutcome::block(reason, max_severity))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Content {
        Content::Text(s.to_owned())
    }

    fn ctx() -> SecurityContext {
        SecurityContext::builder()
            .session_id("test")
            .risk_score(0.0)
            .build()
    }

    #[test]
    fn detect_aws_key() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let findings = scanner.scan("My key is AKIAIOSFODNN7EXAMPLE ok").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern_id, "aws-key");
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn detect_jwt() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let findings = scanner.scan(&format!("token: {token}")).unwrap();
        assert!(
            findings.iter().any(|f| f.pattern_id == "jwt"),
            "expected JWT finding, got: {findings:?}"
        );
    }

    #[test]
    fn detect_private_key_header() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let findings = scanner
            .scan("-----BEGIN RSA PRIVATE KEY-----\nMIIE...")
            .unwrap();
        assert!(findings.iter().any(|f| f.pattern_id == "private-key"));
    }

    #[test]
    fn detect_github_token() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let token = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        let findings = scanner.scan(&format!("gh: {token}")).unwrap();
        assert!(findings.iter().any(|f| f.pattern_id == "github-token"));
    }

    #[test]
    fn no_false_positives_on_benign_text() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let findings = scanner
            .scan("Hello world, how are you today?")
            .unwrap();
        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn high_entropy_string_detected_as_generic_key() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        // 40 unique characters → entropy ≈ log2(40) ≈ 5.32 > 4.5
        let secret = "xK9mPQ2vR4WnB8sT5hY3fG6dL1aZcEiUoNwXrMj";
        let findings = scanner.scan(secret).unwrap();
        assert!(
            findings.iter().any(|f| f.pattern_id == "generic-api-key"),
            "expected generic-api-key finding, got: {findings:?}"
        );
        let gf = findings
            .iter()
            .find(|f| f.pattern_id == "generic-api-key")
            .unwrap();
        assert!(gf.entropy.unwrap() > 4.5);
    }

    #[test]
    fn low_entropy_string_not_detected() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let boring = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let findings = scanner.scan(boring).unwrap();
        assert!(
            !findings.iter().any(|f| f.pattern_id == "generic-api-key"),
            "low-entropy string should not trigger generic-api-key: {findings:?}"
        );
    }

    #[test]
    fn custom_pattern_detected() {
        let config = ScannerConfig::builder()
            .custom_pattern(CustomPattern {
                id: "custom-secret".into(),
                regex: r"MYSECRET_[A-Z]{10}".into(),
                description: "My custom secret".into(),
                severity: Severity::High,
            })
            .build();
        let scanner = TemplateScanner::new(config).unwrap();
        let findings = scanner.scan("key=MYSECRET_ABCDEFGHIJ").unwrap();
        assert!(findings.iter().any(|f| f.pattern_id == "custom-secret"));
    }

    #[tokio::test]
    async fn guardrail_stage_blocks_on_secret() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let content = text("My AWS key: AKIAIOSFODNN7EXAMPLE");
        let outcome = scanner.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_block(), "expected Block, got: {outcome:?}");
    }

    #[tokio::test]
    async fn guardrail_stage_allows_clean_content() {
        let scanner = TemplateScanner::with_defaults().unwrap();
        let content = text("You are a helpful assistant. Answer questions concisely.");
        let outcome = scanner.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_allow(), "expected Allow, got: {outcome:?}");
    }
}
