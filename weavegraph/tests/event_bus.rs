use weavegraph::event_bus::{ChannelSink, Event, EventBus, MemorySink};

#[tokio::test]
async fn stop_listener_flushes_pending_events() {
    let sink = MemorySink::new();
    let sink_snapshot = sink.clone();
    let bus = EventBus::with_sink(sink);

    bus.listen_for_events();

    bus.get_sender()
        .send(Event::node_message_with_meta(
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

    // Same scope twice
    bus.get_sender()
        .send(Event::node_message("Scope1", "one"))
        .expect("send one");
    bus.get_sender()
        .send(Event::node_message("Scope1", "two"))
        .expect("send two");

    // Different scope
    bus.get_sender()
        .send(Event::diagnostic("Scope2", "three"))
        .expect("send three");
    bus.get_sender()
        .send(Event::diagnostic("Scope2", "four"))
        .expect("send four");

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
    let sender = bus.get_sender();
    sender.send(Event::node_message("S", "a")).unwrap();
    sender.send(Event::node_message("S", "b")).unwrap();

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

    let sender = bus.get_sender();
    let mut handles = Vec::new();
    let total = 20u32;
    for i in 0..total {
        let s = sender.clone();
        handles.push(task::spawn(async move {
            // Stagger sends to establish a deterministic order.
            tokio::time::sleep(std::time::Duration::from_millis((i * 2) as u64)).await;
            s.send(Event::node_message("ORDER", format!("m{i}")))
                .expect("send");
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

    bus.get_sender()
        .send(Event::diagnostic("test", "hello world"))
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

    bus.get_sender()
        .send(Event::diagnostic("test", "broadcast message"))
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

    bus.get_sender()
        .send(Event::diagnostic("test", "dynamic sink"))
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
