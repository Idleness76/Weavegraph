use rustc_hash::FxHashMap;
use serde_json::Value;

use crate::channels::Channel;
use crate::{
    message::Message,
    node::NodePartial,
    reducers::{AddMessages, MapMerge, Reducer, ReducerRegistry, ReducerType},
    state::VersionedState,
    types::ChannelType,
};

// Fresh baseline state helper
fn base_state() -> VersionedState {
    VersionedState::new_with_user_message("a")
}

// Local guard prototype mirroring runtime logic
fn channel_guard(channel: ChannelType, partial: &NodePartial) -> bool {
    match channel {
        ChannelType::Message => partial
            .messages
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        ChannelType::Extra => partial
            .extra
            .as_ref()
            .map(|m| !m.is_empty())
            .unwrap_or(false),
        ChannelType::Error => false,
    }
}

/********************
 * AddMessages tests
 ********************/

#[test]
fn test_add_messages_appends_state() {
    let reducer = AddMessages;
    let mut state = base_state();
    let initial_version = state.messages.version();
    let initial_len = state.messages.snapshot().len();

    let partial = NodePartial {
        messages: Some(vec![Message {
            role: "system".into(),
            content: "b".into(),
        }]),
        extra: None,
        errors: None,
    };

    reducer.apply(&mut state, &partial);

    let snapshot = state.messages.snapshot();
    assert_eq!(snapshot.len(), initial_len + 1);
    assert_eq!(snapshot[0].role, "user");
    assert_eq!(snapshot[1].role, "system");
    // Reducer does not bump version (barrier responsibility)
    assert_eq!(state.messages.version(), initial_version);
}

#[test]
fn test_add_messages_empty_partial_noop() {
    let reducer = AddMessages;
    let mut state = base_state();
    let initial_version = state.messages.version();
    let initial_snapshot = state.messages.snapshot();

    let partial = NodePartial {
        messages: Some(vec![]),
        extra: None,
        errors: None,
    };

    reducer.apply(&mut state, &partial);

    assert_eq!(state.messages.snapshot(), initial_snapshot);
    assert_eq!(state.messages.version(), initial_version);
}

/********************
 * MapMerge (extra) tests
 ********************/

#[test]
fn test_map_merge_merges_and_overwrites_state() {
    let reducer = MapMerge;
    let mut state = base_state();
    // Seed extra
    state
        .extra
        .get_mut()
        .insert("k1".into(), Value::String("v1".into()));
    let initial_version = state.extra.version();

    let mut extra_update = FxHashMap::default();
    extra_update.insert("k2".into(), Value::String("v2".into()));
    extra_update.insert("k1".into(), Value::String("v3".into())); // overwrite existing

    let partial = NodePartial {
        messages: None,
        extra: Some(extra_update),
        errors: None,
    };

    reducer.apply(&mut state, &partial);

    let extra_snapshot = state.extra.snapshot();
    assert_eq!(
        extra_snapshot.get("k1"),
        Some(&Value::String("v3".into())),
        "overwrite should succeed"
    );
    assert_eq!(
        extra_snapshot.get("k2"),
        Some(&Value::String("v2".into())),
        "new key should be inserted"
    );
    // Version unchanged (barrier responsibility)
    assert_eq!(state.extra.version(), initial_version);
}

#[test]
fn test_map_merge_empty_partial_noop() {
    let reducer = MapMerge;
    let mut state = base_state();
    state
        .extra
        .get_mut()
        .insert("seed".into(), Value::String("x".into()));
    let initial_version = state.extra.version();
    let initial_snapshot = state.extra.snapshot();

    let partial = NodePartial {
        messages: None,
        extra: Some(FxHashMap::default()),
        errors: None,
    };

    reducer.apply(&mut state, &partial);

    assert_eq!(state.extra.snapshot(), initial_snapshot);
    assert_eq!(state.extra.version(), initial_version);
}

/********************
 * Enum wrapper / dispatch
 ********************/

#[test]
fn test_enum_wrapper_dispatch() {
    let reducers = vec![
        ReducerType::AddMessages(AddMessages),
        ReducerType::JsonShallowMerge(MapMerge),
    ];

    let mut state = base_state();
    state
        .extra
        .get_mut()
        .insert("seed".into(), Value::String("x".into()));

    let mut extra_update = FxHashMap::default();
    extra_update.insert("seed".into(), Value::String("y".into()));

    let partial = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "hi".into(),
        }]),
        extra: Some(extra_update),
        errors: None,
    };

    for r in &reducers {
        r.apply(&mut state, &partial);
    }

    assert_eq!(state.messages.snapshot().len(), 2);
    assert_eq!(
        state.extra.snapshot().get("seed"),
        Some(&Value::String("y".into()))
    );
}

/********************
 * Guard logic
 ********************/

#[test]
fn test_channel_guard_logic() {
    let empty = NodePartial::default();
    assert!(!channel_guard(ChannelType::Message, &empty));
    assert!(!channel_guard(ChannelType::Extra, &empty));

    let msg_partial = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "m".into(),
        }]),
        ..Default::default()
    };
    assert!(channel_guard(ChannelType::Message, &msg_partial));
    assert!(!channel_guard(ChannelType::Extra, &msg_partial));

    let mut extra_map = FxHashMap::default();
    extra_map.insert("k".into(), Value::String("v".into()));
    let extra_partial = NodePartial {
        messages: None,
        extra: Some(extra_map),
        errors: None,
    };
    assert!(channel_guard(ChannelType::Extra, &extra_partial));
}

/********************
 * Registry integration-like flow
 ********************/

#[test]
fn test_registry_integration_like_flow() {
    let registry = ReducerRegistry::default();
    let mut state = base_state();

    let mut extra_update = FxHashMap::default();
    extra_update.insert("origin".into(), Value::String("node".into()));

    let partial = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "from node".into(),
        }]),
        extra: Some(extra_update),
        errors: None,
    };

    // Simulate runtime iterating channels
    for channel in [ChannelType::Message, ChannelType::Extra] {
        if channel_guard(channel.clone(), &partial) {
            let _ = registry.try_update(channel, &mut state, &partial);
        }
    }

    assert!(state
        .messages
        .snapshot()
        .iter()
        .any(|m| m.content == "from node"));
    assert_eq!(
        state.extra.snapshot().get("origin"),
        Some(&Value::String("node".into()))
    );
}
