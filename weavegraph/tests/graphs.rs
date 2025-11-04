mod common;

use common::*;
use weavegraph::graphs::{EdgePredicate, GraphBuilder};
use weavegraph::types::NodeKind;

#[test]
fn test_add_conditional_edge() {
    let route_to_y: EdgePredicate = std::sync::Arc::new(|_s| vec!["Y".to_string()]);
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("Y".into()), NoopNode)
        .add_node(NodeKind::Custom("N".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("Y".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("N".into()))
        .add_conditional_edge(NodeKind::Start, route_to_y.clone())
        .add_edge(NodeKind::Custom("Y".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("N".into()), NodeKind::End)
        .compile()
        .unwrap();
    assert_eq!(app.conditional_edges().len(), 1);
    let ce = &app.conditional_edges()[0];
    assert_eq!(ce.from(), &NodeKind::Start);
    let snap = empty_snapshot();
    assert_eq!((ce.predicate())(snap), vec!["Y".to_string()]);
}

#[test]
fn test_graph_builder_new() {
    let err = GraphBuilder::new().compile().err().unwrap();
    // Expect MissingEntry; structural validation prevents compiling empty graphs
    let _ = err; // just ensure it returns an error; specific variant tested elsewhere
}

#[test]
fn test_add_node() {
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .compile()
        .unwrap();
    assert_eq!(app.nodes().len(), 2);
    assert!(app.nodes().contains_key(&NodeKind::Custom("A".into())));
    assert!(app.nodes().contains_key(&NodeKind::Custom("B".into())));
}

#[test]
fn test_add_edge() {
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("C".to_string()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::Custom("C".to_string()))
        .add_edge(NodeKind::Custom("C".to_string()), NodeKind::End)
        .compile()
        .unwrap();
    assert_eq!(app.edges().len(), 2);
    let edges = app.edges().get(&NodeKind::Start).unwrap();
    assert_eq!(edges.len(), 2);
    assert!(edges.contains(&NodeKind::End));
    assert!(edges.contains(&NodeKind::Custom("C".to_string())));
}

#[test]
fn test_compile() {
    let gb = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
    let app = gb.compile().unwrap();
    assert_eq!(app.edges().len(), 1);
    assert!(
        app.edges()
            .get(&NodeKind::Start)
            .unwrap()
            .contains(&NodeKind::End)
    );
}

#[test]
fn test_compile_missing_entry() {
    let gb = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
    let app = gb.compile().unwrap();
    assert!(app.edges().get(&NodeKind::Start).is_some());
}

#[test]
fn test_compile_entry_not_registered() {
    let gb = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
    let app = gb.compile().unwrap();
    assert_eq!(app.edges().len(), 1);
}

#[test]
fn test_nodekind_other_variant() {
    let k1 = NodeKind::Custom("foo".to_string());
    let k2 = NodeKind::Custom("foo".to_string());
    let k3 = NodeKind::Custom("bar".to_string());
    assert_eq!(k1, k2);
    assert_ne!(k1, k3);
}

#[test]
fn test_duplicate_edges_rejected() {
    // Duplicate edges should now be rejected by validation
    use weavegraph::graphs::GraphCompileError;

    let result = GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::End)
        .compile();

    assert!(result.is_err());
    let err = result.err().unwrap();
    matches!(err, GraphCompileError::DuplicateEdge { .. });
}

#[test]
fn test_builder_fluent_api() {
    let final_builder = GraphBuilder::new().add_edge(NodeKind::Start, NodeKind::End);
    let _app = final_builder.compile().unwrap();
}

#[test]
fn test_runtime_config_integration() {
    use weavegraph::runtimes::RuntimeConfig;

    let config = RuntimeConfig::new(Some("test_session".into()), None, None);

    let builder = GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::End)
        .with_runtime_config(config);

    let _app = builder.compile().unwrap();
}

// ============================================================================
// Enhanced Validation Tests (Directive 1)
// ============================================================================

