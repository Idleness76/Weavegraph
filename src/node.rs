//! Node execution framework for the Weavegraph workflow system.
//!
//! This module provides the core abstractions for executable workflow nodes,
//! including the [`Node`] trait, execution context, state updates, and error handling.
// Standard library and external crates
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use serde_json;
use thiserror::Error;

// Internal crate modules
use crate::channels::errors::ErrorEvent;
use crate::control::{FrontierCommand, NodeRoute};
use crate::event_bus::{Event, EventEmitter, LLMStreamingEvent};
use crate::message::Message;
use crate::state::{StateKey, StateSlotError, StateSnapshot};
use crate::types::NodeKind;
use crate::utils::clock::Clock;
use std::sync::Arc;

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
/// use weavegraph::channels::errors::{ErrorEvent, WeaveError};
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
///         // Demonstrate the fluent API for success with warnings
///         if snapshot.messages.is_empty() {
///             let warning = ErrorEvent {
///                 error: WeaveError {
///                     message: "No messages to validate, but continuing".to_string(),
///                     ..Default::default()
///                 },
///                 ..Default::default()
///             };
///             return Ok(NodePartial::new().with_errors(vec![warning]));
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
#[non_exhaustive]
pub struct NodeContext {
    /// Unique identifier for this node instance.
    pub node_id: String,
    /// Current execution step number.
    pub step: u64,
    /// Channel for emitting events to the workflow's event system.
    pub event_emitter: Arc<dyn EventEmitter>,
    /// Optional runtime clock for deterministic tests and replay.
    pub clock: Option<Arc<dyn Clock>>,
    /// Optional invocation or run identifier attached to node events.
    pub invocation_id: Option<String>,
}

