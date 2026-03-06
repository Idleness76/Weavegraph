use async_trait::async_trait;
use weavegraph::channels::errors::ErrorEvent;
use weavegraph::event_bus::EventBus;
use weavegraph::message::{Message, Role};
use weavegraph::node::{Node, NodeContext, NodeContextError, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::utils::collections::new_extra_map;

fn make_ctx(step: u64) -> (NodeContext, EventBus) {
    let event_bus = EventBus::default();
    event_bus.listen_for_events();
    let ctx = NodeContext {
        node_id: "test-node".to_string(),
        step,
        event_emitter: event_bus.get_emitter(),
    };
    (ctx, event_bus)
}

#[tokio::test]
async fn test_node_context_creation() {
    let (ctx, _event_bus) = make_ctx(5);
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.step, 5);
}

#[test]
fn test_node_partial_default() {
    let np = NodePartial::default();
    assert!(np.messages.is_none());
    assert!(np.extra.is_none());
    assert!(np.errors.is_none());
}

#[test]
fn test_node_partial_with_messages() {
    let messages = vec![Message::with_role(
        Role::Custom("test".to_string()),
        "test message",
    )];
    let partial = NodePartial::new().with_messages(messages.clone());
    assert_eq!(partial.messages, Some(messages));
    assert!(partial.extra.is_none());
    assert!(partial.errors.is_none());
}

#[test]
fn test_node_partial_with_extra() {
    let mut extra = new_extra_map();
    extra.insert("test_key".to_string(), serde_json::json!("test_value"));

    let partial = NodePartial::new().with_extra(extra.clone());
    assert!(partial.messages.is_none());
    assert_eq!(partial.extra, Some(extra));
    assert!(partial.errors.is_none());
}

#[test]
fn test_node_partial_with_errors() {
    let errors = vec![ErrorEvent::default()];
    let partial = NodePartial::new().with_errors(errors.clone());
    assert!(partial.messages.is_none());
    assert!(partial.extra.is_none());
    assert_eq!(partial.errors, Some(errors));
}

#[tokio::test]
async fn test_node_context_emit_error() {
    let (ctx, event_bus) = make_ctx(1);
    drop(event_bus); // Drop the event bus to disconnect sender
    tokio::task::yield_now().await;
    let result = ctx.emit("scope", "message");
    assert!(matches!(result, Err(NodeContextError::EventBusUnavailable)));
}

#[test]
fn test_node_error_variants() {
    // MissingInput
    let err = NodeError::MissingInput { what: "field" };
    match err {
        NodeError::MissingInput { what } => assert_eq!(what, "field"),
        _ => panic!("Wrong variant"),
    }

    // Provider
    let err = NodeError::Provider {
        provider: "svc",
        message: "fail".to_string(),
    };
    match err {
        NodeError::Provider { provider, message } => {
            assert_eq!(provider, "svc");
            assert_eq!(message, "fail");
        }
        _ => panic!("Wrong variant"),
    }

    // Serde
    let json_err = serde_json::from_str::<serde_json::Value>("not_json").unwrap_err();
    let err = NodeError::Serde(json_err);
    match err {
        NodeError::Serde(_) => (),
        _ => panic!("Wrong variant"),
    }

    // ValidationFailed
    let err = NodeError::ValidationFailed("bad input".to_string());
    match err {
        NodeError::ValidationFailed(msg) => assert_eq!(msg, "bad input"),
        _ => panic!("Wrong variant"),
    }

    // EventBus
    let err = NodeError::EventBus(NodeContextError::EventBusUnavailable);
    match err {
        NodeError::EventBus(NodeContextError::EventBusUnavailable) => (),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_node_context_error_variant() {
    let err = NodeContextError::EventBusUnavailable;
    match err {
        NodeContextError::EventBusUnavailable => (),
    }
}

struct DummyNode;
#[async_trait]
impl Node for DummyNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("dummy", "executed").map_err(NodeError::EventBus)?;
        Ok(NodePartial::new().with_messages(vec![Message::with_role(
            Role::Custom("dummy".to_string()),
            "ok",
        )]))
    }
}

#[tokio::test]
async fn test_node_trait_success() {
    let (ctx, _event_bus) = make_ctx(0);
    let node = DummyNode;
    let snapshot = VersionedState::new_with_user_message("dummy").snapshot();
    let result = node.run(snapshot, ctx).await;
    assert!(result.is_ok());
    let partial = result.unwrap();
    assert_eq!(partial.messages.unwrap()[0].role, "dummy");
}

#[tokio::test]
async fn test_node_trait_eventbus_error() {
    let (ctx, event_bus) = make_ctx(0);
    drop(event_bus); // disconnect event bus
    tokio::task::yield_now().await;
    let node = DummyNode;
    let snapshot = VersionedState::new_with_user_message("dummy").snapshot();
    let result = node.run(snapshot, ctx).await;
    assert!(matches!(
        result,
        Err(NodeError::EventBus(NodeContextError::EventBusUnavailable))
    ));
}
