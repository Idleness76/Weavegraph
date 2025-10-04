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
//! - **Entry Point**: The starting node for workflow execution
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
//! // Build a simple workflow: Start -> MyNode -> End
//! let app = GraphBuilder::new()
//!     .add_node(NodeKind::Start, MyNode)
//!     .add_node(NodeKind::End, MyNode)
//!     .add_edge(NodeKind::Start, NodeKind::End)
//!     .set_entry(NodeKind::Start)
//!     .compile()
//!     .expect("Failed to compile graph");
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
//! let has_messages: EdgePredicate = Arc::new(|snapshot| {
//!     !snapshot.messages.is_empty()
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
//!     .add_node(NodeKind::Start, MyNode)
//!     .add_node(NodeKind::Other("process".into()), MyNode)
//!     .add_node(NodeKind::Other("skip".into()), MyNode)
//!     .add_node(NodeKind::End, MyNode)
//!     // Add a basic edge from Start to satisfy validation
//!     .add_edge(NodeKind::Start, NodeKind::Other("process".into()))
//!     .add_conditional_edge(
//!         NodeKind::Start,
//!         NodeKind::Other("process".into()), // If predicate returns true
//!         NodeKind::Other("skip".into()),    // If predicate returns false
//!         has_messages,
//!     )
//!     .add_edge(NodeKind::Other("process".into()), NodeKind::End)
//!     .add_edge(NodeKind::Other("skip".into()), NodeKind::End)
//!     .set_entry(NodeKind::Start)
//!     .compile()
//!     .expect("Failed to compile conditional graph");
//! ```

use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::app::*;
use crate::node::*;
use crate::runtimes::RuntimeConfig;
use crate::types::*;

/// Errors that can occur during graph compilation.
///
/// These errors indicate problems with the graph structure or configuration
/// that prevent it from being compiled into an executable [`App`].
#[derive(Debug, thiserror::Error)]
pub enum GraphCompileError {
    /// No entry point was set for the graph.
    ///
    /// Every graph must have exactly one entry point specified via
    /// [`GraphBuilder::set_entry`]. This error occurs when [`GraphBuilder::compile`]
    /// is called without setting an entry point.
    #[error("Graph compilation failed: no entry point specified")]
    MissingEntry,

    /// The specified entry point is not registered as a node.
    ///
    /// This error occurs when the entry point refers to a [`NodeKind`] that
    /// was not added to the graph via [`GraphBuilder::add_node`]. The entry
    /// point must be a valid, registered node.
    #[error("Graph compilation failed: entry point {0:?} is not a registered node")]
    EntryNotRegistered(NodeKind),
}

/// Predicate function for conditional edge routing.
///
/// Takes a [`StateSnapshot`] and returns `true` or `false` to determine
/// which branch of a conditional edge should be taken. Predicates are
/// used with [`GraphBuilder::add_conditional_edge`] to create dynamic
/// routing based on the current state.
///
/// # Examples
///
/// ```
/// use weavegraph::graph::EdgePredicate;
/// use std::sync::Arc;
///
/// // Route based on message count
/// let has_messages: EdgePredicate = Arc::new(|snapshot| {
///     !snapshot.messages.is_empty()
/// });
///
/// // Route based on extra data
/// let has_error_flag: EdgePredicate = Arc::new(|snapshot| {
///     snapshot.extra.get("error").is_some()
/// });
/// ```
pub type EdgePredicate = Arc<dyn Fn(crate::state::StateSnapshot) -> bool + Send + Sync + 'static>;

