use chrono::Utc;
use futures_util::{pin_mut, StreamExt};
use proptest::prelude::*;
use rustc_hash::FxHashMap;
use serde_json::{Number, Value};
use std::sync::Arc;
use std::time::Duration;
use weavegraph::channels::Channel;
use weavegraph::event_bus::{
    ChannelSink, Event, EventBus, EventEmitter, LLMStreamingEvent, MemorySink, NodeEvent,
    STREAM_END_SCOPE,
};

#[tokio::test]
async fn stop_listener_flushes_pending_events() {
    let sink = MemorySink::new();
    let sink_snapshot = sink.clone();
    let bus = EventBus::with_sink(sink);

    bus.listen_for_events();

    let emitter = bus.get_emitter();
    emitter
        .emit(Event::node_message_with_meta(
            "test-node",
            42,
            "scope",
            "payload",
        ))
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    bus.stop_listener().await;

    let entries = sink_snapshot.snapshot();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message(), "payload");
}

#[tokio::test]
async fn stopping_without_events_is_noop() {
    let bus = EventBus::with_sink(MemorySink::new());
    bus.listen_for_events();
    bus.stop_listener().await;
}

#[tokio::test]
async fn memory_sink_captures_events_with_scope_and_messages() {
    let sink = MemorySink::new();
    let sink_snapshot = sink.clone();
    let bus = EventBus::with_sink(sink);

    bus.listen_for_events();

    let emitter = bus.get_emitter();

    // Same scope twice
    emitter
        .emit(Event::node_message("Scope1", "one"))
        .expect("emit one");
    emitter
        .emit(Event::node_message("Scope1", "two"))
        .expect("emit two");

    // Different scope
    emitter
        .emit(Event::diagnostic("Scope2", "three"))
        .expect("emit three");
    emitter
        .emit(Event::diagnostic("Scope2", "four"))
        .expect("emit four");

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    bus.stop_listener().await;

    let entries = sink_snapshot.snapshot();
    assert_eq!(entries.len(), 4);

    // Verify events captured with correct scope and message
    assert_eq!(entries[0].scope_label(), Some("Scope1"));
    assert_eq!(entries[0].message(), "one");

    assert_eq!(entries[1].scope_label(), Some("Scope1"));
    assert_eq!(entries[1].message(), "two");

    assert_eq!(entries[2].scope_label(), Some("Scope2"));
    assert_eq!(entries[2].message(), "three");

    assert_eq!(entries[3].scope_label(), Some("Scope2"));
    assert_eq!(entries[3].message(), "four");
}

#[tokio::test]
async fn multiple_listen_calls_are_idempotent() {
    let sink = MemorySink::new();
    let sink_snapshot = sink.clone();
    let bus = EventBus::with_sink(sink);

    // Call listen multiple times; only one listener should be active.
    bus.listen_for_events();
    bus.listen_for_events();
    bus.listen_for_events();

    // Emit a couple of events and ensure we don't get duplicate output.
    let emitter = bus.get_emitter();
    emitter.emit(Event::node_message("S", "a")).unwrap();
    emitter.emit(Event::node_message("S", "b")).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    bus.stop_listener().await;

    let entries = sink_snapshot.snapshot();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|e| e.message() == "a"));
    assert!(entries.iter().any(|e| e.message() == "b"));
}

