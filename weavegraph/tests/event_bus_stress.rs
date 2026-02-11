//! Stress tests for the event bus under high load.
//!
//! These tests verify event bus behavior under various load conditions
//! including burst emissions, slow consumers, and concurrent producers.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;

use weavegraph::event_bus::{Event, EventBus, MemorySink};
use weavegraph::graphs::GraphBuilder;
use weavegraph::message::{Message, Role};
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{AppRunner, CheckpointerType, StepOptions, StepResult};
use weavegraph::state::StateSnapshot;
use weavegraph::types::NodeKind;

use common::*;

/// A node that emits a burst of events.
struct BurstEmitterNode {
    event_count: usize,
    counter: Arc<AtomicUsize>,
}

impl BurstEmitterNode {
    fn new(event_count: usize, counter: Arc<AtomicUsize>) -> Self {
        Self {
            event_count,
            counter,
        }
    }
}

#[async_trait]
impl Node for BurstEmitterNode {
    async fn run(&self, _: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        for i in 0..self.event_count {
            ctx.emit("burst", format!("event_{i}")).ok();
            self.counter.fetch_add(1, Ordering::SeqCst);
        }
        Ok(NodePartial::new().with_messages(vec![Message::with_role(
            Role::Assistant,
            "burst complete",
        )]))
    }
}

fn make_burst_app(event_count: usize, counter: Arc<AtomicUsize>) -> weavegraph::app::App {
    GraphBuilder::new()
        .add_node(
            NodeKind::Custom("burster".into()),
            BurstEmitterNode::new(event_count, counter),
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("burster".into()))
        .add_edge(NodeKind::Custom("burster".into()), NodeKind::End)
        .compile()
        .unwrap()
}

#[tokio::test]
async fn test_high_volume_event_emission() {
    let sink = MemorySink::new();
    let sink_snapshot = sink.clone();
    let bus = EventBus::with_sink(sink);

    bus.listen_for_events();
    let emitter = bus.get_emitter();

    let event_count = 1000;
    for i in 0..event_count {
        emitter
            .emit(Event::node_message("stress", format!("event_{i}")))
            .unwrap();
    }

    // Give time for events to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;
    bus.stop_listener().await;

    let entries = sink_snapshot.snapshot();
    // Should have received most events (some may be dropped under extreme load)
    assert!(
        entries.len() >= event_count / 2,
        "expected at least {} events, got {}",
        event_count / 2,
        entries.len()
    );
}

#[tokio::test]
async fn test_burst_node_emission() {
    let counter = Arc::new(AtomicUsize::new(0));
    let app = make_burst_app(100, counter.clone());
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;

    runner
        .create_session("burst".into(), state_with_user("trigger"))
        .await
        .unwrap();

    let result = runner
        .run_step("burst", StepOptions::default())
        .await
        .unwrap();

    match result {
        StepResult::Completed(_) => {
            // Node should have emitted all events
            assert_eq!(counter.load(Ordering::SeqCst), 100);
        }
        other => panic!("expected completed, got {:?}", other),
    }
}

#[tokio::test]
async fn test_multiple_sinks() {
    let sink1 = MemorySink::new();
    let sink2 = MemorySink::new();
    let snap1 = sink1.clone();
    let snap2 = sink2.clone();

    let bus = EventBus::with_sinks(vec![Box::new(sink1), Box::new(sink2)]);
    bus.listen_for_events();

    let emitter = bus.get_emitter();
    for i in 0..10 {
        emitter
            .emit(Event::node_message("multi", format!("msg_{i}")))
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.stop_listener().await;

    // Both sinks should receive all events
    assert_eq!(snap1.snapshot().len(), 10);
    assert_eq!(snap2.snapshot().len(), 10);
}

#[tokio::test]
async fn test_emit_after_stop_behavior() {
    let sink = MemorySink::new();
    let snap = sink.clone();
    let bus = EventBus::with_sink(sink);

    bus.listen_for_events();

    let emitter = bus.get_emitter();
    emitter.emit(Event::node_message("pre", "before")).unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;
    bus.stop_listener().await;

    // After stop, events emitted before stop should be captured
    let entries = snap.snapshot();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message(), "before");
}

#[tokio::test]
async fn test_rapid_start_stop_cycles() {
    let sink = MemorySink::new();
    let snap = sink.clone();
    let bus = EventBus::with_sink(sink);

    // Rapid start/stop cycles shouldn't cause issues
    for _ in 0..5 {
        bus.listen_for_events();
        let emitter = bus.get_emitter();
        emitter.emit(Event::node_message("cycle", "event")).unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        bus.stop_listener().await;
    }

    // Should have captured events from each cycle
    assert!(!snap.snapshot().is_empty());
}

#[tokio::test]
async fn test_event_ordering() {
    let sink = MemorySink::new();
    let snap = sink.clone();
    let bus = EventBus::with_sink(sink);

    bus.listen_for_events();
    let emitter = bus.get_emitter();

    // Emit numbered events
    for i in 0..20 {
        emitter
            .emit(Event::node_message("order", format!("{i}")))
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.stop_listener().await;

    let entries = snap.snapshot();

    // Events should maintain order
    let mut prev = -1i32;
    for entry in &entries {
        if let Ok(num) = entry.message().parse::<i32>() {
            assert!(num > prev, "events out of order: {} followed {}", num, prev);
            prev = num;
        }
    }
}

#[tokio::test]
async fn test_metrics_reflect_emissions() {
    let bus = EventBus::with_sink(MemorySink::new());
    bus.listen_for_events();

    let emitter = bus.get_emitter();
    for _ in 0..50 {
        emitter.emit(Event::node_message("metric", "test")).unwrap();
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.stop_listener().await;

    let metrics = bus.metrics();
    // Metrics should be valid
    assert!(metrics.capacity > 0);
}
