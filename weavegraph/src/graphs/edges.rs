//! Edge types and routing predicates for conditional graph flow.
//!
//! This module contains the types and predicates used for dynamic routing
//! in workflow graphs, including conditional edges that can route based
//! on runtime state evaluation.

use crate::types::NodeKind;
use std::sync::Arc;

/// Predicate function for conditional edge routing.
///
/// Takes a [`StateSnapshot`] and returns target node names to determine
/// which nodes should be executed next. Predicates are used with
/// [`GraphBuilder::add_conditional_edge`] to create dynamic routing based
/// on the current state.
///
/// # Examples
///
/// ```
/// use weavegraph::graphs::EdgePredicate;
/// use std::sync::Arc;
///
/// // Route based on message count
/// let route_by_messages: EdgePredicate = Arc::new(|snapshot| {
///     if snapshot.messages.len() > 5 {
///         vec!["many_messages".to_string()]
///     } else {
///         vec!["few_messages".to_string()]
///     }
/// });
///
/// // Route based on extra data - fan out to multiple nodes
/// let route_by_error: EdgePredicate = Arc::new(|snapshot| {
///     if snapshot.extra.get("error").is_some() {
///         vec!["error_handler".to_string(), "logger".to_string()]
///     } else {
///         vec!["normal_flow".to_string()]
///     }
/// });
/// ```
pub type EdgePredicate =
    Arc<dyn Fn(crate::state::StateSnapshot) -> Vec<String> + Send + Sync + 'static>;

/// A conditional edge that routes based on a predicate function.
///
/// Conditional edges allow dynamic routing in workflows based on the current
/// state. When the scheduler encounters a conditional edge, it evaluates the
/// predicate function and routes to the returned target nodes.
///
/// # Purpose
///
/// This type encapsulates conditional routing logic to enable clean builder patterns
/// and maintain consistency with other edge types. The private fields ensure that
/// conditional edges are constructed through proper APIs rather than direct field access.
///
/// # Examples
///
/// ```
/// use weavegraph::graphs::{ConditionalEdge, EdgePredicate};
/// use weavegraph::types::NodeKind;
/// use std::sync::Arc;
///
/// let predicate: EdgePredicate = Arc::new(|snapshot| {
///     if snapshot.messages.len() > 5 {
///         vec!["many_messages".to_string()]
///     } else {
///         vec!["few_messages".to_string()]
///     }
/// });
/// let edge = ConditionalEdge::new(NodeKind::Start, predicate);
/// ```
#[derive(Clone)]
pub struct ConditionalEdge {
    /// The source node for this conditional edge.
    from: NodeKind,
    /// The predicate function that determines target node.
    predicate: EdgePredicate,
}

impl ConditionalEdge {
    /// Creates a new conditional edge.
    ///
    /// This is the preferred way to construct conditional edges, providing a clean
    /// API that works with the builder pattern while ensuring proper encapsulation.
    ///
    /// # Parameters
    ///
    /// - `from`: The source node identifier
    /// - `predicate`: The routing predicate function
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::graphs::{ConditionalEdge, EdgePredicate};
    /// use weavegraph::types::NodeKind;
    /// use std::sync::Arc;
    ///
    /// let predicate: EdgePredicate = Arc::new(|snapshot| {
    ///     vec!["target_node".to_string()]
    /// });
    ///
    /// let edge = ConditionalEdge::new(NodeKind::Custom("source".into()), predicate.clone());
    /// let edge2 = ConditionalEdge::new(NodeKind::Start, predicate);
    /// ```
    pub fn new(from: impl Into<NodeKind>, predicate: EdgePredicate) -> Self {
        Self {
            from: from.into(),
            predicate,
        }
    }

    /// Returns the source node of this conditional edge.
    pub fn from(&self) -> &NodeKind {
        &self.from
    }

    /// Returns the predicate function of this conditional edge.
    pub fn predicate(&self) -> &EdgePredicate {
        &self.predicate
    }
}
