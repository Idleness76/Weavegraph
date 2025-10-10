//! Graph definition and compilation for workflow execution.
//!
//! This module provides the core graph building functionality for creating
//! workflow graphs with nodes, edges, and conditional routing. The main
//! entry point is [`GraphBuilder`], which uses a builder pattern to
//! construct workflows that compile into executable [`App`] instances.
//!
//! # Core Concepts
//!
//! - **Nodes**: Executable units of work implementing the [`Node`] trait
//! - **Edges**: Connections between nodes defining execution flow
//! - **Conditional Edges**: Dynamic routing based on state predicates
//! - **Virtual Endpoints**: `NodeKind::Start` and `NodeKind::End` for structural definition
//! - **Compilation**: Validation and conversion to executable [`App`]
//!
//! # Quick Start
//!
//! ```
//! use weavegraph::graph::GraphBuilder;
//! use weavegraph::types::NodeKind;
//! use weavegraph::node::{Node, NodeContext, NodePartial, NodeError};
//! use weavegraph::state::StateSnapshot;
//! use async_trait::async_trait;
//!
//! // Define a simple node
//! struct MyNode;
//!
//! #[async_trait]
//! impl Node for MyNode {
//!     async fn run(&self, _: StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
//!         Ok(NodePartial::default())
//!     }
//! }
//!
//! // Build a simple workflow (virtual Start/End):
//! // Start (virtual) -> process -> End (virtual)
//! let app = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("process".into()), MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
//!     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
//!     .compile();
//! ```
//!
//! # Advanced Usage
//!
//! ## Conditional Routing
//!
//! ```
//! use weavegraph::graph::{GraphBuilder, EdgePredicate};
//! use weavegraph::types::NodeKind;
//! use std::sync::Arc;
//!
//! // Create a predicate that routes based on message count
//! let route_by_messages: EdgePredicate = Arc::new(|snapshot| {
//!     if snapshot.messages.len() > 5 {
//!         "process".to_string()
//!     } else {
//!         "skip".to_string()
//!     }
//! });
//!
//! # struct MyNode;
//! # #[async_trait::async_trait]
//! # impl weavegraph::node::Node for MyNode {
//! #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
//! #         Ok(weavegraph::node::NodePartial::default())
//! #     }
//! # }
//!
//! let app = GraphBuilder::new()
//!     .add_node(NodeKind::Custom("process".into()), MyNode)
//!     .add_node(NodeKind::Custom("skip".into()), MyNode)
//!     // Basic structural edge from virtual Start
//!     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
//!     .add_conditional_edge(NodeKind::Start, route_by_messages)
//!     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
//!     .add_edge(NodeKind::Custom("skip".into()), NodeKind::End)
//!     .compile();
//! ```

use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::app::*;
use crate::node::*;
use crate::runtimes::RuntimeConfig;
use crate::types::*;

/// Predicate function for conditional edge routing.
///
/// Takes a [`StateSnapshot`] and returns the target node name to determine
/// which node should be executed next. Predicates are used with
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
///         "many_messages".to_string()
///     } else {
///         "few_messages".to_string()
///     }
/// });
///
/// // Route based on extra data
/// let route_by_error: EdgePredicate = Arc::new(|snapshot| {
///     if snapshot.extra.get("error").is_some() {
///         "error_handler".to_string()
///     } else {
///         "normal_flow".to_string()
///     }
/// });
/// ```
pub type EdgePredicate = Arc<dyn Fn(crate::state::StateSnapshot) -> String + Send + Sync + 'static>;

/// A conditional edge that routes based on a predicate function.
///
/// Conditional edges allow dynamic routing in workflows based on the current
/// state. When the scheduler encounters a conditional edge, it evaluates the
/// predicate function and routes to the returned target node.
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
///         "many_messages".to_string()
///     } else {
///         "few_messages".to_string()
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

