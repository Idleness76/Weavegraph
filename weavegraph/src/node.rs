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
use crate::control::{FrontierCommand, NodeRoute};
use crate::event_bus::{Event, EventEmitter, LLMStreamingEvent};
use crate::message::Message;
use crate::state::StateSnapshot;
use crate::types::NodeKind;
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
/// use weavegraph::channels::errors::{ErrorEvent, LadderError};
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
///                 error: LadderError {
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
pub struct NodeContext {
    /// Unique identifier for this node instance.
    pub node_id: String,
    /// Current execution step number.
    pub step: u64,
    /// Channel for emitting events to the workflow's event system.
    pub event_emitter: Arc<dyn EventEmitter>,
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
        self.emit_node(scope, message)
    }

    /// Emit a node event using this context's node identifier and step metadata.
    pub fn emit_node(
        &self,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), NodeContextError> {
        self.emit_event(Event::node_message_with_meta(
            self.node_id.clone(),
            self.step,
            scope,
            message,
        ))
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
/// use weavegraph::message::Message;
/// use weavegraph::channels::errors::{ErrorEvent, LadderError};
/// use serde_json::json;
/// use weavegraph::utils::collections::new_extra_map;
///
/// // Simple message-only response
/// let partial = NodePartial::new().with_messages(vec![Message::assistant("Done")]);
///
/// // Rich response with metadata
/// let mut extra = new_extra_map();
/// extra.insert("status".to_string(), json!("success"));
/// extra.insert("duration_ms".to_string(), json!(150));
/// let partial = NodePartial::new()
///     .with_messages(vec![Message::assistant("Processing complete")])
///     .with_extra(extra);
///
/// // Response with warnings
/// let errors = vec![ErrorEvent {
///     error: LadderError {
///         message: "Low confidence result".to_string(),
///         ..Default::default()
///     },
///     ..Default::default()
/// }];
/// let partial = NodePartial::new()
///     .with_messages(vec![Message::assistant("Result with warnings")])
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
        help("Check that the previous node produced the required data: {what}.")
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
