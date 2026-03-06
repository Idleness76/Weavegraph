#[macro_use]
extern crate proptest;

use proptest::prelude::{Just, Strategy, any, prop};
use proptest::prop_oneof;
use rustc_hash::FxHashMap;
use serde_json::Value;
use weavegraph::channels::Channel;
use weavegraph::runtimes::checkpointer::Checkpoint;
use weavegraph::runtimes::persistence::*;
use weavegraph::state::VersionedState;
use weavegraph::types::NodeKind;
use weavegraph::utils::json_ext::JsonSerializable;

mod common;
use common::*;

#[test]
fn test_state_round_trip() {
    let mut vs = state_with_user("hello");
    vs.extra
        .get_mut()
        .insert("k1".into(), Value::String("v1".into()));
    vs.extra.get_mut().insert("n".into(), Value::from(123));
    let persisted = PersistedState::from(&vs);
    let json = persisted.to_json_string().unwrap();
    let back = PersistedState::from_json_str(&json).unwrap();
    let vs2 = VersionedState::try_from(back).unwrap();
    assert_eq!(vs.messages.snapshot(), vs2.messages.snapshot());
    assert_eq!(vs.extra.snapshot(), vs2.extra.snapshot());
    assert_eq!(vs.messages.version(), vs2.messages.version());
    assert_eq!(vs.extra.version(), vs2.extra.version());
}

#[test]
fn test_state_deserialize_without_errors_channel() {
    let json = r#"{
        "messages": {"version": 1, "items": []},
        "extra": {"version": 1, "map": {}}
    }"#;
    let persisted: PersistedState = serde_json::from_str(json).unwrap();
    assert_eq!(persisted.errors.version, 1);
    assert!(persisted.errors.items.is_empty());
}

#[test]
fn test_checkpoint_round_trip() {
    // Build synthetic checkpoint
    let vs = state_with_user("seed");
    let cp = Checkpoint {
        session_id: "sess123".into(),
        step: 7,
        state: vs.clone(),
        frontier: vec![
            weavegraph::types::NodeKind::Start,
            weavegraph::types::NodeKind::Custom("X".into()),
            weavegraph::types::NodeKind::End,
        ],
        versions_seen: FxHashMap::from_iter([
            (
                "Start".into(),
                FxHashMap::from_iter([("messages".into(), 1_u64), ("extra".into(), 1_u64)]),
            ),
            (
                "Other(\"X\")".into(),
                FxHashMap::from_iter([("messages".into(), 1_u64)]),
            ),
        ]),
        concurrency_limit: 4,
        created_at: chrono::Utc::now(),
        ran_nodes: vec![
            weavegraph::types::NodeKind::Start,
            weavegraph::types::NodeKind::Custom("X".into()),
        ],
        skipped_nodes: vec![weavegraph::types::NodeKind::End],
        updated_channels: vec!["messages".to_string(), "extra".to_string()],
    };
    let persisted = PersistedCheckpoint::from(&cp);
    let json = persisted.to_json_string().unwrap();
    let back = PersistedCheckpoint::from_json_str(&json).unwrap();
    let cp2 = Checkpoint::try_from(back).unwrap();
    assert_eq!(cp.session_id, cp2.session_id);
    assert_eq!(cp.step, cp2.step);
    assert_eq!(cp.state.messages.snapshot(), cp2.state.messages.snapshot());
    assert_eq!(cp.frontier.len(), cp2.frontier.len());
    assert_eq!(cp.concurrency_limit, cp2.concurrency_limit);
    assert_eq!(cp.versions_seen, cp2.versions_seen);
    assert_eq!(cp.ran_nodes, cp2.ran_nodes);
    assert_eq!(cp.skipped_nodes, cp2.skipped_nodes);
    assert_eq!(cp.updated_channels, cp2.updated_channels);
}

#[test]
fn test_nodekind_encode_decode() {
    use weavegraph::types::NodeKind;

    let kinds = vec![
        NodeKind::Start,
        NodeKind::End,
        NodeKind::Custom("Alpha".into()),
        // Forward-compat style previously used "Other:Nested"; updated to "Custom:Nested"
        NodeKind::Custom("Custom:Nested".into()),
    ];
    for k in kinds {
        let enc = k.encode();
        let dec = NodeKind::decode(&enc);
        match (&k, &dec) {
            (NodeKind::Custom(orig), NodeKind::Custom(back)) => {
                assert_eq!(back, orig);
            }
            _ => assert_eq!(format!("{:?}", k), format!("{:?}", dec)),
        }
    }
}

fn nodekind_strategy() -> impl Strategy<Value = NodeKind> {
    let base = prop::collection::vec(any::<char>(), 0..16)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    prop_oneof![
        Just(NodeKind::Start),
        Just(NodeKind::End),
        base.clone().prop_map(NodeKind::Custom),
        base.prop_map(|s| NodeKind::Custom(format!("Custom:{s}"))),
    ]
}

fn versions_seen_strategy() -> impl Strategy<Value = FxHashMap<String, FxHashMap<String, u64>>> {
    let inner = prop::collection::hash_map(
        prop::string::string_regex("[A-Za-z0-9:_]{0,8}").unwrap(),
        any::<u64>(),
        0..4,
    )
    .prop_map(|hm: std::collections::HashMap<String, u64>| FxHashMap::from_iter(hm));

    prop::collection::hash_map(
        prop::string::string_regex("[A-Za-z0-9:_]{0,8}").unwrap(),
        inner,
        0..4,
    )
    .prop_map(
        |hm: std::collections::HashMap<String, FxHashMap<String, u64>>| FxHashMap::from_iter(hm),
    )
}

proptest! {
    #[test]
    fn prop_nodekind_round_trip(kind in nodekind_strategy()) {
        let encoded = kind.encode();
        let decoded = NodeKind::decode(&encoded);
        prop_assert_eq!(decoded, kind);
    }

    #[test]
    fn prop_versions_seen_round_trip(map in versions_seen_strategy()) {
        let persisted = PersistedVersionsSeen::from(&map);
        let json = serde_json::to_string(&persisted).unwrap();
        let back: PersistedVersionsSeen = serde_json::from_str(&json).unwrap();
        let restored: FxHashMap<String, FxHashMap<String, u64>> = back.into();
        prop_assert_eq!(restored, map);
    }
}
