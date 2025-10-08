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
/// use weavegraph::utils::collections::new_extra_map;
///
/// // Essential constructors
/// let partial = NodePartial::new().with_messages(vec![Message::assistant("Done")]);
///
/// let mut extra = new_extra_map();
/// extra.insert("status".to_string(), json!("success"));
/// let partial = NodePartial::new().with_extra(extra);
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
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
    /// Create a `NodePartial` with multiple messages.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = Some(messages);
        self
    }

    /// Create a `NodePartial` with extra data.
    #[must_use]
    pub fn with_extra(mut self, extra: FxHashMap<String, serde_json::Value>) -> Self {
        self.extra = Some(extra);
        self
    }

    /// Create a `NodePartial` with multiple errors.
    #[must_use]
    pub fn with_errors(mut self, errors: Vec<ErrorEvent>) -> Self {
        self.errors = Some(errors);
        self
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
    use crate::state::VersionedState;
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
        let partial = NodePartial::new().with_messages(messages.clone());
        assert_eq!(partial.messages, Some(messages));
        assert!(partial.extra.is_none());
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_with_extra() {
        let mut extra = new_extra_map();
        extra.insert("test_key".to_string(), serde_json::json!("test_value"));

        let partial = NodePartial::new().with_extra(extra.clone());
        assert!(partial.messages.is_none());
        assert_eq!(partial.extra, Some(extra));
        assert!(partial.errors.is_none());
    }

    #[test]
    fn test_node_partial_with_errors() {
        let errors = vec![ErrorEvent::default()];
        let partial = NodePartial::new().with_errors(errors.clone());
        assert!(partial.messages.is_none());
        assert!(partial.extra.is_none());
        assert_eq!(partial.errors, Some(errors));
    }

    #[test]
    fn test_node_context_emit_error() {
        // Create a NodeContext with a disconnected event bus
        let (ctx, event_bus) = make_ctx(1);
        drop(event_bus); // Drop the event bus to disconnect sender
        let result = ctx.emit("scope", "message");
        assert!(matches!(result, Err(NodeContextError::EventBusUnavailable)));
    }

    #[test]
    fn test_node_error_variants() {
        // MissingInput
        let err = NodeError::MissingInput { what: "field" };
        match err {
            NodeError::MissingInput { what } => assert_eq!(what, "field"),
            _ => panic!("Wrong variant"),
        }

        // Provider
        let err = NodeError::Provider {
            provider: "svc",
            message: "fail".to_string(),
        };
        match err {
            NodeError::Provider { provider, message } => {
                assert_eq!(provider, "svc");
                assert_eq!(message, "fail");
            }
            _ => panic!("Wrong variant"),
        }

        // Serde
        let json_err = serde_json::from_str::<serde_json::Value>("not_json").unwrap_err();
        let err = NodeError::Serde(json_err);
        match err {
            NodeError::Serde(_) => (),
            _ => panic!("Wrong variant"),
        }

        // ValidationFailed
        let err = NodeError::ValidationFailed("bad input".to_string());
        match err {
            NodeError::ValidationFailed(msg) => assert_eq!(msg, "bad input"),
            _ => panic!("Wrong variant"),
        }

        // EventBus
        let err = NodeError::EventBus(NodeContextError::EventBusUnavailable);
        match err {
            NodeError::EventBus(NodeContextError::EventBusUnavailable) => (),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_node_context_error_variant() {
        let err = NodeContextError::EventBusUnavailable;
        match err {
            NodeContextError::EventBusUnavailable => (),
        }
    }

    // Dummy Node trait implementation for coverage
    use async_trait::async_trait;
    struct DummyNode;
    #[async_trait]
    impl Node for DummyNode {
        async fn run(
            &self,
            _snapshot: StateSnapshot,
            ctx: NodeContext,
        ) -> Result<NodePartial, NodeError> {
            // Emit event and return a message
            ctx.emit("dummy", "executed").map_err(NodeError::EventBus)?;
            Ok(NodePartial::new().with_messages(vec![Message {
                role: "dummy".to_string(),
                content: "ok".to_string(),
            }]))
        }
    }

    #[tokio::test]
    async fn test_node_trait_success() {
        let (ctx, _event_bus) = make_ctx(0);
        let node = DummyNode;
        let snapshot = VersionedState::new_with_user_message("dummy").snapshot();
        let result = node.run(snapshot, ctx).await;
        assert!(result.is_ok());
        let partial = result.unwrap();
        assert_eq!(partial.messages.unwrap()[0].role, "dummy");
    }

    #[tokio::test]
    async fn test_node_trait_eventbus_error() {
        let (ctx, event_bus) = make_ctx(0);
        drop(event_bus); // disconnect event bus
        let node = DummyNode;
        let snapshot = VersionedState::new_with_user_message("dummy").snapshot();
        let result = node.run(snapshot, ctx).await;
        assert!(matches!(
            result,
            Err(NodeError::EventBus(NodeContextError::EventBusUnavailable))
        ));
    }
}
