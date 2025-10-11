use weavegraph::app::App;
use weavegraph::types::NodeKind;

#[allow(dead_code)]
pub fn assert_edge(app: &App, from: NodeKind, to: NodeKind) {
    let edges = app.edges();
    let outs = edges.get(&from).expect("source node has edges");
    assert!(outs.contains(&to), "expected edge {from:?} -> {to:?}");
}
