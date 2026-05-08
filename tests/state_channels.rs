use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weavegraph::channels::Channel;
use weavegraph::message::{Message, Role};
use weavegraph::node::NodePartial;
use weavegraph::state::{StateKey, StateSlotError, VersionedState};

use proptest::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PortfolioSnapshot {
    cash_cents: i64,
    position_count: u32,
}

const PORTFOLIO: StateKey<PortfolioSnapshot> = StateKey::new("wq", "portfolio", 1);
const PORTFOLIO_V2: StateKey<PortfolioSnapshot> = StateKey::new("wq", "portfolio", 2);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PropertyPayload {
    label: String,
    amount: i64,
    flags: Vec<bool>,
}

const PROPERTY_PAYLOAD: StateKey<PropertyPayload> = StateKey::new("prop", "payload", 1);

struct AlwaysFailsSerialize;

impl Serialize for AlwaysFailsSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom(
            "intentional serialization failure",
        ))
    }
}

const FAILING_SLOT: StateKey<AlwaysFailsSerialize> = StateKey::new("wg", "failing", 1);

#[test]
fn test_new_with_user_message_initializes_fields() {
    let s = VersionedState::new_with_user_message("hello");
    let snap = s.snapshot();
    assert_eq!(snap.messages.len(), 1);
    assert_eq!(snap.messages[0].role, Role::User);
    assert_eq!(snap.messages[0].content, "hello");
    assert_eq!(snap.messages_version, 1);
    assert!(snap.extra.is_empty());
    assert_eq!(snap.extra_version, 1);
    assert!(snap.errors.is_empty());
    assert_eq!(snap.errors_version, 1);
}

#[test]
fn test_new_with_messages_initializes_fields() {
    let messages = vec![
        Message::with_role(Role::User, "hello"),
        Message::with_role(Role::Assistant, "hi there"),
    ];
    let state = VersionedState::new_with_messages(messages.clone());
    let snapshot = state.snapshot();

    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0], messages[0]);
    assert_eq!(snapshot.messages[1], messages[1]);
    assert_eq!(snapshot.messages_version, 1);
    assert!(snapshot.extra.is_empty());
    assert_eq!(snapshot.extra_version, 1);
    assert!(snapshot.errors.is_empty());
    assert_eq!(snapshot.errors_version, 1);
}

#[test]
fn test_snapshot_is_deep_copy() {
    let mut s = VersionedState::new_with_user_message("x");
    let snap = s.snapshot();
    s.messages.get_mut()[0].content = "changed".into();
    s.extra
        .get_mut()
        .insert("k".into(), Value::String("v".into()));
    assert_eq!(snap.messages[0].content, "x");
    assert!(!snap.extra.contains_key("k"));
}

#[test]
fn test_new_with_messages_snapshot_is_deep_copy() {
    let mut state = VersionedState::new_with_messages(vec![
        Message::with_role(Role::User, "original"),
        Message::with_role(Role::Assistant, "response"),
    ]);
    let snapshot = state.snapshot();

    state.messages.get_mut()[0].content = "changed".into();
    state
        .extra
        .get_mut()
        .insert("k".into(), Value::String("v".into()));

    assert_eq!(snapshot.messages[0].content, "original");
    assert_eq!(snapshot.messages[1].content, "response");
    assert!(!snapshot.extra.contains_key("k"));
}

#[test]
fn test_extra_flexible_types() {
    let mut s = VersionedState::new_with_user_message("y");
    s.extra.get_mut().insert("number".into(), json!(123));
    s.extra.get_mut().insert("text".into(), json!("abc"));
    s.extra.get_mut().insert("array".into(), json!([1, 2, 3]));
    let snap = s.snapshot();
    assert_eq!(snap.extra["number"], json!(123));
    assert_eq!(snap.extra["text"], json!("abc"));
    assert_eq!(snap.extra["array"], json!([1, 2, 3]));
}

#[test]
fn test_clone_is_deep() {
    let mut s = VersionedState::new_with_user_message("msg");
    s.extra
        .get_mut()
        .insert("k1".into(), Value::String("v1".into()));
    let cloned = s.clone();
    s.messages.get_mut()[0].content = "changed".into();
    s.extra
        .get_mut()
        .insert("k2".into(), Value::String("v2".into()));
    assert_ne!(cloned.messages.snapshot(), s.messages.snapshot());
    assert_ne!(cloned.extra.snapshot(), s.extra.snapshot());
    assert_eq!(cloned.messages.snapshot()[0].content, "msg");
    assert_eq!(
        cloned.extra.snapshot().get("k1"),
        Some(&Value::String("v1".into()))
    );
    assert!(!cloned.extra.snapshot().contains_key("k2"));
}

