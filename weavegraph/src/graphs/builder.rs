//! GraphBuilder implementation for constructing workflow graphs.
//!
//! This module contains the main GraphBuilder type and its fluent API
//! for constructing workflow graphs with nodes, edges, and configuration.

use rustc_hash::FxHashMap;
use std::sync::Arc;

use super::edges::{ConditionalEdge, EdgePredicate};
use crate::node::Node;
use crate::reducers::{Reducer, ReducerRegistry};
use crate::runtimes::{EventBusConfig, RuntimeConfig};
use crate::types::{ChannelType, NodeKind};

/// Type alias for the internal parts of a GraphBuilder.
/// Used to reduce type complexity in the `into_parts()` method.
type GraphParts = (
    FxHashMap<NodeKind, Arc<dyn Node>>,
    FxHashMap<NodeKind, Vec<NodeKind>>,
    Vec<ConditionalEdge>,
    RuntimeConfig,
    ReducerRegistry,
);

/// Builder for constructing workflow graphs with fluent API.
///
/// `GraphBuilder` provides a builder pattern for constructing workflow graphs
/// by adding nodes, edges, and configuration before compiling to an executable
/// [`App`](crate::app::App). The builder ensures type safety and provides clear error messages
/// for common configuration mistakes.
///
/// # Required Configuration
///
/// Every graph must have:
/// - At least one executable node added via [`GraphBuilder::add_node`](Self::add_node)
/// - Edges connecting from `NodeKind::Start` to define entry points
/// - Edges connecting to `NodeKind::End` to define exit points
///
/// Note: `NodeKind::Start` and `NodeKind::End` are virtual endpoints and should
/// never be registered with `add_node`. They exist only for structural definition.
///
/// # Examples
///
/// ## Basic Usage
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
/// // Linear workflow: Start -> worker -> End
/// let app = GraphBuilder::new()
///     .add_node(NodeKind::Custom("worker".into()), MyNode)
///     .add_edge(NodeKind::Start, NodeKind::Custom("worker".into()))
///     .add_edge(NodeKind::Custom("worker".into()), NodeKind::End)
///     .compile();
/// ```
///
/// ## Conditional Routing
/// ```
/// use weavegraph::graphs::{GraphBuilder, EdgePredicate};
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
/// let route_by_count: EdgePredicate = Arc::new(|snapshot| {
///     if snapshot.messages.len() > 5 {
///         vec!["heavy_processing".to_string()]
///     } else {
///         vec!["light_processing".to_string()]
///     }
/// });
///
/// let app = GraphBuilder::new()
///     .add_node(NodeKind::Custom("heavy_processing".into()), MyNode)
///     .add_node(NodeKind::Custom("light_processing".into()), MyNode)
///     .add_conditional_edge(NodeKind::Start, route_by_count)
///     .add_edge(NodeKind::Custom("heavy_processing".into()), NodeKind::End)
///     .add_edge(NodeKind::Custom("light_processing".into()), NodeKind::End)
///     .compile();
/// ```
pub struct GraphBuilder {
    /// Registry of all nodes in the graph, keyed by their identifier.
    nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    /// Unconditional edges defining static graph topology.
    edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    /// Conditional edges for dynamic routing based on state.
    conditional_edges: Vec<ConditionalEdge>,
    /// Runtime configuration for the compiled application.
    runtime_config: RuntimeConfig,
    /// Reducer registry for channel update operations.
    reducer_registry: ReducerRegistry,
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
    /// use weavegraph::graphs::GraphBuilder;
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
            reducer_registry: ReducerRegistry::default(),
        }
    }

    /// Adds a conditional edge to the graph.
    ///
    /// Conditional edges enable dynamic routing based on the current state.
    /// When execution reaches the `from` node, the `predicate` function is
    /// evaluated with the current [`StateSnapshot`](crate::state::StateSnapshot) and returns the target
    /// node names for routing.
    ///
    /// # Parameters
    ///
    /// - `from`: The source node for the conditional edge
    /// - `predicate`: Function that determines target nodes based on state
    #[must_use]
    pub fn add_conditional_edge(mut self, from: NodeKind, predicate: EdgePredicate) -> Self {
        self.conditional_edges
            .push(ConditionalEdge::new(from, predicate));
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
    #[must_use]
    pub fn with_runtime_config(mut self, runtime_config: RuntimeConfig) -> Self {
        self.runtime_config = runtime_config;
        self
    }

    /// Overrides only the event bus configuration while keeping other runtime settings.
    #[must_use]
    pub fn with_event_bus_config(mut self, config: EventBusConfig) -> Self {
        let mut runtime_config = self.runtime_config.clone();
        runtime_config.event_bus = config;
        self.runtime_config = runtime_config;
        self
    }

    /// Registers a custom reducer for a specific channel.
    ///
    /// This method enables registration of custom reducers to extend or replace
    /// the default reducer behavior for a channel. Multiple reducers can be
    /// registered for the same channel and will be applied in registration order.
    ///
    /// # Parameters
    ///
    /// - `channel`: The channel type to register the reducer for
    /// - `reducer`: The reducer implementation wrapped in Arc
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use weavegraph::graphs::GraphBuilder;
    /// use weavegraph::reducers::{Reducer, AddMessages};
    /// use weavegraph::types::{ChannelType, NodeKind};
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
    ///     .with_reducer(ChannelType::Message, Arc::new(AddMessages))
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("worker".into()))
    ///     .add_edge(NodeKind::Custom("worker".into()), NodeKind::End)
    ///     .compile();
    /// ```
    #[must_use]
    pub fn with_reducer(mut self, channel: ChannelType, reducer: Arc<dyn Reducer>) -> Self {
        self.reducer_registry.register(channel, reducer);
        self
    }

    /// Replaces the entire reducer registry with a custom one.
    ///
    /// This method allows complete control over reducer configuration by
    /// replacing the default registry. Useful when you need fine-grained
    /// control over reducer ordering or want to start with an empty registry.
    ///
    /// # Parameters
    ///
    /// - `registry`: The reducer registry to use
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use weavegraph::graphs::GraphBuilder;
    /// use weavegraph::reducers::{ReducerRegistry, AddMessages};
    /// use weavegraph::types::{ChannelType, NodeKind};
    ///
    /// # struct MyNode;
    /// # #[async_trait::async_trait]
    /// # impl weavegraph::node::Node for MyNode {
    /// #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext) -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
    /// #         Ok(weavegraph::node::NodePartial::default())
    /// #     }
    /// # }
    ///
    /// let custom_registry = ReducerRegistry::new()
    ///     .with_reducer(ChannelType::Message, Arc::new(AddMessages));
    ///
    /// let app = GraphBuilder::new()
    ///     .add_node(NodeKind::Custom("worker".into()), MyNode)
    ///     .with_reducer_registry(custom_registry)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("worker".into()))
    ///     .add_edge(NodeKind::Custom("worker".into()), NodeKind::End)
    ///     .compile();
    /// ```
    #[must_use]
    pub fn with_reducer_registry(mut self, registry: ReducerRegistry) -> Self {
        self.reducer_registry = registry;
        self
    }

    // =========================================================================
    // Iterators (petgraph-style API)
    // =========================================================================

    /// Returns an iterator over all registered nodes in the graph.
    ///
    /// This iterates over custom nodes only; virtual `Start` and `End` nodes
    /// are not included as they are not stored in the registry.
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
    ///     .add_node(NodeKind::Custom("B".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
    ///     .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
    ///     .add_edge(NodeKind::Custom("B".into()), NodeKind::End);
    ///
    /// let node_count = builder.nodes().count();
    /// assert_eq!(node_count, 2);
    /// ```
    pub fn nodes(&self) -> super::iteration::NodesIter<'_> {
        super::iteration::NodesIter::new(self.nodes.keys())
    }

    /// Returns an iterator over all edges in the graph as (source, target) pairs.
    ///
    /// Includes edges from/to virtual `Start` and `End` nodes.
    /// The iteration order is not deterministic due to hash map iteration;
    /// use [`topological_sort`](Self::topological_sort) for ordered traversal.
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
    /// let edge_count = builder.edges().count();
    /// assert_eq!(edge_count, 2);
    /// ```
    pub fn edges(&self) -> super::iteration::EdgesIter<'_> {
        super::iteration::EdgesIter::new(&self.edges)
    }

    /// Returns the number of registered nodes in the graph.
    ///
    /// Does not include virtual `Start` and `End` nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of edges in the graph.
    ///
    /// Counts all edges including those from/to virtual nodes.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }

    // =========================================================================
    // Graph Algorithms
    // =========================================================================

    /// Returns a topologically sorted list of all nodes in the graph.
    ///
    /// The result includes virtual `Start` (always first) and `End` (always last)
    /// nodes along with all custom nodes. Nodes at the same topological level
    /// are sorted lexicographically for deterministic ordering.
    ///
    /// This is useful for:
    /// - Deterministic iteration over nodes
    /// - Dependency analysis
    /// - Visualization and debugging
    ///
    /// # Note
    ///
    /// This method assumes the graph is acyclic. If the graph contains cycles,
    /// the result will exclude nodes involved in cycles. Use [`compile`](Self::compile)
    /// to validate the graph before relying on topological sort.
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
    ///     .add_node(NodeKind::Custom("B".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
    ///     .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
    ///     .add_edge(NodeKind::Custom("B".into()), NodeKind::End);
    ///
    /// let sorted = builder.topological_sort();
    /// assert_eq!(sorted[0], NodeKind::Start);
    /// assert_eq!(sorted[sorted.len() - 1], NodeKind::End);
    ///
    /// // A comes before B due to edge A -> B
    /// let a_pos = sorted.iter().position(|n| n == &NodeKind::Custom("A".into())).unwrap();
    /// let b_pos = sorted.iter().position(|n| n == &NodeKind::Custom("B".into())).unwrap();
    /// assert!(a_pos < b_pos);
    /// ```
    #[must_use]
    pub fn topological_sort(&self) -> Vec<crate::types::NodeKind> {
        super::iteration::topological_sort(&self.edges)
    }

    // =========================================================================
    // petgraph Compatibility (feature-gated)
    // =========================================================================

    /// Converts the graph to a petgraph `DiGraph` for advanced algorithms.
    ///
    /// This is useful for:
    /// - Advanced graph algorithms (shortest path, max flow, etc.)
    /// - Graph analysis and metrics
    /// - Integration with petgraph ecosystem tools
    ///
    /// # Feature Gate
    ///
    /// This method requires the `petgraph-compat` feature:
    /// ```toml
    /// weavegraph = { features = ["petgraph-compat"] }
    /// ```
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use weavegraph::graphs::GraphBuilder;
    /// use petgraph::algo::is_cyclic_directed;
    ///
    /// let builder = GraphBuilder::new()
    ///     .add_node(NodeKind::Custom("A".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
    ///     .add_edge(NodeKind::Custom("A".into()), NodeKind::End);
    ///
    /// let pg = builder.to_petgraph();
    /// assert!(!is_cyclic_directed(&pg.graph));
    /// ```
    #[cfg(feature = "petgraph-compat")]
    #[must_use]
    pub fn to_petgraph(&self) -> super::petgraph_compat::PetgraphConversion {
        super::petgraph_compat::to_petgraph(&self.edges)
    }

    /// Exports the graph to DOT format for visualization.
    ///
    /// The output can be rendered using Graphviz (`dot -Tpng graph.dot -o graph.png`)
    /// or online tools like <https://dreampuf.github.io/GraphvizOnline/>.
    ///
    /// # Feature Gate
    ///
    /// This method requires the `petgraph-compat` feature:
    /// ```toml
    /// weavegraph = { features = ["petgraph-compat"] }
    /// ```
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use weavegraph::graphs::GraphBuilder;
    /// use std::fs;
    ///
    /// let builder = GraphBuilder::new()
    ///     .add_node(NodeKind::Custom("A".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
    ///     .add_edge(NodeKind::Custom("A".into()), NodeKind::End);
    ///
    /// let dot = builder.to_dot();
    /// fs::write("workflow.dot", &dot)?;
    /// // Then run: dot -Tpng workflow.dot -o workflow.png
    /// ```
    #[cfg(feature = "petgraph-compat")]
    #[must_use]
    pub fn to_dot(&self) -> String {
        super::petgraph_compat::to_dot(&self.edges)
    }

    /// Checks if the graph contains cycles using petgraph's algorithm.
    ///
    /// This provides an alternative to the built-in cycle detection for
    /// cross-verification or when you need petgraph's specific behavior.
    ///
    /// # Feature Gate
    ///
    /// This method requires the `petgraph-compat` feature.
    #[cfg(feature = "petgraph-compat")]
    #[must_use]
    pub fn is_cyclic_petgraph(&self) -> bool {
        super::petgraph_compat::is_cyclic(&self.edges)
    }

    // =========================================================================
    // Internal Helpers
    // =========================================================================

    /// Extracts the components for compilation (internal use only).
    pub(super) fn into_parts(self) -> GraphParts {
        (
            self.nodes,
            self.edges,
            self.conditional_edges,
            self.runtime_config,
            self.reducer_registry,
        )
    }

    // Internal read-only accessors for validation in sibling modules
    pub(super) fn nodes_ref(&self) -> &FxHashMap<NodeKind, Arc<dyn Node>> {
        &self.nodes
    }
    pub(super) fn edges_ref(&self) -> &FxHashMap<NodeKind, Vec<NodeKind>> {
        &self.edges
    }
    pub(super) fn conditional_edges_ref(&self) -> &Vec<ConditionalEdge> {
        &self.conditional_edges
    }
}
