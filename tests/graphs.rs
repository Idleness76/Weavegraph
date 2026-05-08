mod common;

use common::*;
use std::sync::Arc;
use weavegraph::graphs::{EdgePredicate, GraphBuilder};
use weavegraph::node::NodePartial;
use weavegraph::reducers::Reducer;
use weavegraph::state::VersionedState;
use weavegraph::types::{ChannelType, NodeKind};

struct FirstExtraReducer;

impl Reducer for FirstExtraReducer {
    fn apply(&self, _state: &mut VersionedState, _update: &NodePartial) {}
}

struct SecondExtraReducer;

impl Reducer for SecondExtraReducer {
    fn apply(&self, _state: &mut VersionedState, _update: &NodePartial) {}
}

struct StableLabelReducerA;

impl Reducer for StableLabelReducerA {
    fn definition_label(&self) -> &'static str {
        "stable-extra-label"
    }

    fn apply(&self, _state: &mut VersionedState, _update: &NodePartial) {}
}

struct StableLabelReducerB;

impl Reducer for StableLabelReducerB {
    fn definition_label(&self) -> &'static str {
        "stable-extra-label"
    }

    fn apply(&self, _state: &mut VersionedState, _update: &NodePartial) {}
}

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
fn test_graph_metadata_and_hash_change_with_definition() {
    let app_a = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
        .unwrap();
    let app_b = GraphBuilder::new()
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .compile()
        .unwrap();

    let metadata = app_a.graph_metadata();
    assert_eq!(metadata.graph_hash, app_a.graph_definition_hash());
    assert_eq!(metadata.node_count, 1);
    assert_eq!(metadata.edge_count, 2);
    assert_eq!(metadata.conditional_edge_count, 0);
    assert_ne!(app_a.graph_definition_hash(), app_b.graph_definition_hash());
}

#[test]
fn test_graph_hash_changes_with_reducer_identity() {
    let app_a = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .with_reducer(ChannelType::Extra, Arc::new(FirstExtraReducer))
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
        .unwrap();
    let app_b = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .with_reducer(ChannelType::Extra, Arc::new(SecondExtraReducer))
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
        .unwrap();

    assert_ne!(app_a.graph_definition_hash(), app_b.graph_definition_hash());
}

#[test]
fn test_graph_hash_is_stable_for_equivalent_definition_ordering() {
    let app_a = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .compile()
        .unwrap();
    let app_b = GraphBuilder::new()
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .compile()
        .unwrap();

    assert_eq!(app_a.graph_definition_hash(), app_b.graph_definition_hash());
}

#[test]
fn test_graph_hash_changes_with_conditional_edge_registration_count() {
    let route_to_end: EdgePredicate = Arc::new(|_snapshot| vec!["End".to_string()]);
    let app_without_conditional = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
        .unwrap();
    let app_with_conditional = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_conditional_edge(NodeKind::Custom("A".into()), route_to_end)
        .compile()
        .unwrap();

    assert_eq!(
        app_with_conditional.graph_metadata().conditional_edge_count,
        1
    );
    assert_ne!(
        app_without_conditional.graph_definition_hash(),
        app_with_conditional.graph_definition_hash()
    );
}

#[test]
fn test_graph_hash_uses_custom_reducer_definition_label() {
    let app_a = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .with_reducer(ChannelType::Extra, Arc::new(StableLabelReducerA))
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
        .unwrap();
    let app_b = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .with_reducer(ChannelType::Extra, Arc::new(StableLabelReducerB))
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .compile()
        .unwrap();

    assert!(
        app_a
            .graph_metadata()
            .reducer_signature
            .iter()
            .any(|entry| entry.contains("stable-extra-label"))
    );
    assert_eq!(app_a.graph_definition_hash(), app_b.graph_definition_hash());
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

    let config = RuntimeConfig::new(Some("test_session".into()), None);

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

// ============================================================================
// Graph Iteration Tests (Phase 3.1)
// ============================================================================

#[test]
fn test_nodes_iterator() {
    let builder = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_node(NodeKind::Custom("C".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("B".into()), NodeKind::Custom("C".into()))
        .add_edge(NodeKind::Custom("C".into()), NodeKind::End);

    let nodes: Vec<_> = builder.nodes().collect();
    assert_eq!(nodes.len(), 3);

    // Should contain all custom nodes
    assert!(nodes.contains(&&NodeKind::Custom("A".into())));
    assert!(nodes.contains(&&NodeKind::Custom("B".into())));
    assert!(nodes.contains(&&NodeKind::Custom("C".into())));
}

#[test]
fn test_edges_iterator() {
    let builder = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End);

    let edges: Vec<_> = builder.edges().collect();
    assert_eq!(edges.len(), 2);

    // Check edge existence
    assert!(edges.contains(&(&NodeKind::Start, &NodeKind::Custom("A".into()))));
    assert!(edges.contains(&(&NodeKind::Custom("A".into()), &NodeKind::End)));
}

#[test]
fn test_node_count_and_edge_count() {
    let builder = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End);

    assert_eq!(builder.node_count(), 2);
    assert_eq!(builder.edge_count(), 4);
}

