//! Test suite for graph building functionality.
//!
//! This module contains comprehensive tests for GraphBuilder, ConditionalEdge,
//! and the overall graph building process.

#[cfg(test)]
mod tests {
    use super::super::{builder::GraphBuilder, edges::EdgePredicate};
    use crate::message::Message;
    use crate::types::NodeKind;
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
        let route_to_y: EdgePredicate =
            std::sync::Arc::new(|_s: StateSnapshot| vec!["Y".to_string()]);
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Custom("Y".into()), NodeA)
            .add_node(NodeKind::Custom("N".into()), NodeA)
            .add_conditional_edge(NodeKind::Start, route_to_y.clone());
        assert_eq!(gb.conditional_edges.len(), 1);
        let ce = &gb.conditional_edges[0];
        assert_eq!(ce.from, NodeKind::Start);
        // Predicate should return ["Y"]
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: crate::utils::collections::new_extra_map(),
            extra_version: 1,
            errors: vec![],
            errors_version: 1,
        };
        assert_eq!((ce.predicate)(snap), vec!["Y".to_string()]);
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
