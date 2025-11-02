//! Graph compilation logic and validation.
//!
//! This module contains the logic for compiling a GraphBuilder into an
//! executable App, including future validation and error handling.

use crate::app::App;
use crate::types::NodeKind;

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
}

/// Compilation logic for GraphBuilder.
impl super::builder::GraphBuilder {
    /// Compiles the graph into an executable application.
    ///
    /// Validates the graph configuration and converts it into an [`App`] that
    /// can execute workflows. This method performs several validation checks:
    ///
    /// - Future: cycle detection, reachability analysis
    /// - Future: validation that at least one edge originates from Start
    ///
    /// # Returns
    ///
    /// - `Ok(App)`: Successfully compiled application ready for execution
    ///
    /// # Errors
    ///
    /// Currently none. (Reserved for future structural validation errors.)
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
    /// let app = GraphBuilder::new()
    ///     .add_node(NodeKind::Custom("process".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
    ///     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
    ///     .compile();
    ///
    /// // App is ready for execution
    /// ```
    pub fn compile(self) -> Result<App, GraphCompileError> {
        // Validate without consuming self
        self.validate()?;

        let (nodes, edges, conditional_edges, runtime_config) = self.into_parts();
        Ok(App::from_parts(
            nodes,
            edges,
            conditional_edges,
            runtime_config,
        ))
    }

    /// Validates the graph for common structural issues.
    ///
    /// Validation rules:
    /// - There must be at least one entry edge from Start (unconditional or conditional)
    /// - No edge may originate from End
    /// - Any Custom node referenced by an edge (as from/to) must be registered
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

        // Rule 2 and 3: Validate each unconditional edge
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
