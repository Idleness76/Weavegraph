use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::telemetry::{PlainFormatter, TelemetryFormatter};

// Avoid depending on serde for NodeKind by using encoded string form for kind.

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

pub fn pretty_print(events: &[ErrorEvent]) -> String {
    let formatter = PlainFormatter;
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
