use std::fmt;

#[derive(Clone, Debug)]
pub enum Event {
    Node(NodeEvent),
    Diagnostic(DiagnosticEvent),
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
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Event::Node(node) => node.message(),
            Event::Diagnostic(diag) => diag.message(),
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
        }
    }
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
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
