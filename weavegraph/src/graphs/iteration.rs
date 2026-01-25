//! Graph iteration utilities and algorithms.
//!
//! This module provides idiomatic iterators and common graph algorithms
//! for inspecting and analyzing workflow graphs. Inspired by petgraph's
//! visit module patterns.
//!
//! # Iterators
//!
//! - [`NodesIter`]: Iterate over all nodes in the graph
//! - [`EdgesIter`]: Iterate over all edges as (source, target) pairs
//!
//! # Algorithms
//!
//! - [`topological_sort`](crate::graphs::GraphBuilder::topological_sort): Deterministic node ordering
//!
//! # Examples
//!
//! ```
//! use weavegraph::graphs::GraphBuilder;
//! use weavegraph::types::NodeKind;
//!
//! # struct MyNode;
//! # #[async_trait::async_trait]
//! # impl weavegraph::node::Node for MyNode {
//! #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
//! #         Ok(weavegraph::node::NodePartial::default())
//! #     }
//! # }
//!
//! let builder = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("A".into()), MyNode)
//!     .add_node(NodeKind::Custom("B".into()), MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
//!     .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
//!     .add_edge(NodeKind::Custom("B".into()), NodeKind::End);
//!
//! // Iterate over nodes
//! for node_kind in builder.nodes() {
//!     println!("Node: {:?}", node_kind);
//! }
//!
//! // Iterate over edges
//! for (from, to) in builder.edges() {
//!     println!("Edge: {:?} -> {:?}", from, to);
//! }
//!
//! // Get deterministic topological ordering
//! let sorted = builder.topological_sort();
//! println!("Topological order: {:?}", sorted);
//! ```

use crate::types::NodeKind;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

/// Iterator over node kinds in a graph.
///
/// Yields each registered custom node kind. Does not include virtual
/// `Start` or `End` nodes as they are not stored in the node registry.
///
/// # Examples
///
/// ```
/// use weavegraph::graphs::GraphBuilder;
/// use weavegraph::types::NodeKind;
///
/// # struct MyNode;
/// # #[async_trait::async_trait]
/// # impl weavegraph::node::Node for MyNode {
/// #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
/// #         Ok(weavegraph::node::NodePartial::default())
/// #     }
/// # }
///
/// let builder = GraphBuilder::new()
/// .add_node(NodeKind::Custom("A".into()), MyNode)
///     .add_node(NodeKind::Custom("B".into()), MyNode)
///     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
///     .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
///     .add_edge(NodeKind::Custom("B".into()), NodeKind::End);
///
/// let nodes: Vec<_> = builder.nodes().collect();
/// assert_eq!(nodes.len(), 2);
/// ```
pub struct NodesIter<'a> {
    inner: std::collections::hash_map::Keys<'a, NodeKind, std::sync::Arc<dyn crate::node::Node>>,
}

impl<'a> NodesIter<'a> {
    pub(super) fn new(
        inner: std::collections::hash_map::Keys<'a, NodeKind, std::sync::Arc<dyn crate::node::Node>>,
    ) -> Self {
        Self { inner }
    }
}

impl<'a> Iterator for NodesIter<'a> {
    type Item = &'a NodeKind;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a> ExactSizeIterator for NodesIter<'a> {}

/// Iterator over edges in a graph as (source, target) pairs.
///
/// Yields each edge in the graph, including edges from/to virtual
/// `Start` and `End` nodes. The iteration order is not guaranteed
/// to be deterministic due to hash map iteration.
///
/// # Examples
///
/// ```
/// use weavegraph::graphs::GraphBuilder;
/// use weavegraph::types::NodeKind;
///
/// # struct MyNode;
/// # #[async_trait::async_trait]
/// # impl weavegraph::node::Node for MyNode {
/// #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
/// #         Ok(weavegraph::node::NodePartial::default())
/// #     }
/// # }
///
/// let builder = GraphBuilder::new()
///     .add_node(NodeKind::Custom("A".into()), MyNode)
///     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
///     .add_edge(NodeKind::Custom("A".into()), NodeKind::End);
///
/// let edges: Vec<_> = builder.edges().collect();
/// assert_eq!(edges.len(), 2);
/// ```
pub struct EdgesIter<'a> {
    outer: std::collections::hash_map::Iter<'a, NodeKind, Vec<NodeKind>>,
    current_from: Option<&'a NodeKind>,
    current_targets: std::slice::Iter<'a, NodeKind>,
}

