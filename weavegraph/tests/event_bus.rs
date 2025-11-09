use chrono::Utc;
use futures_util::{StreamExt, pin_mut};
use parking_lot::Mutex as ParkingMutex;
use proptest::prelude::*;
use rustc_hash::FxHashMap;
use serde_json::{Number, Value, json};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use weavegraph::channels::Channel;
use weavegraph::event_bus::{
    ChannelSink, Event, EventBus, EventEmitter, EventSink, JsonLinesSink, LLMStreamingEvent,
    MemorySink, NodeEvent, STREAM_END_SCOPE,
};
use weavegraph::node::NodeContext;

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

    assert!(
        stream
            .next_timeout(Duration::from_millis(10))
            .await
            .is_none()
    );

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
async fn stop_listener_drains_multiple_sinks() {
    use std::time::Duration;

    let sink1 = MemorySink::new();
    let sink2 = MemorySink::new();
    let snapshot1 = sink1.clone();
    let snapshot2 = sink2.clone();

    let bus = EventBus::with_sinks(vec![Box::new(sink1), Box::new(sink2)]);
    bus.listen_for_events();

    let emitter = bus.get_emitter();
    for i in 0..10 {
        emitter
            .emit(Event::diagnostic("test", format!("msg {i}")))
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.stop_listener().await;

    // Both sinks should have received all events
    assert_eq!(snapshot1.snapshot().len(), 10);
    assert_eq!(snapshot2.snapshot().len(), 10);
}

#[tokio::test]
async fn stop_listener_during_emission() {
    use std::sync::Arc;
    use tokio::task;

    let bus = Arc::new(EventBus::with_sink(MemorySink::new()));
    bus.listen_for_events();

    let emitter = bus.get_emitter();
    let emit_task = task::spawn(async move {
        for i in 0..1000u32 {
            let _ = emitter.emit(Event::diagnostic("stress", format!("{i}")));
            task::yield_now().await;
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    // Should not panic and should shut down cleanly
    bus.stop_listener().await;
    // Clean up emission task if still running
    emit_task.abort();
}

#[tokio::test]
async fn restart_after_stop() {
    use std::time::Duration;

    let sink = MemorySink::new();
    let snapshot = sink.clone();
    let bus = EventBus::with_sink(sink);

    // First cycle
    bus.listen_for_events();
    bus.get_emitter()
        .emit(Event::diagnostic("cycle1", "msg1"))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    bus.stop_listener().await;

    // Second cycle
    bus.listen_for_events();
    bus.get_emitter()
        .emit(Event::diagnostic("cycle2", "msg2"))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    bus.stop_listener().await;

    let events = snapshot.snapshot();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].message(), "msg1");
    assert_eq!(events[1].message(), "msg2");
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

#[derive(Default)]
struct RecordingEmitter {
    events: Arc<ParkingMutex<Vec<Event>>>,
}

impl RecordingEmitter {
    fn new() -> Self {
        Self {
            events: Arc::new(ParkingMutex::new(Vec::new())),
        }
    }

    fn record(&self, event: Event) {
        self.events.lock().push(event);
    }

    fn snapshot(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl fmt::Debug for RecordingEmitter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordingEmitter")
            .field("event_count", &self.events.lock().len())
            .finish()
    }
}

impl EventEmitter for RecordingEmitter {
    fn emit(&self, event: Event) -> Result<(), weavegraph::event_bus::EmitterError> {
        self.record(event);
        Ok(())
    }
}

#[test]
fn node_context_emits_all_event_variants() {
    let emitter = Arc::new(RecordingEmitter::new());
    let event_emitter: Arc<dyn EventEmitter> = emitter.clone();
    let ctx = NodeContext {
        node_id: "node-a".to_string(),
        step: 7,
        event_emitter,
    };

    ctx.emit("progress", "started").unwrap();
    ctx.emit_diagnostic("diagnostic", "all good").unwrap();

    let mut metadata = FxHashMap::default();
    metadata.insert("token_count".into(), json!(42));
    ctx.emit_llm_chunk(
        Some("session-1".into()),
        Some("stream-1".into()),
        "chunk text",
        Some(metadata),
    )
    .unwrap();

    ctx.emit_llm_final(
        Some("session-1".into()),
        Some("stream-1".into()),
        "final chunk",
        None,
    )
    .unwrap();

    ctx.emit_llm_error(
        Some("session-1".into()),
        Some("stream-1".into()),
        "error occurred",
    )
    .unwrap();

    let events = emitter.snapshot();
    assert_eq!(events.len(), 5);

    match &events[0] {
        Event::Node(node) => {
            assert_eq!(node.node_id(), Some("node-a"));
            assert_eq!(node.step(), Some(7));
            assert_eq!(node.scope(), "progress");
            assert_eq!(node.message(), "started");
        }
        other => panic!("expected node event, got {other:?}"),
    }

    match &events[1] {
        Event::Diagnostic(diag) => {
            assert_eq!(diag.scope(), "diagnostic");
            assert_eq!(diag.message(), "all good");
        }
        other => panic!("expected diagnostic event, got {other:?}"),
    }

    match &events[2] {
        Event::LLM(llm) => {
            assert_eq!(llm.session_id(), Some("session-1"));
            assert_eq!(llm.node_id(), Some("node-a"));
            assert_eq!(llm.stream_id(), Some("stream-1"));
            assert!(!llm.is_final());
            assert_eq!(llm.chunk(), "chunk text");
            assert_eq!(llm.metadata().get("token_count"), Some(&json!(42)));
        }
        other => panic!("expected LLM chunk event, got {other:?}"),
    }

    match &events[3] {
        Event::LLM(llm) => {
            assert!(llm.is_final());
            assert_eq!(llm.chunk(), "final chunk");
            assert!(llm.metadata().is_empty());
        }
        other => panic!("expected final LLM event, got {other:?}"),
    }

    match &events[4] {
        Event::LLM(llm) => {
            assert!(llm.is_final());
            assert_eq!(llm.chunk(), "error occurred");
            assert_eq!(llm.metadata().get("severity"), Some(&json!("error")));
        }
        other => panic!("expected LLM error event, got {other:?}"),
    }
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
                    None,
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

// ============================================================================
// JSON Serialization Tests
// ============================================================================

// Helper for shared writer in tests
struct SharedWriter(Arc<ParkingMutex<std::io::Cursor<Vec<u8>>>>);

impl std::io::Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().flush()
    }
}

#[test]
fn test_node_event_to_json_value() {
    let event = Event::node_message_with_meta("router", 5, "routing", "Processing request");
    let json = event.to_json_value();

    assert_eq!(json["type"], "node");
    assert_eq!(json["scope"], "routing");
    assert_eq!(json["message"], "Processing request");
    assert_eq!(json["metadata"]["node_id"], "router");
    assert_eq!(json["metadata"]["step"], 5);
    assert!(json["timestamp"].is_string());
}

#[test]
fn test_node_event_partial_metadata() {
    // Node without node_id or step
    let event = Event::node_message("test_scope", "test message");
    let json = event.to_json_value();

    assert_eq!(json["type"], "node");
    assert_eq!(json["scope"], "test_scope");
    assert_eq!(json["message"], "test message");
    assert!(json["metadata"].is_object());
    assert!(json["metadata"]["node_id"].is_null());
    assert!(json["metadata"]["step"].is_null());
}

#[test]
fn test_diagnostic_event_to_json_value() {
    let event = Event::diagnostic("error_scope", "Something went wrong");
    let json = event.to_json_value();

    assert_eq!(json["type"], "diagnostic");
    assert_eq!(json["scope"], "error_scope");
    assert_eq!(json["message"], "Something went wrong");
    assert!(json["timestamp"].is_string());
    // Diagnostic events have minimal metadata
    assert!(json["metadata"].is_object());
    let metadata = json["metadata"].as_object().unwrap();
    assert!(metadata.is_empty());
}

#[test]
fn test_llm_event_to_json_value() {
    let mut metadata = FxHashMap::default();
    metadata.insert("content_type".to_string(), json!("reasoning"));
    metadata.insert("token_count".to_string(), json!(42));

    let timestamp = Utc::now();
    let llm_event = LLMStreamingEvent::new(
        Some("session-123".to_string()),
        Some("node-abc".to_string()),
        Some("stream-xyz".to_string()),
        "Thinking step by step...".to_string(),
        false,
        None,
        metadata,
        timestamp,
    );
    let event = Event::LLM(llm_event);
    let json = event.to_json_value();

    assert_eq!(json["type"], "llm");
    assert_eq!(json["message"], "Thinking step by step...");
    assert_eq!(json["metadata"]["session_id"], "session-123");
    assert_eq!(json["metadata"]["node_id"], "node-abc");
    assert_eq!(json["metadata"]["stream_id"], "stream-xyz");
    assert_eq!(json["metadata"]["is_final"], false);
    assert_eq!(json["metadata"]["content_type"], "reasoning");
    assert_eq!(json["metadata"]["token_count"], 42);
    assert_eq!(json["timestamp"], timestamp.to_rfc3339());
}

#[test]
fn test_llm_event_final_chunk() {
    let llm_event = LLMStreamingEvent::new(
        None,
        None,
        Some("stream-999".to_string()),
        "Final chunk".to_string(),
        true,
        None,
        FxHashMap::default(),
        Utc::now(),
    );
    let event = Event::LLM(llm_event);
    let json = event.to_json_value();

    assert_eq!(json["type"], "llm");
    assert_eq!(json["metadata"]["is_final"], true);
    assert_eq!(json["metadata"]["stream_id"], "stream-999");
    assert!(json["metadata"]["session_id"].is_null());
    assert!(json["metadata"]["node_id"].is_null());
}

#[test]
fn test_to_json_string_compact() {
    let event = Event::diagnostic("test", "message");
    let json_str = event.to_json_string().unwrap();

    // Compact format has no extra whitespace
    assert!(json_str.contains("\"type\":\"diagnostic\""));
    assert!(json_str.contains("\"scope\":\"test\""));
    assert!(json_str.contains("\"message\":\"message\""));
    assert!(!json_str.contains("  ")); // No indentation
}

#[test]
fn test_to_json_pretty_formatted() {
    let event = Event::node_message("test", "hello");
    let json_str = event.to_json_pretty().unwrap();

    // Pretty format has indentation
    assert!(json_str.contains("  \"type\": \"node\""));
    assert!(json_str.contains("  \"scope\": \"test\""));
    assert!(json_str.contains("  \"message\": \"hello\""));
}

#[test]
fn test_json_roundtrip_via_to_json_string() {
    let original = Event::node_message_with_meta("node1", 10, "scope1", "msg1");
    let json_str = original.to_json_string().unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["type"], "node");
    assert_eq!(parsed["metadata"]["node_id"], "node1");
    assert_eq!(parsed["metadata"]["step"], 10);
}

#[tokio::test]
async fn test_jsonlines_sink_stdout() {
    use std::io::Cursor;

    // Create in-memory buffer to capture output
    let buffer = Arc::new(ParkingMutex::new(Cursor::new(Vec::new())));
    let buffer_clone = buffer.clone();

    let mut sink = JsonLinesSink::new(Box::new(SharedWriter(buffer)));

    let event1 = Event::diagnostic("test1", "first message");
    let event2 = Event::node_message("test2", "second message");

    sink.handle(&event1).unwrap();
    sink.handle(&event2).unwrap();

    // Extract buffer contents
    let locked = buffer_clone.lock();
    let output = String::from_utf8(locked.get_ref().clone()).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    assert_eq!(lines.len(), 2);

    // Parse first line
    let json1: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json1["type"], "diagnostic");
    assert_eq!(json1["scope"], "test1");
    assert_eq!(json1["message"], "first message");

    // Parse second line
    let json2: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(json2["type"], "node");
    assert_eq!(json2["scope"], "test2");
    assert_eq!(json2["message"], "second message");
}

