//! Unicode NFKC normalization, HTML sanitization, control character stripping,
//! and content truncation.
//!
//! [`NormalizationStage`] is a [`GuardrailStage`] that runs first in the
//! pipeline (priority 10) to canonicalize content before downstream detectors
//! inspect it.  This prevents attackers from using invisible characters, bidi
//! overrides, homoglyph substitutions, or embedded HTML to evade injection
//! detection.

use std::borrow::Cow;
use std::ops::Range;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::pipeline::content::{Content, Message, RetrievedChunk};
use crate::pipeline::outcome::{StageError, StageOutcome};
use crate::pipeline::stage::{GuardrailStage, SecurityContext};

// ── NormalizationConfig ────────────────────────────────────────────────

/// Configuration for the [`NormalizationStage`].
///
/// Uses a builder pattern — all setters are `#[must_use]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NormalizationConfig {
    /// Maximum content size in bytes.  Content exceeding this is truncated
    /// at a UTF-8 char boundary.  Default: 1 MiB.
    pub max_content_bytes: usize,
    /// Whether to strip HTML tags (default `true`).
    pub strip_html: bool,
    /// Whether to apply Unicode NFKC normalization (default `true`).
    pub normalize_unicode: bool,
    /// Whether to remove invisible / control characters (default `true`).
    pub strip_control_chars: bool,
    /// Whether to detect mixed-script usage within words (default `true`).
    pub detect_script_mixing: bool,
    /// Whether to truncate oversize content (default `true`).
    pub truncate: bool,
}

impl Default for NormalizationConfig {
    fn default() -> Self {
        Self {
            max_content_bytes: 1_048_576, // 1 MiB
            strip_html: true,
            normalize_unicode: true,
            strip_control_chars: true,
            detect_script_mixing: true,
            truncate: true,
        }
    }
}

impl NormalizationConfig {
    /// Create a new config with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum content size in bytes.
    #[must_use]
    pub fn max_content_bytes(mut self, bytes: usize) -> Self {
        self.max_content_bytes = bytes;
        self
    }

    /// Enable or disable HTML stripping.
    #[must_use]
    pub fn strip_html(mut self, enabled: bool) -> Self {
        self.strip_html = enabled;
        self
    }

    /// Enable or disable Unicode NFKC normalization.
    #[must_use]
    pub fn normalize_unicode(mut self, enabled: bool) -> Self {
        self.normalize_unicode = enabled;
        self
    }

    /// Enable or disable control character stripping.
    #[must_use]
    pub fn strip_control_chars(mut self, enabled: bool) -> Self {
        self.strip_control_chars = enabled;
        self
    }

    /// Enable or disable script mixing detection.
    #[must_use]
    pub fn detect_script_mixing(mut self, enabled: bool) -> Self {
        self.detect_script_mixing = enabled;
        self
    }

    /// Enable or disable truncation.
    #[must_use]
    pub fn truncate(mut self, enabled: bool) -> Self {
        self.truncate = enabled;
        self
    }
}

// ── ScriptMixingWarning ────────────────────────────────────────────────

/// Warning emitted when mixed scripts are detected within a single word.
///
/// This is a common indicator of homoglyph attacks (e.g. Cyrillic "а"
/// mixed with Latin "a" to spell "pаypal").
#[derive(Debug, Clone)]
pub struct ScriptMixingWarning {
    /// Byte range in the original text where mixing was found.
    pub position: Range<usize>,
    /// Script names detected (e.g. `["Latin", "Cyrillic"]`).
    pub scripts_found: Vec<String>,
}

// ── NormalizationStage ─────────────────────────────────────────────────

/// Guardrail stage that canonicalizes content before downstream analysis.
///
/// # Pipeline order
///
/// Priority 10 — runs before injection detection and other heuristic stages.
#[derive(Debug, Clone)]
pub struct NormalizationStage {
    config: NormalizationConfig,
}

impl NormalizationStage {
    /// Create a new stage with the given configuration.
    #[must_use]
    pub fn new(config: NormalizationConfig) -> Self {
        Self { config }
    }

    /// Create a new stage with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(NormalizationConfig::default())
    }
}

// ── Core normalization functions ───────────────────────────────────────

/// Returns `true` if the char is a control/invisible character that should
/// be stripped for security purposes.
fn is_dangerous_control_char(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'             // ZWSP
        | '\u{200C}'           // ZWNJ
        | '\u{200D}'           // ZWJ
        | '\u{FEFF}'           // BOM
        | '\u{00AD}'           // soft hyphen
        | '\u{2060}'           // word joiner
        | '\u{202A}'..='\u{202E}'  // bidi controls
        | '\u{2066}'..='\u{2069}'  // bidi isolates
        | '\u{E0001}'..='\u{E007F}' // tag characters
        | '\u{FE00}'..='\u{FE0F}'   // variation selectors
    )
}