#[test]
fn test_builder_pattern() {
    let state = VersionedState::builder()
        .with_user_message("Hello")
        .with_assistant_message("Hi there!")
        .with_system_message("System ready")
        .with_extra("session_id", json!("sess_123"))
        .with_extra("priority", json!("high"))
        .build();

    let snapshot = state.snapshot();
    assert_eq!(snapshot.messages.len(), 3);
    assert_eq!(snapshot.messages[0].role, Role::User);
    assert_eq!(snapshot.messages[0].content, "Hello");
    assert_eq!(snapshot.messages[1].role, Role::Assistant);
    assert_eq!(snapshot.messages[1].content, "Hi there!");
    assert_eq!(snapshot.messages[2].role, Role::System);
    assert_eq!(snapshot.messages[2].content, "System ready");

    assert_eq!(snapshot.extra.len(), 2);
    assert_eq!(snapshot.extra.get("session_id"), Some(&json!("sess_123")));
    assert_eq!(snapshot.extra.get("priority"), Some(&json!("high")));
}

#[test]
fn test_convenience_methods() {
    let mut state = VersionedState::new_with_user_message("Initial");
    let _ = state
        .add_message("assistant", "Response")
        .add_extra("key1", json!("value1"))
        .add_extra("key2", json!(42));

    let snapshot = state.snapshot();
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[1].role, Role::Assistant);
    assert_eq!(snapshot.messages[1].content, "Response");

    assert_eq!(snapshot.extra.len(), 2);
    assert_eq!(snapshot.extra.get("key1"), Some(&json!("value1")));
    assert_eq!(snapshot.extra.get("key2"), Some(&json!(42)));
}

#[test]
fn test_typed_state_slots_round_trip() {
    let portfolio = PortfolioSnapshot {
        cash_cents: 12_345,
        position_count: 2,
    };

    let state = VersionedState::builder()
        .with_user_message("portfolio")
        .with_typed_extra(PORTFOLIO, portfolio.clone())
        .unwrap()
        .build();

    let snapshot = state.snapshot();
    assert_eq!(PORTFOLIO.storage_key(), "wq:portfolio:v1");
    assert_eq!(
        snapshot.get_typed(PORTFOLIO).unwrap(),
        Some(portfolio.clone())
    );
    assert_eq!(snapshot.require_typed(PORTFOLIO).unwrap(), portfolio);
}

#[test]
fn test_state_key_accessors_and_schema_versions() {
    assert_eq!(PORTFOLIO.namespace(), "wq");
    assert_eq!(PORTFOLIO.name(), "portfolio");
    assert_eq!(PORTFOLIO.schema_version(), 1);
    assert_eq!(PORTFOLIO.storage_key(), "wq:portfolio:v1");
    assert_eq!(PORTFOLIO_V2.storage_key(), "wq:portfolio:v2");
    assert_ne!(PORTFOLIO.storage_key(), PORTFOLIO_V2.storage_key());
}

#[test]
fn test_typed_state_slots_missing_and_optional_reads() {
    let snapshot = VersionedState::builder().build().snapshot();

    assert_eq!(snapshot.get_typed(PORTFOLIO).unwrap(), None);
    match snapshot.require_typed::<PortfolioSnapshot>(PORTFOLIO) {
        Err(StateSlotError::Missing { key }) => assert_eq!(key, "wq:portfolio:v1"),
        other => panic!("expected missing slot error, got {other:?}"),
    }
}

#[test]
fn test_typed_state_slots_report_deserialization_errors_with_key() {
    let state = VersionedState::builder()
        .with_extra(
            &PORTFOLIO.storage_key(),
            json!({ "cash_cents": "not-an-integer", "position_count": 1 }),
        )
        .build();

    match state
        .snapshot()
        .require_typed::<PortfolioSnapshot>(PORTFOLIO)
    {
        Err(StateSlotError::Deserialize { key, source }) => {
            assert_eq!(key, "wq:portfolio:v1");
            assert!(source.to_string().contains("invalid type"));
        }
        other => panic!("expected deserialize slot error, got {other:?}"),
    }
}

