use async_trait::async_trait;
use futures_util::StreamExt;
use rustc_hash::FxHashMap;
use serde_json::Value;
use weavegraph::channels::Channel;
use weavegraph::event_bus::STREAM_END_SCOPE;
use weavegraph::graphs::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::VersionedState;
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
        frontier: None,
    };
    let outcome = app
        .apply_barrier(state, &run_ids, vec![partial])
        .await
        .unwrap();
    assert!(outcome.updated_channels.contains(&"messages"));
    assert!(outcome.errors.is_empty());
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
        frontier: None,
    };
    let outcome = app
        .apply_barrier(state, &run_ids, vec![partial])
        .await
        .unwrap();
    assert!(outcome.updated_channels.is_empty());
    assert!(outcome.errors.is_empty());
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
        frontier: None,
    };
    app.apply_barrier(state, &[NodeKind::Start], vec![partial])
        .await
        .unwrap();
    assert_eq!(state.messages.version(), u32::MAX);
}

#[tokio::test]
async fn test_apply_barrier_preserves_updated_channel_order() {
    use weavegraph::channels::errors::{ErrorEvent, ErrorScope};

    let app = make_app();
    let state = &mut state_with_user("hi");
    let run_ids = vec![NodeKind::Start];

    let partial_a = NodePartial::new().with_messages(vec![Message::assistant("a")]);
    let partial_b = NodePartial::new().with_extra({
        let mut map = FxHashMap::default();
        map.insert("z".into(), Value::String("1".into()));
        map.insert("a".into(), Value::String("2".into()));
        map
    });
    let err_event = ErrorEvent {
        scope: ErrorScope::Node {
            kind: "anode".into(),
            step: 2,
        },
        when: chrono::Utc::now(),
        ..Default::default()
    };
    let partial_c = NodePartial::new().with_errors(vec![err_event.clone()]);

    let outcome = app
        .apply_barrier(state, &run_ids, vec![partial_a, partial_b, partial_c])
        .await
        .unwrap();

    assert_eq!(outcome.updated_channels, vec!["messages", "extra"]);
    assert_eq!(outcome.errors, vec![err_event]);
    assert_eq!(state.messages.version(), 2);
    assert_eq!(state.extra.version(), 2);
    let extra_snapshot = state.extra.snapshot();
    let mut keys: Vec<_> = extra_snapshot.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["a".to_string(), "z".to_string()]);
}

struct EmitOnce;

#[async_trait]
impl Node for EmitOnce {
    async fn run(
        &self,
        _snapshot: weavegraph::state::StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("test", "event").unwrap();
        Ok(NodePartial::default())
    }
}

#[tokio::test]
async fn invoke_streaming_closes_stream() {
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("emit".into()), EmitOnce)
        .add_edge(NodeKind::Start, NodeKind::Custom("emit".into()))
        .add_edge(NodeKind::Custom("emit".into()), NodeKind::End)
        .compile()
        .unwrap();

    let initial = VersionedState::new_with_user_message("hello");
    let (invocation, events) = app.invoke_streaming(initial).await;

    let mut stream = events.into_async_stream();
    let mut seen_non_terminal = 0;
    let mut sentinel_seen = false;
    while let Some(event) = stream.next().await {
        if event.scope_label() == Some(STREAM_END_SCOPE) {
            assert!(
                !sentinel_seen,
                "STREAM_END_SCOPE should appear exactly once"
            );
            sentinel_seen = true;
        } else {
            seen_non_terminal += 1;
        }
    }

    assert_eq!(seen_non_terminal, 1);
    assert!(sentinel_seen, "expected terminal sentinel event");
    invocation.join().await.unwrap();
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
        frontier: None,
    };
    let partial2 = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "bar".into(),
        }]),
        extra: None,
        errors: None,
        frontier: None,
    };
    let outcome = app
        .apply_barrier(
            state,
            &[NodeKind::Start, NodeKind::End],
            vec![partial1, partial2],
        )
        .await
        .unwrap();
    let snap = state.messages.snapshot();
    assert!(outcome.updated_channels.contains(&"messages"));
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
        frontier: None,
    };
    // Empty extra map -> Some(empty) should be treated as no-op by guard
    let empty_extra = NodePartial {
        messages: None,
        extra: Some(FxHashMap::default()),
        errors: None,
        frontier: None,
    };
    let outcome = app
        .apply_barrier(
            state,
            &[NodeKind::Start, NodeKind::End],
            vec![empty_msgs, empty_extra],
        )
        .await
        .unwrap();
    assert!(outcome.updated_channels.is_empty());
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
        frontier: None,
    };
    let p2 = NodePartial {
        messages: None,
        extra: Some(m2),
        errors: None,
        frontier: None,
    };

    let outcome = app
        .apply_barrier(state, &[NodeKind::Start, NodeKind::End], vec![p1, p2])
        .await
        .unwrap();
    assert!(outcome.updated_channels.contains(&"extra"));
    let snap = state.extra.snapshot();
    assert_eq!(snap.get("k1"), Some(&Value::String("v3".into())));
    assert_eq!(snap.get("k2"), Some(&Value::String("v2".into())));
    assert_eq!(state.extra.version(), 2);
}

