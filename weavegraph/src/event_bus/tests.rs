use super::{Event, EventBus, MemorySink};

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
    assert!(entries[0].contains("payload"));
}

#[tokio::test]
async fn stopping_without_events_is_noop() {
    let bus = EventBus::with_sink(MemorySink::new());
    bus.listen_for_events();
    bus.stop_listener().await;
}
