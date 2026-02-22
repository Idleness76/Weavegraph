//! [`SecureTemplate`] — typed placeholders with max-length enforcement,
//! auto-escaping, and role marker protection.
//!
//! Templates use the syntax `{{name:type}}` or `{{name:type:constraint}}` to
//! define typed placeholders that are validated and auto-escaped at render time.

use crate::prompt::scanner::TemplateScanner;
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

// ── Placeholder ────────────────────────────────────────────────────────

/// A typed placeholder extracted from a template string.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Placeholder {
    /// A text placeholder with an optional max-length constraint.
    Text {
        /// Placeholder name.
        name: String,
        /// Maximum allowed character length.
        max_length: usize,
        /// Whether a value must be supplied at render time.
        required: bool,
    },

    /// A numeric placeholder with optional min/max bounds.
    Number {
        /// Placeholder name.
        name: String,
        /// Minimum allowed value (inclusive).
        min: Option<f64>,
        /// Maximum allowed value (inclusive).
        max: Option<f64>,
    },

    /// An enum placeholder restricted to a fixed set of allowed values.
    Enum {
        /// Placeholder name.
        name: String,
        /// The set of permitted values.
        allowed_values: Vec<String>,
    },

    /// A JSON placeholder accepting arbitrary JSON text.
    Json {
        /// Placeholder name.
        name: String,
        /// Optional schema hint for documentation/validation.
        schema_hint: Option<String>,
    },
}

impl Placeholder {
    /// Returns the name of this placeholder regardless of variant.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Text { name, .. }
            | Self::Number { name, .. }
            | Self::Enum { name, .. }
            | Self::Json { name, .. } => name,
        }
    }
}

// ── TemplateError ──────────────────────────────────────────────────────

/// Errors arising from template compilation or rendering.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TemplateError {
    /// A required placeholder was not supplied during rendering.
    #[error("missing required placeholder: '{name}'")]
    MissingRequired {
        /// Name of the missing placeholder.
        name: String,
    },

    /// A supplied value exceeds the placeholder's max-length constraint.
    #[error("value for '{name}' exceeds max length ({actual} > {max})")]
    ExceedsMaxLength {
        /// Placeholder name.
        name: String,
        /// Actual character count.
        actual: usize,
        /// Configured maximum.
        max: usize,
    },

    /// A supplied value does not match the expected placeholder type.
    #[error("invalid type for '{name}': expected {expected}, got {actual}")]
    InvalidType {
        /// Placeholder name.
        name: String,
        /// Expected type description.
        expected: String,
        /// Actual value description.
        actual: String,
    },

    /// Secrets were detected in an interpolated value.
    #[error("secrets detected in value for '{name}': {findings:?}")]
    ContainsSecrets {
        /// Placeholder name.
        name: String,
        /// Redacted descriptions of the matched secrets.
        findings: Vec<String>,
    },

    /// A placeholder at the given byte position could not be parsed.
    #[error("invalid placeholder at position {position}: {reason}")]
    InvalidPlaceholder {
        /// Byte offset in the template string.
        position: usize,
        /// Why parsing failed.
        reason: String,
    },

    /// General parse error during template compilation.
    #[error("template parse error: {reason}")]
    ParseError {
        /// What went wrong.
        reason: String,
    },
}

// ── Role-marker escaping ───────────────────────────────────────────────

/// Pairs of (needle, replacement) for role-marker auto-escaping.
///
/// Replacements use Unicode fullwidth equivalents where possible so the
/// text remains human-readable but cannot be interpreted as control tokens.
const ROLE_MARKER_ESCAPES: &[(&str, &str)] = &[
    ("[SYSTEM_START", "\u{FF3B}SYSTEM_START"),
    ("[SYSTEM_END", "\u{FF3B}SYSTEM_END"),
    ("[INST]", "\u{FF3B}INST\u{FF3D}"),
    ("</s>", "\u{FF1C}/s\u{FF1E}"),
    ("<|", "\u{FF1C}\u{FF5C}"),
    ("|>", "\u{FF5C}\u{FF1E}"),
];