/// Strip dangerous control characters from text.
///
/// Returns `Cow::Borrowed` if no changes are needed (zero allocation).
fn do_strip_control_chars(input: &str) -> Cow<'_, str> {
    if !input.chars().any(is_dangerous_control_char) {
        return Cow::Borrowed(input);
    }
    Cow::Owned(
        input
            .chars()
            .filter(|c| !is_dangerous_control_char(*c))
            .collect(),
    )
}

/// Apply Unicode NFKC normalization.
///
/// Fast path: if the text is already in NFKC form, returns `Cow::Borrowed`.
fn normalize_nfkc(input: &str) -> Cow<'_, str> {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::{is_nfkc_quick, IsNormalized};

    match is_nfkc_quick(input.chars()) {
        IsNormalized::Yes => Cow::Borrowed(input),
        _ => {
            let normalized: String = input.nfkc().collect();
            if normalized == input {
                Cow::Borrowed(input)
            } else {
                Cow::Owned(normalized)
            }
        }
    }
}

/// Strip HTML tags using `lol_html` streaming parser.
///
/// `<script>` and `<style>` elements are removed entirely (including content).
/// All other tags are removed but their text content is preserved.
#[cfg(feature = "normalization-html")]
fn strip_html_lol(input: &str) -> Result<String, String> {
    use lol_html::{element, HtmlRewriter, Settings};

    let mut output = Vec::with_capacity(input.len());

    {
        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![element!("*", |el| {
                    let tag = el.tag_name();
                    if tag == "script" || tag == "style" {
                        el.remove();
                    } else {
                        el.remove_and_keep_content();
                    }
                    Ok(())
                })],
                ..Settings::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        );

        rewriter
            .write(input.as_bytes())
            .map_err(|e| e.to_string())?;
        rewriter.end().map_err(|e| e.to_string())?;
    }

    String::from_utf8(output).map_err(|e| e.to_string())
}

/// Regex-based fallback for HTML stripping.
fn strip_html_regex(input: &str) -> Cow<'_, str> {
    if !input.contains('<') {
        return Cow::Borrowed(input);
    }

    use regex::Regex;
    use std::sync::LazyLock;

    static SCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<script\b[^>]*>.*?</script\s*>").unwrap()
    });
    static STYLE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<style\b[^>]*>.*?</style\s*>").unwrap()
    });
    static TAG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());

    let after_scripts: String = SCRIPT_RE.replace_all(input, "").into_owned();
    let after_styles: String = STYLE_RE.replace_all(&after_scripts, "").into_owned();
    let result: String = TAG_RE.replace_all(&after_styles, "").into_owned();

    if result == input {
        Cow::Borrowed(input)
    } else {
        Cow::Owned(result)
    }
}

/// Strip HTML from text, using the best available method.
fn do_strip_html(input: &str) -> Cow<'_, str> {
    #[cfg(feature = "normalization-html")]
    {
        match strip_html_lol(input) {
            Ok(result) if result == input => Cow::Borrowed(input),
            Ok(result) => Cow::Owned(result),
            Err(_) => strip_html_regex(input),
        }
    }
    #[cfg(not(feature = "normalization-html"))]
    {
        strip_html_regex(input)
    }
}

/// Truncate text to `max_bytes` at a UTF-8 character boundary.
fn truncate_text(input: &str, max_bytes: usize) -> Cow<'_, str> {
    if input.len() <= max_bytes {
        return Cow::Borrowed(input);
    }
    // Find the largest index <= max_bytes that is a char boundary.
    let mut boundary = max_bytes;
    while boundary > 0 && !input.is_char_boundary(boundary) {
        boundary -= 1;
    }
    Cow::Owned(input[..boundary].to_string())
}

// ── Script mixing detection ────────────────────────────────────────────

/// Rough script classification for homoglyph detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptClass {
    Latin,
    Cyrillic,
    Other,
}

fn classify_script(c: char) -> ScriptClass {
    if matches!(c, 'A'..='Z' | 'a'..='z' | '\u{00C0}'..='\u{024F}') {
        ScriptClass::Latin
    } else if matches!(c, '\u{0400}'..='\u{04FF}' | '\u{0500}'..='\u{052F}') {
        ScriptClass::Cyrillic
    } else {
        ScriptClass::Other
    }
}