/// A conditional edge that routes based on a predicate function.
///
/// Conditional edges allow dynamic routing in workflows based on the current
/// state. When the scheduler encounters a conditional edge, it evaluates the
/// predicate function and routes to either the `yes` or `no` target node.
///
/// # Examples
///
/// ```
/// use weavegraph::graph::{ConditionalEdge, EdgePredicate};
/// use weavegraph::types::NodeKind;
/// use std::sync::Arc;
///
/// let predicate: EdgePredicate = Arc::new(|_| true);
/// let edge = ConditionalEdge {
///     from: NodeKind::Start,
///     yes: NodeKind::Other("success".into()),
///     no: NodeKind::Other("failure".into()),
///     predicate,
/// };
/// ```
#[derive(Clone)]
pub struct ConditionalEdge {
    /// The source node for this conditional edge.
    pub from: NodeKind,
    /// The target node when the predicate returns `true`.
    pub yes: NodeKind,
    /// The target node when the predicate returns `false`.
    pub no: NodeKind,
    /// The predicate function that determines routing.
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
/// - At least one node added via [`add_node`](Self::add_node)
/// - An entry point set via [`set_entry`](Self::set_entry)
/// - The entry point must be a registered node
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
///     .add_node(NodeKind::Start, MyNode)
///     .add_node(NodeKind::End, MyNode)
///     .add_edge(NodeKind::Start, NodeKind::End)
///     .set_entry(NodeKind::Start)
///     .compile()
///     .expect("Graph should compile successfully");
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
///     .add_node(NodeKind::Start, MyNode)
///     .add_node(NodeKind::Other("processor_a".into()), MyNode)
///     .add_node(NodeKind::Other("processor_b".into()), MyNode)
///     .add_node(NodeKind::End, MyNode)
///     // Fan-out: Start -> A and Start -> B
///     .add_edge(NodeKind::Start, NodeKind::Other("processor_a".into()))
///     .add_edge(NodeKind::Start, NodeKind::Other("processor_b".into()))
///     // Fan-in: A -> End and B -> End
///     .add_edge(NodeKind::Other("processor_a".into()), NodeKind::End)
///     .add_edge(NodeKind::Other("processor_b".into()), NodeKind::End)
///     .set_entry(NodeKind::Start)
///     .compile()
///     .expect("Fan-out graph should compile successfully");
/// ```
pub struct GraphBuilder {
    /// Registry of all nodes in the graph, keyed by their identifier.
    pub nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    /// Unconditional edges defining static graph topology.
    pub edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    /// Conditional edges for dynamic routing based on state.
    pub conditional_edges: Vec<ConditionalEdge>,
    /// The entry point for graph execution.
    pub entry: Option<NodeKind>,
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
            entry: None,
            runtime_config: RuntimeConfig::default(),
        }
    }

    /// Adds a conditional edge to the graph.
    ///
    /// Conditional edges enable dynamic routing based on the current state.
    /// When execution reaches the `from` node, the `predicate` function is
    /// evaluated with the current [`StateSnapshot`]. If it returns `true`,
    /// execution continues to the `yes` node; if `false`, to the `no` node.
    ///
    /// # Parameters
    ///
    /// - `from`: The source node for the conditional edge
    /// - `yes`: Target node when predicate returns `true`
    /// - `no`: Target node when predicate returns `false`
    /// - `predicate`: Function that determines routing based on state
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
    ///     snapshot.messages.len() > 5
    /// });
    ///
    /// let builder = GraphBuilder::new()
    ///     .add_node(NodeKind::Start, MyNode)
    ///     .add_node(NodeKind::Other("many_messages".into()), MyNode)
    ///     .add_node(NodeKind::Other("few_messages".into()), MyNode)
    ///     .add_conditional_edge(
    ///         NodeKind::Start,
    ///         NodeKind::Other("many_messages".into()),
    ///         NodeKind::Other("few_messages".into()),
    ///         predicate,
    ///     );
    /// ```
    #[must_use]
    pub fn add_conditional_edge(
        mut self,
        from: NodeKind,
        yes: NodeKind,
        no: NodeKind,
        predicate: EdgePredicate,
    ) -> Self {
        self.conditional_edges.push(ConditionalEdge {
            from,
            yes,
            no,
            predicate,
        });
        self
    }

    /// Adds a node to the graph.
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
    ///     .add_node(NodeKind::Start, ProcessorNode { name: "start".into() })
    ///     .add_node(NodeKind::Other("custom".into()), ProcessorNode { name: "custom".into() });
    /// ```
    #[must_use]
    pub fn add_node(mut self, id: NodeKind, node: impl Node + 'static) -> Self {
        self.nodes.insert(id, Arc::new(node));
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
    ///     .add_node(NodeKind::Start, MyNode)
    ///     .add_node(NodeKind::End, MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::End); // Linear workflow
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
    ///     .add_node(NodeKind::Start, MyNode)
    ///     .add_node(NodeKind::Other("worker_a".into()), MyNode)
    ///     .add_node(NodeKind::Other("worker_b".into()), MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::Other("worker_a".into()))
    ///     .add_edge(NodeKind::Start, NodeKind::Other("worker_b".into())); // Fan-out
    /// ```
    #[must_use]
    pub fn add_edge(mut self, from: NodeKind, to: NodeKind) -> Self {
        self.edges.entry(from).or_default().push(to);
        self
    }

    /// Sets the entry point for graph execution.
    ///
    /// The entry point determines where workflow execution begins. It must
    /// be a [`NodeKind`] that has been registered via [`add_node`](Self::add_node).
    /// Every graph must have exactly one entry point.
    ///
    /// # Parameters
    ///
    /// - `entry`: The [`NodeKind`] identifier for the starting node
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
    ///     .add_node(NodeKind::Start, MyNode)
    ///     .add_node(NodeKind::End, MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::End)
    ///     .set_entry(NodeKind::Start); // Start execution from the Start node
    /// ```
    #[must_use]
    pub fn set_entry(mut self, entry: NodeKind) -> Self {
        self.entry = Some(entry);
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
    /// - Ensures an entry point has been set
    /// - Verifies the entry point is a registered node
    /// - Future: cycle detection, reachability analysis
    ///
    /// # Returns
    ///
    /// - `Ok(App)`: Successfully compiled application ready for execution
    /// - `Err(GraphCompileError)`: Validation failed, see error for details
    ///
    /// # Errors
    ///
    /// - [`GraphCompileError::MissingEntry`]: No entry point was set
    /// - [`GraphCompileError::EntryNotRegistered`]: Entry point is not a registered node
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
    ///     .add_node(NodeKind::Start, MyNode)
    ///     .add_node(NodeKind::End, MyNode)
    ///     .add_edge(NodeKind::Start, NodeKind::End)
    ///     .set_entry(NodeKind::Start)
    ///     .compile()?;
    ///
    /// // App is ready for execution
    /// # Ok::<(), weavegraph::graph::GraphCompileError>(())
    /// ```
    pub fn compile(self) -> Result<App, GraphCompileError> {
        let entry = self.entry.as_ref().ok_or(GraphCompileError::MissingEntry)?;
        if !self.edges.contains_key(entry) {
            return Err(GraphCompileError::EntryNotRegistered(entry.clone()));
        }
        Ok(App::from_parts(
            self.nodes,
            self.edges,
            self.conditional_edges,
            self.runtime_config,
        ))
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
            Ok(crate::node::NodePartial {
                messages: Some(vec![Message::assistant("NodeA executed")]),
                ..Default::default()
            })
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
            Ok(crate::node::NodePartial {
                messages: Some(vec![Message::assistant("NodeB executed")]),
                ..Default::default()
            })
        }
    }

    #[test]
    /// Tests adding conditional edges to a graph builder.
    ///
    /// Verifies that conditional edges are properly stored and that predicates
    /// can be evaluated correctly. This test uses a simple always-true predicate
    /// and validates the edge structure.
    fn test_add_conditional_edge() {
        use crate::state::StateSnapshot;
        let always_true: super::EdgePredicate = std::sync::Arc::new(|_s: StateSnapshot| true);
        let gb = super::GraphBuilder::new()
            .add_node(super::NodeKind::Start, NodeA)
            .add_node(super::NodeKind::Other("Y".into()), NodeA)
            .add_node(super::NodeKind::Other("N".into()), NodeA)
            .add_conditional_edge(
                super::NodeKind::Start,
                super::NodeKind::Other("Y".into()),
                super::NodeKind::Other("N".into()),
                always_true.clone(),
            );
        assert_eq!(gb.conditional_edges.len(), 1);
        let ce = &gb.conditional_edges[0];
        assert_eq!(ce.from, super::NodeKind::Start);
        assert_eq!(ce.yes, super::NodeKind::Other("Y".into()));
        assert_eq!(ce.no, super::NodeKind::Other("N".into()));
        // Predicate should return true
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: crate::utils::collections::new_extra_map(),
            extra_version: 1,
            errors: vec![],
            errors_version: 1,
        };
        assert!((ce.predicate)(snap));
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
        assert!(gb.entry.is_none());
    }

    #[test]
    /// Checks that nodes can be added to the GraphBuilder and are stored correctly.
    ///
    /// Validates that the builder properly stores node implementations and that
    /// they can be retrieved by their NodeKind identifiers.
    fn test_add_node() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB);
        assert_eq!(gb.nodes.len(), 2);
        assert!(gb.nodes.contains_key(&NodeKind::Start));
        assert!(gb.nodes.contains_key(&NodeKind::End));
    }

    #[test]
    /// Ensures edges can be added between nodes and are tracked properly in the builder.
    ///
    /// Tests that edges are stored in the correct adjacency list structure and that
    /// multiple edges from the same source node are properly accumulated.
    fn test_add_edge() {
        let gb = GraphBuilder::new()
            .add_edge(NodeKind::Start, NodeKind::End)
            .add_edge(NodeKind::Start, NodeKind::Other("C".to_string()));
        assert_eq!(gb.edges.len(), 1);
        let edges = gb.edges.get(&NodeKind::Start).unwrap();
        assert_eq!(edges.len(), 2);
        assert!(edges.contains(&NodeKind::End));
        assert!(edges.contains(&NodeKind::Other("C".to_string())));
    }

    #[test]
    /// Validates that compiling a GraphBuilder produces an App with correct structure.
    ///
    /// Tests the compilation process for a valid graph configuration and verifies
    /// that the resulting App contains the expected nodes and edges.
    fn test_compile() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB)
            .add_edge(NodeKind::Start, NodeKind::End)
            .set_entry(NodeKind::Start);
        let app = gb.compile().unwrap();
        assert_eq!(app.nodes().len(), 2);
        assert!(app.nodes().contains_key(&NodeKind::Start));
        assert!(app.nodes().contains_key(&NodeKind::End));
        assert_eq!(app.edges().len(), 1);
        assert!(app
            .edges()
            .get(&NodeKind::Start)
            .unwrap()
            .contains(&NodeKind::End));
    }

    #[test]
    /// Compiling without setting entry should return MissingEntry error.
    ///
    /// Validates that the builder properly detects when no entry point has been
    /// configured and returns the appropriate error.
    fn test_compile_missing_entry() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB)
            .add_edge(NodeKind::Start, NodeKind::End);
        let result = gb.compile();
        match result {
            Err(GraphCompileError::MissingEntry) => (),
            _ => panic!("Expected MissingEntry error"),
        }
    }

    #[test]
    /// Compiling with entry set to a node that is not registered should return EntryNotRegistered error.
    ///
    /// Tests the validation that ensures the entry point refers to an actual node
    /// that has been added to the graph.
    fn test_compile_entry_not_registered() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB)
            .add_edge(NodeKind::Start, NodeKind::End)
            .set_entry(NodeKind::Other("NotRegistered".to_string()));
        let result = gb.compile();
        assert!(matches!(
            result,
            Err(GraphCompileError::EntryNotRegistered(NodeKind::Other(ref s))) if s == "NotRegistered"
        ));
    }

    #[test]
    /// Tests equality and inequality for NodeKind::Other variant with different string values.
    ///
    /// Validates that NodeKind comparison works correctly for custom node types.
    fn test_nodekind_other_variant() {
        let k1 = NodeKind::Other("foo".to_string());
        let k2 = NodeKind::Other("foo".to_string());
        let k3 = NodeKind::Other("bar".to_string());
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
        let final_builder = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB)
            .add_edge(NodeKind::Start, NodeKind::End)
            .set_entry(NodeKind::Start);

        // Should be able to compile successfully
        assert!(final_builder.compile().is_ok());
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
            .add_node(NodeKind::Start, NodeA)
            .add_edge(NodeKind::Start, NodeKind::End)
            .set_entry(NodeKind::Start)
            .with_runtime_config(config);

        // Should compile successfully with custom runtime config
        assert!(builder.compile().is_ok());
    }
}
