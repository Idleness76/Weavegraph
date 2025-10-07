//! Node execution framework for the Weavegraph workflow system.
//!
//! This module provides the core abstractions for executable workflow nodes,
//! including the [`Node`] trait, execution context, state updates, and error handling.

// Standard library and external crates
use async_trait::async_trait;
use miette::Diagnostic;
use rustc_hash::FxHashMap;
use serde_json;
use thiserror::Error;

// Internal crate modules
use crate::channels::errors::ErrorEvent;
use crate::event_bus::Event;
use crate::message::Message;
use crate::state::StateSnapshot;

// ============================================================================
// Core Trait
// ============================================================================

/// Core trait defining executable workflow nodes.
///
/// The `Node` trait represents a single unit of computation within a workflow.
/// Nodes receive the current state snapshot and execution context, perform
/// their work, and return partial state updates.
///
/// # Design Principles
///
/// - **Stateless**: Nodes should be stateless and deterministic
/// - **Focused**: Each node should have a single, well-defined responsibility  
/// - **Composable**: Nodes should be easily combined into larger workflows
/// - **Observable**: Use the context to emit events for monitoring and debugging
///
/// # Error Handling
///
/// Nodes can handle errors in two ways:
/// 1. **Fatal errors**: Return `Err(NodeError)` to stop workflow execution
/// 2. **Recoverable errors**: Add to `NodePartial.errors` and return `Ok`
///
/// # Examples
///
/// ```rust,no_run
/// use weavegraph::node::{Node, NodeContext, NodePartial, NodeError};
/// use weavegraph::state::StateSnapshot;
/// use async_trait::async_trait;
///
/// struct ValidationNode {
///     required_fields: Vec<String>,
/// }
///
/// #[async_trait]
/// impl Node for ValidationNode {
///     async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
///         ctx.emit("validation", "Starting validation")?;
///         
///         for field in &self.required_fields {
///             if !snapshot.extra.contains_key(field) {
///                 return Err(NodeError::ValidationFailed(format!("Missing field: {}", field)));
///             }
///         }
///         
///         Ok(NodePartial::default())
///     }
/// }
/// ```
#[async_trait]
pub trait Node: Send + Sync {
    /// Execute this node with the given state snapshot and context.
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError>;
}

// ============================================================================
// Execution Context
// ============================================================================

/// Execution context passed to nodes during workflow execution.
///
/// Provides nodes with access to their execution environment, including step
/// information, node identity, and communication channels for observability.
#[derive(Clone, Debug)]
pub struct NodeContext {
    /// Unique identifier for this node instance.
    pub node_id: String,
    /// Current execution step number.
    pub step: u64,
    /// Channel for emitting events to the workflow's event system.
    pub event_bus_sender: flume::Sender<Event>,
}

impl NodeContext {
    /// Emit a node-scoped event enriched with this context's metadata.
    ///
    /// Creates structured events that include the node's ID and step information,
    /// making them traceable in the workflow execution log.
    pub fn emit(
        &self,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), NodeContextError> {
        self.event_bus_sender
            .send(Event::node_message_with_meta(
                self.node_id.clone(),
                self.step,
                scope,
                message,
            ))
            .map_err(|_| NodeContextError::EventBusUnavailable)
    }
}

// ============================================================================
// State Updates
// ============================================================================

/// Partial state updates returned by node execution.
///
/// Represents the changes a node wants to make to the workflow state.
/// All fields are optional, allowing nodes to update only the state aspects
/// they care about. The workflow runtime merges these partial updates.
///
/// # Examples
///
/// ```rust
/// use weavegraph::node::NodePartial;
/// use weavegraph::message::Message;
/// use serde_json::json;
///
/// // Builder pattern (recommended)
/// let partial = NodePartial::builder()
///     .message(Message::assistant("Task completed"))
///     .extra("status", json!("success"))
///     .build();
///
/// // Convenience methods
/// let partial = NodePartial::with_message(Message::assistant("Done"));
/// let partial = NodePartial::with_extra_pair("key", json!("value"));
///
/// // From trait conversions
/// let partial: NodePartial = Message::assistant("Task done").into();
/// ```
#[derive(Clone, Debug, Default)]
pub struct NodePartial {
    /// Messages to add to the workflow's message history.
    pub messages: Option<Vec<Message>>,
    /// Additional key-value data to merge into the workflow's extra storage.
    pub extra: Option<FxHashMap<String, serde_json::Value>>,
    /// Errors to add to the workflow's error collection.
    pub errors: Option<Vec<ErrorEvent>>,
}

impl NodePartial {
    /// Create a new builder for constructing a `NodePartial`.
    #[must_use]
    pub fn builder() -> NodePartialBuilder {
        NodePartialBuilder::new()
    }

    /// Create a `NodePartial` with a single message.
    #[must_use]
    pub fn with_message(message: Message) -> Self {
        Self::with_messages(vec![message])
    }

