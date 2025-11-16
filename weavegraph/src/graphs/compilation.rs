//! Graph compilation logic and validation.
//!
//! This module contains the logic for compiling a GraphBuilder into an
//! executable App, including structural validation and actionable errors.

use crate::app::App;
use crate::types::NodeKind;
use rustc_hash::FxHashMap;

/// Errors that can occur when compiling a graph.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum GraphCompileError {
    /// No entry edge was defined from the virtual Start node.
    #[error("missing entry: no edge or conditional edge originates from Start")]
    MissingEntry,

    /// An edge references a node that is not registered in the graph.
    #[error("unknown node referenced in edge: {0}")]
    UnknownNode(NodeKind),

    /// An edge originates from the virtual End node, which is terminal.
    #[error("invalid edge: cannot originate from End")]
    EdgeFromEnd,

    /// A cycle was detected in the graph.
    #[error("cycle detected in graph: {}", .cycle.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" -> "))]
    CycleDetected {
        /// The cycle path showing nodes forming the cycle.
        cycle: Vec<NodeKind>,
    },

    /// One or more nodes are unreachable from the Start node.
    #[error("unreachable nodes detected (no path from Start): {}", .nodes.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", "))]
    UnreachableNodes {
        /// List of nodes with no path from Start.
        nodes: Vec<NodeKind>,
    },

    /// One or more nodes have no path to the End node.
    #[error("nodes with no path to End: {}", .nodes.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", "))]
    NoPathToEnd {
        /// List of nodes that cannot reach End.
        nodes: Vec<NodeKind>,
    },

    /// A duplicate edge was detected.
    #[error("duplicate edge detected: {} -> {}", .from, .to)]
    DuplicateEdge {
        /// The source node of the duplicate edge.
        from: NodeKind,
        /// The target node of the duplicate edge.
        to: NodeKind,
    },
}

/// Compilation logic for GraphBuilder.
impl super::builder::GraphBuilder {
    /// Compiles the graph into an executable application.
    ///
    /// Validates the graph configuration and converts it into an [`App`] that
    /// can execute workflows. This method performs validation checks to prevent
    /// common topology issues (missing entry, cycles, unknown nodes, duplicates).
    ///
    /// # Returns
    ///
    /// - `Ok(App)`: Successfully compiled application ready for execution
    /// - `Err(GraphCompileError)`: Structural validation failed; inspect the variant
    ///
    /// # Examples
    ///
    /// Basic pattern with error propagation:
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
    /// let app = GraphBuilder::new()
    ///     .add_node(NodeKind::Custom("process".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
    ///     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
    ///     .compile()?;
    /// # Ok::<_, weavegraph::graphs::GraphCompileError>(())
    /// ```
    ///
    /// Explicit handling with pattern matching:
    /// ```
    /// use weavegraph::graphs::{GraphBuilder, GraphCompileError};
    /// use weavegraph::types::NodeKind;
    ///
    /// let result = GraphBuilder::new()
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
    ///     .compile();
    ///
    /// match result {
    ///     Ok(_app) => {}
    ///     Err(GraphCompileError::MissingEntry) => {
    ///         eprintln!("graph has no entry edge from Start");
    ///     }
    ///     Err(GraphCompileError::UnknownNode(nk)) => {
    ///         eprintln!("unknown node referenced: {nk}");
    ///     }
    ///     Err(e) => {
    ///         eprintln!("graph validation failed: {e}");
    ///     }
    /// }
    /// ```
    pub fn compile(self) -> Result<App, GraphCompileError> {
        // Validate without consuming self
        self.validate()?;

        let (nodes, edges, conditional_edges, runtime_config, reducer_registry) = self.into_parts();
        Ok(App::from_parts(
            nodes,
            edges,
            conditional_edges,
            runtime_config,
            reducer_registry,
        ))
    }