/// Builder for constructing workflow graphs with fluent API.
///
/// `GraphBuilder` provides a builder pattern for constructing workflow graphs
/// by adding nodes, edges, and configuration before compiling to an executable
/// [`App`]. The builder ensures type safety and provides clear error messages
/// for common configuration mistakes.
///
/// # Required Configuration
///
/// Every graph must have:
/// - At least one executable node added via [`add_node`](Self::add_node)
/// - Edges connecting from `NodeKind::Start` to define entry points
/// - Edges connecting to `NodeKind::End` to define exit points
///
/// Note: `NodeKind::Start` and `NodeKind::End` are virtual endpoints and should
/// never be registered with `add_node`. They exist only for structural definition.
///
/// # Examples
///
/// ## Simple Linear Workflow
/// ```
/// use weavegraph::graph::GraphBuilder;
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
///     .add_node(NodeKind::Custom("worker".into()), MyNode)
///     .add_edge(NodeKind::Start, NodeKind::Custom("worker".into()))
///     .add_edge(NodeKind::Custom("worker".into()), NodeKind::End)
///     .compile();
/// ```
///
/// ## Complex Workflow with Fan-out
/// ```
/// use weavegraph::graph::GraphBuilder;
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
///     .add_node(NodeKind::Custom("processor_a".into()), MyNode)
///     .add_node(NodeKind::Custom("processor_b".into()), MyNode)
///     // Fan-out: Start -> A and Start -> B (Start virtual)
///     .add_edge(NodeKind::Start, NodeKind::Custom("processor_a".into()))
///     .add_edge(NodeKind::Start, NodeKind::Custom("processor_b".into()))
///     // Fan-in: A -> End and B -> End
///     .add_edge(NodeKind::Custom("processor_a".into()), NodeKind::End)
///     .add_edge(NodeKind::Custom("processor_b".into()), NodeKind::End)
///     .compile();
/// ```
pub struct GraphBuilder {
    /// Registry of all nodes in the graph, keyed by their identifier.
    pub nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    /// Unconditional edges defining static graph topology.
    pub edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    /// Conditional edges for dynamic routing based on state.
    pub conditional_edges: Vec<ConditionalEdge>,
    /// Runtime configuration for the compiled application.
    pub runtime_config: RuntimeConfig,
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphBuilder {
    /// Creates a new, empty graph builder.
    ///
    /// The builder starts with no nodes, edges, or configuration.
    /// Use the fluent API methods to add components before calling
    /// [`compile`](Self::compile).
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::graph::GraphBuilder;
    ///
    /// let builder = GraphBuilder::new();
    /// // Add nodes, edges, and configuration...
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: FxHashMap::default(),
            edges: FxHashMap::default(),
            conditional_edges: Vec::new(),
            runtime_config: RuntimeConfig::default(),
        }
    }

    /// Adds a conditional edge to the graph.
    ///
    /// Conditional edges enable dynamic routing based on the current state.
    /// When execution reaches the `from` node, the `predicate` function is
    /// evaluated with the current [`StateSnapshot`] and returns the target
    /// node name for routing.
    ///
    /// # Parameters
    ///
    /// - `from`: The source node for the conditional edge
    /// - `predicate`: Function that determines target node based on state
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::graph::{GraphBuilder, EdgePredicate};
    /// use weavegraph::types::NodeKind;
    /// use std::sync::Arc;
    ///
    /// # struct MyNode;
    /// # #[async_trait::async_trait]
    /// # impl weavegraph::node::Node for MyNode {
    /// #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
    /// #         Ok(weavegraph::node::NodePartial::default())
    /// #     }
    /// # }
    ///
    /// let predicate: EdgePredicate = Arc::new(|snapshot| {
    ///     if snapshot.messages.len() > 5 {
    ///         "many_messages".to_string()
    ///     } else {
    ///         "few_messages".to_string()
    ///     }
    /// });
    ///
    /// let builder = GraphBuilder::new()
    ///     .add_node(NodeKind::Custom("many_messages".into()), MyNode)
    ///     .add_node(NodeKind::Custom("few_messages".into()), MyNode)
    ///     .add_conditional_edge(NodeKind::Start, predicate);
    /// ```
    #[must_use]
    pub fn add_conditional_edge(mut self, from: NodeKind, predicate: EdgePredicate) -> Self {
        self.conditional_edges
            .push(ConditionalEdge { from, predicate });
        self
    }

    /// Adds a node to the graph.
    ///
    /// NOTE: `NodeKind::Start` and `NodeKind::End` are virtual structural endpoints.
    /// If either is passed to `add_node`, the registration is ignored and a warning
    /// is emitted. They are not stored in the node registry and are never executed;
    /// the scheduler skips them automatically while still allowing edges from
    /// `Start` and to `End` for topology.
    ///
    /// Registers a node implementation with the given identifier. Each node
    /// must have a unique [`NodeKind`] identifier within the graph. The node
    /// implementation must implement the [`Node`] trait.
    ///
    /// # Parameters
    ///
    /// - `id`: Unique identifier for this node in the graph
    /// - `node`: Implementation of the [`Node`] trait
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::graph::GraphBuilder;
    /// use weavegraph::types::NodeKind;
    /// use weavegraph::node::{Node, NodeContext, NodePartial, NodeError};
    /// use weavegraph::state::StateSnapshot;
    /// use async_trait::async_trait;
    ///
    /// struct ProcessorNode {
    ///     name: String,
    /// }
    ///
    /// #[async_trait]
    /// impl Node for ProcessorNode {
    ///     async fn run(&self, _: StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
    ///         // Node implementation
    ///         Ok(NodePartial::default())
    ///     }
    /// }
    ///
    /// let builder = GraphBuilder::new()
    ///     .add_node(NodeKind::Custom("custom".into()), ProcessorNode { name: "custom".into() });
    /// // Edge from virtual Start
    /// // .add_edge(NodeKind::Start, NodeKind::Custom("custom".into()));
    /// ```
    #[must_use]
    pub fn add_node(mut self, id: NodeKind, node: impl Node + 'static) -> Self {
        // Ignore attempts to register virtual Start/End node kinds; emit a warning.
        match id {
            NodeKind::Start | NodeKind::End => {
                tracing::warn!(
                    ?id,
                    "Ignoring registration of virtual node kind (Start/End are virtual)"
                );
                // Do not insert into registry.
            }
            _ => {
                self.nodes.insert(id, Arc::new(node));
            }
        }
        self
    }

    /// Adds an unconditional edge between two nodes.
    ///
    /// Creates a direct connection from one node to another. When the `from`
    /// node completes execution, the scheduler will consider the `to` node
    /// for execution in the next step. Multiple edges from the same node
    /// create fan-out patterns, while multiple edges to the same node
    /// create fan-in patterns.
    ///
    /// # Parameters
    ///
    /// - `from`: Source node identifier
    /// - `to`: Target node identifier
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::graph::GraphBuilder;
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
    ///     .add_node(NodeKind::Custom("step".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("step".into()))
    ///     .add_edge(NodeKind::Custom("step".into()), NodeKind::End); // Linear workflow with virtual endpoints
    /// ```
    ///
    /// ## Fan-out Pattern
    /// ```
    /// use weavegraph::graph::GraphBuilder;
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
    ///     .add_node(NodeKind::Custom("worker_a".into()), MyNode)
    ///     .add_node(NodeKind::Custom("worker_b".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("worker_a".into()))
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("worker_b".into())); // Fan-out from virtual Start
    /// ```
    #[must_use]
    pub fn add_edge(mut self, from: NodeKind, to: NodeKind) -> Self {
        self.edges.entry(from).or_default().push(to);
        self
    }

    /// Configures runtime settings for the compiled application.
    ///
    /// Runtime configuration controls execution behavior such as concurrency
    /// limits, checkpointing, and session management. If not specified,
    /// default configuration is used.
    ///
    /// # Parameters
    ///
    /// - `runtime_config`: Configuration for the compiled application
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::graph::GraphBuilder;
    /// use weavegraph::runtimes::RuntimeConfig;
    ///
    /// # struct MyNode;
    /// # #[async_trait::async_trait]
    /// # impl weavegraph::node::Node for MyNode {
    /// #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
    /// #         Ok(weavegraph::node::NodePartial::default())
    /// #     }
    /// # }
    ///
    /// let config = RuntimeConfig::new(
    ///     Some("my_session".into()),
    ///     None, // Default checkpointer
    ///     None, // Default database
    /// );
    ///
    /// let builder = GraphBuilder::new()
    ///     .with_runtime_config(config);
    /// ```
    #[must_use]
    pub fn with_runtime_config(mut self, runtime_config: RuntimeConfig) -> Self {
        self.runtime_config = runtime_config;
        self
    }

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
    /// use weavegraph::graph::GraphBuilder;
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
        App::from_parts(
            self.nodes,
            self.edges,
            self.conditional_edges,
            self.runtime_config,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use async_trait::async_trait;

    // Simple test nodes for graph testing
    #[derive(Debug, Clone)]
    struct NodeA;

    #[async_trait]
    impl crate::node::Node for NodeA {
        async fn run(
            &self,
            _snapshot: crate::state::StateSnapshot,
            _ctx: crate::node::NodeContext,
        ) -> Result<crate::node::NodePartial, crate::node::NodeError> {
            Ok(crate::node::NodePartial::new()
                .with_messages(vec![Message::assistant("NodeA executed")]))
        }
    }

    #[derive(Debug, Clone)]
    struct NodeB;

    #[async_trait]
    impl crate::node::Node for NodeB {
        async fn run(
            &self,
            _snapshot: crate::state::StateSnapshot,
            _ctx: crate::node::NodeContext,
        ) -> Result<crate::node::NodePartial, crate::node::NodeError> {
            Ok(crate::node::NodePartial::new()
                .with_messages(vec![Message::assistant("NodeB executed")]))
        }
    }

    #[test]
    /// Tests adding conditional edges to a graph builder.
    ///
    /// Verifies that conditional edges are properly stored and that predicates
    /// can be evaluated correctly. This test uses a simple predicate that returns
    /// a target node name and validates the edge structure.
    fn test_add_conditional_edge() {
        use crate::state::StateSnapshot;
        let route_to_y: super::EdgePredicate =
            std::sync::Arc::new(|_s: StateSnapshot| "Y".to_string());
        let gb = super::GraphBuilder::new()
            .add_node(super::NodeKind::Custom("Y".into()), NodeA)
            .add_node(super::NodeKind::Custom("N".into()), NodeA)
            .add_conditional_edge(super::NodeKind::Start, route_to_y.clone());
        assert_eq!(gb.conditional_edges.len(), 1);
        let ce = &gb.conditional_edges[0];
        assert_eq!(ce.from, super::NodeKind::Start);
        // Predicate should return "Y"
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: crate::utils::collections::new_extra_map(),
            extra_version: 1,
            errors: vec![],
            errors_version: 1,
        };
        assert_eq!((ce.predicate)(snap), "Y");
    }

    #[test]
    /// Verifies that a new GraphBuilder is initialized with empty collections.
    ///
    /// Tests the default state of a new builder to ensure clean initialization
    /// before any nodes or edges are added.
    fn test_graph_builder_new() {
        let gb = GraphBuilder::new();
        assert!(gb.nodes.is_empty());
        assert!(gb.edges.is_empty());
        assert!(gb.conditional_edges.is_empty());
        // entry field removed; no explicit entry point tracking required
    }

    #[test]
    /// Checks that nodes can be added to the GraphBuilder and are stored correctly.
    ///
    /// Validates that the builder properly stores node implementations and that
    /// they can be retrieved by their NodeKind identifiers.
    fn test_add_node() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Custom("A".into()), NodeA)
            .add_node(NodeKind::Custom("B".into()), NodeB);
        assert_eq!(gb.nodes.len(), 2);
        assert!(gb.nodes.contains_key(&NodeKind::Custom("A".into())));
        assert!(gb.nodes.contains_key(&NodeKind::Custom("B".into())));
    }

    #[test]
    /// Ensures edges can be added between nodes and are tracked properly in the builder.
    ///
    /// Tests that edges are stored in the correct adjacency list structure and that
    /// multiple edges from the same source node are properly accumulated.
    fn test_add_edge() {
        let gb = GraphBuilder::new()
            .add_edge(NodeKind::Start, NodeKind::End)
            .add_edge(NodeKind::Start, NodeKind::Custom("C".to_string()));
        assert_eq!(gb.edges.len(), 1);
        let edges = gb.edges.get(&NodeKind::Start).unwrap();
        assert_eq!(edges.len(), 2);
        assert!(edges.contains(&NodeKind::End));
        assert!(edges.contains(&NodeKind::Custom("C".to_string())));
    }

    #[test]
    /// Validates that compiling a GraphBuilder produces an App with correct structure.
    ///
    /// Tests the compilation process for a valid graph configuration and verifies
    /// that the resulting App contains the expected nodes and edges.
    fn test_compile() {
        let gb = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
        let app = gb.compile();
        // Only edge topology is guaranteed when using virtual Start/End.
        assert_eq!(app.edges().len(), 1);
        assert!(app
            .edges()
            .get(&NodeKind::Start)
            .unwrap()
            .contains(&NodeKind::End));
    }

    #[test]
    /// Tests basic graph compilation with virtual Start/End nodes.
    ///
    /// Validates that graphs compile successfully when using virtual Start/End
    /// endpoints without requiring explicit entry point configuration.
    fn test_compile_missing_entry() {
        let gb = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
        let app = gb.compile();
        assert!(app.edges().get(&NodeKind::Start).is_some());
    }

    #[test]
    /// Tests graph compilation with virtual endpoints.
    ///
    /// Validates that graphs using virtual Start/End nodes compile successfully
    /// and maintain proper edge topology without entry point validation.
    fn test_compile_entry_not_registered() {
        let gb = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
        let app = gb.compile();
        // Virtual Start/End: verify edge topology only
        assert_eq!(app.edges().len(), 1);
    }

    #[test]
    /// Tests equality and inequality for NodeKind::Other variant with different string values.
    ///
    /// Validates that NodeKind comparison works correctly for custom node types.
    fn test_nodekind_other_variant() {
        let k1 = NodeKind::Custom("foo".to_string());
        let k2 = NodeKind::Custom("foo".to_string());
        let k3 = NodeKind::Custom("bar".to_string());
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    /// Checks that duplicate edges between the same nodes are allowed and counted correctly.
    ///
    /// Tests that the builder supports multiple edges between the same pair of nodes,
    /// which is useful for fan-out patterns and ensuring certain execution sequences.
    fn test_duplicate_edges() {
        let gb = GraphBuilder::new()
            .add_edge(NodeKind::Start, NodeKind::End)
            .add_edge(NodeKind::Start, NodeKind::End);
        let edges = gb.edges.get(&NodeKind::Start).unwrap();
        // Both edges should be present (duplicates allowed)
        let count = edges.iter().filter(|k| **k == NodeKind::End).count();
        assert_eq!(count, 2);
    }

    #[test]
    /// Tests that the builder pattern maintains immutability and fluent API design.
    ///
    /// Validates that each method returns a new builder instance with the added
    /// configuration, enabling method chaining.
    fn test_builder_fluent_api() {
        let final_builder = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
        // Should compile successfully
        let _app = final_builder.compile();
    }

    #[test]
    /// Tests runtime configuration integration with GraphBuilder.
    ///
    /// Validates that runtime configuration is properly stored and passed through
    /// to the compiled App instance.
    fn test_runtime_config_integration() {
        use crate::runtimes::RuntimeConfig;

        let config = RuntimeConfig::new(Some("test_session".into()), None, None);

        let builder = GraphBuilder::new()
            .add_edge(NodeKind::Start, NodeKind::End)
            .with_runtime_config(config);

        // Should compile successfully with custom runtime config
        let _app = builder.compile();
    }
}