/// Escape role markers in a user-supplied value.
fn escape_role_markers(value: &str) -> String {
    let mut result = value.to_owned();
    for &(needle, replacement) in ROLE_MARKER_ESCAPES {
        result = result.replace(needle, replacement);
    }
    result
}

// ── Placeholder-regex helper ───────────────────────────────────────────

fn placeholder_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_]*):([a-z]+)(?::([^}]*))?\}\}").unwrap()
    })
}

/// Parse a single placeholder from its captured parts.
fn parse_placeholder(
    name: &str,
    typ: &str,
    constraint: Option<&str>,
    position: usize,
) -> Result<Placeholder, TemplateError> {
    match typ {
        "text" => {
            let (max_length, required) = match constraint {
                Some(c) => {
                    let (num_str, req) = if let Some(stripped) = c.strip_suffix('?') {
                        (stripped, false)
                    } else {
                        (c, true)
                    };
                    let max = num_str.parse::<usize>().map_err(|_| {
                        TemplateError::InvalidPlaceholder {
                            position,
                            reason: format!("invalid max_length: '{num_str}'"),
                        }
                    })?;
                    (max, req)
                }
                None => (usize::MAX, true),
            };
            Ok(Placeholder::Text {
                name: name.to_owned(),
                max_length,
                required,
            })
        }
        "number" => {
            let (min, max) = match constraint {
                Some(c) if c.contains('-') => {
                    // Handle negative numbers: split on the pattern "number-number"
                    // We need to find the separator '-' that is between two numbers.
                    let parts =
                        parse_number_range(c).ok_or_else(|| TemplateError::InvalidPlaceholder {
                            position,
                            reason: format!("invalid number range: '{c}'"),
                        })?;
                    (Some(parts.0), Some(parts.1))
                }
                Some(c) => {
                    return Err(TemplateError::InvalidPlaceholder {
                        position,
                        reason: format!("invalid number constraint: '{c}'"),
                    });
                }
                None => (None, None),
            };
            Ok(Placeholder::Number {
                name: name.to_owned(),
                min,
                max,
            })
        }
        "enum" => {
            let values = constraint
                .ok_or_else(|| TemplateError::InvalidPlaceholder {
                    position,
                    reason: "enum placeholder requires allowed values (e.g. val1|val2)".into(),
                })?
                .split('|')
                .map(|s| s.trim().to_owned())
                .collect::<Vec<_>>();
            if values.is_empty() || values.iter().any(std::string::String::is_empty) {
                return Err(TemplateError::InvalidPlaceholder {
                    position,
                    reason: "enum values must not be empty".into(),
                });
            }
            Ok(Placeholder::Enum {
                name: name.to_owned(),
                allowed_values: values,
            })
        }
        "json" => Ok(Placeholder::Json {
            name: name.to_owned(),
            schema_hint: constraint.map(std::borrow::ToOwned::to_owned),
        }),
        other => Err(TemplateError::InvalidPlaceholder {
            position,
            reason: format!("unknown placeholder type: '{other}'"),
        }),
    }
}

/// Parse a range like "0-100" into (min, max).
fn parse_number_range(s: &str) -> Option<(f64, f64)> {
    // Find the separator dash. Skip a leading '-' (negative min).
    let search_start = usize::from(s.starts_with('-'));
    let sep = s[search_start..].find('-').map(|i| i + search_start)?;
    let min = s[..sep].parse::<f64>().ok()?;
    let max = s[sep + 1..].parse::<f64>().ok()?;
    Some((min, max))
}

// ── SecureTemplate ─────────────────────────────────────────────────────