#[test]
fn test_typed_state_slots_report_serialization_errors_with_key() {
    let builder_error = VersionedState::builder()
        .with_typed_extra(FAILING_SLOT, AlwaysFailsSerialize)
        .unwrap_err();
    match builder_error {
        StateSlotError::Serialize { key, source } => {
            assert_eq!(key, "wg:failing:v1");
            assert!(
                source
                    .to_string()
                    .contains("intentional serialization failure")
            );
        }
        other => panic!("expected serialize slot error, got {other:?}"),
    }

    let partial_error = NodePartial::new()
        .with_typed_extra(FAILING_SLOT, AlwaysFailsSerialize)
        .unwrap_err();
    assert!(matches!(
        partial_error,
        StateSlotError::Serialize { key, .. } if key == "wg:failing:v1"
    ));
}

#[test]
fn test_typed_state_slots_schema_versions_can_coexist() {
    let v1 = PortfolioSnapshot {
        cash_cents: 100,
        position_count: 1,
    };
    let v2 = PortfolioSnapshot {
        cash_cents: 200,
        position_count: 2,
    };

    let state = VersionedState::builder()
        .with_typed_extra(PORTFOLIO, v1.clone())
        .unwrap()
        .with_typed_extra(PORTFOLIO_V2, v2.clone())
        .unwrap()
        .build();
    let snapshot = state.snapshot();

    assert_eq!(snapshot.require_typed(PORTFOLIO).unwrap(), v1);
    assert_eq!(snapshot.require_typed(PORTFOLIO_V2).unwrap(), v2);
}

#[test]
fn test_versioned_state_add_typed_extra_chains_and_overwrites_slot() {
    let first = PortfolioSnapshot {
        cash_cents: 10,
        position_count: 1,
    };
    let second = PortfolioSnapshot {
        cash_cents: 20,
        position_count: 2,
    };
    let mut state = VersionedState::new_with_user_message("typed");

    state
        .add_typed_extra(PORTFOLIO, first)
        .unwrap()
        .add_typed_extra(PORTFOLIO, second.clone())
        .unwrap();

    assert_eq!(state.snapshot().require_typed(PORTFOLIO).unwrap(), second);
}

#[test]
fn test_node_partial_with_typed_extra() {
    let portfolio = PortfolioSnapshot {
        cash_cents: 500,
        position_count: 1,
    };

    let partial = NodePartial::new()
        .with_typed_extra(PORTFOLIO, portfolio.clone())
        .unwrap();
    let extra = partial.extra.expect("typed extra should be inserted");
    let stored = extra
        .get(&PORTFOLIO.storage_key())
        .expect("typed storage key should exist");

    assert_eq!(
        serde_json::from_value::<PortfolioSnapshot>(stored.clone()).unwrap(),
        portfolio
    );
}

#[test]
fn test_node_partial_with_typed_extra_merges_with_existing_extra_and_overwrites_same_slot() {
    let old = PortfolioSnapshot {
        cash_cents: 1,
        position_count: 1,
    };
    let new = PortfolioSnapshot {
        cash_cents: 999,
        position_count: 3,
    };
    let mut extra = weavegraph::utils::collections::new_extra_map();
    extra.insert("untouched".to_string(), json!(true));
    extra.insert(PORTFOLIO.storage_key(), serde_json::to_value(old).unwrap());

    let partial = NodePartial::new()
        .with_extra(extra)
        .with_typed_extra(PORTFOLIO, new.clone())
        .unwrap();
    let extra = partial.extra.expect("extra should be present");

    assert_eq!(extra.get("untouched"), Some(&json!(true)));
    assert_eq!(
        serde_json::from_value::<PortfolioSnapshot>(extra[&PORTFOLIO.storage_key()].clone())
            .unwrap(),
        new
    );
}

proptest! {
    #[test]
    fn prop_typed_state_slots_round_trip_generated_payload(
        label in "[A-Za-z0-9 _:-]{0,48}",
        amount in any::<i64>(),
        flags in prop::collection::vec(any::<bool>(), 0..16),
    ) {
        let payload = PropertyPayload { label, amount, flags };
        let state = VersionedState::builder()
            .with_typed_extra(PROPERTY_PAYLOAD, payload.clone())
            .expect("generated payload should serialize")
            .build();

        prop_assert_eq!(
            state.snapshot().get_typed(PROPERTY_PAYLOAD).expect("generated payload should deserialize"),
            Some(payload)
        );
    }
}