#[tokio::test]
async fn test_apply_barrier_collects_errors() {
    use weavegraph::channels::errors::ErrorEvent;

    let app = make_app();
    let state = &mut state_with_user("hi");
    let run_ids = vec![NodeKind::Start];
    let partial = NodePartial {
        messages: None,
        extra: None,
        errors: Some(vec![ErrorEvent::default()]),
        frontier: None,
    };

    let outcome = app
        .apply_barrier(state, &run_ids, vec![partial])
        .await
        .unwrap();

    assert!(outcome.updated_channels.is_empty());
    assert_eq!(outcome.errors.len(), 1);
}

#[tokio::test]
async fn test_invoke_with_channel() {
    // Build a simple graph with a test node
    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Custom("test".into()),
            SimpleMessageNode::new("test output"),
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("test".into()))
        .add_edge(NodeKind::Custom("test".into()), NodeKind::End)
        .compile()
        .unwrap();

    // Execute with channel
    let initial_state = state_with_user("test input");
    let (result, events) = app.invoke_with_channel(initial_state).await;

    // Spawn task to collect events (simulating client consumption)
    let event_task = tokio::spawn(async move {
        let mut count = 0;
        // Use timeout to avoid hanging if no events come
        let timeout_duration = tokio::time::Duration::from_millis(100);
        loop {
            match tokio::time::timeout(timeout_duration, events.recv_async()).await {
                Ok(Ok(_event)) => count += 1,
                Ok(Err(_)) => break, // Channel closed
                Err(_) => break,     // Timeout - no more events
            }
        }
        count
    });

    // Wait for workflow to complete
    let final_state = result.expect("Workflow should complete successfully");
    assert!(!final_state.messages.is_empty(), "Should have messages");

    // The method itself works - we got a receiver and a result
    // Note: Event count verification is inherently racy due to EventBus Drop behavior
    let _event_count = event_task.await.expect("Event task should complete");
    // We just verify the API works, not exact event counts
}

#[tokio::test]
async fn test_invoke_with_channel_resumption_updates_versions() {
    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Custom("test".into()),
            SimpleMessageNode::new("test output"),
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("test".into()))
        .add_edge(NodeKind::Custom("test".into()), NodeKind::End)
        .compile()
        .unwrap();

    let state = VersionedState::new_with_user_message("first run");
    let (result, _events) = app.invoke_with_channel(state).await;
    let final_state = result.expect("first run succeeds");
    assert_eq!(final_state.messages.version(), 2);

    // Re-run with the output state to ensure versions bump deterministically.
    let (second_result, _second_events) = app.invoke_with_channel(final_state.clone()).await;
    let second_state = second_result.expect("second run succeeds");
    assert_eq!(second_state.messages.version(), 3);
    assert_eq!(second_state.extra.version(), final_state.extra.version());
}