#[tokio::test]
async fn test_jsonlines_sink_pretty_print() {
    use std::io::Cursor;

    let buffer = Arc::new(ParkingMutex::new(Cursor::new(Vec::new())));
    let buffer_clone = buffer.clone();

    let mut sink = JsonLinesSink::with_pretty_print(Box::new(SharedWriter(buffer)));

    let event = Event::diagnostic("pretty_test", "formatted output");
    sink.handle(&event).unwrap();

    let locked = buffer_clone.lock();
    let output = String::from_utf8(locked.get_ref().clone()).unwrap();

    // Pretty printed JSON should have indentation
    assert!(output.contains("  \"type\": \"diagnostic\""));
    assert!(output.contains("  \"scope\": \"pretty_test\""));
}

#[tokio::test]
async fn test_jsonlines_sink_file_output() {
    use std::fs;

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let path = temp_file.path().to_path_buf();

    {
        let mut sink = JsonLinesSink::to_file(&path).unwrap();

        let event1 = Event::node_message_with_meta("file_node", 1, "file_scope", "first");
        let event2 = Event::diagnostic("file_scope", "second");
        let event3 = Event::node_message("file_scope", "third");

        sink.handle(&event1).unwrap();
        sink.handle(&event2).unwrap();
        sink.handle(&event3).unwrap();
    } // sink dropped, file flushed

    // Read file contents
    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();

    assert_eq!(lines.len(), 3);

    let json1: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json1["metadata"]["node_id"], "file_node");
    assert_eq!(json1["metadata"]["step"], 1);

    let json2: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(json2["type"], "diagnostic");

    let json3: Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(json3["message"], "third");
}