    /// Create a `NodePartial` with multiple messages.
    #[must_use]
    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self {
            messages: Some(messages),
            ..Default::default()
        }
    }

    /// Create a `NodePartial` with a single extra key-value pair.
    #[must_use]
    pub fn with_extra_pair(key: impl Into<String>, value: serde_json::Value) -> Self {
        let mut extra = crate::utils::collections::new_extra_map();
        extra.insert(key.into(), value);
        Self::with_extra(extra)
    }

    /// Create a `NodePartial` with extra data.
    #[must_use]
    pub fn with_extra(extra: FxHashMap<String, serde_json::Value>) -> Self {
        Self {
            extra: Some(extra),
            ..Default::default()
        }
    }

    /// Create a `NodePartial` with a single error.
    #[must_use]
    pub fn with_error(error: ErrorEvent) -> Self {
        Self::with_errors(vec![error])
    }

    /// Create a `NodePartial` with multiple errors.
    #[must_use]
    pub fn with_errors(errors: Vec<ErrorEvent>) -> Self {
        Self {
            errors: Some(errors),
            ..Default::default()
        }
    }

    /// Create a `NodePartial` with both messages and extra data.
    #[must_use]
    pub fn with_messages_and_extra(
        messages: Vec<Message>,
        extra: FxHashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            messages: Some(messages),
            extra: Some(extra),
            ..Default::default()
        }
    }
}

// ============================================================================
// Builder Pattern
// ============================================================================

/// Builder for constructing `NodePartial` instances with a fluent API.
///
/// Enables ergonomic, chainable construction of `NodePartial` values.
///
/// # Examples
///
/// ```rust
/// use weavegraph::node::NodePartial;
/// use weavegraph::message::Message;
/// use serde_json::json;
///
/// let partial = NodePartial::builder()
///     .message(Message::assistant("Step 1 complete"))
///     .message(Message::assistant("Step 2 complete"))
///     .extra("status", json!("completed"))
///     .extra("duration_ms", json!(150))
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct NodePartialBuilder {
    messages: Vec<Message>,
    extra: FxHashMap<String, serde_json::Value>,
    errors: Vec<ErrorEvent>,
}

impl NodePartialBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a single message.
    #[must_use]
    pub fn message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    /// Add multiple messages.
    #[must_use]
    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages.extend(messages);
        self
    }

    /// Add a key-value pair to the extra data.
    #[must_use]
    pub fn extra(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    /// Add multiple key-value pairs to the extra data.
    #[must_use]
    pub fn extra_map(mut self, extra: FxHashMap<String, serde_json::Value>) -> Self {
        self.extra.extend(extra);
        self
    }

    /// Add a single error.
    #[must_use]
    pub fn error(mut self, error: ErrorEvent) -> Self {
        self.errors.push(error);
        self
    }

    /// Add multiple errors.
    #[must_use]
    pub fn errors(mut self, errors: Vec<ErrorEvent>) -> Self {
        self.errors.extend(errors);
        self
    }

    /// Build the final `NodePartial` instance.
    #[must_use]
    pub fn build(self) -> NodePartial {
        NodePartial {
            messages: if self.messages.is_empty() {
                None
            } else {
                Some(self.messages)
            },
            extra: if self.extra.is_empty() {
                None
            } else {
                Some(self.extra)
            },
            errors: if self.errors.is_empty() {
                None
            } else {
                Some(self.errors)
            },
        }
    }
}

// ============================================================================
// From Trait Implementations
// ============================================================================

impl From<Message> for NodePartial {
    /// Convert a single message into a NodePartial.
    fn from(message: Message) -> Self {
        Self::with_messages(vec![message])
    }
}

impl From<Vec<Message>> for NodePartial {
    /// Convert a vector of messages into a NodePartial.
    fn from(messages: Vec<Message>) -> Self {
        Self::with_messages(messages)
    }
}

impl From<FxHashMap<String, serde_json::Value>> for NodePartial {
    /// Convert an extra data map into a NodePartial.
    fn from(extra: FxHashMap<String, serde_json::Value>) -> Self {
        Self::with_extra(extra)
    }
}

impl From<Vec<ErrorEvent>> for NodePartial {
    /// Convert a vector of error events into a NodePartial.
    fn from(errors: Vec<ErrorEvent>) -> Self {
        Self::with_errors(errors)
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when using NodeContext methods.
#[derive(Debug, Error, Diagnostic)]
pub enum NodeContextError {
    /// Event could not be sent due to event bus disconnection or capacity issues.
    #[error("failed to emit event: event bus unavailable")]
    #[diagnostic(
        code(weavegraph::node::event_bus_unavailable),
        help("The event bus may be disconnected or at capacity. Check workflow state.")
    )]
    EventBusUnavailable,
}

