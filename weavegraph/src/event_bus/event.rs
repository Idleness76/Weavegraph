use std::fmt;

use chrono::{DateTime, Utc};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STREAM_END_SCOPE: &str = "__weavegraph_stream_end__";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Event {
    Node(NodeEvent),
    Diagnostic(DiagnosticEvent),
    LLM(LLMStreamingEvent),
}

impl Event {
    pub fn node_message(scope: impl Into<String>, message: impl Into<String>) -> Self {
        Event::Node(NodeEvent::new(None, None, scope.into(), message.into()))
    }

    pub fn node_message_with_meta(
        node_id: impl Into<String>,
        step: u64,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Event::Node(NodeEvent::new(
            Some(node_id.into()),
            Some(step),
            scope.into(),
            message.into(),
        ))
    }

    pub fn diagnostic(scope: impl Into<String>, message: impl Into<String>) -> Self {
        Event::Diagnostic(DiagnosticEvent {
            scope: scope.into(),
            message: message.into(),
        })
    }

    pub fn scope_label(&self) -> Option<&str> {
        match self {
            Event::Node(node) => Some(node.scope()),
            Event::Diagnostic(diag) => Some(diag.scope()),
            Event::LLM(llm) => Some(llm.scope().as_ref()),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Event::Node(node) => node.message(),
            Event::Diagnostic(diag) => diag.message(),
            Event::LLM(llm) => llm.chunk(),
        }
    }

    /// Convert event to structured JSON value with normalized schema.
    ///
    /// Returns a JSON object with the following structure:
    /// ```json
    /// {
    ///   "type": "node" | "diagnostic" | "llm",
    ///   "scope": "scope_label",
    ///   "message": "event_message",
    ///   "timestamp": "2025-11-03T12:34:56.789Z",
    ///   "metadata": { /* variant-specific fields */ }
    /// }
    /// ```
    ///
    /// # Example
    ///
    /// ```
    /// use weavegraph::event_bus::Event;
    ///
    /// let event = Event::node_message_with_meta("router", 5, "routing", "Processing request");
    /// let json = event.to_json_value();
    ///
    /// assert_eq!(json["type"], "node");
    /// assert_eq!(json["scope"], "routing");
    /// assert_eq!(json["message"], "Processing request");
    /// assert_eq!(json["metadata"]["node_id"], "router");
    /// assert_eq!(json["metadata"]["step"], 5);
    /// ```
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::json;

        let (event_type, metadata) = match self {
            Event::Node(node) => {
                let mut meta = serde_json::Map::new();
                if let Some(node_id) = node.node_id() {
                    meta.insert("node_id".to_string(), json!(node_id));
                }
                if let Some(step) = node.step() {
                    meta.insert("step".to_string(), json!(step));
                }
                ("node", Value::Object(meta))
            }
            Event::Diagnostic(_) => {
                let meta = serde_json::Map::new();
                ("diagnostic", Value::Object(meta))
            }
            Event::LLM(llm) => {
                let mut meta = serde_json::Map::new();
                if let Some(session_id) = llm.session_id() {
                    meta.insert("session_id".to_string(), json!(session_id));
                }
                if let Some(node_id) = llm.node_id() {
                    meta.insert("node_id".to_string(), json!(node_id));
                }
                if let Some(stream_id) = llm.stream_id() {
                    meta.insert("stream_id".to_string(), json!(stream_id));
                }
                meta.insert("is_final".to_string(), json!(llm.is_final()));

                // Include LLM metadata fields
                for (key, value) in llm.metadata() {
                    meta.insert(key.clone(), value.clone());
                }

                ("llm", Value::Object(meta))
            }
        };

        let timestamp = match self {
            Event::LLM(llm) => llm.timestamp(),
            _ => Utc::now(),
        };

