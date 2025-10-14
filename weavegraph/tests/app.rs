use rustc_hash::FxHashMap;
use serde_json::Value;
use weavegraph::channels::Channel;
use weavegraph::graphs::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::NodePartial;
use weavegraph::types::NodeKind;

mod common;
use common::*;

fn make_app() -> weavegraph::app::App {
    // Minimal app via GraphBuilder; node graph is irrelevant for apply_barrier
    GraphBuilder::new()
        .add_edge(NodeKind::Start, NodeKind::End)
        .compile()
        .unwrap()
}

#[tokio::test]
async fn test_apply_barrier_messages_update() {
    let app = make_app();
    let state = &mut state_with_user("hi");
    let run_ids = vec![NodeKind::Start];
    let partial = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "foo".into(),
        }]),
        extra: None,
        errors: None,
    };
    let updated = app
        .apply_barrier(state, &run_ids, vec![partial])
        .await
        .unwrap();
    assert!(updated.contains(&"messages"));
    assert_eq!(state.messages.snapshot().last().unwrap().content, "foo");
    assert_eq!(state.messages.version(), 2);
    assert_eq!(state.extra.version(), 1);
}

#[tokio::test]
async fn test_apply_barrier_no_update() {
    let app = make_app();
    let state = &mut state_with_user("hi");
    let run_ids = vec![NodeKind::Start];
    let partial = NodePartial {
        messages: None,
        extra: None,
        errors: None,
    };
    let updated = app
        .apply_barrier(state, &run_ids, vec![partial])
        .await
        .unwrap();
    assert!(updated.is_empty());
    assert_eq!(state.messages.version(), 1);
    assert_eq!(state.extra.version(), 1);
}

#[tokio::test]
async fn test_apply_barrier_saturating_version() {
    let app = make_app();
    let state = &mut state_with_user("hi");
    // push messages version to max to verify saturating add behavior
    state.messages.set_version(u32::MAX);
    let partial = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "x".into(),
        }]),
        extra: None,
        errors: None,
    };
    app.apply_barrier(state, &[NodeKind::Start], vec![partial])
        .await
        .unwrap();
    assert_eq!(state.messages.version(), u32::MAX);
}

#[tokio::test]
async fn test_apply_barrier_multiple_updates() {
    let app = make_app();
    let state = &mut state_with_user("hi");
    let partial1 = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "foo".into(),
        }]),
        extra: None,
        errors: None,
    };
    let partial2 = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "bar".into(),
        }]),
        extra: None,
        errors: None,
    };
    let updated = app
        .apply_barrier(
            state,
            &[NodeKind::Start, NodeKind::End],
            vec![partial1, partial2],
        )
        .await
        .unwrap();
    let snap = state.messages.snapshot();
    assert!(updated.contains(&"messages"));
    assert_eq!(snap[snap.len() - 2].content, "foo");
    assert_eq!(snap[snap.len() - 1].content, "bar");
    assert_eq!(state.messages.version(), 2);
}

#[tokio::test]
async fn test_apply_barrier_empty_vectors_and_maps() {
    let app = make_app();
    let state = &mut state_with_user("hi");
    // Empty messages vector -> Some(vec![]) should be treated as no-op by guard
    let empty_msgs = NodePartial {
        messages: Some(vec![]),
        extra: None,
        errors: None,
    };
    // Empty extra map -> Some(empty) should be treated as no-op by guard
    let empty_extra = NodePartial {
        messages: None,
        extra: Some(FxHashMap::default()),
        errors: None,
    };
    let updated = app
        .apply_barrier(
            state,
            &[NodeKind::Start, NodeKind::End],
            vec![empty_msgs, empty_extra],
        )
        .await
        .unwrap();
    assert!(updated.is_empty());
    assert_eq!(state.messages.version(), 1);
    assert_eq!(state.extra.version(), 1);
}

#[tokio::test]
async fn test_apply_barrier_extra_merge_and_version() {
    let app = make_app();
    let state = &mut state_with_user("hi");

    let mut m1 = FxHashMap::default();
    m1.insert("k1".into(), Value::String("v1".into()));
    let mut m2 = FxHashMap::default();
    m2.insert("k2".into(), Value::String("v2".into()));
    // Overwrite k1 in second partial to test key overwrite still counts as change
    m2.insert("k1".into(), Value::String("v3".into()));

    let p1 = NodePartial {
        messages: None,
        extra: Some(m1),
        errors: None,
    };
    let p2 = NodePartial {
        messages: None,
        extra: Some(m2),
        errors: None,
    };

    let updated = app
        .apply_barrier(state, &[NodeKind::Start, NodeKind::End], vec![p1, p2])
        .await
        .unwrap();
    assert!(updated.contains(&"extra"));
    let snap = state.extra.snapshot();
    assert_eq!(snap.get("k1"), Some(&Value::String("v3".into())));
    assert_eq!(snap.get("k2"), Some(&Value::String("v2".into())));
    assert_eq!(state.extra.version(), 2);
}
