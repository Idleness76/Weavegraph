use serde_json::{Value, json};
use weavegraph::channels::Channel;
use weavegraph::message::Message;
use weavegraph::state::VersionedState;

#[test]
fn test_new_with_user_message_initializes_fields() {
    let s = VersionedState::new_with_user_message("hello");
    let snap = s.snapshot();
    assert_eq!(snap.messages.len(), 1);
    assert_eq!(snap.messages[0].role, "user");
    assert_eq!(snap.messages[0].content, "hello");
    assert_eq!(snap.messages_version, 1);
    assert!(snap.extra.is_empty());
    assert_eq!(snap.extra_version, 1);
    assert!(snap.errors.is_empty());
    assert_eq!(snap.errors_version, 1);
}

#[test]
fn test_new_with_messages_initializes_fields() {
    let messages = vec![Message::user("hello"), Message::assistant("hi there")];
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
        Message::user("original"),
        Message::assistant("response"),
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
    assert_eq!(snapshot.messages[0].role, "user");
    assert_eq!(snapshot.messages[0].content, "Hello");
    assert_eq!(snapshot.messages[1].role, "assistant");
    assert_eq!(snapshot.messages[1].content, "Hi there!");
    assert_eq!(snapshot.messages[2].role, "system");
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
    assert_eq!(snapshot.messages[1].role, "assistant");
    assert_eq!(snapshot.messages[1].content, "Response");

    assert_eq!(snapshot.extra.len(), 2);
    assert_eq!(snapshot.extra.get("key1"), Some(&json!("value1")));
    assert_eq!(snapshot.extra.get("key2"), Some(&json!(42)));
}