/// Detect words that mix Latin and Cyrillic characters.
fn detect_script_mixing(input: &str) -> Vec<ScriptMixingWarning> {
    let mut warnings = Vec::new();

    for (word_start, word) in split_words(input) {
        let mut has_latin = false;
        let mut has_cyrillic = false;

        for c in word.chars() {
            match classify_script(c) {
                ScriptClass::Latin => has_latin = true,
                ScriptClass::Cyrillic => has_cyrillic = true,
                ScriptClass::Other => {}
            }
            if has_latin && has_cyrillic {
                warnings.push(ScriptMixingWarning {
                    position: word_start..word_start + word.len(),
                    scripts_found: vec!["Latin".to_string(), "Cyrillic".to_string()],
                });
                break;
            }
        }
    }

    warnings
}

/// Split input into `(byte_offset, word_str)` pairs.
fn split_words(input: &str) -> Vec<(usize, &str)> {
    let mut words = Vec::new();
    let mut start = None;

    for (i, c) in input.char_indices() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            words.push((s, &input[s..i]));
        }
    }
    if let Some(s) = start {
        words.push((s, &input[s..]));
    }

    words
}

// ── Unified normalization pipeline ─────────────────────────────────────

/// Apply the full normalization pipeline to a single text string.
///
/// Returns `(normalized_text, changed, script_warnings)`.
fn normalize_text<'a>(
    input: &'a str,
    config: &NormalizationConfig,
) -> (Cow<'a, str>, bool, Vec<ScriptMixingWarning>) {
    let mut current = Cow::Borrowed(input);
    let mut changed = false;

    // 1. Truncate
    if config.truncate {
        let truncated = truncate_text(&current, config.max_content_bytes);
        if let Cow::Owned(s) = truncated {
            changed = true;
            current = Cow::Owned(s);
        }
    }

    // 2. Strip control chars
    if config.strip_control_chars {
        let stripped = do_strip_control_chars(&current);
        if let Cow::Owned(s) = stripped {
            changed = true;
            current = Cow::Owned(s);
        }
    }

    // 3. NFKC normalization
    if config.normalize_unicode {
        let normalized = normalize_nfkc(&current);
        if let Cow::Owned(s) = normalized {
            changed = true;
            current = Cow::Owned(s);
        }
    }

    // 4. HTML stripping
    if config.strip_html {
        let stripped = do_strip_html(&current);
        if let Cow::Owned(s) = stripped {
            changed = true;
            current = Cow::Owned(s);
        }
    }

    // 5. Script mixing detection (non-blocking, metadata only)
    let warnings = if config.detect_script_mixing {
        detect_script_mixing(&current)
    } else {
        Vec::new()
    };

    (current, changed, warnings)
}

/// Recursively normalize string values in a JSON tree.
fn normalize_json_value(
    value: &serde_json::Value,
    config: &NormalizationConfig,
) -> (serde_json::Value, bool) {
    match value {
        serde_json::Value::String(s) => {
            let (normalized, changed, _) = normalize_text(s, config);
            (serde_json::Value::String(normalized.into_owned()), changed)
        }
        serde_json::Value::Array(arr) => {
            let mut any_changed = false;
            let new_arr: Vec<_> = arr
                .iter()
                .map(|v| {
                    let (nv, c) = normalize_json_value(v, config);
                    any_changed |= c;
                    nv
                })
                .collect();
            (serde_json::Value::Array(new_arr), any_changed)
        }
        serde_json::Value::Object(obj) => {
            let mut any_changed = false;
            let new_obj: serde_json::Map<_, _> = obj
                .iter()
                .map(|(k, v)| {
                    let (nv, c) = normalize_json_value(v, config);
                    any_changed |= c;
                    (k.clone(), nv)
                })
                .collect();
            (serde_json::Value::Object(new_obj), any_changed)
        }
        other => (other.clone(), false),
    }
}

// ── GuardrailStage implementation ──────────────────────────────────────

#[async_trait]
impl GuardrailStage for NormalizationStage {
    fn id(&self) -> &str {
        "normalization"
    }

    fn priority(&self) -> u32 {
        10
    }

    fn degradable(&self) -> bool {
        true
    }