impl<'a> EdgesIter<'a> {
    pub(super) fn new(edges: &'a FxHashMap<NodeKind, Vec<NodeKind>>) -> Self {
        let mut outer = edges.iter();
        let (current_from, current_targets) = match outer.next() {
            Some((from, targets)) => (Some(from), targets.iter()),
            None => (None, [].iter()),
        };
        Self {
            outer,
            current_from,
            current_targets,
        }
    }
}

impl<'a> Iterator for EdgesIter<'a> {
    type Item = (&'a NodeKind, &'a NodeKind);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(to) = self.current_targets.next() {
                return Some((self.current_from.unwrap(), to));
            }
            match self.outer.next() {
                Some((from, targets)) => {
                    self.current_from = Some(from);
                    self.current_targets = targets.iter();
                }
                None => return None,
            }
        }
    }
}

/// Performs Kahn's algorithm for topological sorting.
///
/// Returns nodes in topological order (dependencies before dependents).
/// Virtual `Start` node is always first, `End` is always last.
/// Ties are broken lexicographically for deterministic ordering.
///
/// # Panics
///
/// This function assumes the graph is acyclic. If called on a graph with
/// cycles, it will return a partial ordering that excludes cycle members.
/// Use [`GraphBuilder::compile`] to validate acyclicity before calling.
pub(super) fn topological_sort(
    edges: &FxHashMap<NodeKind, Vec<NodeKind>>,
) -> Vec<NodeKind> {
    // Build in-degree map and collect all nodes
    let mut in_degree: FxHashMap<NodeKind, usize> = FxHashMap::default();
    let mut all_nodes: FxHashSet<NodeKind> = FxHashSet::default();

    // Collect all nodes from edges
    for (from, tos) in edges {
        all_nodes.insert(from.clone());
        in_degree.entry(from.clone()).or_insert(0);
        for to in tos {
            all_nodes.insert(to.clone());
            *in_degree.entry(to.clone()).or_insert(0) += 1;
        }
    }

    // Initialize queue with nodes that have in-degree 0
    // Use a Vec and sort for deterministic ordering
    let mut queue: VecDeque<NodeKind> = VecDeque::new();
    let mut zero_in_degree: Vec<_> = in_degree
        .iter()
        .filter(|entry| *entry.1 == 0)
        .map(|(node, _)| node.clone())
        .collect();
    
    // Sort for deterministic ordering - Start always first
    zero_in_degree.sort_by(|a, b| {
        match (a, b) {
            (NodeKind::Start, _) => std::cmp::Ordering::Less,
            (_, NodeKind::Start) => std::cmp::Ordering::Greater,
            (NodeKind::End, _) => std::cmp::Ordering::Greater,
            (_, NodeKind::End) => std::cmp::Ordering::Less,
            (NodeKind::Custom(a_name), NodeKind::Custom(b_name)) => a_name.cmp(b_name),
        }
    });
    
    queue.extend(zero_in_degree);

    let mut result: Vec<NodeKind> = Vec::with_capacity(all_nodes.len());

    while let Some(node) = queue.pop_front() {
        result.push(node.clone());

        if let Some(neighbors) = edges.get(&node) {
            // Collect neighbors that become zero in-degree after removing this node
            let mut new_zero: Vec<NodeKind> = Vec::new();
            for neighbor in neighbors {
                if let Some(deg) = in_degree.get_mut(neighbor) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        new_zero.push(neighbor.clone());
                    }
                }
            }
            // Sort new zero-degree nodes for determinism
            new_zero.sort_by(|a, b| {
                match (a, b) {
                    (NodeKind::Start, _) => std::cmp::Ordering::Less,
                    (_, NodeKind::Start) => std::cmp::Ordering::Greater,
                    (NodeKind::End, _) => std::cmp::Ordering::Greater,
                    (_, NodeKind::End) => std::cmp::Ordering::Less,
                    (NodeKind::Custom(a_name), NodeKind::Custom(b_name)) => a_name.cmp(b_name),
                }
            });
            queue.extend(new_zero);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topological_sort_linear() {
        let mut edges: FxHashMap<NodeKind, Vec<NodeKind>> = FxHashMap::default();
        edges.insert(
            NodeKind::Start,
            vec![NodeKind::Custom("A".into())],
        );
        edges.insert(
            NodeKind::Custom("A".into()),
            vec![NodeKind::Custom("B".into())],
        );
        edges.insert(
            NodeKind::Custom("B".into()),
            vec![NodeKind::End],
        );

        let sorted = topological_sort(&edges);
        
        // Start should be first, End should be last
        assert_eq!(sorted[0], NodeKind::Start);
        assert_eq!(sorted[sorted.len() - 1], NodeKind::End);
        
        // A should come before B
        let a_pos = sorted.iter().position(|n| n == &NodeKind::Custom("A".into())).unwrap();
        let b_pos = sorted.iter().position(|n| n == &NodeKind::Custom("B".into())).unwrap();
        assert!(a_pos < b_pos);
    }

    #[test]
    fn test_topological_sort_diamond() {
        // Start -> A, B -> C -> End (diamond pattern)
        let mut edges: FxHashMap<NodeKind, Vec<NodeKind>> = FxHashMap::default();
        edges.insert(
            NodeKind::Start,
            vec![NodeKind::Custom("A".into()), NodeKind::Custom("B".into())],
        );
        edges.insert(
            NodeKind::Custom("A".into()),
            vec![NodeKind::Custom("C".into())],
        );
        edges.insert(
            NodeKind::Custom("B".into()),
            vec![NodeKind::Custom("C".into())],
        );
        edges.insert(
            NodeKind::Custom("C".into()),
            vec![NodeKind::End],
        );

        let sorted = topological_sort(&edges);
        
        assert_eq!(sorted[0], NodeKind::Start);
        assert_eq!(sorted[sorted.len() - 1], NodeKind::End);
        
        // A and B should both come before C
        let a_pos = sorted.iter().position(|n| n == &NodeKind::Custom("A".into())).unwrap();
        let b_pos = sorted.iter().position(|n| n == &NodeKind::Custom("B".into())).unwrap();
        let c_pos = sorted.iter().position(|n| n == &NodeKind::Custom("C".into())).unwrap();
        assert!(a_pos < c_pos);
        assert!(b_pos < c_pos);
        
        // A should come before B due to lexicographic ordering
        assert!(a_pos < b_pos);
    }

    #[test]
    fn test_topological_sort_deterministic() {
        // Multiple runs should produce the same order
        let mut edges: FxHashMap<NodeKind, Vec<NodeKind>> = FxHashMap::default();
        edges.insert(
            NodeKind::Start,
            vec![NodeKind::Custom("X".into()), NodeKind::Custom("Y".into()), NodeKind::Custom("Z".into())],
        );
        edges.insert(NodeKind::Custom("X".into()), vec![NodeKind::End]);
        edges.insert(NodeKind::Custom("Y".into()), vec![NodeKind::End]);
        edges.insert(NodeKind::Custom("Z".into()), vec![NodeKind::End]);

        let sorted1 = topological_sort(&edges);
        let sorted2 = topological_sort(&edges);
        
        assert_eq!(sorted1, sorted2);
    }
}