impl NodeContext {
    /// Construct a node context with no runtime clock or invocation metadata.
    pub fn new(
        node_id: impl Into<String>,
        step: u64,
        event_emitter: Arc<dyn EventEmitter>,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            step,
            event_emitter,
            clock: None,
            invocation_id: None,
        }
    }

    /// Return the current runtime clock timestamp in Unix milliseconds, if configured.
    #[must_use]
    pub fn now_unix_ms(&self) -> Option<i64> {
        self.clock.as_ref().map(|clock| clock.now_unix_ms())
    }

    /// Return the invocation identifier, if configured.
    #[must_use]
    pub fn invocation_id(&self) -> Option<&str> {
        self.invocation_id.as_deref()
    }

    /// Emit a node-scoped event enriched with this context's metadata.
    ///
    /// Creates structured events that include the node's ID and step information,
    /// making them traceable in the workflow execution log.
    pub fn emit(
        &self,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), NodeContextError> {
        self.emit_node(scope, message)
    }

    /// Emit a node event using this context's node identifier and step metadata.
    pub fn emit_node(
        &self,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), NodeContextError> {
        let mut metadata = FxHashMap::default();
        if let Some(invocation_id) = &self.invocation_id {
            metadata.insert(
                "invocation_id".to_string(),
                serde_json::Value::String(invocation_id.clone()),
            );
        }
        if let Some(now_unix_ms) = self.now_unix_ms() {
            metadata.insert("now_unix_ms".to_string(), serde_json::json!(now_unix_ms));
        }

        if metadata.is_empty() {
            self.emit_event(Event::node_message_with_meta(
                self.node_id.clone(),
                self.step,
                scope,
                message,
            ))
        } else {
            self.emit_event(Event::node_message_with_metadata(
                self.node_id.clone(),
                self.step,
                scope,
                message,
                metadata,
            ))
        }
    }

    /// Emit a diagnostic event for general workflow telemetry.
    pub fn emit_diagnostic(
        &self,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), NodeContextError> {
        self.emit_event(Event::diagnostic(scope, message))
    }

    /// Emit an LLM streaming chunk event with optional metadata.
    pub fn emit_llm_chunk(
        &self,
        session_id: Option<String>,
        stream_id: Option<String>,
        chunk: impl Into<String>,
        metadata: Option<FxHashMap<String, serde_json::Value>>,
    ) -> Result<(), NodeContextError> {
        let event = LLMStreamingEvent::chunk_event(
            session_id,
            Some(self.node_id.clone()),
            stream_id,
            chunk,
            metadata.unwrap_or_default(),
        );
        self.emit_event(Event::LLM(event))
    }

    /// Emit a final LLM streaming event signalling completion.
    pub fn emit_llm_final(
        &self,
        session_id: Option<String>,
        stream_id: Option<String>,
        chunk: impl Into<String>,
        metadata: Option<FxHashMap<String, serde_json::Value>>,
    ) -> Result<(), NodeContextError> {
        let event = LLMStreamingEvent::final_event(
            session_id,
            Some(self.node_id.clone()),
            stream_id,
            chunk,
            metadata.unwrap_or_default(),
        );
        self.emit_event(Event::LLM(event))
    }

    /// Emit an LLM error event with the provided error message.
    pub fn emit_llm_error(
        &self,
        session_id: Option<String>,
        stream_id: Option<String>,
        error_message: impl Into<String>,
    ) -> Result<(), NodeContextError> {
        let event = LLMStreamingEvent::error_event(
            session_id,
            Some(self.node_id.clone()),
            stream_id,
            error_message,
        );
        self.emit_event(Event::LLM(event))
    }

    fn emit_event(&self, event: Event) -> Result<(), NodeContextError> {
        self.event_emitter
            .emit(event)
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
/// use weavegraph::message::{Message, Role};
/// use weavegraph::channels::errors::{ErrorEvent, WeaveError};
/// use serde_json::json;
/// use weavegraph::utils::collections::new_extra_map;
///
/// // Simple message-only response
/// let partial = NodePartial::new()
///     .with_messages(vec![Message::with_role(Role::Assistant, "Done")]);
///
/// // Rich response with metadata
/// let mut extra = new_extra_map();
/// extra.insert("status".to_string(), json!("success"));
/// extra.insert("duration_ms".to_string(), json!(150));
/// let partial = NodePartial::new()
///     .with_messages(vec![Message::with_role(
///         Role::Assistant,
///         "Processing complete",
///     )])
///     .with_extra(extra);
///
/// // Response with warnings
/// let errors = vec![ErrorEvent {
///     error: WeaveError {
///         message: "Low confidence result".to_string(),
///         ..Default::default()
///     },
///     ..Default::default()
/// }];
/// let partial = NodePartial::new()
///     .with_messages(vec![Message::with_role(
///         Role::Assistant,
///         "Result with warnings",
///     )])
///     .with_errors(errors);
/// ```
#[derive(Clone, Debug, Default)]
pub struct NodePartial {
    /// Messages to add to the workflow's message history.
    pub messages: Option<Vec<Message>>,
    /// Additional key-value data to merge into the workflow's extra storage.
    pub extra: Option<FxHashMap<String, serde_json::Value>>,
    /// Errors to add to the workflow's error collection.
    pub errors: Option<Vec<ErrorEvent>>,
    /// Frontier commands emitted by the node to influence subsequent routing.
    pub frontier: Option<FrontierCommand>,
}

impl NodePartial {
    /// Create an empty `NodePartial` with all fields set to `None`.
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
    /// Create a `NodePartial` with one or more messages.
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

    /// Insert a typed value into this partial's extra updates.
    ///
    /// The value is serialized to JSON and stored under the key returned by
    /// [`StateKey::storage_key`]. If this partial already contains extra data,
    /// the typed slot is merged into it and any existing value at the same
    /// storage key is replaced.
    pub fn with_typed_extra<T: serde::Serialize>(
        mut self,
        key: StateKey<T>,
        value: T,
    ) -> Result<Self, StateSlotError> {
        let storage_key = key.storage_key();
        let json_value =
            serde_json::to_value(value).map_err(|source| StateSlotError::Serialize {
                key: storage_key.clone(),
                source,
            })?;
        self.extra
            .get_or_insert_with(FxHashMap::default)
            .insert(storage_key, json_value);
        Ok(self)
    }

    /// Create a `NodePartial` with one or more errors.
    #[must_use]
    pub fn with_errors(mut self, errors: Vec<ErrorEvent>) -> Self {
        self.errors = Some(errors);
        self
    }

    /// Replace the default frontier with the provided list of targets.
    ///
    /// The runner will skip conditional edges for the originating node when a
    /// replace command is present.
    #[must_use]
    pub fn with_frontier_replace<I>(mut self, targets: I) -> Self
    where
        I: IntoIterator<Item = NodeKind>,
    {
        let routes = targets.into_iter().map(NodeRoute::from).collect();
        self.frontier = Some(FrontierCommand::Replace(routes));
        self
    }

    /// Append additional targets to the frontier alongside the default routes.
    ///
    /// The default unconditional edges remain in place and the supplied
    /// routes are appended in-order for deterministic processing.
    #[must_use]
    pub fn with_frontier_append<I>(mut self, targets: I) -> Self
    where
        I: IntoIterator<Item = NodeKind>,
    {
        let routes = targets.into_iter().map(NodeRoute::from).collect();
        self.frontier = Some(FrontierCommand::Append(routes));
        self
    }

    /// Attach a pre-built frontier command.
    #[must_use]
    pub fn with_frontier_command(mut self, command: FrontierCommand) -> Self {
        self.frontier = Some(command);
        self
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when using NodeContext methods.
#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum NodeContextError {
    /// Event could not be sent due to event bus disconnection or capacity issues.
    #[error("failed to emit event: event bus unavailable")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::node::event_bus_unavailable),
            help("The event bus may be disconnected or at capacity. Check workflow state.")
        )
    )]
    EventBusUnavailable,
}