#[tokio::test]
async fn memory_sink_preserves_order_under_concurrency() {
    use tokio::task;

    let sink = MemorySink::new();
    let sink_snapshot = sink.clone();
    let bus = EventBus::with_sink(sink);
    bus.listen_for_events();

    let emitter = bus.get_emitter();
    let mut handles = Vec::new();
    let total = 20u32;
    for i in 0..total {
        let emitter = Arc::clone(&emitter);
        handles.push(task::spawn(async move {
            // Stagger sends to establish a deterministic order.
            tokio::time::sleep(std::time::Duration::from_millis((i * 2) as u64)).await;
            emitter
                .emit(Event::node_message("ORDER", format!("m{i}")))
                .expect("emit");
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    // Allow listener to drain the channel.
    tokio::time::sleep(std::time::Duration::from_millis((total * 3) as u64)).await;
    bus.stop_listener().await;

    let entries = sink_snapshot.snapshot();
    assert_eq!(entries.len() as u32, total);
    for (idx, entry) in entries.iter().enumerate() {
        let expected = format!("m{idx}");
        assert_eq!(
            entry.message(),
            &expected,
            "entry {idx} should have message {expected}, got: {}",
            entry.message()
        );
    }
}

#[tokio::test]
async fn channel_sink_forwards_events() {
    let (tx, rx) = flume::unbounded();
    let bus = EventBus::with_sink(ChannelSink::new(tx));
    bus.listen_for_events();

    bus.get_emitter()
        .emit(Event::diagnostic("test", "hello world"))
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let received = rx.recv_async().await.unwrap();
    assert_eq!(received.message(), "hello world");
    assert_eq!(received.scope_label(), Some("test"));
}

#[tokio::test]
async fn multi_sink_broadcast() {
    let memory = MemorySink::new();
    let (tx, rx) = flume::unbounded();

    let bus = EventBus::with_sinks(vec![
        Box::new(memory.clone()),
        Box::new(ChannelSink::new(tx)),
    ]);
    bus.listen_for_events();

    bus.get_emitter()
        .emit(Event::diagnostic("test", "broadcast message"))
        .unwrap();

    // Give listener time to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Both sinks received the event
    let memory_events = memory.snapshot();
    assert_eq!(memory_events.len(), 1);
    assert_eq!(memory_events[0].message(), "broadcast message");

    let channel_event = rx.recv_async().await.unwrap();
    assert_eq!(channel_event.message(), "broadcast message");
}

#[tokio::test]
async fn add_sink_dynamically() {
    let bus = EventBus::default(); // Starts with StdOutSink
    bus.listen_for_events();

    let (tx, rx) = flume::unbounded();
    bus.add_sink(ChannelSink::new(tx));

    bus.get_emitter()
        .emit(Event::diagnostic("test", "dynamic sink"))
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let received = rx.recv_async().await.unwrap();
    assert_eq!(received.message(), "dynamic sink");
}

#[tokio::test]
async fn channel_sink_handles_dropped_receiver() {
    use std::io::ErrorKind;
    use weavegraph::event_bus::sink::EventSink;

    let (tx, rx) = flume::unbounded();
    let mut sink = ChannelSink::new(tx);

    // Drop receiver
    drop(rx);

    let event = Event::diagnostic("test", "msg");
    let result = sink.handle(&event);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), ErrorKind::BrokenPipe);
}

#[tokio::test]
async fn async_stream_adapter_yields_events() {
    let bus = EventBus::with_sink(MemorySink::new());
    let emitter = bus.get_emitter();

    let stream = bus.subscribe().into_async_stream();
    pin_mut!(stream);
    emitter
        .emit(Event::diagnostic("async", "stream"))
        .expect("emit");

    let event = stream.next().await.expect("event");
    assert_eq!(event.message(), "stream");
    assert_eq!(event.scope_label(), Some("async"));
}

#[tokio::test]
async fn next_timeout_reports_timeouts_and_events() {
    let bus = EventBus::with_sink(MemorySink::new());
    let emitter = bus.get_emitter();
    let mut stream = bus.subscribe();

    assert!(stream
        .next_timeout(Duration::from_millis(10))
        .await
        .is_none());

    emitter
        .emit(Event::diagnostic("timeout", "delivered"))
        .expect("emit");

    let event = stream
        .next_timeout(Duration::from_secs(1))
        .await
        .expect("event after emit");
    assert_eq!(event.message(), "delivered");
    assert_eq!(event.scope_label(), Some("timeout"));
}

#[tokio::test]
async fn blocking_iterator_receives_events() {
    let bus = EventBus::with_sink(MemorySink::new());
    let emitter = bus.get_emitter();
    let iter = bus.subscribe().into_blocking_iter();

    let handle = tokio::task::spawn_blocking(move || {
        let mut iter = iter;
        iter.next()
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    emitter
        .emit(Event::diagnostic("blocking", "iter"))
        .expect("emit");

    let event = handle.await.expect("join").expect("event");
    assert_eq!(event.message(), "iter");
    assert_eq!(event.scope_label(), Some("blocking"));
}

#[tokio::test]
async fn event_stream_closes_when_bus_dropped() {
    use std::time::Duration;

    let mut stream = {
        let bus = EventBus::with_sink(MemorySink::new());
        bus.listen_for_events();
        bus.subscribe()
    };

    assert!(
        stream
            .next_timeout(Duration::from_millis(50))
            .await
            .is_none(),
        "expected broadcast stream to close after EventBus drop"
    );
}

#[tokio::test]
async fn invoke_streaming_emits_terminal_event() {
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use weavegraph::graphs::GraphBuilder;
    use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
    use weavegraph::state::{StateSnapshot, VersionedState};
    use weavegraph::types::NodeKind;

    struct TerminalNode;

    #[async_trait]
    impl Node for TerminalNode {
        async fn run(&self, _: StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
            Ok(NodePartial::default())
        }
    }

    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("terminal".into()), TerminalNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("terminal".into()))
        .add_edge(NodeKind::Custom("terminal".into()), NodeKind::End)
        .compile()
        .expect("graph");

    let initial = VersionedState::new_with_user_message("finish");
    let (handle, event_stream) = app.invoke_streaming(initial).await;

    let collector = tokio::spawn(async move {
        let mut collected = Vec::new();
        let mut stream = event_stream.into_async_stream();
        while let Some(event) = stream.next().await {
            collected.push(event);
        }
        collected
    });

    let final_state = handle.join().await.expect("workflow");
    assert_eq!(final_state.messages.snapshot().len(), 1);

    let events = collector.await.expect("collector join");
    let end_event = events.last().expect("at least one terminal event");
    assert_eq!(end_event.scope_label(), Some(STREAM_END_SCOPE));
}

#[tokio::test]
async fn event_hub_metrics_track_drops() {
    use tokio::sync::broadcast::error::RecvError;
    use weavegraph::event_bus::EventHub;

    let hub = EventHub::new(1);
    let emitter = hub.emitter();
    let mut stream = hub.subscribe();

    emitter
        .emit(Event::diagnostic("metrics", "first"))
        .expect("emit first event");
    emitter
        .emit(Event::diagnostic("metrics", "second"))
        .expect("emit second event");

    let missed = match stream.recv().await {
        Err(RecvError::Lagged(missed)) => missed,
        Ok(event) => {
            panic!("expected lagged error, received event: {:?}", event);
        }
        Err(err) => panic!("unexpected recv error: {err:?}"),
    };

    assert_eq!(missed, 1);

    let metrics = hub.metrics();
    assert_eq!(metrics.capacity, 1);
    assert_eq!(metrics.dropped, 1);
}

#[test]
fn event_bus_metrics_expose_capacity() {
    let bus = EventBus::default();
    let metrics = bus.metrics();
    assert_eq!(metrics.capacity, 1024);
    assert_eq!(metrics.dropped, 0);
}

fn text_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[A-Za-z0-9 _-]{0,32}").unwrap()
}

fn json_value_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        text_strategy().prop_map(Value::String),
        any::<bool>().prop_map(Value::Bool),
        prop::num::f64::NORMAL.prop_map(|f| {
            let bounded = f.clamp(-1_000_000.0, 1_000_000.0).trunc();
            Number::from_f64(bounded).map_or(Value::Number(Number::from(0)), Value::Number)
        }),
    ]
}

