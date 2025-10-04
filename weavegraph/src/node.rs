use crate::channels::errors::ErrorEvent;
use crate::event_bus::Event;
use crate::message::*;
use crate::state::*;
use async_trait::async_trait;
use miette::Diagnostic;
use rustc_hash::FxHashMap;
use thiserror::Error;

/// Execution context passed to nodes during workflow execution.
///
/// `NodeContext` provides nodes with access to their execution environment,
/// including step information, node identity, and communication channels.
/// This context enables nodes to emit events, track their execution state,
/// and integrate with the workflow's observability system.
///
/// # Examples
///
/// ```rust,no_run
/// use weavegraph::node::{Node, NodeContext, NodePartial};
/// use weavegraph::state::StateSnapshot;
/// # use async_trait::async_trait;
///
/// struct MyNode;
///
/// #[async_trait]
/// impl Node for MyNode {
///     async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, weavegraph::node::NodeError> {
///         // Emit a progress event
///         ctx.emit("processing", "Starting node execution")?;
///         
///         // Your node logic here
///         Ok(NodePartial::default())
///     }
/// }
/// ```
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
    /// This method creates structured events that include the node's ID and step
    /// information, making them traceable in the workflow execution log.
    ///
    /// # Parameters
    /// * `scope` - The scope or category of the event (e.g., "processing", "validation")
    /// * `message` - The human-readable message describing the event
    ///
    /// # Returns
    /// * `Ok(())` - Event was successfully queued
    /// * `Err(flume::SendError)` - Event bus is disconnected or full
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use weavegraph::node::NodeContext;
    /// # async fn example(ctx: NodeContext) -> Result<(), Box<dyn std::error::Error>> {
    /// ctx.emit("validation", "Input parameters validated successfully")?;
    /// ctx.emit("processing", format!("Processing {} items", 42))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn emit(
        &self,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), flume::SendError<Event>> {
        self.event_bus_sender.send(Event::node_message_with_meta(
            self.node_id.clone(),
            self.step,
            scope,
            message,
        ))
    }

    /// Emit a diagnostic event that is not tied to node metadata.
    ///
    /// Use this for system-level diagnostics or events that don't belong to
    /// a specific node's execution context.
    ///
    /// # Parameters
    /// * `scope` - The diagnostic scope (e.g., "system", "performance")
    /// * `message` - The diagnostic message
    ///
    /// # Returns
    /// * `Ok(())` - Event was successfully queued
    /// * `Err(flume::SendError)` - Event bus is disconnected or full
    pub fn emit_diagnostic(
        &self,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), flume::SendError<Event>> {
        self.event_bus_sender
            .send(Event::diagnostic(scope.into(), message.into()))
    }
}

/// Partial state updates returned by node execution.
///
/// `NodePartial` represents the changes a node wants to make to the workflow state.
/// Nodes return this instead of a full state to enable efficient state merging
/// and to clearly express what aspects of the state they modify.
///
/// All fields are optional, allowing nodes to update only the state aspects
/// they care about. The workflow runtime will merge these partial updates
/// into the complete state.
///
/// # Design Philosophy
///
/// This pattern follows the principle of "partial updates" where nodes specify
/// only what they want to change, rather than managing the entire state.
/// This improves:
/// - **Composability**: Multiple nodes can update different state aspects
/// - **Performance**: Avoid unnecessary state copying
/// - **Clarity**: Clear intent about what each node modifies
///
/// # Examples
///
/// ```rust
/// use weavegraph::node::NodePartial;
/// use weavegraph::utils::collections::new_extra_map;
///
/// // Node that only adds messages
/// let partial = NodePartial {
///     messages: Some(vec![/* messages */]),
///     ..Default::default()
/// };
///
/// // Node that only updates extra data
/// let mut extras = new_extra_map();
/// extras.insert("status".to_string(), serde_json::json!("completed"));
/// let partial = NodePartial {
///     extra: Some(extras),
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Debug, Default)]
pub struct NodePartial {
    /// Messages to add to the workflow's message history.
    /// These are typically status updates, logs, or user-facing notifications.
    pub messages: Option<Vec<Message>>,

    /// Additional key-value data to merge into the workflow's extra storage.
    /// Use this for custom metadata, intermediate results, or node-specific state.
    pub extra: Option<FxHashMap<String, serde_json::Value>>,

    /// Errors to add to the workflow's error collection.
    /// Use this to report non-fatal errors that should be tracked.
    pub errors: Option<Vec<ErrorEvent>>,
}