/// Errors that can occur during node execution.
///
/// `NodeError` represents fatal errors that should halt workflow execution.
/// For recoverable errors that should be tracked but not halt execution,
/// use `NodePartial.errors` instead.
#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum NodeError {
    /// Expected input data is missing from the state snapshot.
    #[error("missing expected input: {what}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::node::missing_input),
            help("Check that the previous node produced the required data: {what}.")
        )
    )]
    MissingInput {
        /// Description of the missing input data.
        what: &'static str,
    },

    /// External provider or service error.
    #[error("provider error ({provider}): {message}")]
    #[cfg_attr(feature = "diagnostics", diagnostic(code(weavegraph::node::provider)))]
    Provider {
        /// Name of the external provider that produced the error.
        provider: &'static str,
        /// Human-readable description of the error.
        message: String,
    },

    /// Arbitrary external error for cases that do not fit structured variants.
    #[error(transparent)]
    #[cfg_attr(feature = "diagnostics", diagnostic(code(weavegraph::node::other)))]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),

    /// JSON serialization/deserialization error.
    #[error(transparent)]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::node::serde_json))
    )]
    Serde(#[from] serde_json::Error),

    /// Input validation failed.
    #[error("validation failed: {0}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::node::validation),
            help("Check input data format and required fields.")
        )
    )]
    ValidationFailed(String),

    /// Event bus communication error.
    #[error("event bus error: {0}")]
    #[cfg_attr(feature = "diagnostics", diagnostic(code(weavegraph::node::event_bus)))]
    EventBus(#[from] NodeContextError),
}

impl NodeError {
    /// Wrap an arbitrary error into [`NodeError::Other`].
    #[must_use]
    pub fn other<E>(error: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Other(Box::new(error))
    }
}

/// Canonical node result type for framework and user node implementations.
pub type NodeResult<T> = std::result::Result<T, NodeError>;

/// Extension trait for ergonomic conversion into [`NodeError`].
pub trait NodeResultExt<T> {
    /// Convert any error type into [`NodeError::Other`] for `?` propagation.
    fn node_err(self) -> NodeResult<T>;
}

impl<T, E> NodeResultExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn node_err(self) -> NodeResult<T> {
        self.map_err(NodeError::other)
    }
}