/// A compiled prompt template with typed, validated placeholders.
///
/// Placeholder syntax: `{{name:type}}` or `{{name:type:constraint}}`.
///
/// # Example
///
/// ```rust,ignore
/// use wg_bastion::prompt::template::SecureTemplate;
///
/// let tpl = SecureTemplate::compile("Hello, {{user:text:50}}!")?;
/// let out = tpl.render([("user", "Alice")])?;
/// assert_eq!(out, "Hello, Alice!");
/// ```
#[derive(Debug, Clone)]
pub struct SecureTemplate {
    template_string: String,
    placeholders: Vec<Placeholder>,
    scanner: Option<Arc<TemplateScanner>>,
}

impl SecureTemplate {
    /// Compile a template string, extracting and validating all placeholders.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::InvalidPlaceholder`] or [`TemplateError::ParseError`]
    /// if the template contains malformed placeholder syntax.
    ///
    /// # Panics
    ///
    /// Calls `.unwrap()` on `caps.get(0)`, which is guaranteed to succeed because
    /// capture group 0 always exists when the regex matches.
    pub fn compile(template: &str) -> Result<Self, TemplateError> {
        let re = placeholder_regex();
        let mut placeholders = Vec::new();

        for caps in re.captures_iter(template) {
            let m = caps.get(0).unwrap();
            let name = &caps[1];
            let typ = &caps[2];
            let constraint = caps.get(3).map(|c| c.as_str());
            placeholders.push(parse_placeholder(name, typ, constraint, m.start())?);
        }

        let scanner = TemplateScanner::with_defaults().ok().map(Arc::new);

        Ok(Self {
            template_string: template.to_owned(),
            placeholders,
            scanner,
        })
    }

    /// Access the extracted placeholders.
    #[must_use]
    pub fn placeholders(&self) -> &[Placeholder] {
        &self.placeholders
    }

    /// Render the template by substituting placeholder values.
    ///
    /// Values are validated against placeholder constraints, auto-escaped for
    /// role-marker injection, and scanned for embedded secrets.
    ///
    /// # Errors
    ///
    /// Returns a [`TemplateError`] if validation fails for any value.
    ///
    /// # Panics
    ///
    /// Calls `.unwrap()` on `caps.get(0)`, which is guaranteed to succeed because
    /// capture group 0 always exists when the regex matches.
    pub fn render(
        &self,
        values: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
    ) -> Result<String, TemplateError> {
        let map: HashMap<String, String> = values
            .into_iter()
            .map(|(k, v)| (k.as_ref().to_owned(), v.as_ref().to_owned()))
            .collect();

        // Validate all placeholders against supplied values.
        self.validate_values(&map)?;

        // Perform substitution with auto-escaping.
        let re = placeholder_regex();
        let mut result = self.template_string.clone();

        // Replace from end to start to preserve byte offsets.
        let matches: Vec<_> = re
            .captures_iter(&self.template_string)
            .map(|caps| {
                let m = caps.get(0).unwrap();
                let name = caps[1].to_owned();
                (m.start(), m.end(), name)
            })
            .collect();

        for (start, end, name) in matches.into_iter().rev() {
            if let Some(raw_value) = map.get(&name) {
                let escaped = escape_role_markers(raw_value);
                result.replace_range(start..end, &escaped);
            }
        }

        // Scan rendered output for secrets.
        if let Some(scanner) = self.scanner.as_ref()
            && let Ok(findings) = scanner.scan(&result)
            && !findings.is_empty()
        {
            // Attribute findings to the first placeholder whose value triggered them.
            let finding_strs: Vec<String> = findings
                .iter()
                .map(|f| format!("{}: {}", f.pattern_id, f.matched_text_redacted))
                .collect();
            // Find which placeholder's value contains a secret.
            let responsible = self
                .placeholders
                .iter()
                .find(|ph| {
                    map.get(ph.name())
                        .is_some_and(|v| scanner.scan(v).is_ok_and(|f| !f.is_empty()))
                })
                .map_or_else(|| "unknown".into(), |ph| ph.name().to_owned());

            return Err(TemplateError::ContainsSecrets {
                name: responsible,
                findings: finding_strs,
            });
        }

        Ok(result)
    }

