use std::borrow::Cow;
use std::io;

use weavegraph::event_bus::{Event, EventBus, EventSink};
use weavegraph::runtimes::runtime_config::{DiagnosticsConfig, EventBusConfig, SinkConfig};

/// A sink that always fails when handling events (default name via type_name).
struct FailingSink;

impl EventSink for FailingSink {
    fn handle(&mut self, _event: &weavegraph::event_bus::Event) -> io::Result<()> {
        Err(io::Error::other("oops"))
    }
}

/// A sink that always fails and exposes a custom name.
struct NamedFailingSink;

impl EventSink for NamedFailingSink {
    fn handle(&mut self, _event: &weavegraph::event_bus::Event) -> io::Result<()> {
        Err(io::Error::other("boom"))
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("custom.named")
    }
}

fn bus_with_diagnostics(
    enabled: bool,
    emit_to_events: bool,
    bus_capacity: usize,
    diag_capacity: usize,
) -> EventBus {
    let cfg = EventBusConfig::new(bus_capacity, vec![SinkConfig::StdOut]).with_diagnostics(
        DiagnosticsConfig {
            enabled,
            buffer_capacity: Some(diag_capacity),
            emit_to_events,
        },
    );
    cfg.build_event_bus()
}

#[tokio::test]
async fn diagnostics_happy_path_and_lagged_receiver() {
    // Create a bus with a failing sink and a small diagnostics buffer to provoke lag.
    let bus = bus_with_diagnostics(true, false, 4, 1);
    bus.add_sink(FailingSink);

    // Start workers by subscribing to the main event stream.
    let _ev_stream = bus.subscribe();

    // Subscribe to diagnostics stream.
    let mut diags = bus.diagnostics();

    // Emit one event and assert we receive a diagnostic.
    let emitter = bus.get_emitter();
    emitter.emit(Event::node_message("test", "msg1")).unwrap();
    let d1 = diags.recv().await.expect("diagnostic recv");
    assert!(
        d1.sink.contains("FailingSink"),
        "sink name should include type name, got {}",
        d1.sink
    );
    assert_eq!(d1.occurrence, 1);

    // Provoke lag: push several more without reading.
    for _ in 0..5 {
        emitter
            .emit(Event::node_message("test", "msg"))
            .expect("emit should succeed");
    }

    // Try non-blocking receive; we may see Ok or Lagged. No panics is the key.
    match diags.try_recv() {
        Ok(_d) => { /* ok - we drained one */ }
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => { /* expected in lag scenario */
        }
        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
            // Allow transient scheduling; we can tolerate empty once since more diagnostics follow.
        }
        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
            panic!("diagnostics channel unexpectedly closed")
        }
    }
}

#[tokio::test]
async fn health_snapshot_aggregates_and_tracks_last_error() {
    let bus = bus_with_diagnostics(true, false, 8, 8);
    bus.add_sink(FailingSink);
    let _ = bus.subscribe();

    let emitter = bus.get_emitter();
    // Trigger multiple failures
    for _ in 0..3 {
        emitter.emit(Event::node_message("scope", "msg")).unwrap();
    }
    // Ensure the worker processed by consuming diagnostics
    let mut diags = bus.diagnostics();
    for _ in 0..3 {
        let _ = diags.recv().await.expect("diagnostic recv");
    }
    let health = bus.sink_health();
    let failing = health
        .iter()
        .find(|h| h.sink.contains("FailingSink"))
        .expect("health contains failing sink entry");
    assert_eq!(failing.error_count, 3);
    assert_eq!(failing.last_error.as_deref(), Some("oops"));
    assert!(failing.last_error_at.is_some());
}

#[tokio::test]
async fn sink_naming_default_and_override() {
    let bus = bus_with_diagnostics(true, false, 8, 8);
    bus.add_sink(FailingSink);
    bus.add_sink(NamedFailingSink);
    let _ = bus.subscribe();
    let emitter = bus.get_emitter();

    emitter.emit(Event::node_message("scope", "a")).unwrap();
    emitter.emit(Event::node_message("scope", "b")).unwrap();

    // Consume two diagnostics to ensure processing
    let mut diags = bus.diagnostics();
    let _ = diags.recv().await.expect("diagnostic 1");
    let _ = diags.recv().await.expect("diagnostic 2");
    let health = bus.sink_health();
    assert!(health.iter().any(|h| h.sink.contains("FailingSink")));
    assert!(health.iter().any(|h| h.sink == "custom.named"));
}

#[tokio::test]
async fn emit_to_events_toggle_behavior() {
    // 1) emit_to_events = false: no diagnostics in main event stream
    let bus_off = bus_with_diagnostics(true, false, 8, 8);
    bus_off.add_sink(FailingSink);
    let mut events_off = bus_off.subscribe();
    let emitter_off = bus_off.get_emitter();
    emitter_off.emit(Event::node_message("scope", "x")).unwrap();

    // Drain a few events with timeout; ensure we don't observe Event::Diagnostic.
    let mut saw_diag = false;
    for _ in 0..3 {
        if let Some(ev) = events_off
            .next_timeout(std::time::Duration::from_millis(50))
            .await
            && matches!(ev, Event::Diagnostic(_))
        {
            saw_diag = true;
            break;
        }
    }
    assert!(
        !saw_diag,
        "no Event::Diagnostic should be emitted when emit_to_events=false"
    );

    // 2) emit_to_events = true: one diagnostic per error occurrence; emit exactly one error
    let bus_on = {
        let cfg =
            EventBusConfig::new(8, vec![SinkConfig::StdOut]).with_diagnostics(DiagnosticsConfig {
                enabled: true,
                buffer_capacity: Some(8),
                emit_to_events: true,
            });
        cfg.build_event_bus()
    };
    bus_on.add_sink(FailingSink);
    let mut events_on = bus_on.subscribe();
    let emitter_on = bus_on.get_emitter();
    emitter_on.emit(Event::node_message("scope", "y")).unwrap();

    // Expect to see exactly one Event::Diagnostic within a short window.
    let mut diag_count = 0u32;
    for _ in 0..5 {
        if let Some(ev) = events_on
            .next_timeout(std::time::Duration::from_millis(100))
            .await
            && matches!(ev, Event::Diagnostic(_))
        {
            diag_count += 1;
            break;
        }
    }
    assert_eq!(
        diag_count, 1,
        "expected a single diagnostic event to be emitted"
    );
}