impl NodePartial {
    /// Create a `NodePartial` with only messages.
    ///
    /// # Parameters
    /// * `messages` - Messages to add to the workflow
    ///
    /// # Returns
    /// A `NodePartial` with only the messages field set
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::node::NodePartial;
    /// use weavegraph::message::Message;
    ///
    /// let messages = vec![Message {
    ///     role: "assistant".to_string(),
    ///     content: "Processing completed".to_string(),
    /// }];
    /// let partial = NodePartial::with_messages(messages);
    /// ```
    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self {
            messages: Some(messages),
            ..Default::default()
        }
    }

    /// Create a `NodePartial` with only extra data.
    ///
    /// # Parameters
    /// * `extra` - Extra data to add to the workflow state
    ///
    /// # Returns
    /// A `NodePartial` with only the extra field set
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::node::NodePartial;
    /// use weavegraph::utils::collections::new_extra_map;
    /// use serde_json::json;
    ///
    /// let mut extra = new_extra_map();
    /// extra.insert("status".to_string(), json!("completed"));
    /// let partial = NodePartial::with_extra(extra);
    /// ```
    pub fn with_extra(extra: FxHashMap<String, serde_json::Value>) -> Self {
        Self {
            extra: Some(extra),
            ..Default::default()
        }
    }

    /// Create a `NodePartial` with only errors.
    ///
    /// # Parameters
    /// * `errors` - Error events to add to the workflow
    ///
    /// # Returns
    /// A `NodePartial` with only the errors field set
    pub fn with_errors(errors: Vec<ErrorEvent>) -> Self {
        Self {
            errors: Some(errors),
            ..Default::default()
        }
    }

    /// Create a `NodePartial` with both messages and extra data.
    ///
    /// This is a common pattern for nodes that both log progress and store results.
    ///
    /// # Parameters
    /// * `messages` - Messages to add to the workflow
    /// * `extra` - Extra data to add to the workflow state
    ///
    /// # Returns
    /// A `NodePartial` with messages and extra fields set
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
/// /// A node that validates input data
/// struct ValidationNode {
///     required_fields: Vec<String>,
/// }
///
/// #[async_trait]
/// impl Node for ValidationNode {
///     async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
///         ctx.emit("validation", "Starting input validation")?;
///         
///         // Validate the snapshot's extra data
///         for field in &self.required_fields {
///             if !snapshot.extra.contains_key(field) {
///                 return Err(NodeError::ValidationFailed(format!("Missing required field: {}", field)));
///             }
///         }
///         
///         ctx.emit("validation", "All required fields present")?;
///         Ok(NodePartial::default())
///     }
/// }
/// ```
///
/// # Implementation Notes
///
/// - Implementations should be `Send + Sync` for concurrent execution
/// - Consider using structured logging via the context for observability
/// - Keep node logic focused and testable in isolation
#[async_trait]
pub trait Node: Send + Sync {
    /// Execute this node with the given state snapshot and context.
    ///
    /// # Parameters
    /// * `snapshot` - Immutable view of the current workflow state
    /// * `ctx` - Execution context for this node invocation
    ///
    /// # Returns
    /// * `Ok(NodePartial)` - Successful execution with optional state updates
    /// * `Err(NodeError)` - Fatal error that should stop workflow execution
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError>;
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
    EventBus(#[from] flume::SendError<Event>),
}

/****************
  Example nodes available in examples/

  See examples/basic_nodes.rs and examples/advanced_patterns.rs
  for comprehensive demonstrations of node patterns.

  Run with:
  - cargo run --example basic_nodes
  - cargo run --example advanced_patterns
*****************/

#[cfg(test)]
mod tests {
    use crate::event_bus::EventBus;
    use crate::utils::collections::new_extra_map;

    use super::*;

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
    fn test_snapshot_extra_flexible_types() {
        use serde_json::json;
        let mut extra = new_extra_map();
        extra.insert("number".into(), json!(123));
        extra.insert("text".into(), json!("abc"));
        extra.insert("array".into(), json!([1, 2, 3]));
        extra.insert("obj".into(), json!({"foo": "bar"}));
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: extra.clone(),
            extra_version: 1,
            errors: vec![],
            errors_version: 1,
        };
        assert_eq!(snap.extra["number"], json!(123));
        assert_eq!(snap.extra["text"], json!("abc"));
        assert_eq!(snap.extra["array"], json!([1, 2, 3]));
        assert_eq!(snap.extra["obj"], json!({"foo": "bar"}));
    }
}