    /// Detects cycles in the graph using DFS with color marking.
    ///
    /// Only checks unconditional edges, as conditional edge targets are runtime-determined.
    /// Returns the first cycle found as a path of nodes.
    fn detect_cycle(&self) -> Option<Vec<NodeKind>> {
        #[derive(Clone, Copy, PartialEq)]
        enum Color {
            White, // Not visited
            Gray,  // Currently visiting
            Black, // Fully visited
        }

        let mut colors: FxHashMap<NodeKind, Color> = FxHashMap::default();
        let mut path: Vec<NodeKind> = Vec::new();

        // Initialize all nodes as White
        for from in self.edges_ref().keys() {
            colors.entry(from.clone()).or_insert(Color::White);
        }
        for tos in self.edges_ref().values() {
            for to in tos {
                colors.entry(to.clone()).or_insert(Color::White);
            }
        }

        // DFS helper function
        fn dfs(
            node: &NodeKind,
            colors: &mut FxHashMap<NodeKind, Color>,
            path: &mut Vec<NodeKind>,
            edges: &FxHashMap<NodeKind, Vec<NodeKind>>,
        ) -> Option<Vec<NodeKind>> {
            colors.insert(node.clone(), Color::Gray);
            path.push(node.clone());

            if let Some(neighbors) = edges.get(node) {
                for neighbor in neighbors {
                    match colors.get(neighbor).copied().unwrap_or(Color::White) {
                        Color::White => {
                            if let Some(cycle) = dfs(neighbor, colors, path, edges) {
                                return Some(cycle);
                            }
                        }
                        Color::Gray => {
                            // Found a back edge - extract the cycle
                            if let Some(cycle_start) = path.iter().position(|n| n == neighbor) {
                                let mut cycle = path[cycle_start..].to_vec();
                                cycle.push(neighbor.clone()); // Complete the cycle
                                return Some(cycle);
                            }
                        }
                        Color::Black => {
                            // Already fully explored, skip
                        }
                    }
                }
            }

            path.pop();
            colors.insert(node.clone(), Color::Black);
            None
        }

        // Try DFS from each unvisited node
        for node in colors.clone().keys() {
            if colors.get(node).copied().unwrap_or(Color::White) == Color::White
                && let Some(cycle) = dfs(node, &mut colors, &mut path, self.edges_ref())
            {
                return Some(cycle);
            }
        }

        None
    }

