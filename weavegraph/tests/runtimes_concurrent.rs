//! Tests for multi-session concurrent execution.
//!
//! Validates that the runtime can handle multiple sessions with the same runner,
//! session isolation, and correct state management across sessions.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rustc_hash::FxHashMap;

use weavegraph::graphs::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{AppRunner, CheckpointerType, StepOptions, StepResult};
use weavegraph::state::StateSnapshot;
use weavegraph::types::NodeKind;

use common::*;

/// A node that increments a counter and optionally adds a delay.
struct CountingNode {
    counter: Arc<AtomicUsize>,
    delay_ms: u64,
}

impl CountingNode {
    fn new(counter: Arc<AtomicUsize>, delay_ms: u64) -> Self {
        Self { counter, delay_ms }
    }
}

#[async_trait]
impl Node for CountingNode {
    async fn run(&self, _: StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }
        self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(NodePartial::new().with_messages(vec![Message::assistant("counted")]))
    }
}

/// A node that marks the session with a specific marker value.
struct SessionMarkerNode {
    marker: String,
}

impl SessionMarkerNode {
    fn new(marker: impl Into<String>) -> Self {
        Self {
            marker: marker.into(),
        }
    }
}

#[async_trait]
impl Node for SessionMarkerNode {
    async fn run(&self, _: StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
        let mut extra = FxHashMap::default();
        extra.insert("marker".to_string(), serde_json::json!(self.marker.clone()));
        Ok(NodePartial::new()
            .with_messages(vec![Message::assistant(&self.marker)])
            .with_extra(extra))
    }
}

fn make_counting_app(counter: Arc<AtomicUsize>, delay_ms: u64) -> weavegraph::app::App {
    GraphBuilder::new()
        .add_node(
            NodeKind::Custom("counter".into()),
            CountingNode::new(counter, delay_ms),
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("counter".into()))
        .add_edge(NodeKind::Custom("counter".into()), NodeKind::End)
        .compile()
        .unwrap()
}

fn make_marker_app(marker: &str) -> weavegraph::app::App {
    GraphBuilder::new()
        .add_node(
            NodeKind::Custom("marker".into()),
            SessionMarkerNode::new(marker),
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("marker".into()))
        .add_edge(NodeKind::Custom("marker".into()), NodeKind::End)
        .compile()
        .unwrap()
}

#[tokio::test]
async fn test_multiple_sessions_same_runner() {
    let counter = Arc::new(AtomicUsize::new(0));
    let app = make_counting_app(counter.clone(), 0);
    let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;

    let session_count = 5;

    // Create multiple sessions
    for i in 0..session_count {
        let session_id = format!("session_{i}");
        let state = state_with_user(&format!("message {i}"));
        runner
            .create_session(session_id, state)
            .await
            .expect("session creation");
    }

    // Run steps on each session sequentially
    for i in 0..session_count {
        let session_id = format!("session_{i}");
        let result = runner.run_step(&session_id, StepOptions::default()).await;
        match result.unwrap() {
            StepResult::Completed(_) => {}
            other => panic!("expected completed, got {:?}", other),
        }
    }

    // Counter should reflect all executions
    assert_eq!(counter.load(Ordering::SeqCst), session_count);
}

#[tokio::test]
async fn test_session_isolation() {
    // Each session gets its own marker to verify state isolation
    let app1 = make_marker_app("session_A_marker");
    let app2 = make_marker_app("session_B_marker");

    let mut runner1 = AppRunner::builder().app(app1).checkpointer(CheckpointerType::InMemory).build().await;
    let mut runner2 = AppRunner::builder().app(app2).checkpointer(CheckpointerType::InMemory).build().await;

    // Create and run both sessions
    runner1
        .create_session("session_A".into(), state_with_user("A"))
        .await
        .unwrap();
    runner2
        .create_session("session_B".into(), state_with_user("B"))
        .await
        .unwrap();

    let result_a = runner1
        .run_step("session_A", StepOptions::default())
        .await
        .unwrap();
    let result_b = runner2
        .run_step("session_B", StepOptions::default())
        .await
        .unwrap();

    // Verify both completed
    match (result_a, result_b) {
        (StepResult::Completed(rep_a), StepResult::Completed(rep_b)) => {
            // Both should have run their marker nodes
            assert!(rep_a.ran_nodes.contains(&NodeKind::Custom("marker".into())));
            assert!(rep_b.ran_nodes.contains(&NodeKind::Custom("marker".into())));
        }
        other => panic!("expected both completed, got {:?}", other),
    }
}

#[tokio::test]
async fn test_session_state_independence() {
    // Create a single app but multiple sessions to verify state is isolated
    let counter = Arc::new(AtomicUsize::new(0));
    let app = make_counting_app(counter.clone(), 0);
    let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;

    // Create sessions with different initial states
    runner
        .create_session("session_1".into(), state_with_user("initial_1"))
        .await
        .unwrap();
    runner
        .create_session("session_2".into(), state_with_user("initial_2"))
        .await
        .unwrap();

    // Run session 1 - the graph completes in one step (Start -> counter -> End)
    let result1 = runner
        .run_step("session_1", StepOptions::default())
        .await
        .unwrap();

    // Run session 2 once
    let result2 = runner
        .run_step("session_2", StepOptions::default())
        .await
        .unwrap();

    // Both sessions should complete
    assert!(matches!(result1, StepResult::Completed(_)));
    assert!(matches!(result2, StepResult::Completed(_)));

    // Counter should reflect 2 executions (one per session)
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_high_session_count() {
    let counter = Arc::new(AtomicUsize::new(0));
    let app = make_counting_app(counter.clone(), 1); // 1ms delay
    let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;

    let session_count = 50;

    // Create all sessions
    for i in 0..session_count {
        runner
            .create_session(format!("stress_{i}"), state_with_user(&format!("{i}")))
            .await
            .unwrap();
    }

    // Run all sessions sequentially
    let mut success_count = 0;
    for i in 0..session_count {
        let result = runner
            .run_step(&format!("stress_{i}"), StepOptions::default())
            .await;
        if matches!(result, Ok(StepResult::Completed(_))) {
            success_count += 1;
        }
    }

    assert_eq!(success_count, session_count);
    assert_eq!(counter.load(Ordering::SeqCst), session_count);
}

#[tokio::test]
async fn test_session_resume_different_order() {
    let counter = Arc::new(AtomicUsize::new(0));
    let app = make_counting_app(counter.clone(), 0);
    let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;

    // Create sessions
    runner
        .create_session("first".into(), state_with_user("1"))
        .await
        .unwrap();
    runner
        .create_session("second".into(), state_with_user("2"))
        .await
        .unwrap();
    runner
        .create_session("third".into(), state_with_user("3"))
        .await
        .unwrap();

    // Run in reverse order
    runner
        .run_step("third", StepOptions::default())
        .await
        .unwrap();
    runner
        .run_step("second", StepOptions::default())
        .await
        .unwrap();
    runner
        .run_step("first", StepOptions::default())
        .await
        .unwrap();

    // All three should have executed
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}
