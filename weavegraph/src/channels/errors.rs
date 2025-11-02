use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::telemetry::{FormatterMode, PlainFormatter, TelemetryFormatter};

// Avoid depending on serde for NodeKind by using encoded string form for kind.

/// Represents an error event with scope, error details, tags, and context.
///
/// # JSON Serialization Format
///
/// `ErrorEvent` serializes to JSON with the following structure:
///
/// ```json
/// {
///   "when": "2025-11-02T10:30:00Z",
///   "scope": {
///     "scope": "node",
///     "kind": "Parser",
///     "step": 1
///   },
///   "error": {
///     "message": "Failed to parse input",
///     "cause": {
///       "message": "Invalid JSON syntax",
///       "cause": null,
///       "details": {"line": 3, "column": 15}
///     },
///     "details": {"input_length": 1024}
///   },
///   "tags": ["validation", "retryable"],
///   "context": {
///     "file": "/tmp/input.json",
///     "user_id": 12345
///   }
/// }
/// ```
///
/// The `scope` field uses a tagged union format with a discriminator field named `"scope"`.
/// Supported scope variants are:
/// - `"node"`: Requires `kind` (string) and `step` (u64)
/// - `"scheduler"`: Requires `step` (u64)
/// - `"runner"`: Requires `session` (string) and `step` (u64)
/// - `"app"`: No additional fields
///
/// See `docs/schemas/error_event.json` for the complete JSON Schema specification.
///
/// # Examples
///
/// Using constructors and builders:
///
/// ```
/// use weavegraph::channels::errors::{ErrorEvent, LadderError};
/// use serde_json::json;
///
/// let event = ErrorEvent::node("Parser", 1, LadderError::msg("Parse error"))
///     .with_tag("validation")
///     .with_context(json!({"line": 42}));
///
/// // Serialize to JSON
/// let json_str = serde_json::to_string(&event).unwrap();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ErrorEvent {
    #[serde(default = "chrono::Utc::now")]
    pub when: DateTime<Utc>,
    #[serde(default)]
    pub scope: ErrorScope,
    #[serde(default)]
    pub error: LadderError,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub context: serde_json::Value,
}

impl ErrorEvent {
    /// Create a node-scoped error event.
    ///
    /// # Example
    /// ```
    /// use weavegraph::channels::errors::{ErrorEvent, LadderError};
    ///
    /// let err = ErrorEvent::node("my_node", 1, LadderError::msg("Something failed"));
    /// ```
    pub fn node<S: Into<String>>(kind: S, step: u64, error: LadderError) -> Self {
        Self {
            when: Utc::now(),
            scope: ErrorScope::Node {
                kind: kind.into(),
                step,
            },
            error,
            tags: Vec::new(),
            context: serde_json::Value::Null,
        }
    }

    /// Create a scheduler-scoped error event.
    ///
    /// # Example
    /// ```
    /// use weavegraph::channels::errors::{ErrorEvent, LadderError};
    ///
    /// let err = ErrorEvent::scheduler(5, LadderError::msg("Scheduling conflict"));
    /// ```
    pub fn scheduler(step: u64, error: LadderError) -> Self {
        Self {
            when: Utc::now(),
            scope: ErrorScope::Scheduler { step },
            error,
            tags: Vec::new(),
            context: serde_json::Value::Null,
        }
    }

    /// Create a runner-scoped error event.
    ///
    /// # Example
    /// ```
    /// use weavegraph::channels::errors::{ErrorEvent, LadderError};
    ///
    /// let err = ErrorEvent::runner("session_123", 10, LadderError::msg("Runtime error"));
    /// ```
    pub fn runner<S: Into<String>>(session: S, step: u64, error: LadderError) -> Self {
        Self {
            when: Utc::now(),
            scope: ErrorScope::Runner {
                session: session.into(),
                step,
            },
            error,
            tags: Vec::new(),
            context: serde_json::Value::Null,
        }
    }

    /// Create an app-scoped error event.
    ///
    /// # Example
    /// ```
    /// use weavegraph::channels::errors::{ErrorEvent, LadderError};
    ///
    /// let err = ErrorEvent::app(LadderError::msg("Application startup failed"));
    /// ```
    pub fn app(error: LadderError) -> Self {
        Self {
            when: Utc::now(),
            scope: ErrorScope::App,
            error,
            tags: Vec::new(),
            context: serde_json::Value::Null,
        }
    }

