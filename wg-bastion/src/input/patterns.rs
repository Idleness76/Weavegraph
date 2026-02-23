//! Static pattern library for prompt injection detection.
//!
//! Contains 50+ regex patterns organised into five [`PatternCategory`]
//! categories.  [`builtin_patterns`] returns the full set; callers may
//! also supply [`CustomPattern`]s to extend coverage.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::pipeline::outcome::Severity;

// ── PatternCategory ────────────────────────────────────────────────────

/// High-level classification of an injection pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PatternCategory {
    /// Attempts to redefine the model's identity or role.
    RoleConfusion,
    /// Attempts to override or cancel prior instructions.
    InstructionOverride,
    /// Abuse of delimiters, special tokens, or formatting to inject context.
    DelimiterManipulation,
    /// Attempts to exfiltrate the system prompt or hidden instructions.
    SystemPromptExtraction,
    /// Use of encoding tricks (base64, URL-encoding, Unicode escapes) to
    /// evade literal pattern matching.
    EncodingEvasion,
}

impl std::fmt::Display for PatternCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoleConfusion => write!(f, "role_confusion"),
            Self::InstructionOverride => write!(f, "instruction_override"),
            Self::DelimiterManipulation => write!(f, "delimiter_manipulation"),
            Self::SystemPromptExtraction => write!(f, "system_prompt_extraction"),
            Self::EncodingEvasion => write!(f, "encoding_evasion"),
        }
    }
}

// ── InjectionPattern ───────────────────────────────────────────────────

/// A built-in injection detection pattern.
#[derive(Debug, Clone)]
pub struct InjectionPattern {
    /// Unique identifier (e.g. `"RC-001"`).
    pub id: Cow<'static, str>,
    /// Which threat category this pattern belongs to.
    pub category: PatternCategory,
    /// Human-readable description of what this pattern detects.
    pub description: Cow<'static, str>,
    /// Raw regex pattern string (will be compiled into a `RegexSet`).
    pub regex_str: Cow<'static, str>,
    /// Severity when this pattern matches.
    pub severity: Severity,
    /// Weight for ensemble scoring (0.0–1.0).
    pub weight: f32,
}

// ── CustomPattern ──────────────────────────────────────────────────────

/// A user-provided injection detection pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPattern {
    /// Unique identifier.
    pub id: String,
    /// Which threat category this pattern belongs to.
    pub category: PatternCategory,
    /// Human-readable description.
    pub description: String,
    /// Raw regex pattern string.
    pub regex_str: String,
    /// Severity when this pattern matches.
    pub severity: Severity,
    /// Weight for ensemble scoring (0.0–1.0).
    pub weight: f32,
}

// ── Built-in patterns ──────────────────────────────────────────────────

/// Helper to reduce boilerplate when defining static patterns.
macro_rules! pat {
    ($id:expr, $cat:expr, $desc:expr, $re:expr, $sev:expr, $w:expr) => {
        InjectionPattern {
            id: Cow::Borrowed($id),
            category: $cat,
            description: Cow::Borrowed($desc),
            regex_str: Cow::Borrowed($re),
            severity: $sev,
            weight: $w,
        }
    };
}

