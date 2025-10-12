//! Graph compilation logic and validation.
//!
//! This module contains the logic for compiling a GraphBuilder into an
//! executable App, including future validation and error handling.

use crate::app::App;

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
    pub fn compile(self) -> App {
        let (nodes, edges, conditional_edges, runtime_config) = self.into_parts();
        App::from_parts(nodes, edges, conditional_edges, runtime_config)
    }
}