/// Errors that can occur during node execution.
///
/// `NodeError` represents fatal errors that should halt workflow execution.
/// For recoverable errors that should be tracked but not halt execution,
/// use `NodePartial.errors` instead.
#[derive(Debug, Error, Diagnostic)]
pub enum NodeError {
    /// Expected input data is missing from the state snapshot.
    #[error("missing expected input: {what}")]
    #[diagnostic(
        code(weavegraph::node::missing_input),
        help("Check that the previous node produced the required data.")
    )]
    MissingInput { what: &'static str },

    /// External provider or service error.
    #[error("provider error ({provider}): {message}")]
    #[diagnostic(code(weavegraph::node::provider))]
    Provider {
        provider: &'static str,
        message: String,
    },

    /// JSON serialization/deserialization error.
    #[error(transparent)]
    #[diagnostic(code(weavegraph::node::serde_json))]
    Serde(#[from] serde_json::Error),

    /// Input validation failed.
    #[error("validation failed: {0}")]
    #[diagnostic(
        code(weavegraph::node::validation),
        help("Check input data format and required fields.")
    )]
    ValidationFailed(String),

    /// Event bus communication error.
    #[error("event bus error: {0}")]
    #[diagnostic(code(weavegraph::node::event_bus))]
    EventBus(#[from] NodeContextError),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventBus;
    use crate::utils::collections::new_extra_map;

    fn make_ctx(step: u64) -> (NodeContext, EventBus) {
        let event_bus = EventBus::default();
        let ctx = NodeContext {
            node_id: "test-node".to_string(),
            step,
            event_bus_sender: event_bus.get_sender(),
        };
        (ctx, event_bus)
    }

    #[test]
    fn test_node_context_creation() {
        let (ctx, _event_bus) = make_ctx(5);
        assert_eq!(ctx.node_id, "test-node");
        assert_eq!(ctx.step, 5);
    }

    #[test]
    fn test_node_partial_default() {
        let np = NodePartial::default();
        assert!(np.messages.is_none());
        assert!(np.extra.is_none());
        assert!(np.errors.is_none());
    }

    #[test]
    fn test_node_partial_with_messages() {
        let messages = vec![Message {
            role: "test".to_string(),
            content: "test message".to_string(),
        }];
        let partial = NodePartial::with_messages(messages.clone());
        assert_eq!(partial.messages, Some(messages));
        assert!(partial.extra.is_none());
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_with_extra() {
        let mut extra = new_extra_map();
        extra.insert("test_key".to_string(), serde_json::json!("test_value"));

        let partial = NodePartial::with_extra(extra.clone());
        assert!(partial.messages.is_none());
        assert_eq!(partial.extra, Some(extra));
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_with_messages_and_extra() {
        let messages = vec![Message {
            role: "test".to_string(),
            content: "test message".to_string(),
        }];
        let mut extra = new_extra_map();
        extra.insert("test_key".to_string(), serde_json::json!("test_value"));

        let partial = NodePartial::with_messages_and_extra(messages.clone(), extra.clone());
        assert_eq!(partial.messages, Some(messages));
        assert_eq!(partial.extra, Some(extra));
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_builder() {
        use serde_json::json;

        let message = Message {
            role: "assistant".to_string(),
            content: "Task completed".to_string(),
        };

        let partial = NodePartial::builder()
            .message(message.clone())
            .extra("status", json!("success"))
            .extra("count", json!(42))
            .build();

        assert_eq!(partial.messages, Some(vec![message]));
        assert!(partial.extra.is_some());
        let extra = partial.extra.unwrap();
        assert_eq!(extra["status"], json!("success"));
        assert_eq!(extra["count"], json!(42));
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_from_message() {
        let message = Message {
            role: "assistant".to_string(),
            content: "Test message".to_string(),
        };

        let partial: NodePartial = message.clone().into();
        assert_eq!(partial.messages, Some(vec![message]));
        assert!(partial.extra.is_none());
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_from_messages() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: "Message 1".to_string(),
            },
            Message {
                role: "assistant".to_string(),
                content: "Message 2".to_string(),
            },
        ];

        let partial: NodePartial = messages.clone().into();
        assert_eq!(partial.messages, Some(messages));
        assert!(partial.extra.is_none());
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_convenience_methods() {
        use serde_json::json;

        let message = Message {
            role: "assistant".to_string(),
            content: "Single message".to_string(),
        };

        // Test with_message
        let partial = NodePartial::with_message(message.clone());
        assert_eq!(partial.messages, Some(vec![message]));

        // Test with_extra_pair
        let partial = NodePartial::with_extra_pair("key", json!("value"));
        assert!(partial.extra.is_some());
        let extra = partial.extra.unwrap();
        assert_eq!(extra["key"], json!("value"));
    }

    #[test]
    fn test_node_partial_builder_empty() {
        let partial = NodePartial::builder().build();
        assert!(partial.messages.is_none());
        assert!(partial.extra.is_none());
        assert!(partial.errors.is_none());
    }
}
