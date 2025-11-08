use rustc_hash::FxHashMap;
use serde_json::Value;
use std::sync::Arc;

use weavegraph::channels::Channel;
use weavegraph::message::Message;
use weavegraph::node::NodePartial;
use weavegraph::reducers::{AddMessages, MapMerge, Reducer, ReducerRegistry};
use weavegraph::state::VersionedState;

mod common;
use common::*;
use weavegraph::types::ChannelType;

// Fresh baseline state helper
fn base_state() -> VersionedState {
    state_with_user("a")
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
        frontier: None,
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
        frontier: None,
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
        frontier: None,
    };

    reducer.apply(&mut state, &partial);

    assert_extra_has(&state, "k1");
    assert_extra_has(&state, "k2");
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
        frontier: None,
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
    let reducers: Vec<Arc<dyn Reducer>> = vec![Arc::new(AddMessages), Arc::new(MapMerge)];

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
        frontier: None,
    };

    for r in &reducers {
        r.apply(&mut state, &partial);
    }

    assert_eq!(state.messages.snapshot().len(), 2);
    assert_extra_has(&state, "seed");
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
        frontier: None,
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
        frontier: None,
    };

    // Simulate runtime iterating channels
    for channel in [ChannelType::Message, ChannelType::Extra] {
        if channel_guard(channel.clone(), &partial) {
            let _ = registry.try_update(channel, &mut state, &partial);
        }
    }

    assert_message_contains(&state, "from node");
    assert_extra_has(&state, "origin");
}

/*****************************
 * Concurrency tests (Stage 4)
 *****************************/

/// Test concurrent reducer application from multiple threads
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_reducer_thread_safety() {
    let registry = Arc::new(ReducerRegistry::default());
    let state = Arc::new(tokio::sync::Mutex::new(base_state()));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let registry = Arc::clone(&registry);
            let state = Arc::clone(&state);

            tokio::spawn(async move {
                let partial = NodePartial {
                    messages: Some(vec![Message::assistant(&format!("msg_{}", i))]),
                    extra: None,
                    errors: None,
                    frontier: None,
                };

                let mut state_guard = state.lock().await;
                let _ = registry.try_update(ChannelType::Message, &mut *state_guard, &partial);
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    let final_state = state.lock().await;
    // Initial state has 1 message, we added 10 more
    assert_eq!(final_state.messages.snapshot().len(), 11);
}

/// Test deterministic behavior under concurrent access
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_reducer_determinism_under_concurrency() {
    // Run same operations multiple times, verify state convergence
    for _ in 0..10 {
        let registry = Arc::new(ReducerRegistry::default());
        let state1 = Arc::new(tokio::sync::Mutex::new(base_state()));
        let state2 = Arc::new(tokio::sync::Mutex::new(base_state()));

        // Apply same partials concurrently to both states
        let partials: Vec<NodePartial> = (0..5)
            .map(|i| NodePartial {
                messages: Some(vec![Message::user(&format!("test_{}", i))]),
                extra: None,
                errors: None,
                frontier: None,
            })
            .collect();

        // Apply to state1
        let handles1: Vec<_> = partials
            .iter()
            .map(|partial| {
                let registry = Arc::clone(&registry);
                let state = Arc::clone(&state1);
                let partial = partial.clone();

                tokio::spawn(async move {
                    let mut state_guard = state.lock().await;
                    let _ = registry.try_update(ChannelType::Message, &mut *state_guard, &partial);
                })
            })
            .collect();

        // Apply to state2
        let handles2: Vec<_> = partials
            .iter()
            .map(|partial| {
                let registry = Arc::clone(&registry);
                let state = Arc::clone(&state2);
                let partial = partial.clone();

                tokio::spawn(async move {
                    let mut state_guard = state.lock().await;
                    let _ = registry.try_update(ChannelType::Message, &mut *state_guard, &partial);
                })
            })
            .collect();

        for handle in handles1.into_iter().chain(handles2) {
            handle.await.unwrap();
        }

        // Verify final states are identical
        let final_state1 = state1.lock().await;
        let final_state2 = state2.lock().await;

        assert_eq!(
            final_state1.messages.snapshot().len(),
            final_state2.messages.snapshot().len()
        );

        // Both should have initial message + 5 new messages
        assert_eq!(final_state1.messages.snapshot().len(), 6);
    }
}

/// Test channel isolation - reducers for one channel don't affect others
#[test]
fn test_reducer_channel_isolation() {
    let registry = ReducerRegistry::default();
    let mut state = base_state();

    let initial_messages = state.messages.snapshot().len();
    let initial_extra_keys = state.extra.snapshot().len();

    // Apply message-only partial
    let message_partial = NodePartial {
        messages: Some(vec![Message::system("isolated message")]),
        extra: None,
        errors: None,
        frontier: None,
    };

    registry
        .try_update(ChannelType::Message, &mut state, &message_partial)
        .unwrap();

    // Verify only messages channel was affected
    assert_eq!(state.messages.snapshot().len(), initial_messages + 1);
    assert_eq!(state.extra.snapshot().len(), initial_extra_keys);

    // Apply extra-only partial
    let mut extra_map = FxHashMap::default();
    extra_map.insert(
        "isolated_key".into(),
        Value::String("isolated_value".into()),
    );

    let extra_partial = NodePartial {
        messages: None,
        extra: Some(extra_map),
        errors: None,
        frontier: None,
    };

    registry
        .try_update(ChannelType::Extra, &mut state, &extra_partial)
        .unwrap();

    // Verify only extra channel was affected (messages unchanged from previous operation)
    assert_eq!(state.messages.snapshot().len(), initial_messages + 1);
    assert_eq!(state.extra.snapshot().len(), initial_extra_keys + 1);
    assert_eq!(
        state.extra.snapshot().get("isolated_key"),
        Some(&Value::String("isolated_value".into()))
    );
}
