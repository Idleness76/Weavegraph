//! Optional petgraph compatibility layer.
//!
//! This module provides conversion between Weavegraph's graph representation
//! and petgraph's `DiGraph` type, enabling use of petgraph's rich algorithm
//! library for advanced analysis and DOT visualization.
//!
//! # Feature Gate
//!
//! This module is only available when the `petgraph-compat` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! weavegraph = { version = "0.1", features = ["petgraph-compat"] }
//! ```
//!
//! # Examples
//!
//! ## Convert to petgraph for analysis
//!
//! ```ignore
//! use weavegraph::graphs::GraphBuilder;
//! use weavegraph::types::NodeKind;
//!
//! let builder = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("A".into()), MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
//!     .add_edge(NodeKind::Custom("A".into()), NodeKind::End);
//!
//! let petgraph = builder.to_petgraph();
//!
//! // Use petgraph algorithms
//! use petgraph::algo::is_cyclic_directed;
//! assert!(!is_cyclic_directed(&petgraph));
//! ```
//!
//! ## Export to DOT format
//!
//! ```ignore
//! use weavegraph::graphs::GraphBuilder;
//! use weavegraph::types::NodeKind;
//!
//! let builder = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("A".into()), MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
//!     .add_edge(NodeKind::Custom("A".into()), NodeKind::End);
//!
//! let dot = builder.to_dot();
//! println!("{}", dot);
//! // Outputs:
//! // digraph {
//! //     0 [ label = "Start" ]
//! //     1 [ label = "A" ]
//! //     2 [ label = "End" ]
//! //     0 -> 1 [ ]
//! //     1 -> 2 [ ]
//! // }
//! ```

use crate::types::NodeKind;
use petgraph::graph::{DiGraph, NodeIndex};
use rustc_hash::FxHashMap;

/// A petgraph-compatible directed graph representation of a Weavegraph workflow.
///
/// Node weights are `NodeKind` values, edge weights are unit type `()`.
pub type WeaveDiGraph = DiGraph<NodeKind, ()>;

/// Mapping from NodeKind to petgraph NodeIndex.
///
/// Useful for looking up nodes in the converted graph when you need to
/// perform queries or modifications.
pub type NodeIndexMap = FxHashMap<NodeKind, NodeIndex>;

/// Result of converting a Weavegraph to petgraph format.
///
/// Contains both the graph and a mapping from NodeKind to petgraph indices
/// for convenient lookup.
#[derive(Debug, Clone)]
pub struct PetgraphConversion {
    /// The petgraph directed graph.
    pub graph: WeaveDiGraph,
    /// Mapping from Weavegraph NodeKind to petgraph NodeIndex.
    pub index_map: NodeIndexMap,
}

impl PetgraphConversion {
    /// Look up the petgraph index for a NodeKind.
    #[must_use]
    pub fn index_of(&self, node: &NodeKind) -> Option<NodeIndex> {
        self.index_map.get(node).copied()
    }

    /// Get the NodeKind at a petgraph index.
    #[must_use]
    pub fn node_at(&self, index: NodeIndex) -> Option<&NodeKind> {
        self.graph.node_weight(index)
    }
}

/// Convert a Weavegraph edge map to a petgraph DiGraph.
///
/// This is the internal conversion function used by `GraphBuilder::to_petgraph()`.
/// It creates nodes for all NodeKinds referenced in edges (including Start/End)
/// and preserves the edge topology.
pub(super) fn to_petgraph(edges: &FxHashMap<NodeKind, Vec<NodeKind>>) -> PetgraphConversion {
    let mut graph = DiGraph::new();
    let mut index_map: NodeIndexMap = FxHashMap::default();

    // Collect all unique nodes
    let mut all_nodes: Vec<NodeKind> = Vec::new();
    for (from, tos) in edges {
        if !index_map.contains_key(from) {
            all_nodes.push(from.clone());
            index_map.insert(from.clone(), NodeIndex::new(0)); // placeholder
        }
        for to in tos {
            if !index_map.contains_key(to) {
                all_nodes.push(to.clone());
                index_map.insert(to.clone(), NodeIndex::new(0)); // placeholder
            }
        }
    }

    // Sort for deterministic node indices (Start first, End last, custom alphabetically)
    all_nodes.sort_by(|a, b| {
        match (a, b) {
            (NodeKind::Start, _) => std::cmp::Ordering::Less,
            (_, NodeKind::Start) => std::cmp::Ordering::Greater,
            (NodeKind::End, _) => std::cmp::Ordering::Greater,
            (_, NodeKind::End) => std::cmp::Ordering::Less,
            (NodeKind::Custom(a_name), NodeKind::Custom(b_name)) => a_name.cmp(b_name),
        }
    });

    // Add nodes to graph and update index map
    for node in &all_nodes {
        let idx = graph.add_node(node.clone());
        index_map.insert(node.clone(), idx);
    }

    // Add edges
    for (from, tos) in edges {
        let from_idx = index_map[from];
        for to in tos {
            let to_idx = index_map[to];
            graph.add_edge(from_idx, to_idx, ());
        }
    }

    PetgraphConversion { graph, index_map }
}