/// Returns the full set of built-in injection detection patterns (50+).
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn builtin_patterns() -> Vec<InjectionPattern> {
    use PatternCategory::{
        DelimiterManipulation, EncodingEvasion, InstructionOverride, RoleConfusion,
        SystemPromptExtraction,
    };
    use Severity::{Critical, High, Low, Medium};

    vec![
        // ── Category 1: Role Confusion (14 patterns) ───────────────
        pat!(
            "RC-001",
            RoleConfusion,
            "You are now",
            r"(?i)you\s+are\s+now\b",
            High,
            0.9
        ),
        pat!(
            "RC-002",
            RoleConfusion,
            "Ignore previous instructions",
            r"(?i)ignore\s+(all\s+)?previous\s+instructions",
            Critical,
            1.0
        ),
        pat!(
            "RC-003",
            RoleConfusion,
            "Act as a/an",
            r"(?i)act\s+as\s+(a|an)\b",
            High,
            0.8
        ),
        pat!(
            "RC-004",
            RoleConfusion,
            "Pretend to be",
            r"(?i)pretend\s+(to\s+be|you\s+are)\b",
            High,
            0.85
        ),
        pat!(
            "RC-005",
            RoleConfusion,
            "Forget everything",
            r"(?i)forget\s+everything",
            Critical,
            0.95
        ),
        pat!(
            "RC-006",
            RoleConfusion,
            "Your new role is",
            r"(?i)your\s+new\s+role\s+is\b",
            High,
            0.9
        ),
        pat!(
            "RC-007",
            RoleConfusion,
            "Disregard prior",
            r"(?i)disregard\s+(all\s+)?prior\b",
            Critical,
            0.95
        ),
        pat!(
            "RC-008",
            RoleConfusion,
            "Switch to mode",
            r"(?i)switch\s+to\s+\w+\s+mode",
            Medium,
            0.7
        ),
        pat!(
            "RC-009",
            RoleConfusion,
            "From now on",
            r"(?i)from\s+now\s+on\b",
            Medium,
            0.6
        ),
        pat!(
            "RC-010",
            RoleConfusion,
            "You must now",
            r"(?i)you\s+must\s+now\b",
            High,
            0.8
        ),
        pat!(
            "RC-011",
            RoleConfusion,
            "System: prefix",
            r"(?i)^system:\s",
            High,
            0.9
        ),
        pat!(
            "RC-012",
            RoleConfusion,
            "Override your programming",
            r"(?i)override\s+your\s+programming",
            Critical,
            0.95
        ),
        pat!(
            "RC-013",
            RoleConfusion,
            "You have been reprogrammed",
            r"(?i)you\s+have\s+been\s+reprogrammed",
            Critical,
            0.95
        ),
        pat!(
            "RC-014",
            RoleConfusion,
            "New instructions:",
            r"(?i)new\s+instructions?\s*:",
            High,
            0.85
        ),
        // ── Category 2: Instruction Override (12 patterns) ─────────
        pat!(
            "IO-001",
            InstructionOverride,
            "Ignore the above",
            r"(?i)ignore\s+the\s+above",
            Critical,
            1.0
        ),
        pat!(
            "IO-002",
            InstructionOverride,
            "Disregard the ... and",
            r"(?i)disregard\s+the\s+\w+\s+and\b",
            High,
            0.9
        ),
        pat!(
            "IO-003",
            InstructionOverride,
            "Do not follow",
            r"(?i)do\s+not\s+follow",
            High,
            0.85
        ),
        pat!(
            "IO-004",
            InstructionOverride,
            "Cancel all restrictions",
            r"(?i)cancel\s+all\s+restrictions",
            Critical,
            0.95
        ),
        pat!(
            "IO-005",
            InstructionOverride,
            "Remove safety",
            r"(?i)remove\s+(all\s+)?safety",
            Critical,
            0.95
        ),
        pat!(
            "IO-006",
            InstructionOverride,
            "Disable filters",
            r"(?i)disable\s+(all\s+)?filters",
            Critical,
            0.95
        ),
        pat!(
            "IO-007",
            InstructionOverride,
            "Bypass the",
            r"(?i)bypass\s+the\b",
            High,
            0.85
        ),
        pat!(
            "IO-008",
            InstructionOverride,
            "Override: prefix",
            r"(?i)^override\s*:",
            High,
            0.9
        ),
        pat!(
            "IO-009",
            InstructionOverride,
            "Admin tag injection",
            r"<admin>",
            Critical,
            0.95
        ),
        pat!(
            "IO-010",
            InstructionOverride,
            "Execute the following",
            r"(?i)execute\s+the\s+following",
            Medium,
            0.7
        ),
        pat!(
            "IO-011",
            InstructionOverride,
            "Unlock hidden/secret mode",
            r"(?i)unlock\s+(hidden|secret)\s+mode",
            High,
            0.9
        ),
        pat!(
            "IO-012",
            InstructionOverride,
            "Enter developer/debug/god mode",
            r"(?i)enter\s+(developer|debug|god)\s+mode",
            High,
            0.9
        ),
        // ── Category 3: Delimiter Manipulation (10 patterns) ───────
        pat!(
            "DM-001",
            DelimiterManipulation,
            "Dash-delimited system: prefix",
            r"---\n.*(?i)system\s*:",
            High,
            0.85
        ),
        pat!(
            "DM-002",
            DelimiterManipulation,
            "Code block system: hijack",
            r"```\w*\n.*(?i)system\s*:",
            High,
            0.85
        ),
        pat!(
            "DM-003",
            DelimiterManipulation,
            "[INST] tag injection",
            r"\[INST\]",
            High,
            0.9
        ),
        pat!(
            "DM-004",
            DelimiterManipulation,
            "Special token injection",
            r"<\|[a-z_]+\|>",
            Critical,
            0.95
        ),
        pat!(
            "DM-005",
            DelimiterManipulation,
            "End of prompt marker",
            r"(?i)end\s+of\s+prompt",
            Medium,
            0.7
        ),
        pat!(
            "DM-006",
            DelimiterManipulation,
            "Template variable abuse",
            r"(?i)\{\{[^}]*\}\}",
            Medium,
            0.6
        ),
        pat!(
            "DM-007",
            DelimiterManipulation,
            "HTML comment injection",
            r"<!--.*?-->",
            Medium,
            0.6
        ),
        pat!(
            "DM-008",
            DelimiterManipulation,
            "End-of-sequence token",
            r"</s>",
            High,
            0.9
        ),
        pat!(
            "DM-009",
            DelimiterManipulation,
            "ChatML markers",
            r"(?i)<\|im_start\|>|<\|im_end\|>",
            Critical,
            0.95
        ),
        pat!(
            "DM-010",
            DelimiterManipulation,
            "Excessive newline context break",
            r"\n{10,}",
            Medium,
            0.5
        ),
        // ── Category 4: System Prompt Extraction (8 patterns) ──────
        pat!(
            "SE-001",
            SystemPromptExtraction,
            "Repeat the/your/all instructions",
            r"(?i)repeat\s+(the|your|all)\s+instructions",
            High,
            0.9
        ),
        pat!(
            "SE-002",
            SystemPromptExtraction,
            "Print/show/display prompt",
            r"(?i)(print|show|display)\s+(the|your)\s+(prompt|instructions)",
            High,
            0.9
        ),
        pat!(
            "SE-003",
            SystemPromptExtraction,
            "Show me the original",
            r"(?i)show\s+me\s+the\s+original",
            Medium,
            0.7
        ),
        pat!(
            "SE-004",
            SystemPromptExtraction,
            "Tell me your rules",
            r"(?i)tell\s+me\s+your\s+rules",
            High,
            0.85
        ),
        pat!(
            "SE-005",
            SystemPromptExtraction,
            "What were you told / your instructions",
            r"(?i)what\s+(were\s+you|are\s+your)\s+(told|instructions|rules)",
            High,
            0.85
        ),
        pat!(
            "SE-006",
            SystemPromptExtraction,
            "What is your system prompt",
            r"(?i)what\s+is\s+your\s+system\s+prompt",
            Critical,
            0.95
        ),
        pat!(
            "SE-007",
            SystemPromptExtraction,
            "Start response with quote",
            r#"(?i)start\s+your\s+response\s+with\s+["']"#,
            Medium,
            0.7
        ),
        pat!(
            "SE-008",
            SystemPromptExtraction,
            "Reproduce the text above",
            r"(?i)reproduce\s+the\s+text\s+above",
            High,
            0.9
        ),
        // ── Category 5: Encoding Evasion (6 patterns) ──────────────
        pat!(
            "EE-001",
            EncodingEvasion,
            "Unicode escape sequences",
            r"\\u[0-9a-fA-F]{4}",
            Medium,
            0.6
        ),
        pat!(
            "EE-002",
            EncodingEvasion,
            "URL-encoded characters",
            r"%[0-9a-fA-F]{2}",
            Medium,
            0.5
        ),
        pat!(
            "EE-003",
            EncodingEvasion,
            "HTML entities",
            r"&#x?[0-9a-fA-F]+;",
            Medium,
            0.6
        ),
        pat!(
            "EE-004",
            EncodingEvasion,
            "Base64-like high-entropy string",
            r"(?i)[a-zA-Z0-9+/]{20,}={0,2}",
            Low,
            0.4
        ),
        pat!(
            "EE-005",
            EncodingEvasion,
            "Encoding method reference",
            r"(?i)\brot13\b|\bbase64\b|\bhex\s+encode",
            Medium,
            0.65
        ),
        pat!(
            "EE-006",
            EncodingEvasion,
            "Decode this/the following",
            r"(?i)decode\s+(this|the\s+following)",
            Medium,
            0.6
        ),
    ]
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_has_at_least_50_patterns() {
        assert!(builtin_patterns().len() >= 50);
    }

    #[test]
    fn all_five_categories_represented() {
        let patterns = builtin_patterns();
        let categories: std::collections::HashSet<_> =
            patterns.iter().map(|p| p.category).collect();
        assert!(categories.contains(&PatternCategory::RoleConfusion));
        assert!(categories.contains(&PatternCategory::InstructionOverride));
        assert!(categories.contains(&PatternCategory::DelimiterManipulation));
        assert!(categories.contains(&PatternCategory::SystemPromptExtraction));
        assert!(categories.contains(&PatternCategory::EncodingEvasion));
    }

    #[test]
    fn pattern_ids_are_unique() {
        let patterns = builtin_patterns();
        let ids: std::collections::HashSet<_> = patterns.iter().map(|p| &p.id).collect();
        assert_eq!(ids.len(), patterns.len(), "duplicate pattern IDs detected");
    }

    #[test]
    fn weights_in_range() {
        for p in &builtin_patterns() {
            assert!(
                (0.0..=1.0).contains(&p.weight),
                "pattern {} has weight {} outside [0.0, 1.0]",
                p.id,
                p.weight,
            );
        }
    }

    #[test]
    fn all_patterns_compile() {
        for p in &builtin_patterns() {
            regex::Regex::new(&p.regex_str).unwrap_or_else(|e| {
                panic!("pattern {} has invalid regex: {e}", p.id);
            });
        }
    }
}