fn event_strategy() -> impl Strategy<Value = Event> {
    let diagnostic = (text_strategy(), text_strategy())
        .prop_map(|(scope, message)| Event::diagnostic(scope, message));

    let node = (
        prop::option::of(text_strategy()),
        prop::option::of(any::<u64>()),
        text_strategy(),
        text_strategy(),
    )
        .prop_map(|(node_id, step, scope, message)| {
            Event::Node(NodeEvent::new(node_id, step, scope, message))
        });

    let llm = (
        prop::option::of(text_strategy()),
        prop::option::of(text_strategy()),
        prop::option::of(text_strategy()),
        text_strategy(),
        prop::collection::hash_map(text_strategy(), json_value_strategy(), 0..4),
        any::<bool>(),
    )
        .prop_map(
            |(session_id, node_id, stream_id, chunk, metadata, is_final)| {
                let meta: FxHashMap<String, Value> = metadata.into_iter().collect();
                let event = LLMStreamingEvent::new(
                    session_id,
                    node_id,
                    stream_id,
                    chunk,
                    is_final,
                    meta,
                    Utc::now(),
                );
                Event::LLM(event)
            },
        );

    prop_oneof![diagnostic, node, llm]
}

proptest! {
    #[test]
    fn event_serialization_roundtrip(event in event_strategy()) {
        let json = serde_json::to_string(&event).expect("serialize");
        let decoded: Event = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(decoded, event);
    }
}