    async fn evaluate(
        &self,
        content: &Content,
        _ctx: &SecurityContext,
    ) -> Result<StageOutcome, StageError> {
        match content {
            Content::Text(text) => {
                let (normalized, changed, _warnings) =
                    normalize_text(text, &self.config);
                if changed {
                    Ok(StageOutcome::transform(
                        Content::Text(normalized.into_owned()),
                        "normalization applied",
                    ))
                } else {
                    Ok(StageOutcome::allow(1.0))
                }
            }

            Content::Messages(msgs) => {
                let mut any_changed = false;
                let new_msgs: Vec<Message> = msgs
                    .iter()
                    .map(|m| {
                        let (normalized, changed, _) =
                            normalize_text(&m.content, &self.config);
                        if changed {
                            any_changed = true;
                            Message {
                                role: m.role.clone(),
                                content: normalized.into_owned(),
                            }
                        } else {
                            m.clone()
                        }
                    })
                    .collect();

                if any_changed {
                    Ok(StageOutcome::transform(
                        Content::Messages(new_msgs),
                        "normalization applied to messages",
                    ))
                } else {
                    Ok(StageOutcome::allow(1.0))
                }
            }

            Content::RetrievedChunks(chunks) => {
                let mut any_changed = false;
                let new_chunks: Vec<RetrievedChunk> = chunks
                    .iter()
                    .map(|c| {
                        let (normalized, changed, _) =
                            normalize_text(&c.text, &self.config);
                        if changed {
                            any_changed = true;
                            RetrievedChunk {
                                text: normalized.into_owned(),
                                score: c.score,
                                source: c.source.clone(),
                                metadata: c.metadata.clone(),
                            }
                        } else {
                            c.clone()
                        }
                    })
                    .collect();

                if any_changed {
                    Ok(StageOutcome::transform(
                        Content::RetrievedChunks(new_chunks),
                        "normalization applied to retrieved chunks",
                    ))
                } else {
                    Ok(StageOutcome::allow(1.0))
                }
            }

            Content::ToolCall {
                tool_name,
                arguments,
            } => {
                let (new_args, changed) =
                    normalize_json_value(arguments, &self.config);
                if changed {
                    Ok(StageOutcome::transform(
                        Content::ToolCall {
                            tool_name: tool_name.clone(),
                            arguments: new_args,
                        },
                        "normalization applied to tool call arguments",
                    ))
                } else {
                    Ok(StageOutcome::allow(1.0))
                }
            }

            Content::ToolResult { tool_name, result } => {
                let (new_result, changed) =
                    normalize_json_value(result, &self.config);
                if changed {
                    Ok(StageOutcome::transform(
                        Content::ToolResult {
                            tool_name: tool_name.clone(),
                            result: new_result,
                        },
                        "normalization applied to tool result",
                    ))
                } else {
                    Ok(StageOutcome::allow(1.0))
                }
            }

            #[allow(unreachable_patterns)]
            _ => Ok(StageOutcome::skip("unsupported content variant")),
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }

    fn ctx() -> SecurityContext {
        SecurityContext::default()
    }

    // 1. NFKC normalization: ligature → decomposed
    #[tokio::test]
    async fn nfkc_normalization_ligature() {
        let stage = NormalizationStage::with_defaults();
        let content = text("\u{FB01}nd"); // "ﬁnd"
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            assert_eq!(content.as_text().as_ref(), "find");
        }
    }

