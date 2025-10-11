use weavegraph::event_bus::{Event, EventBus, MemorySink};
use weavegraph::telemetry::{PlainFormatter, CONTEXT_COLOR, LINE_COLOR, RESET_COLOR};

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

#[tokio::test]
async fn formatting_with_plainformatter_includes_scope_and_colors() {
    let sink = MemorySink::new();
    let sink_snapshot = sink.clone();
    let bus = EventBus::with_sink_and_formatter(sink, PlainFormatter);

    bus.listen_for_events();

    // Same scope twice: first should include colored scope prefix, second should not.
    bus.get_sender()
        .send(Event::node_message("Scope1", "one"))
        .expect("send one");
    bus.get_sender()
        .send(Event::node_message("Scope1", "two"))
        .expect("send two");

    // New scope: should include colored scope prefix again, then omit for following event.
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

    // Entry 0: has scope prefix and body coloring.
    assert!(entries[0].contains(&format!("{CONTEXT_COLOR}{}{RESET_COLOR}", "Scope1")));
    assert!(entries[0].contains(LINE_COLOR));
    assert!(entries[0].contains(RESET_COLOR));
    assert!(entries[0].contains("one"));

    // Entry 1: same scope, no scope prefix.
    assert!(!entries[1].contains(&format!("{CONTEXT_COLOR}{}{RESET_COLOR}", "Scope1")));
    assert!(entries[1].contains(LINE_COLOR));
    assert!(entries[1].contains("two"));

    // Entry 2: new scope, has scope prefix.
    assert!(entries[2].contains(&format!("{CONTEXT_COLOR}{}{RESET_COLOR}", "Scope2")));
    assert!(entries[2].contains("three"));

    // Entry 3: same new scope, no prefix.
    assert!(!entries[3].contains(&format!("{CONTEXT_COLOR}{}{RESET_COLOR}", "Scope2")));
    assert!(entries[3].contains("four"));
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
    assert!(entries.iter().any(|e| e.contains("a")));
    assert!(entries.iter().any(|e| e.contains("b")));
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
        assert!(
            entry.contains(&expected),
            "entry {idx} should contain {expected}, got: {entry}"
        );
    }
}
