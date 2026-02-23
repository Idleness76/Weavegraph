//! Content types flowing through the security pipeline.
//!
//! [`Content`] is the core unit of inspection — every guardrail stage receives
//! a `Content` value and evaluates it against its rules.  The enum is
//! intentionally **non-exhaustive** so new modalities can be added without a
//! breaking change.
//!
//! # Design rationale
//!
//! The previous `SecurityStage::execute(&str, …)` API accepted only plain text.
//! Real LLM applications pass structured messages, tool calls, RAG chunks, and
//! multimodal blobs.  `Content` captures all of these while remaining
//! `Clone + Debug + Send + Sync` for safe async pipeline usage.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write;

// ── Content ────────────────────────────────────────────────────────────

/// The payload being inspected by a guardrail stage.
///
/// Each variant represents a different *shape* of data that passes through
/// an LLM application.  Stages can pattern-match to decide relevance:
///
/// ```rust
/// use wg_bastion::pipeline::content::Content;
///
/// let c = Content::Text("hello".into());
/// assert!(matches!(c, Content::Text(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Content {
    /// Plain text — user prompts, assistant responses, system messages.
    Text(String),

    /// Structured chat messages with role annotations.
    Messages(Vec<Message>),

    /// A request to invoke a tool (pre-execution).
    ToolCall {
        /// Canonical tool name (e.g. `"web_search"`, `"code_interpreter"`).
        tool_name: String,
        /// Tool arguments as an arbitrary JSON value.
        arguments: serde_json::Value,
    },

    /// The result returned from a tool invocation (post-execution).
    ToolResult {
        /// Canonical tool name.
        tool_name: String,
        /// Tool output as an arbitrary JSON value.
        result: serde_json::Value,
    },

    /// Chunks retrieved from a vector store or RAG pipeline.
    RetrievedChunks(Vec<RetrievedChunk>),
}

impl Content {
    /// Returns a human-readable label for the content variant.
    ///
    /// Useful for logging and metrics without exposing payload data.
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Messages(_) => "messages",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolResult { .. } => "tool_result",
            Self::RetrievedChunks(_) => "retrieved_chunks",
        }
    }

    /// Extracts the plaintext surface of the content for stages that operate
    /// on raw strings (e.g. regex-based injection detection).
    ///
    /// Returns `Cow::Borrowed` for the [`Text`](Self::Text) variant (zero-copy),
    /// and `Cow::Owned` for structured variants where a lossy flattening is
    /// computed.  The result is suitable for heuristic scanning but not for
    /// faithful reproduction.
    #[must_use]
    pub fn as_text(&self) -> Cow<'_, str> {
        match self {
            Self::Text(s) => Cow::Borrowed(s),
            Self::Messages(msgs) => {
                let mut buf = String::new();
                for (i, m) in msgs.iter().enumerate() {
                    if i > 0 {
                        buf.push('\n');
                    }
                    // `write!` on `String` is infallible.
                    let _ = write!(buf, "[{}] {}", m.role, m.content);
                }
                Cow::Owned(buf)
            }
            Self::ToolCall {
                tool_name,
                arguments,
            } => Cow::Owned(format!("tool_call:{tool_name} {arguments}")),
            Self::ToolResult { tool_name, result } => {
                Cow::Owned(format!("tool_result:{tool_name} {result}"))
            }
            Self::RetrievedChunks(chunks) => {
                let mut buf = String::new();
                for (i, c) in chunks.iter().enumerate() {
                    if i > 0 {
                        buf.push('\n');
                    }
                    buf.push_str(&c.text);
                }
                Cow::Owned(buf)
            }
        }
    }
}

// ── Message ────────────────────────────────────────────────────────────

/// A single chat message with role and content.
///
/// Mirrors the standard `{role, content}` shape used by `OpenAI`, Anthropic,
/// and most LLM API providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Role identifier — typically `"system"`, `"user"`, or `"assistant"`.
    pub role: String,
    /// Textual content of the message.
    pub content: String,
}

impl Message {
    /// Create a new message.
    #[must_use]
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    /// Shorthand for a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", content)
    }

    /// Shorthand for a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    /// Shorthand for an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }
}

// ── RetrievedChunk ─────────────────────────────────────────────────────

/// A chunk of text retrieved from a RAG corpus, with provenance metadata.
///
/// Provenance fields are optional because not all vector stores track them.
/// The security pipeline uses them for trust scoring and audit logging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedChunk {
    /// The chunk text.
    pub text: String,

    /// Similarity / relevance score from the vector search (0.0–1.0).
    pub score: Option<f64>,

    /// Source identifier (URL, file path, document ID, etc.).
    pub source: Option<String>,

    /// Arbitrary provenance metadata (domain hash, ingestion date, …).
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl RetrievedChunk {
    /// Create a chunk with only text and score.
    #[must_use]
    pub fn new(text: impl Into<String>, score: f64) -> Self {
        Self {
            text: text.into(),
            score: Some(score),
            source: None,
            metadata: HashMap::new(),
        }
    }

    /// Attach a source identifier.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Attach a single metadata entry.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_variant_name() {
        let c = Content::Text("hello".into());
        assert_eq!(c.variant_name(), "text");
    }

    #[test]
    fn messages_as_text_includes_roles() {
        let c = Content::Messages(vec![
            Message::system("You are helpful."),
            Message::user("Hi!"),
        ]);
        let flat = c.as_text();
        assert!(flat.contains("[system]"));
        assert!(flat.contains("[user]"));
        assert!(flat.contains("You are helpful."));
    }

    #[test]
    fn tool_call_as_text_contains_name() {
        let c = Content::ToolCall {
            tool_name: "web_search".into(),
            arguments: serde_json::json!({"query": "rust"}),
        };
        assert!(c.as_text().contains("web_search"));
    }

    #[test]
    fn retrieved_chunks_as_text_joins() {
        let c = Content::RetrievedChunks(vec![
            RetrievedChunk::new("chunk one", 0.9),
            RetrievedChunk::new("chunk two", 0.8),
        ]);
        let flat = c.as_text();
        assert!(flat.contains("chunk one"));
        assert!(flat.contains("chunk two"));
    }

    #[test]
    fn content_round_trips_json() {
        let original = Content::Text("round-trip test".into());
        let json = serde_json::to_string(&original).unwrap();
        let restored: Content = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored, Content::Text(s) if s == "round-trip test"));
    }

    #[test]
    fn message_constructors() {
        let m = Message::user("hello");
        assert_eq!(m.role, "user");
        assert_eq!(m.content, "hello");
    }

    #[test]
    fn retrieved_chunk_builder() {
        let chunk = RetrievedChunk::new("text", 0.95)
            .with_source("https://example.com")
            .with_metadata("domain_hash", "abc123");
        assert_eq!(chunk.source.as_deref(), Some("https://example.com"));
        assert_eq!(chunk.metadata.get("domain_hash").unwrap(), "abc123");
    }
}