        json!({
            "type": event_type,
            "scope": self.scope_label(),
            "message": self.message(),
            "timestamp": timestamp.to_rfc3339(),
            "metadata": metadata,
        })
    }

    /// Convert event to compact JSON string representation.
    ///
    /// # Example
    ///
    /// ```
    /// use weavegraph::event_bus::Event;
    ///
    /// let event = Event::diagnostic("test", "message");
    /// let json_str = event.to_json_string().unwrap();
    /// assert!(json_str.contains("\"type\":\"diagnostic\""));
    /// ```
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.to_json_value())
    }

    /// Convert event to pretty-printed JSON string with indentation.
    ///
    /// Useful for debugging and log files where human readability is important.
    ///
    /// # Example
    ///
    /// ```
    /// use weavegraph::event_bus::Event;
    ///
    /// let event = Event::node_message("test", "hello");
    /// let json_str = event.to_json_pretty().unwrap();
    /// assert!(json_str.contains("  \"type\": \"node\""));
    /// ```
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.to_json_value())
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Node(node) => match (node.node_id(), node.step()) {
                (Some(id), Some(step)) => write!(f, "[{id}@{step}] {}", node.message()),
                (Some(id), None) => write!(f, "[{id}] {}", node.message()),
                (None, Some(step)) => write!(f, "[step {step}] {}", node.message()),
                (None, None) => write!(f, "{}", node.message()),
            },
            Event::Diagnostic(diag) => write!(f, "{}", diag.message()),
            Event::LLM(llm) => {
                if let Some(stream_id) = llm.stream_id() {
                    write!(f, "[LLM {stream_id}] {}", llm.chunk())
                } else if let Some(node_id) = llm.node_id() {
                    write!(f, "[LLM {node_id}] {}", llm.chunk())
                } else {
                    write!(f, "{}", llm.chunk())
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeEvent {
    node_id: Option<String>,
    step: Option<u64>,
    scope: String,
    message: String,
}

impl NodeEvent {
    pub fn new(node_id: Option<String>, step: Option<u64>, scope: String, message: String) -> Self {
        Self {
            node_id,
            step,
            scope,
            message,
        }
    }

    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }

    pub fn step(&self) -> Option<u64> {
        self.step
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticEvent {
    scope: String,
    message: String,
}

impl DiagnosticEvent {
    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum LLMStreamingEventScope {
    Streaming,
    Chunk,
    Final,
    Error,
}

impl AsRef<str> for LLMStreamingEventScope {
    fn as_ref(&self) -> &str {
        match self {
            LLMStreamingEventScope::Chunk => "chunk",
            LLMStreamingEventScope::Streaming => "stream",
            LLMStreamingEventScope::Final => STREAM_END_SCOPE,
            LLMStreamingEventScope::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LLMStreamingEvent {
    session_id: Option<String>,
    node_id: Option<String>,
    stream_id: Option<String>,
    chunk: String,
    is_final: bool,
    scope: LLMStreamingEventScope,
    metadata: FxHashMap<String, Value>,
    timestamp: DateTime<Utc>,
}

impl LLMStreamingEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: Option<String>,
        node_id: Option<String>,
        stream_id: Option<String>,
        chunk: impl Into<String>,
        is_final: bool,
        scope: Option<LLMStreamingEventScope>,
        metadata: FxHashMap<String, Value>,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            session_id,
            node_id,
            stream_id,
            chunk: chunk.into(),
            is_final,
            scope: scope.unwrap_or(LLMStreamingEventScope::Streaming),
            metadata,
            timestamp,
        }
    }

    pub fn chunk_event(
        session_id: Option<String>,
        node_id: Option<String>,
        stream_id: Option<String>,
        chunk: impl Into<String>,
        metadata: FxHashMap<String, Value>,
    ) -> Self {
        Self::new(
            session_id,
            node_id,
            stream_id,
            chunk,
            false,
            Some(LLMStreamingEventScope::Chunk),
            metadata,
            Utc::now(),
        )
    }

    pub fn final_event(
        session_id: Option<String>,
        node_id: Option<String>,
        stream_id: Option<String>,
        chunk: impl Into<String>,
        metadata: FxHashMap<String, Value>,
    ) -> Self {
        Self::new(
            session_id,
            node_id,
            stream_id,
            chunk,
            true,
            Some(LLMStreamingEventScope::Final),
            metadata,
            Utc::now(),
        )
    }

    pub fn error_event(
        session_id: Option<String>,
        node_id: Option<String>,
        stream_id: Option<String>,
        error_message: impl Into<String>,
    ) -> Self {
        let mut metadata = FxHashMap::default();
        metadata.insert("severity".to_string(), Value::String("error".to_string()));
        Self::new(
            session_id,
            node_id,
            stream_id,
            error_message,
            true,
            Some(LLMStreamingEventScope::Error),
            metadata,
            Utc::now(),
        )
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }

    pub fn stream_id(&self) -> Option<&str> {
        self.stream_id.as_deref()
    }

    pub fn chunk(&self) -> &str {
        &self.chunk
    }

    pub fn is_final(&self) -> bool {
        self.is_final
    }

    pub fn scope(&self) -> &LLMStreamingEventScope {
        &self.scope
    }

    pub fn metadata(&self) -> &FxHashMap<String, Value> {
        &self.metadata
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    pub fn with_metadata(mut self, metadata: FxHashMap<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }
}
