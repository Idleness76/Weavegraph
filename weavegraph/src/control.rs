//! Control-flow primitives emitted by nodes to influence subsequent scheduling.
//!
//! Frontier commands are kept separate from state updates so nodes can
//! express routing intent without mutating application state directly. The
//! barrier aggregates these directives in a deterministic order and the runner
//! reconciles them with unconditional / conditional edges.

use crate::types::NodeKind;

/// Route identifier used by frontier commands.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeRoute {
    /// Route to another node in the graph.
    Node(NodeKind),
}

impl NodeRoute {
    /// Return the concrete `NodeKind` for this route.
    #[must_use]
    pub fn kind(&self) -> &NodeKind {
        match self {
            NodeRoute::Node(kind) => kind,
        }
    }

    /// Clone the underlying `NodeKind`.
    #[must_use]
    pub fn to_node_kind(&self) -> NodeKind {
        self.kind().clone()
    }
}

impl From<NodeKind> for NodeRoute {
    fn from(kind: NodeKind) -> Self {
        NodeRoute::Node(kind)
    }
}

/// Command emitted by a node to manipulate the next frontier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontierCommand {
    /// Append additional routes to the existing frontier calculation.
    Append(Vec<NodeRoute>),
    /// Replace the default routes emitted for the node.
    Replace(Vec<NodeRoute>),
}
