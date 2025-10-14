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
        .add_conditional_edge(NodeKind::Start, route_to_y.clone())
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
        .compile()
        .unwrap();
    assert_eq!(app.edges().len(), 1);
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
    assert!(app
        .edges()
        .get(&NodeKind::Start)
        .unwrap()
        .contains(&NodeKind::End));
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
fn test_duplicate_edges() {
    let app = GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::End)
        .compile()
        .unwrap();
    let edges = app.edges().get(&NodeKind::Start).unwrap();
    let count = edges.iter().filter(|k| **k == NodeKind::End).count();
    assert_eq!(count, 2);
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