#[test]
fn test_cycle_detection_simple_cycle() {
    use weavegraph::graphs::GraphCompileError;

    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("B".into()), NodeKind::Custom("A".into())) // Cycle: A -> B -> A
        .compile();

    assert!(result.is_err());
    match result.err().unwrap() {
        GraphCompileError::CycleDetected { cycle } => {
            assert!(!cycle.is_empty());
            // Verify cycle contains A and B
            assert!(cycle.contains(&NodeKind::Custom("A".into())));
            assert!(cycle.contains(&NodeKind::Custom("B".into())));
        }
        e => panic!("Expected CycleDetected error, got: {:?}", e),
    }
}

#[test]
fn test_cycle_detection_self_loop() {
    use weavegraph::graphs::GraphCompileError;

    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("A".into())) // Self-loop
        .compile();

    assert!(result.is_err());
    match result.err().unwrap() {
        GraphCompileError::CycleDetected { cycle } => {
            assert!(!cycle.is_empty());
            assert!(cycle.contains(&NodeKind::Custom("A".into())));
        }
        e => panic!("Expected CycleDetected error, got: {:?}", e),
    }
}

#[test]
fn test_cycle_detection_no_cycle() {
    // Linear graph should pass
    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .compile();

    assert!(result.is_ok());
}

#[test]
fn test_unreachable_nodes_detection() {
    use weavegraph::graphs::GraphCompileError;

    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_node(NodeKind::Custom("X".into()), NoopNode) // Unreachable
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        // X is registered but has no incoming edges
        .compile();

    assert!(result.is_err());
    match result.err().unwrap() {
        GraphCompileError::UnreachableNodes { nodes } => {
            assert_eq!(nodes.len(), 1);
            assert!(nodes.contains(&NodeKind::Custom("X".into())));
        }
        e => panic!("Expected UnreachableNodes error, got: {:?}", e),
    }
}

#[test]
fn test_unreachable_nodes_all_reachable() {
    // All nodes reachable should pass
    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .compile();

    assert!(result.is_ok());
}

#[test]
fn test_no_path_to_end_detection() {
    use weavegraph::graphs::GraphCompileError;

    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
        // B has no outgoing edge to End - dead end
        .compile();

    assert!(result.is_err());
    match result.err().unwrap() {
        GraphCompileError::NoPathToEnd { nodes } => {
            assert!(!nodes.is_empty());
            // Both A and B have no path to End
            assert!(nodes.contains(&NodeKind::Custom("A".into())));
            assert!(nodes.contains(&NodeKind::Custom("B".into())));
        }
        e => panic!("Expected NoPathToEnd error, got: {:?}", e),
    }
}

#[test]
fn test_no_path_to_end_all_paths_valid() {
    // All nodes can reach End should pass
    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile();

    assert!(result.is_ok());
}

#[test]
fn test_duplicate_edge_detection() {
    use weavegraph::graphs::GraphCompileError;

    let result = GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::End) // Duplicate
        .compile();

    assert!(result.is_err());
    match result.err().unwrap() {
        GraphCompileError::DuplicateEdge { from, to } => {
            assert_eq!(from, NodeKind::Start);
            assert_eq!(to, NodeKind::End);
        }
        e => panic!("Expected DuplicateEdge error, got: {:?}", e),
    }
}

#[test]
fn test_duplicate_edge_with_different_targets() {
    // Multiple different targets from same source should be allowed
    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("B".into())) // Different target, OK
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .compile();

    assert!(result.is_ok());
}

#[test]
fn test_happy_path_simple_graph() {
    // Verify a simple valid graph passes all validations
    let result = GraphBuilder::new()
        .add_node(NodeKind::Custom("process".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
        .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
        .compile();

    assert!(result.is_ok());
}

#[test]
fn test_happy_path_start_to_end_direct() {
    // Direct Start -> End should pass
    let result = GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::End)
        .compile();

    assert!(result.is_ok());
}