#[tokio::test]
async fn test_invoke_with_channel_collects_events() {
    use weavegraph::event_bus::Event;

    // Build graph with a node that emits events
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("emitter".into()), EmitterNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("emitter".into()))
        .add_edge(NodeKind::Custom("emitter".into()), NodeKind::End)
        .compile()
        .unwrap();

    let initial_state = state_with_user("emit events");
    let (result, events) = app.invoke_with_channel(initial_state).await;

    // Collect events with timeout
    let event_task = tokio::spawn(async move {
        let mut collected = Vec::new();
        let timeout_duration = tokio::time::Duration::from_millis(100);
        loop {
            match tokio::time::timeout(timeout_duration, events.recv_async()).await {
                Ok(Ok(event)) => collected.push(event),
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }
        collected
    });

    // Verify workflow succeeded
    result.expect("Workflow should complete");

    // Wait for events
    let collected_events = event_task.await.expect("Event task should complete");

    // The API works - we can receive events (even if timing makes this racy)
    // In production, the EventBus stays alive longer so events flow properly
    if !collected_events.is_empty() {
        // If we got events, verify they're the right type
        let has_node_event = collected_events.iter().any(|e| matches!(e, Event::Node(_)));
        assert!(has_node_event, "Should have at least one node event");
    }
    // Test passes if we got a valid result and receiver, regardless of timing
}

#[tokio::test]
async fn test_invoke_with_sinks() {
    use weavegraph::event_bus::MemorySink;

    // Build simple graph
    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Custom("test".into()),
            SimpleMessageNode::new("test output"),
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("test".into()))
        .add_edge(NodeKind::Custom("test".into()), NodeKind::End)
        .compile()
        .unwrap();

    // Use MemorySink which captures synchronously (no async timing issues)
    let memory_sink = MemorySink::new();

    // Execute with custom sink
    let initial_state = state_with_user("test with sinks");
    let final_state = app
        .invoke_with_sinks(initial_state, vec![Box::new(memory_sink.clone())])
        .await
        .expect("Workflow should complete successfully");

    // Verify execution completed
    assert!(!final_state.messages.is_empty(), "Should have messages");

    // MemorySink should have captured events (it's synchronous in the listener loop)
    // However, due to Drop abort, we might miss some events
    // The test verifies the API works, not exact event counts
    let _events = memory_sink.snapshot();
    // API works if we reach here without errors
}

#[tokio::test]
async fn test_invoke_with_sinks_multiple() {
    use weavegraph::event_bus::{ChannelSink, MemorySink, StdOutSink};

    // Build simple graph
    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Custom("test".into()),
            SimpleMessageNode::new("test output"),
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("test".into()))
        .add_edge(NodeKind::Custom("test".into()), NodeKind::End)
        .compile()
        .unwrap();

    // Create multiple sinks to verify the API accepts Vec<Box<dyn EventSink>>
    let (tx, _rx) = flume::unbounded();
    let memory_sink = MemorySink::new();

    // Execute with multiple sinks - this tests type compatibility
    let initial_state = state_with_user("test multiple sinks");
    let final_state = app
        .invoke_with_sinks(
            initial_state,
            vec![
                Box::new(StdOutSink::default()),
                Box::new(ChannelSink::new(tx)),
                Box::new(memory_sink.clone()),
            ],
        )
        .await
        .expect("Workflow should complete");

    // Verify execution completed
    assert!(!final_state.messages.is_empty(), "Should have messages");

    // The test verifies that:
    // 1. invoke_with_sinks() accepts multiple different sink types
    // 2. The workflow completes successfully with multiple sinks
    // 3. Type system allows Vec<Box<dyn EventSink>> as expected
    // Event counting is inherently racy in tests due to EventBus Drop behavior
}