#[tokio::test]
async fn test_jsonlines_sink_flush_behavior() {
    use std::io::Cursor;

    let buffer = Arc::new(ParkingMutex::new(Cursor::new(Vec::new())));
    let buffer_clone = buffer.clone();

    let mut sink = JsonLinesSink::new(Box::new(SharedWriter(buffer)));

    let event = Event::diagnostic("flush_test", "should be flushed immediately");
    sink.handle(&event).unwrap();

    // After single handle() call, buffer should already contain the event
    // because EventSink::handle flushes after each event
    let locked = buffer_clone.lock();
    let output = String::from_utf8(locked.get_ref().clone()).unwrap();

    assert!(output.contains("\"message\":\"should be flushed immediately\""));
}

#[tokio::test]
async fn test_jsonlines_sink_with_eventbus() {
    let buffer = Arc::new(ParkingMutex::new(std::io::Cursor::new(Vec::new())));
    let buffer_clone = buffer.clone();

    // Create sink with shared buffer
    let sink = JsonLinesSink::new(Box::new(SharedWriter(buffer)));
    let bus = EventBus::with_sink(sink);
    bus.listen_for_events();

    let emitter = bus.get_emitter();
    emitter
        .emit(Event::node_message("integration", "message1"))
        .unwrap();
    emitter
        .emit(Event::diagnostic("integration", "message2"))
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.stop_listener().await;

    // Extract and verify output
    let locked = buffer_clone.lock();
    let output = String::from_utf8(locked.get_ref().clone()).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    assert_eq!(lines.len(), 2);
    let json1: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json1["message"], "message1");
}
