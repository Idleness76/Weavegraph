use weavegraph::app::App;
use weavegraph::channels::Channel;
use weavegraph::state::VersionedState;
use weavegraph::types::NodeKind;

#[allow(dead_code)]
pub fn assert_edge(app: &App, from: NodeKind, to: NodeKind) {
    let edges = app.edges();
    let outs = edges.get(&from).expect("source node has edges");
    assert!(outs.contains(&to), "expected edge {from:?} -> {to:?}");
}

#[allow(dead_code)]
pub fn assert_message_contains(state: &VersionedState, needle: &str) {
    let msgs = state.messages.snapshot();
    let found = msgs.iter().any(|m| m.content.contains(needle));
    assert!(
        found,
        "expected at least one message containing '{needle}', got: {:?}",
        msgs
    );
}

#[allow(dead_code)]
pub fn assert_extra_has(state: &VersionedState, key: &str) {
    let extra = state.extra.snapshot();
    assert!(
        extra.contains_key(key),
        "expected extra to have key '{key}', got keys: {:?}",
        extra.keys().collect::<Vec<_>>()
    );
}