    /// Detects unreachable nodes (nodes with no path from Start).
    ///
    /// Only checks unconditional edges. Returns registered Custom nodes that
    /// cannot be reached from Start via unconditional edges.
    fn detect_unreachable_nodes(&self) -> Vec<NodeKind> {
        use std::collections::VecDeque;

        let mut reachable: FxHashMap<NodeKind, bool> = FxHashMap::default();
        let mut queue: VecDeque<NodeKind> = VecDeque::new();

        // Start BFS from Start node
        queue.push_back(NodeKind::Start);
        reachable.insert(NodeKind::Start, true);

        while let Some(node) = queue.pop_front() {
            if let Some(neighbors) = self.edges_ref().get(&node) {
                for neighbor in neighbors {
                    if !reachable.contains_key(neighbor) {
                        reachable.insert(neighbor.clone(), true);
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }

        // Find registered Custom nodes that are not reachable
        let mut unreachable: Vec<NodeKind> = self
            .nodes_ref()
            .keys()
            .filter(|node| !reachable.contains_key(node))
            .cloned()
            .collect();

        unreachable.sort_by_key(|a| a.to_string());
        unreachable
    }

    /// Detects nodes with no path to End.
    ///
    /// Only checks unconditional edges. Returns registered Custom nodes that
    /// cannot reach End via unconditional edges.
    fn detect_no_path_to_end(&self) -> Vec<NodeKind> {
        use std::collections::VecDeque;

        // Build reverse graph (for backward traversal from End)
        let mut reverse_edges: FxHashMap<NodeKind, Vec<NodeKind>> = FxHashMap::default();
        for (from, tos) in self.edges_ref() {
            for to in tos {
                reverse_edges
                    .entry(to.clone())
                    .or_default()
                    .push(from.clone());
            }
        }

        let mut can_reach_end: FxHashMap<NodeKind, bool> = FxHashMap::default();
        let mut queue: VecDeque<NodeKind> = VecDeque::new();

        // Start BFS from End node (backward)
        queue.push_back(NodeKind::End);
        can_reach_end.insert(NodeKind::End, true);

        while let Some(node) = queue.pop_front() {
            if let Some(predecessors) = reverse_edges.get(&node) {
                for predecessor in predecessors {
                    if !can_reach_end.contains_key(predecessor) {
                        can_reach_end.insert(predecessor.clone(), true);
                        queue.push_back(predecessor.clone());
                    }
                }
            }
        }

        // Find registered Custom nodes that cannot reach End
        let mut no_path: Vec<NodeKind> = self
            .nodes_ref()
            .keys()
            .filter(|node| !can_reach_end.contains_key(node))
            .cloned()
            .collect();

        no_path.sort_by_key(|a| a.to_string());
        no_path
    }

    /// Detects duplicate edges in the graph.
    ///
    /// Returns the first duplicate edge found.
    fn detect_duplicate_edge(&self) -> Option<(NodeKind, NodeKind)> {
        use rustc_hash::FxHashSet;

        for (from, tos) in self.edges_ref() {
            let mut seen: FxHashSet<NodeKind> = FxHashSet::default();
            for to in tos {
                if !seen.insert(to.clone()) {
                    // Found a duplicate
                    return Some((from.clone(), to.clone()));
                }
            }
        }
        None
    }

    /// Validates the graph for common structural issues.
    ///
    /// Validation rules:
    /// - There must be at least one entry edge from Start (unconditional or conditional)
    /// - No edge may originate from End
    /// - Any Custom node referenced by an edge (as from/to) must be registered
    /// - The graph must not contain cycles (checked on unconditional edges only)
    /// - All registered nodes must be reachable from Start (unconditional edges only)
    /// - All registered nodes must have a path to End (unconditional edges only)
    /// - No duplicate edges are allowed
    pub fn validate(&self) -> Result<(), GraphCompileError> {
        // Rule 1: Entry edge from Start exists (either unconditional or conditional)
        let has_start_edge = self
            .edges_ref()
            .get(&NodeKind::Start)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
            || self
                .conditional_edges_ref()
                .iter()
                .any(|ce| ce.from() == &NodeKind::Start);

        if !has_start_edge {
            return Err(GraphCompileError::MissingEntry);
        }

        // Rule 2: Detect cycles in unconditional edges
        if let Some(cycle) = self.detect_cycle() {
            return Err(GraphCompileError::CycleDetected { cycle });
        }

        // Rule 3 and 4: Reachability validations (skip when conditional edges exist)
        let has_conditional = !self.conditional_edges_ref().is_empty();
        if !has_conditional {
            // Detect unreachable nodes
            let unreachable = self.detect_unreachable_nodes();
            if !unreachable.is_empty() {
                return Err(GraphCompileError::UnreachableNodes { nodes: unreachable });
            }

            // Detect nodes with no path to End
            let no_path_to_end = self.detect_no_path_to_end();
            if !no_path_to_end.is_empty() {
                return Err(GraphCompileError::NoPathToEnd {
                    nodes: no_path_to_end,
                });
            }
        }

        // Rule 5: Detect duplicate edges
        if let Some((from, to)) = self.detect_duplicate_edge() {
            return Err(GraphCompileError::DuplicateEdge { from, to });
        }

        // Rule 6 and 7: Validate each unconditional edge
        for (from, tos) in self.edges_ref() {
            // End cannot have outgoing edges
            if matches!(from, NodeKind::End) {
                return Err(GraphCompileError::EdgeFromEnd);
            }

            // If from is Custom, it must be registered
            if let NodeKind::Custom(_) = from
                && !self.nodes_ref().contains_key(from)
            {
                return Err(GraphCompileError::UnknownNode(from.clone()));
            }

            for to in tos {
                // If to is Custom, it must be registered
                if let NodeKind::Custom(_) = to
                    && !self.nodes_ref().contains_key(to)
                {
                    return Err(GraphCompileError::UnknownNode(to.clone()));
                }
            }
        }

        Ok(())
    }
}