/// Export a graph to DOT format for visualization.
///
/// The DOT output can be rendered using Graphviz tools (`dot`, `neato`, etc.)
/// or online viewers like https://dreampuf.github.io/GraphvizOnline/.
///
/// # Node Labels
///
/// - `Start` → "Start" (with special styling)
/// - `End` → "End" (with special styling)
/// - `Custom("name")` → "name"
///
/// # Examples
///
/// ```ignore
/// let dot = to_dot(&edges);
/// std::fs::write("workflow.dot", dot)?;
/// // Then: dot -Tpng workflow.dot -o workflow.png
/// ```
pub(super) fn to_dot(edges: &FxHashMap<NodeKind, Vec<NodeKind>>) -> String {
    use std::fmt::Write;

    let conversion = to_petgraph(edges);
    let mut output = String::new();

    writeln!(output, "digraph {{").unwrap();
    writeln!(output, "    rankdir=TB;").unwrap();
    writeln!(output, "    node [shape=box, style=rounded];").unwrap();

    // Write nodes with labels and styling
    for idx in conversion.graph.node_indices() {
        let node = conversion.graph.node_weight(idx).unwrap();
        let (label, style) = match node {
            NodeKind::Start => ("Start", " style=\"filled\" fillcolor=\"lightgreen\""),
            NodeKind::End => ("End", " style=\"filled\" fillcolor=\"lightcoral\""),
            NodeKind::Custom(name) => (name.as_str(), ""),
        };
        writeln!(
            output,
            "    {} [ label=\"{}\"{} ];",
            idx.index(),
            label,
            style
        )
        .unwrap();
    }

    writeln!(output).unwrap();

    // Write edges
    for edge in conversion.graph.edge_indices() {
        let (from, to) = conversion.graph.edge_endpoints(edge).unwrap();
        writeln!(output, "    {} -> {};", from.index(), to.index()).unwrap();
    }

    writeln!(output, "}}").unwrap();

    output
}

/// Check for cycles using petgraph's algorithm.
///
/// This provides an alternative cycle detection implementation that can be
/// used for validation fallback or cross-verification.
#[must_use]
pub fn is_cyclic(edges: &FxHashMap<NodeKind, Vec<NodeKind>>) -> bool {
    let conversion = to_petgraph(edges);
    petgraph::algo::is_cyclic_directed(&conversion.graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_linear_graph() -> FxHashMap<NodeKind, Vec<NodeKind>> {
        let mut edges = FxHashMap::default();
        edges.insert(NodeKind::Start, vec![NodeKind::Custom("A".into())]);
        edges.insert(NodeKind::Custom("A".into()), vec![NodeKind::End]);
        edges
    }

    fn make_cyclic_graph() -> FxHashMap<NodeKind, Vec<NodeKind>> {
        let mut edges = FxHashMap::default();
        edges.insert(NodeKind::Start, vec![NodeKind::Custom("A".into())]);
        edges.insert(
            NodeKind::Custom("A".into()),
            vec![NodeKind::Custom("B".into())],
        );
        edges.insert(
            NodeKind::Custom("B".into()),
            vec![NodeKind::Custom("A".into())], // cycle!
        );
        edges
    }

    #[test]
    fn test_to_petgraph_linear() {
        let edges = make_linear_graph();
        let conversion = to_petgraph(&edges);

        assert_eq!(conversion.graph.node_count(), 3);
        assert_eq!(conversion.graph.edge_count(), 2);

        // Check nodes exist
        assert!(conversion.index_of(&NodeKind::Start).is_some());
        assert!(conversion.index_of(&NodeKind::Custom("A".into())).is_some());
        assert!(conversion.index_of(&NodeKind::End).is_some());
    }

    #[test]
    fn test_is_cyclic_linear() {
        let edges = make_linear_graph();
        assert!(!is_cyclic(&edges));
    }

    #[test]
    fn test_is_cyclic_with_cycle() {
        let edges = make_cyclic_graph();
        assert!(is_cyclic(&edges));
    }

    #[test]
    fn test_to_dot_output() {
        let edges = make_linear_graph();
        let dot = to_dot(&edges);

        assert!(dot.contains("digraph {"));
        assert!(dot.contains("Start"));
        assert!(dot.contains("End"));
        assert!(dot.contains("->"));
    }

    #[test]
    fn test_deterministic_indices() {
        // Same graph should produce same indices across calls
        let edges = make_linear_graph();

        let conv1 = to_petgraph(&edges);
        let conv2 = to_petgraph(&edges);

        assert_eq!(
            conv1.index_of(&NodeKind::Start),
            conv2.index_of(&NodeKind::Start)
        );
        assert_eq!(
            conv1.index_of(&NodeKind::Custom("A".into())),
            conv2.index_of(&NodeKind::Custom("A".into()))
        );
    }
}
