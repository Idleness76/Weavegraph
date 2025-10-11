//! GraphBuilder implementation for constructing workflow graphs.
//!
//! This module contains the main GraphBuilder type and its fluent API
//! for constructing workflow graphs with nodes, edges, and configuration.

use rustc_hash::FxHashMap;
use std::sync::Arc;

use super::edges::{ConditionalEdge, EdgePredicate};
use crate::node::Node;
use crate::runtimes::RuntimeConfig;
use crate::types::NodeKind;

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
    /// node names for routing.
    ///
    /// # Parameters
    ///
    /// - `from`: The source node for the conditional edge
    /// - `predicate`: Function that determines target nodes based on state
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
    ///         vec!["many_messages".to_string()]
    ///     } else {
    ///         vec!["few_messages".to_string()]
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
}
