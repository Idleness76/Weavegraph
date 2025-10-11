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
/// use weavegraph::graph::EdgePredicate;
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
/// # Examples
///
/// ```
/// use weavegraph::graph::{ConditionalEdge, EdgePredicate};
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
/// let edge = ConditionalEdge {
///     from: NodeKind::Start,
///     predicate,
/// };
/// ```
#[derive(Clone)]
pub struct ConditionalEdge {
    /// The source node for this conditional edge.
    pub from: NodeKind,
    /// The predicate function that determines target node.
    pub predicate: EdgePredicate,
}