    /// Add multiple tags to this error event.
    ///
    /// # Example
    /// ```
    /// use weavegraph::channels::errors::{ErrorEvent, LadderError};
    ///
    /// let err = ErrorEvent::node("my_node", 1, LadderError::msg("Invalid input"))
    ///     .with_tags(vec!["validation".to_string(), "critical".to_string()]);
    /// ```
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Add a single tag to this error event.
    ///
    /// # Example
    /// ```
    /// use weavegraph::channels::errors::{ErrorEvent, LadderError};
    ///
    /// let err = ErrorEvent::node("my_node", 1, LadderError::msg("Invalid input"))
    ///     .with_tag("validation");
    /// ```
    pub fn with_tag<S: Into<String>>(mut self, tag: S) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Add context metadata to this error event.
    ///
    /// # Example
    /// ```
    /// use weavegraph::channels::errors::{ErrorEvent, LadderError};
    /// use serde_json::json;
    ///
    /// let err = ErrorEvent::node("my_node", 1, LadderError::msg("Invalid input"))
    ///     .with_context(json!({"field": "username", "value": ""}));
    /// ```
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = context;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ErrorScope {
    Node {
        kind: String,
        step: u64,
    },
    Scheduler {
        step: u64,
    },
    Runner {
        session: String,
        step: u64,
    },
    #[default]
    App,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LadderError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<LadderError>>,
    #[serde(default)]
    pub details: serde_json::Value,
}

impl Default for LadderError {
    fn default() -> Self {
        LadderError {
            message: String::new(),
            cause: None,
            details: serde_json::Value::Null,
        }
    }
}

impl std::fmt::Display for LadderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LadderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|c| c as &dyn std::error::Error)
    }
}

impl LadderError {
    pub fn msg<M: Into<String>>(m: M) -> Self {
        LadderError {
            message: m.into(),
            cause: None,
            details: serde_json::Value::Null,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    pub fn with_cause(mut self, cause: LadderError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

/// Format error events with explicit color mode control.
///
/// This function allows you to control whether ANSI color codes are included in the output:
/// - [`FormatterMode::Auto`]: Auto-detects TTY capability (checks stderr)
/// - [`FormatterMode::Colored`]: Always includes color codes
/// - [`FormatterMode::Plain`]: Never includes color codes
///
/// # Examples
///
/// ```
/// use weavegraph::channels::errors::{ErrorEvent, LadderError, pretty_print_with_mode};
/// use weavegraph::telemetry::FormatterMode;
///
/// let events = vec![
///     ErrorEvent::node("parser", 1, LadderError::msg("Parse failed"))
/// ];
///
/// // Force plain output (no colors) for log files
/// let plain = pretty_print_with_mode(&events, FormatterMode::Plain);
/// assert!(!plain.contains("\x1b[")); // No ANSI codes
///
/// // Force colored output
/// let colored = pretty_print_with_mode(&events, FormatterMode::Colored);
/// ```
pub fn pretty_print_with_mode(events: &[ErrorEvent], mode: FormatterMode) -> String {
    let formatter = PlainFormatter::with_mode(mode);
    let renders = formatter.render_errors(events);
    let mut out = String::new();
    for (idx, render) in renders.into_iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        for line in render.lines {
            out.push_str(&line);
        }
    }
    out
}

/// Format error events as human-readable text with auto-detected color support.
///
/// Colors are automatically enabled when stderr is a TTY and disabled otherwise.
/// For explicit control over color output, use [`pretty_print_with_mode`].
///
/// # Examples
///
/// ```
/// use weavegraph::channels::errors::{ErrorEvent, LadderError, pretty_print};
///
/// let events = vec![
///     ErrorEvent::node("parser", 1, LadderError::msg("Parse failed"))
/// ];
///
/// let output = pretty_print(&events);
/// // Colors automatically detected based on stderr TTY status
/// ```
pub fn pretty_print(events: &[ErrorEvent]) -> String {
    pretty_print_with_mode(events, FormatterMode::Auto)
}