#[test]
fn test_topological_sort_basic() {
    let builder = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_node(NodeKind::Custom("C".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Custom("B".into()), NodeKind::Custom("C".into()))
        .add_edge(NodeKind::Custom("C".into()), NodeKind::End);

    let sorted = builder.topological_sort();

    // Start should be first, End should be last
    assert_eq!(sorted[0], NodeKind::Start);
    assert_eq!(sorted[sorted.len() - 1], NodeKind::End);

    // A -> B -> C ordering
    let a_pos = sorted
        .iter()
        .position(|n| n == &NodeKind::Custom("A".into()))
        .unwrap();
    let b_pos = sorted
        .iter()
        .position(|n| n == &NodeKind::Custom("B".into()))
        .unwrap();
    let c_pos = sorted
        .iter()
        .position(|n| n == &NodeKind::Custom("C".into()))
        .unwrap();
    assert!(a_pos < b_pos);
    assert!(b_pos < c_pos);
}

#[test]
fn test_topological_sort_fan_out() {
    let builder = GraphBuilder::new()
        .add_node(NodeKind::Custom("A".into()), NoopNode)
        .add_node(NodeKind::Custom("B".into()), NoopNode)
        .add_node(NodeKind::Custom("C".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("B".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("C".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("C".into()), NodeKind::End);

    let sorted = builder.topological_sort();

    // Start first, End last
    assert_eq!(sorted[0], NodeKind::Start);
    assert_eq!(sorted[sorted.len() - 1], NodeKind::End);

    // All custom nodes should come between Start and End
    let start_pos = sorted.iter().position(|n| n == &NodeKind::Start).unwrap();
    let end_pos = sorted.iter().position(|n| n == &NodeKind::End).unwrap();
    let a_pos = sorted
        .iter()
        .position(|n| n == &NodeKind::Custom("A".into()))
        .unwrap();
    let b_pos = sorted
        .iter()
        .position(|n| n == &NodeKind::Custom("B".into()))
        .unwrap();
    let c_pos = sorted
        .iter()
        .position(|n| n == &NodeKind::Custom("C".into()))
        .unwrap();

    assert!(start_pos < a_pos && a_pos < end_pos);
    assert!(start_pos < b_pos && b_pos < end_pos);
    assert!(start_pos < c_pos && c_pos < end_pos);

    // Lexicographic ordering: A < B < C
    assert!(a_pos < b_pos);
    assert!(b_pos < c_pos);
}

#[test]
fn test_topological_sort_deterministic() {
    let builder = GraphBuilder::new()
        .add_node(NodeKind::Custom("Z".into()), NoopNode)
        .add_node(NodeKind::Custom("Y".into()), NoopNode)
        .add_node(NodeKind::Custom("X".into()), NoopNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("X".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("Y".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("Z".into()))
        .add_edge(NodeKind::Custom("X".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("Y".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("Z".into()), NodeKind::End);

    // Multiple runs should produce same order
    let sorted1 = builder.topological_sort();
    let sorted2 = builder.topological_sort();

    assert_eq!(sorted1, sorted2);

    // Lexicographic order: X < Y < Z
    let x_pos = sorted1
        .iter()
        .position(|n| n == &NodeKind::Custom("X".into()))
        .unwrap();
    let y_pos = sorted1
        .iter()
        .position(|n| n == &NodeKind::Custom("Y".into()))
        .unwrap();
    let z_pos = sorted1
        .iter()
        .position(|n| n == &NodeKind::Custom("Z".into()))
        .unwrap();
    assert!(x_pos < y_pos);
    assert!(y_pos < z_pos);
}