    // 2. Control char stripping: ZWSP
    #[tokio::test]
    async fn strip_zwsp() {
        let stage = NormalizationStage::with_defaults();
        let content = text("hello\u{200B}world");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            assert_eq!(content.as_text().as_ref(), "helloworld");
        }
    }

    // 3. Bidi control removal
    #[tokio::test]
    async fn strip_bidi_controls() {
        let stage = NormalizationStage::with_defaults();
        let content = text("abc\u{202A}def\u{202C}ghi");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            assert_eq!(content.as_text().as_ref(), "abcdefghi");
        }
    }

    // 4. HTML stripping (works with both regex and lol_html backends)
    #[tokio::test]
    async fn strip_html_bold_tag() {
        let stage = NormalizationStage::with_defaults();
        let content = text("<b>hello</b>");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            assert_eq!(content.as_text().as_ref(), "hello");
        }
    }

    // 5. Script tag removal
    #[tokio::test]
    async fn strip_script_tag() {
        let stage = NormalizationStage::with_defaults();
        let content = text("<script>alert('xss')</script>text");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            assert_eq!(content.as_text().as_ref(), "text");
        }
    }

    // 6. Truncation at UTF-8 boundary
    #[tokio::test]
    async fn truncation_utf8_boundary() {
        let config = NormalizationConfig::new()
            .max_content_bytes(5)
            .strip_html(false)
            .normalize_unicode(false)
            .strip_control_chars(false);
        let stage = NormalizationStage::new(config);
        // "héllo" — 'é' is 2 bytes, total is 6 bytes. Truncate at 5 should
        // keep "hél" (4 bytes: h + é(2) + l) since floor_char_boundary(5)
        // lands on byte 4 (the start of the second 'l').
        let content = text("h\u{00E9}llo");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            let t = content.as_text().into_owned();
            assert!(t.len() <= 5);
            assert!(t.is_char_boundary(t.len()));
        }
    }

    // 7. Script mixing detection
    #[test]
    fn detect_latin_cyrillic_mixing() {
        // "pаypal" — the 'а' (U+0430) is Cyrillic, rest is Latin
        let input = "p\u{0430}ypal";
        let warnings = detect_script_mixing(input);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].scripts_found.contains(&"Latin".to_string()));
        assert!(warnings[0].scripts_found.contains(&"Cyrillic".to_string()));
    }

    // 8. Already-normalized text returns Allow
    #[tokio::test]
    async fn already_normalized_returns_allow() {
        let stage = NormalizationStage::with_defaults();
        let content = text("plain ascii text");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_allow());
    }

    // 9. Empty input returns Allow
    #[tokio::test]
    async fn empty_input_returns_allow() {
        let stage = NormalizationStage::with_defaults();
        let content = text("");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_allow());
    }

    // 10. Content::Messages normalization
    #[tokio::test]
    async fn messages_normalization() {
        let stage = NormalizationStage::with_defaults();
        let content = Content::Messages(vec![
            Message::user("hello\u{200B}world"),
            Message::assistant("clean text"),
        ]);
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform {
            content: Content::Messages(msgs),
            ..
        } = outcome
        {
            assert_eq!(msgs[0].content, "helloworld");
            assert_eq!(msgs[1].content, "clean text");
        } else {
            panic!("expected Transform with Messages");
        }
    }

    // 11. Content::RetrievedChunks normalization
    #[tokio::test]
    async fn retrieved_chunks_normalization() {
        let stage = NormalizationStage::with_defaults();
        let content = Content::RetrievedChunks(vec![
            RetrievedChunk::new("chunk\u{200B}one", 0.9),
            RetrievedChunk::new("chunk two", 0.8),
        ]);
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform {
            content: Content::RetrievedChunks(chunks),
            ..
        } = outcome
        {
            assert_eq!(chunks[0].text, "chunkone");
            assert_eq!(chunks[1].text, "chunk two");
        } else {
            panic!("expected Transform with RetrievedChunks");
        }
    }

    // 12. Combined normalization: HTML + control chars + NFKC
    #[tokio::test]
    async fn combined_normalization() {
        let stage = NormalizationStage::with_defaults();
        // ZWSP + ligature + HTML tag
        let content = text("\u{200B}\u{FB01}nd <em>it</em>");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            assert_eq!(content.as_text().as_ref(), "find it");
        }
    }

    // 13. Stage metadata
    #[test]
    fn stage_metadata() {
        let stage = NormalizationStage::with_defaults();
        assert_eq!(stage.id(), "normalization");
        assert_eq!(stage.priority(), 10);
        assert!(stage.degradable());
    }

    // 14. Config builder defaults
    #[test]
    fn config_defaults() {
        let config = NormalizationConfig::new();
        assert_eq!(config.max_content_bytes, 1_048_576);
        assert!(config.strip_html);
        assert!(config.normalize_unicode);
        assert!(config.strip_control_chars);
        assert!(config.detect_script_mixing);
        assert!(config.truncate);
    }

    // 15. Soft hyphen removal
    #[tokio::test]
    async fn strip_soft_hyphen() {
        let stage = NormalizationStage::with_defaults();
        let content = text("pass\u{00AD}word");
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
        if let StageOutcome::Transform { content, .. } = outcome {
            assert_eq!(content.as_text().as_ref(), "password");
        }
    }

    // 16. ToolCall normalization
    #[tokio::test]
    async fn tool_call_normalization() {
        let stage = NormalizationStage::with_defaults();
        let content = Content::ToolCall {
            tool_name: "search".into(),
            arguments: serde_json::json!({"query": "hello\u{200B}world"}),
        };
        let outcome = stage.evaluate(&content, &ctx()).await.unwrap();
        assert!(outcome.is_transform());
    }

    // Fast-path unit tests for internal functions
    #[test]
    fn nfkc_fast_path_ascii() {
        let result = normalize_nfkc("plain ascii");
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn control_char_fast_path_clean() {
        let result = do_strip_control_chars("no control chars here");
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn truncate_no_op_when_under_limit() {
        let result = truncate_text("short", 1_048_576);
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn no_script_mixing_in_pure_latin() {
        let warnings = detect_script_mixing("hello world");
        assert!(warnings.is_empty());
    }
}
