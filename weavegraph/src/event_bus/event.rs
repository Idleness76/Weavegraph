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
            Event::LLM(llm) => llm
                .stream_id()
                .or_else(|| llm.node_id())
                .or_else(|| llm.session_id())
                .or(Some("llm_stream")),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Event::Node(node) => node.message(),
            Event::Diagnostic(diag) => diag.message(),
            Event::LLM(llm) => llm.chunk(),
        }
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
pub struct LLMStreamingEvent {
    session_id: Option<String>,
    node_id: Option<String>,
    stream_id: Option<String>,
    chunk: String,
    is_final: bool,
    metadata: FxHashMap<String, Value>,
    timestamp: DateTime<Utc>,
}

impl LLMStreamingEvent {
    pub fn new(
        session_id: Option<String>,
        node_id: Option<String>,
        stream_id: Option<String>,
        chunk: impl Into<String>,
        is_final: bool,
        metadata: FxHashMap<String, Value>,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            session_id,
            node_id,
            stream_id,
            chunk: chunk.into(),
            is_final,
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