    /// Validate all placeholder values against their type constraints.
    fn validate_values(&self, map: &HashMap<String, String>) -> Result<(), TemplateError> {
        for ph in &self.placeholders {
            let value = map.get(ph.name());
            match ph {
                Placeholder::Text {
                    name,
                    max_length,
                    required,
                } => {
                    if *required && value.is_none() {
                        return Err(TemplateError::MissingRequired { name: name.clone() });
                    }
                    if let Some(v) = value
                        && v.chars().count() > *max_length
                    {
                        return Err(TemplateError::ExceedsMaxLength {
                            name: name.clone(),
                            actual: v.chars().count(),
                            max: *max_length,
                        });
                    }
                }
                Placeholder::Number { name, min, max } => {
                    if let Some(v) = value {
                        let num = v.parse::<f64>().map_err(|_| TemplateError::InvalidType {
                            name: name.clone(),
                            expected: "number".into(),
                            actual: v.clone(),
                        })?;
                        if let Some(lo) = min
                            && num < *lo
                        {
                            return Err(TemplateError::InvalidType {
                                name: name.clone(),
                                expected: format!("number >= {lo}"),
                                actual: v.clone(),
                            });
                        }
                        if let Some(hi) = max
                            && num > *hi
                        {
                            return Err(TemplateError::InvalidType {
                                name: name.clone(),
                                expected: format!("number <= {hi}"),
                                actual: v.clone(),
                            });
                        }
                    }
                }
                Placeholder::Enum {
                    name,
                    allowed_values,
                } => {
                    if let Some(v) = value
                        && !allowed_values.iter().any(|a| a == v)
                    {
                        return Err(TemplateError::InvalidType {
                            name: name.clone(),
                            expected: format!("one of [{}]", allowed_values.join(", ")),
                            actual: v.clone(),
                        });
                    }
                }
                Placeholder::Json { name, .. } => {
                    if let Some(v) = value {
                        serde_json::from_str::<serde_json::Value>(v).map_err(|_| {
                            TemplateError::InvalidType {
                                name: name.clone(),
                                expected: "valid JSON".into(),
                                actual: v.clone(),
                            }
                        })?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl TryFrom<&str> for SecureTemplate {
    type Error = TemplateError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::compile(value)
    }
}

// ── TemplateBuilder ────────────────────────────────────────────────────

/// Convenience builder for constructing a [`SecureTemplate`] from a string.
#[derive(Debug)]
#[must_use]
pub struct TemplateBuilder {
    template_str: String,
}

impl TemplateBuilder {
    /// Create a new builder from a template string.
    pub fn new(template_str: impl Into<String>) -> Self {
        Self {
            template_str: template_str.into(),
        }
    }

    /// Compile and return the [`SecureTemplate`].
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError`] if compilation fails.
    pub fn build(self) -> Result<SecureTemplate, TemplateError> {
        SecureTemplate::compile(&self.template_str)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_valid_text_placeholder() {
        let tpl = SecureTemplate::compile("Hello, {{user:text:50}}!").unwrap();
        assert_eq!(tpl.placeholders().len(), 1);
        assert_eq!(tpl.placeholders()[0].name(), "user");
        assert!(matches!(
            &tpl.placeholders()[0],
            Placeholder::Text {
                max_length: 50,
                required: true,
                ..
            }
        ));
    }

    #[test]
    fn render_valid_values() {
        let tpl = SecureTemplate::compile("Hello, {{user:text:50}}!").unwrap();
        let result = tpl.render([("user", "Alice")]).unwrap();
        assert_eq!(result, "Hello, Alice!");
    }

    #[test]
    fn render_missing_required_value() {
        let tpl = SecureTemplate::compile("Hello, {{user:text:50}}!").unwrap();
        let err = tpl.render(std::iter::empty::<(&str, &str)>()).unwrap_err();
        assert!(
            matches!(err, TemplateError::MissingRequired { ref name } if name == "user"),
            "expected MissingRequired, got: {err}"
        );
    }

    #[test]
    fn render_exceeds_max_length() {
        let tpl = SecureTemplate::compile("Hi {{name:text:5}}").unwrap();
        let err = tpl.render([("name", "TooLongName")]).unwrap_err();
        assert!(
            matches!(
                err,
                TemplateError::ExceedsMaxLength {
                    ref name,
                    actual: 11,
                    max: 5,
                } if name == "name"
            ),
            "expected ExceedsMaxLength, got: {err}"
        );
    }

    #[test]
    fn auto_escape_role_markers() {
        let tpl = SecureTemplate::compile("Msg: {{msg:text:200}}").unwrap();
        let result = tpl.render([("msg", "inject [SYSTEM_START here")]).unwrap();
        assert!(
            !result.contains("[SYSTEM_START"),
            "role marker should be escaped: {result}"
        );
        assert!(result.contains('\u{FF3B}'));
    }

    #[test]
    fn enum_rejects_invalid_value() {
        let tpl = SecureTemplate::compile("Mode: {{mode:enum:fast|slow}}").unwrap();
        let err = tpl.render([("mode", "invalid")]).unwrap_err();
        assert!(
            matches!(err, TemplateError::InvalidType { ref name, .. } if name == "mode"),
            "expected InvalidType, got: {err}"
        );
    }

    #[test]
    fn secret_in_value_detected() {
        let tpl = SecureTemplate::compile("Key: {{key:text:100}}").unwrap();
        let err = tpl.render([("key", "AKIAIOSFODNN7EXAMPLE")]).unwrap_err();
        assert!(
            matches!(err, TemplateError::ContainsSecrets { ref name, .. } if name == "key"),
            "expected ContainsSecrets, got: {err}"
        );
    }

    #[test]
    fn try_from_works_same_as_compile() {
        let a = SecureTemplate::compile("Hello, {{x:text:10}}!").unwrap();
        let b = SecureTemplate::try_from("Hello, {{x:text:10}}!").unwrap();
        assert_eq!(a.placeholders().len(), b.placeholders().len());
        assert_eq!(a.placeholders()[0].name(), b.placeholders()[0].name());
    }

    #[test]
    fn number_placeholder_validates_range() {
        let tpl = SecureTemplate::compile("Score: {{score:number:0-100}}").unwrap();
        // Valid
        let ok = tpl.render([("score", "50")]).unwrap();
        assert_eq!(ok, "Score: 50");
        // Out of range
        let err = tpl.render([("score", "200")]).unwrap_err();
        assert!(
            matches!(err, TemplateError::InvalidType { ref name, .. } if name == "score"),
            "expected InvalidType for out-of-range, got: {err}"
        );
    }

    #[test]
    fn empty_template_compiles_and_renders() {
        let tpl = SecureTemplate::compile("").unwrap();
        assert!(tpl.placeholders().is_empty());
        let result = tpl.render(std::iter::empty::<(&str, &str)>()).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn template_builder_works() {
        let tpl = TemplateBuilder::new("{{x:text:10}}").build().unwrap();
        assert_eq!(tpl.placeholders().len(), 1);
    }

    #[test]
    fn optional_text_placeholder_not_required() {
        let tpl = SecureTemplate::compile("Hi {{name:text:50?}}!").unwrap();
        assert!(matches!(
            &tpl.placeholders()[0],
            Placeholder::Text {
                required: false,
                ..
            }
        ));
        // Rendering without the optional value should succeed.
        let result = tpl.render(std::iter::empty::<(&str, &str)>()).unwrap();
        assert_eq!(result, "Hi {{name:text:50?}}!");
    }
}
