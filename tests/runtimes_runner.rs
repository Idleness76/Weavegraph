use std::sync::{
    Arc, RwLock,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use weavegraph::channels::Channel;
use weavegraph::event_bus::{
    EventBus, EventStream, INVOCATION_END_SCOPE, MemorySink, STREAM_END_SCOPE,
};
use weavegraph::graphs::{EdgePredicate, GraphBuilder};
use weavegraph::message::{Message, Role};
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{
    AppRunner, Checkpoint, Checkpointer, CheckpointerType, PausedReason, RuntimeConfig,
    SessionInit, SessionState, StepOptions, StepResult,
};
use weavegraph::schedulers::{Scheduler, SchedulerState};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;
use weavegraph::utils::clock::MockClock;
use weavegraph::{FrontierCommand, NodeRoute};

mod common;
use common::*;

// Removed ad-hoc NodeA/NodeB; using common TestNode/FailingNode helpers instead.

fn make_test_app() -> weavegraph::app::App {
    let mut builder = GraphBuilder::new();
    builder = builder.add_node(NodeKind::Custom("test".into()), TestNode { name: "test" });
    builder = builder.add_edge(NodeKind::Start, NodeKind::Custom("test".into()));
    builder = builder.add_edge(NodeKind::Custom("test".into()), NodeKind::End);
    builder.compile().unwrap()
}

#[derive(Default)]
struct ProbeCheckpointer {
    checkpoints: RwLock<std::collections::HashMap<String, Checkpoint>>,
    load_calls: AtomicUsize,
    save_calls: AtomicUsize,
}

impl ProbeCheckpointer {
    fn with_checkpoint(checkpoint: Checkpoint) -> Self {
        let mut checkpoints = std::collections::HashMap::new();
        checkpoints.insert(checkpoint.session_id.clone(), checkpoint);
        Self {
            checkpoints: RwLock::new(checkpoints),
            load_calls: AtomicUsize::new(0),
            save_calls: AtomicUsize::new(0),
        }
    }

    fn load_calls(&self) -> usize {
        self.load_calls.load(Ordering::SeqCst)
    }

    fn save_calls(&self) -> usize {
        self.save_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Checkpointer for ProbeCheckpointer {
    async fn save(&self, checkpoint: Checkpoint) -> weavegraph::runtimes::checkpointer::Result<()> {
        self.save_calls.fetch_add(1, Ordering::SeqCst);
        self.checkpoints
            .write()
            .expect("probe checkpointer lock poisoned")
            .insert(checkpoint.session_id.clone(), checkpoint);
        Ok(())
    }

    async fn load_latest(
        &self,
        session_id: &str,
    ) -> weavegraph::runtimes::checkpointer::Result<Option<Checkpoint>> {
        self.load_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .checkpoints
            .read()
            .expect("probe checkpointer lock poisoned")
            .get(session_id)
            .cloned())
    }

    async fn list_sessions(&self) -> weavegraph::runtimes::checkpointer::Result<Vec<String>> {
        Ok(self
            .checkpoints
            .read()
            .expect("probe checkpointer lock poisoned")
            .keys()
            .cloned()
            .collect())
    }
}

fn checkpoint_from_state(session_id: &str, step: u64, state: VersionedState) -> Checkpoint {
    let session_state = SessionState {
        state,
        step,
        frontier: vec![NodeKind::End],
        scheduler: Scheduler::new(1),
        scheduler_state: SchedulerState::default(),
    };
    Checkpoint::from_session(session_id, &session_state)
}

#[derive(Debug, Clone)]
struct TickAccumulatorNode;

#[async_trait]
impl Node for TickAccumulatorNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let tick = snapshot
            .extra
            .get("tick")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();
        ctx.emit("tick", format!("processed:{tick}"))?;
        let sum = snapshot
            .extra
            .get("sum")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();

        let mut extra = weavegraph::utils::collections::new_extra_map();
        extra.insert("sum".to_string(), serde_json::json!(sum + tick));
        extra.insert("last_tick".to_string(), serde_json::json!(tick));
        extra.insert("last_step".to_string(), serde_json::json!(ctx.step));

        Ok(NodePartial::new().with_extra(extra))
    }
}

fn make_iterative_app() -> weavegraph::app::App {
    GraphBuilder::new()
        .add_node(NodeKind::Custom("accumulate".into()), TickAccumulatorNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("accumulate".into()))
        .add_edge(NodeKind::Custom("accumulate".into()), NodeKind::End)
        .compile()
        .unwrap()
}

fn tick_input(tick: i64) -> NodePartial {
    let mut extra = weavegraph::utils::collections::new_extra_map();
    extra.insert("tick".to_string(), serde_json::json!(tick));
    NodePartial::new().with_extra(extra)
}

async fn recv_matching_event(
    stream: &mut EventStream,
    predicate: impl Fn(&weavegraph::event_bus::Event) -> bool,
) -> weavegraph::event_bus::Event {
    for _ in 0..10 {
        let event = tokio::time::timeout(Duration::from_secs(1), stream.recv())
            .await
            .expect("event stream should receive an event")
            .expect("event stream should stay open");
        if predicate(&event) {
            return event;
        }
    }
    panic!("matching event was not received");
}

#[derive(Debug, Clone)]
struct ClockProbeNode;

#[async_trait]
impl Node for ClockProbeNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("clock", "observed")?;

        let mut extra = weavegraph::utils::collections::new_extra_map();
        extra.insert(
            "now_unix_ms".to_string(),
            serde_json::json!(ctx.now_unix_ms()),
        );
        extra.insert(
            "invocation_id".to_string(),
            serde_json::json!(ctx.invocation_id()),
        );
        Ok(NodePartial::new().with_extra(extra))
    }
}

#[tokio::test]
async fn test_conditional_edge_routing() {
    let pred: EdgePredicate = std::sync::Arc::new(|snap: StateSnapshot| {
        if snap.extra.contains_key("go_yes") {
            vec!["Y".to_string()]
        } else {
            vec!["N".to_string()]
        }
    });
    let gb = GraphBuilder::new()
        .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
        .add_node(NodeKind::Custom("Y".into()), TestNode { name: "yes path" })
        .add_node(NodeKind::Custom("N".into()), TestNode { name: "no path" })
        .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
        .add_edge(NodeKind::Custom("Root".into()), NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::Custom("Y".into()))
        .add_edge(NodeKind::Start, NodeKind::Custom("N".into()))
        .add_edge(NodeKind::Custom("Y".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("N".into()), NodeKind::End)
        .add_conditional_edge(NodeKind::Custom("Root".into()), pred.clone());
    let app = gb.compile().unwrap();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let mut state = state_with_user("hi");
    state
        .extra
        .get_mut()
        .insert("go_yes".to_string(), serde_json::json!(1));
    match runner
        .create_session("sess1".to_string(), state.clone())
        .await
        .unwrap()
    {
        SessionInit::Fresh => {}
        SessionInit::Resumed { .. } => panic!("expected fresh session"),
    }
    let report = runner
        .run_step("sess1", StepOptions::default())
        .await
        .unwrap();
    if let StepResult::Completed(rep) = report {
        assert!(rep.next_frontier.contains(&NodeKind::Custom("Y".into())));
        assert!(!rep.next_frontier.contains(&NodeKind::Custom("N".into())));
    } else {
        panic!("Expected completed step");
    }
    let state2 = state_with_user("hi");
    match runner
        .create_session("sess2".to_string(), state2.clone())
        .await
        .unwrap()
    {
        SessionInit::Fresh => {}
        SessionInit::Resumed { .. } => panic!("expected fresh session"),
    }
    let report2 = runner
        .run_step("sess2", StepOptions::default())
        .await
        .unwrap();
    if let StepResult::Completed(rep2) = report2 {
        assert!(rep2.next_frontier.contains(&NodeKind::Custom("N".into())));
        assert!(!rep2.next_frontier.contains(&NodeKind::Custom("Y".into())));
    } else {
        panic!("Expected completed step");
    }
}

#[tokio::test]
async fn runner_event_stream_only_once() {
    let app = make_test_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;

    let stream = runner
        .event_stream()
        .expect("first event_stream call should succeed");
    drop(stream);

    let result = runner.event_stream();
    assert!(
        result.is_none(),
        "expected None on second event_stream call"
    );
}

#[tokio::test]
async fn test_create_session() {
    let app = make_test_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let initial_state = state_with_user("hello");

    let result = runner
        .create_session("test_session".into(), initial_state)
        .await
        .unwrap();
    assert_eq!(result, SessionInit::Fresh);
    assert!(runner.get_session("test_session").is_some());
}

#[tokio::test]
async fn test_builder_custom_checkpointer_takes_precedence_over_enum() {
    let app = make_test_app();
    let session_id = "builder-custom-precedence";
    let checkpoint = checkpoint_from_state(
        session_id,
        7,
        VersionedState::new_with_user_message("restored-via-custom"),
    );
    let probe = Arc::new(ProbeCheckpointer::with_checkpoint(checkpoint));

    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .checkpointer_custom(probe.clone())
        .build()
        .await;

    let init = runner
        .create_session(session_id.to_string(), state_with_user("ignored"))
        .await
        .expect("create_session should read from custom checkpointer");

    assert_eq!(init, SessionInit::Resumed { checkpoint_step: 7 });
    assert!(probe.load_calls() > 0);
}

#[tokio::test]
async fn test_runtime_config_custom_checkpointer_takes_precedence() {
    let session_id = "runtime-config-custom-precedence";
    let checkpoint = checkpoint_from_state(
        session_id,
        3,
        VersionedState::new_with_user_message("restored-from-runtime-config"),
    );
    let probe = Arc::new(ProbeCheckpointer::with_checkpoint(checkpoint));

    let runtime_config =
        RuntimeConfig::new(Some(session_id.to_string()), None).checkpointer_custom(probe.clone());

    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("test".into()), TestNode { name: "test" })
        .add_edge(NodeKind::Start, NodeKind::Custom("test".into()))
        .add_edge(NodeKind::Custom("test".into()), NodeKind::End)
        .with_runtime_config(runtime_config)
        .compile()
        .expect("app should compile");

    let final_state = app
        .invoke(state_with_user("fresh-state"))
        .await
        .expect("app invoke should succeed");

    assert_message_contains(&final_state, "restored-from-runtime-config");
    assert!(
        probe.load_calls() > 0,
        "custom checkpointer should be invoked"
    );
    assert_eq!(probe.save_calls(), 0);
    assert!(probe.load_calls() > 0);
}

#[tokio::test]
async fn test_run_step_basic() {
    let app = make_test_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let initial_state = state_with_user("hello");

    assert_eq!(
        runner
            .create_session("test_session".into(), initial_state)
            .await
            .unwrap(),
        SessionInit::Fresh
    );

    let result = runner
        .run_step("test_session", StepOptions::default())
        .await;
    assert!(result.is_ok());

    if let Ok(StepResult::Completed(report)) = result {
        assert_eq!(report.step, 1);
        assert_eq!(report.ran_nodes.len(), 1);
        assert!(
            report
                .barrier_outcome
                .updated_channels
                .contains(&"messages")
        );
    } else {
        panic!("Expected completed step, got: {:?}", result);
    }
}

#[tokio::test]
async fn test_run_until_complete() {
    let app = make_test_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let initial_state = VersionedState::new_with_user_message("hello");

    assert_eq!(
        runner
            .create_session("test_session".into(), initial_state)
            .await
            .unwrap(),
        SessionInit::Fresh
    );

    let result = runner.run_until_complete("test_session").await;
    assert!(result.is_ok());

    let final_state = result.unwrap();
    // user + test node message
    assert_eq!(final_state.messages.len(), 2);
    assert_message_contains(&final_state, "ran:test:step:1");
}

#[tokio::test]
async fn test_iterative_invocation_processes_identical_inputs() {
    let app = make_iterative_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;

    let init = runner
        .create_iterative_session(
            "iterative-identical".to_string(),
            state_with_user("start"),
            NodeKind::Start,
        )
        .await
        .unwrap();
    assert_eq!(init, SessionInit::Fresh);

    let first = runner
        .invoke_next("iterative-identical", tick_input(1), NodeKind::Start)
        .await
        .unwrap();
    assert_eq!(
        first.extra.snapshot().get("sum"),
        Some(&serde_json::json!(1))
    );

    let second = runner
        .invoke_next("iterative-identical", tick_input(1), NodeKind::Start)
        .await
        .unwrap();
    let extra = second.extra.snapshot();
    assert_eq!(extra.get("sum"), Some(&serde_json::json!(2)));
    assert_eq!(extra.get("last_step"), Some(&serde_json::json!(2)));

    let session = runner.get_session("iterative-identical").unwrap();
    assert_eq!(session.step, 2);
    assert_eq!(session.frontier, vec![NodeKind::End]);
}

#[tokio::test]
async fn test_iterative_session_rejects_invalid_entry_without_creating_session() {
    let app = make_iterative_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;

    let error = runner
        .create_iterative_session(
            "invalid-entry".to_string(),
            state_with_user("start"),
            NodeKind::End,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        weavegraph::runtimes::runner::RunnerError::InvalidIterativeEntry {
            node: NodeKind::End
        }
    ));
    assert!(runner.get_session("invalid-entry").is_none());
}

#[tokio::test]
async fn test_iterative_invocation_rejects_invalid_entry_without_applying_input() {
    let app = make_iterative_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    runner
        .create_iterative_session(
            "invalid-next".to_string(),
            state_with_user("start"),
            NodeKind::Start,
        )
        .await
        .unwrap();

    let error = runner
        .invoke_next("invalid-next", tick_input(99), NodeKind::End)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        weavegraph::runtimes::runner::RunnerError::InvalidIterativeEntry {
            node: NodeKind::End
        }
    ));
    let session = runner.get_session("invalid-next").unwrap();
    assert_eq!(session.step, 0);
    assert!(!session.state.snapshot().extra.contains_key("tick"));
}

#[tokio::test]
async fn test_iterative_invocation_rejects_unregistered_custom_entry() {
    let app = make_iterative_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    runner
        .create_iterative_session(
            "unknown-custom".to_string(),
            state_with_user("start"),
            NodeKind::Start,
        )
        .await
        .unwrap();

    let unknown = NodeKind::Custom("missing".to_string());
    let error = runner
        .invoke_next("unknown-custom", tick_input(1), unknown.clone())
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        weavegraph::runtimes::runner::RunnerError::InvalidIterativeEntry { node } if node == unknown
    ));
}

#[tokio::test]
async fn test_iterative_custom_entry_runs_from_registered_node() {
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("first".into()), TickAccumulatorNode)
        .add_node(NodeKind::Custom("second".into()), TickAccumulatorNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("first".into()))
        .add_edge(
            NodeKind::Custom("first".into()),
            NodeKind::Custom("second".into()),
        )
        .add_edge(NodeKind::Custom("second".into()), NodeKind::End)
        .compile()
        .unwrap();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    runner
        .create_iterative_session(
            "custom-entry".to_string(),
            state_with_user("start"),
            NodeKind::Custom("second".into()),
        )
        .await
        .unwrap();

    let final_state = runner
        .invoke_next(
            "custom-entry",
            tick_input(5),
            NodeKind::Custom("second".into()),
        )
        .await
        .unwrap();

    assert_eq!(final_state.extra.snapshot().get("sum"), Some(&json!(5)));
    assert_eq!(runner.get_session("custom-entry").unwrap().step, 1);
}

#[tokio::test]
async fn test_iterative_event_stream_stays_open_until_finished() {
    let app = make_iterative_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let mut stream = runner
        .event_stream()
        .expect("iterative subscription should be available");

    runner
        .create_iterative_session(
            "iterative-stream".to_string(),
            state_with_user("start"),
            NodeKind::Start,
        )
        .await
        .unwrap();

    runner
        .invoke_next("iterative-stream", tick_input(1), NodeKind::Start)
        .await
        .unwrap();
    let first_tick = recv_matching_event(&mut stream, |event| {
        event.scope_label() == Some("tick") && event.message() == "processed:1"
    })
    .await;
    assert_eq!(first_tick.scope_label(), Some("tick"));
    let first_end = recv_matching_event(&mut stream, |event| {
        event.scope_label() == Some(INVOCATION_END_SCOPE)
    })
    .await;
    assert!(first_end.message().contains("status=completed"));

    runner
        .invoke_next("iterative-stream", tick_input(2), NodeKind::Start)
        .await
        .unwrap();
    let second_tick = recv_matching_event(&mut stream, |event| {
        event.scope_label() == Some("tick") && event.message() == "processed:2"
    })
    .await;
    assert_eq!(second_tick.scope_label(), Some("tick"));
    let second_end = recv_matching_event(&mut stream, |event| {
        event.scope_label() == Some(INVOCATION_END_SCOPE)
    })
    .await;
    assert!(second_end.message().contains("status=completed"));

    runner.finish_iterative_session("iterative-stream").unwrap();
    let terminal = recv_matching_event(&mut stream, |event| {
        event.scope_label() == Some(STREAM_END_SCOPE)
    })
    .await;
    assert!(terminal.message().contains("status=completed"));

    let closed = tokio::time::timeout(Duration::from_secs(1), stream.recv())
        .await
        .expect("closed stream should resolve promptly");
    assert!(matches!(
        closed,
        Err(tokio::sync::broadcast::error::RecvError::Closed)
    ));
}

#[tokio::test]
async fn test_iterative_event_stream_reports_errors_without_closing_until_finished() {
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("fail".into()), FailingNode::default())
        .add_edge(NodeKind::Start, NodeKind::Custom("fail".into()))
        .add_edge(NodeKind::Custom("fail".into()), NodeKind::End)
        .compile()
        .unwrap();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let mut stream = runner
        .event_stream()
        .expect("event stream should be available");
    runner
        .create_iterative_session(
            "iterative-error-stream".to_string(),
            state_with_user("start"),
            NodeKind::Start,
        )
        .await
        .unwrap();

    let result = runner
        .invoke_next(
            "iterative-error-stream",
            NodePartial::new(),
            NodeKind::Start,
        )
        .await;

    assert!(result.is_err());
    let invocation_end = recv_matching_event(&mut stream, |event| {
        event.scope_label() == Some(INVOCATION_END_SCOPE)
    })
    .await;
    assert!(invocation_end.message().contains("status=error"));

    runner
        .finish_iterative_session("iterative-error-stream")
        .unwrap();
    let terminal = recv_matching_event(&mut stream, |event| {
        event.scope_label() == Some(STREAM_END_SCOPE)
    })
    .await;
    assert!(terminal.message().contains("status=completed"));
}

#[tokio::test]
async fn test_finish_iterative_session_reports_missing_session() {
    let app = make_iterative_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;

    let error = runner
        .finish_iterative_session("missing-session")
        .unwrap_err();

    assert!(matches!(
        error,
        weavegraph::runtimes::runner::RunnerError::SessionNotFound { session_id }
            if session_id == "missing-session"
    ));
}

#[tokio::test]
async fn test_iterative_invocation_resumes_latest_checkpoint() {
    const SESSION_ID: &str = "iterative-resume";

    let mut uninterrupted = AppRunner::builder()
        .app(make_iterative_app())
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    uninterrupted
        .create_iterative_session(
            SESSION_ID.to_string(),
            state_with_user("start"),
            NodeKind::Start,
        )
        .await
        .unwrap();

    let mut uninterrupted_state = state_with_user("unused");
    for tick in 1..=5 {
        uninterrupted_state = uninterrupted
            .invoke_next(SESSION_ID, tick_input(tick), NodeKind::Start)
            .await
            .unwrap();
    }
    let uninterrupted_extra = uninterrupted_state.extra.snapshot();
    let uninterrupted_step = uninterrupted.get_session(SESSION_ID).unwrap().step;

    let probe = Arc::new(ProbeCheckpointer::default());
    let mut before_restart = AppRunner::builder()
        .app(make_iterative_app())
        .checkpointer_custom(probe.clone())
        .build()
        .await;
    before_restart
        .create_iterative_session(
            SESSION_ID.to_string(),
            state_with_user("start"),
            NodeKind::Start,
        )
        .await
        .unwrap();
    for tick in 1..=3 {
        before_restart
            .invoke_next(SESSION_ID, tick_input(tick), NodeKind::Start)
            .await
            .unwrap();
    }
    drop(before_restart);

    let mut after_restart = AppRunner::builder()
        .app(make_iterative_app())
        .checkpointer_custom(probe.clone())
        .build()
        .await;
    let resumed = after_restart
        .create_iterative_session(
            SESSION_ID.to_string(),
            state_with_user("ignored after checkpoint restore"),
            NodeKind::Start,
        )
        .await
        .unwrap();
    assert_eq!(resumed, SessionInit::Resumed { checkpoint_step: 3 });

    let mut resumed_state = state_with_user("unused");
    for tick in 4..=5 {
        resumed_state = after_restart
            .invoke_next(SESSION_ID, tick_input(tick), NodeKind::Start)
            .await
            .unwrap();
    }

    let resumed_extra = resumed_state.extra.snapshot();
    assert_eq!(resumed_extra.get("sum"), uninterrupted_extra.get("sum"));
    assert_eq!(resumed_extra.get("last_tick"), Some(&serde_json::json!(5)));
    assert_eq!(
        after_restart.get_session(SESSION_ID).unwrap().step,
        uninterrupted_step
    );
    assert!(probe.load_calls() > 0);
    assert!(probe.save_calls() > 0);
}

#[tokio::test]
async fn test_runtime_clock_reaches_node_context_and_events() {
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("clock".into()), ClockProbeNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("clock".into()))
        .add_edge(NodeKind::Custom("clock".into()), NodeKind::End)
        .compile()
        .unwrap();
    let event_bus = EventBus::with_sink(MemorySink::new());
    let mut event_stream = event_bus.subscribe();

    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .event_bus(event_bus)
        .clock(Arc::new(MockClock::new(123)))
        .build()
        .await;

    runner
        .create_session("clock-session".to_string(), state_with_user("clock"))
        .await
        .unwrap();
    let final_state = runner.run_until_complete("clock-session").await.unwrap();
    let extra = final_state.extra.snapshot();

    assert_eq!(extra.get("now_unix_ms"), Some(&serde_json::json!(123_000)));
    assert_eq!(
        extra.get("invocation_id"),
        Some(&serde_json::json!("clock-session"))
    );

    let mut node_event = None;
    for _ in 0..5 {
        let event = tokio::time::timeout(Duration::from_secs(1), event_stream.recv())
            .await
            .expect("event stream should receive an event")
            .expect("event stream should stay open");
        if event.scope_label() == Some("clock") {
            node_event = Some(event);
            break;
        }
    }
    let node_event = node_event.expect("clock event should be captured");
    let event_json = node_event.to_json_value();
    assert_eq!(event_json["metadata"]["invocation_id"], "clock-session");
    assert_eq!(event_json["metadata"]["now_unix_ms"], 123_000);
}

#[tokio::test]
async fn test_runner_metadata_reports_graph_runtime_and_backends() {
    let app = make_iterative_app();
    let graph_hash = app.graph_definition_hash();
    let runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .clock(Arc::new(MockClock::new(1)))
        .build()
        .await;

    let metadata = runner.run_metadata();
    assert_eq!(metadata.graph_hash, graph_hash);
    assert!(!metadata.runtime_config_hash.is_empty());
    assert_eq!(metadata.checkpointer_backend, "in-memory");
    assert_eq!(metadata.clock_mode, "configured");
}

#[derive(Debug, Clone)]
struct ReplaceController;

#[async_trait]
impl Node for ReplaceController {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Ok(NodePartial::new().with_frontier_replace(vec![NodeKind::Custom("worker".into())]))
    }
}

#[derive(Debug, Clone)]
struct WorkerNode;

#[async_trait]
impl Node for WorkerNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Ok(NodePartial::new()
            .with_messages(vec![Message::with_role(Role::Assistant, "worker-run")]))
    }
}

#[tokio::test]
async fn test_frontier_command_replace_routes_nodes() {
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("controller".into()), ReplaceController)
        .add_node(NodeKind::Custom("worker".into()), WorkerNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("controller".into()))
        .add_edge(
            NodeKind::Custom("controller".into()),
            NodeKind::Custom("worker".into()),
        )
        .add_edge(NodeKind::Custom("worker".into()), NodeKind::End)
        .compile()
        .unwrap();

    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let initial_state = state_with_user("control");

    runner
        .create_session("frontier-session".into(), initial_state)
        .await
        .expect("create session");

    let first_step = runner
        .run_step("frontier-session", StepOptions::default())
        .await
        .expect("first step");

    match first_step {
        StepResult::Completed(report) => {
            assert_eq!(
                report.ran_nodes,
                vec![NodeKind::Custom("controller".into())]
            );
            assert_eq!(report.barrier_outcome.frontier_commands.len(), 1);
            match &report.barrier_outcome.frontier_commands[0].1 {
                FrontierCommand::Replace(routes) => {
                    let kinds: Vec<NodeKind> = routes.iter().map(NodeRoute::to_node_kind).collect();
                    assert_eq!(kinds.len(), 1);
                    assert_eq!(kinds[0], NodeKind::Custom("worker".into()));
                }
                other => panic!("expected replace command, got {other:?}"),
            }
        }
        other => panic!("expected completed step, got {other:?}"),
    }

    let second_step = runner
        .run_step("frontier-session", StepOptions::default())
        .await
        .expect("second step");

    match second_step {
        StepResult::Completed(report) => {
            assert!(
                report
                    .ran_nodes
                    .contains(&NodeKind::Custom("worker".into()))
            );
        }
        other => panic!("expected completed step, got {other:?}"),
    }
}

#[tokio::test]
async fn test_interrupt_before() {
    let app = make_test_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let initial_state = state_with_user("hello");

    assert_eq!(
        runner
            .create_session("test_session".into(), initial_state)
            .await
            .unwrap(),
        SessionInit::Fresh
    );

    let options = StepOptions {
        interrupt_before: vec![NodeKind::Custom("test".into())],
        ..Default::default()
    };

    let result = runner.run_step("test_session", options).await;
    assert!(result.is_ok());

    if let Ok(StepResult::Paused(paused)) = result {
        assert!(matches!(paused.reason, PausedReason::BeforeNode(_)));
    } else {
        panic!("Expected paused step, got: {:?}", result);
    }
}

#[tokio::test]
async fn test_interrupt_after() {
    let app = make_test_app();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let initial_state = state_with_user("hello");

    assert_eq!(
        runner
            .create_session("test_session".into(), initial_state)
            .await
            .unwrap(),
        SessionInit::Fresh
    );

    let options = StepOptions {
        interrupt_after: vec![NodeKind::Custom("test".into())],
        ..Default::default()
    };

    let result = runner.run_step("test_session", options).await;
    assert!(result.is_ok());

    if let Ok(StepResult::Paused(paused)) = result {
        assert!(matches!(paused.reason, PausedReason::AfterNode(_)));
    } else {
        panic!("Expected paused step, got: {:?}", result);
    }
}

#[tokio::test]
async fn test_resume_from_checkpoint() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test_resume.db");

    // Build the app with a runtime config that points SQLite to our temp path,
    // avoiding any process-wide environment mutation.
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("test".into()), TestNode { name: "test" })
        .add_edge(NodeKind::Start, NodeKind::Custom("test".into()))
        .add_edge(NodeKind::Custom("test".into()), NodeKind::End)
        .with_runtime_config(RuntimeConfig::new(
            None,
            Some(db_path.display().to_string()),
        ))
        .compile()
        .unwrap();

    let mut runner1 = AppRunner::builder()
        .app(app.clone())
        .checkpointer(CheckpointerType::SQLite)
        .build()
        .await;
    let initial_state = state_with_user("hello from checkpoint test");

    let session_id = "checkpoint_test_session";
    assert_eq!(
        runner1
            .create_session(session_id.into(), initial_state.clone())
            .await
            .unwrap(),
        SessionInit::Fresh
    );

    let step1_result = runner1
        .run_step(session_id, StepOptions::default())
        .await
        .unwrap();
    if let StepResult::Completed(report) = step1_result {
        assert_eq!(report.step, 1);
        assert!(!report.ran_nodes.is_empty());
    } else {
        panic!("Expected completed step");
    }

    let session_after_step1 = runner1.get_session(session_id).unwrap().clone();
    assert_eq!(session_after_step1.step, 1);
    drop(runner1);

    let mut runner2 = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::SQLite)
        .build()
        .await;
    let resume_result = runner2
        .create_session(session_id.into(), initial_state)
        .await
        .unwrap();
    assert!(matches!(
        resume_result,
        SessionInit::Resumed { checkpoint_step: 1 }
    ));
    let resumed_session = runner2.get_session(session_id).unwrap();
    assert_eq!(resumed_session.step, session_after_step1.step);
    assert_eq!(resumed_session.frontier, session_after_step1.frontier);
    assert_eq!(
        resumed_session.state.messages.len(),
        session_after_step1.state.messages.len()
    );

    // No environment cleanup necessary; the DB URL was provided via runtime config.
}

#[tokio::test]
async fn test_multi_target_conditional_edge() {
    let multi_pred: EdgePredicate = std::sync::Arc::new(|snap: StateSnapshot| {
        if snap.extra.contains_key("fan_out") {
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        } else {
            vec!["Single".to_string()]
        }
    });

    let gb = GraphBuilder::new()
        .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
        .add_node(NodeKind::Custom("A".into()), TestNode { name: "A" })
        .add_node(NodeKind::Custom("B".into()), TestNode { name: "B" })
        .add_node(NodeKind::Custom("C".into()), TestNode { name: "C" })
        .add_node(
            NodeKind::Custom("Single".into()),
            TestNode { name: "single" },
        )
        .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
        .add_edge(NodeKind::Custom("A".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("B".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("C".into()), NodeKind::End)
        .add_edge(NodeKind::Custom("Single".into()), NodeKind::End)
        .add_conditional_edge(NodeKind::Custom("Root".into()), multi_pred);

    let app = gb.compile().unwrap();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;

    let mut state = state_with_user("test");
    state
        .extra
        .get_mut()
        .insert("fan_out".to_string(), serde_json::json!(true));
    runner
        .create_session("multi_test".to_string(), state)
        .await
        .unwrap();

    let step1 = runner
        .run_step("multi_test", StepOptions::default())
        .await
        .unwrap();
    if let StepResult::Completed(report) = step1 {
        assert_eq!(report.ran_nodes, vec![NodeKind::Custom("Root".into())]);
        assert_eq!(report.next_frontier.len(), 3);
        assert!(report.next_frontier.contains(&NodeKind::Custom("A".into())));
        assert!(report.next_frontier.contains(&NodeKind::Custom("B".into())));
        assert!(report.next_frontier.contains(&NodeKind::Custom("C".into())));
    } else {
        panic!("Expected completed step");
    }

    let state2 = state_with_user("test2");
    runner
        .create_session("single_test".to_string(), state2)
        .await
        .unwrap();
    let step2 = runner
        .run_step("single_test", StepOptions::default())
        .await
        .unwrap();
    if let StepResult::Completed(report) = step2 {
        assert_eq!(
            report.next_frontier,
            vec![NodeKind::Custom("Single".into())]
        );
    } else {
        panic!("Expected completed step");
    }
}

#[tokio::test]
async fn test_conditional_edge_with_invalid_targets() {
    let mixed_pred: EdgePredicate = std::sync::Arc::new(|_snap: StateSnapshot| {
        vec![
            "Valid".to_string(),
            "Invalid".to_string(),
            "End".to_string(),
        ]
    });

    let gb = GraphBuilder::new()
        .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
        .add_node(NodeKind::Custom("Valid".into()), TestNode { name: "valid" })
        .add_edge(
            NodeKind::Custom("Root".into()),
            NodeKind::Custom("Valid".into()),
        )
        .add_edge(NodeKind::Custom("Valid".into()), NodeKind::End)
        .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
        .add_conditional_edge(NodeKind::Custom("Root".into()), mixed_pred);

    let app = gb.compile().unwrap();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;

    let state = state_with_user("test");
    runner
        .create_session("mixed_test".to_string(), state)
        .await
        .unwrap();

    let step = runner
        .run_step("mixed_test", StepOptions::default())
        .await
        .unwrap();
    if let StepResult::Completed(report) = step {
        assert_eq!(report.next_frontier.len(), 2);
        assert!(
            report
                .next_frontier
                .contains(&NodeKind::Custom("Valid".into()))
        );
        assert!(report.next_frontier.contains(&NodeKind::End));
        assert!(
            !report
                .next_frontier
                .contains(&NodeKind::Custom("Invalid".into()))
        );
    } else {
        panic!("Expected completed step");
    }
}

#[tokio::test]
async fn test_error_event_appended_on_failure() {
    let mut gb = GraphBuilder::new();
    gb = gb.add_node(NodeKind::Custom("X".into()), FailingNode::default());
    gb = gb.add_edge(NodeKind::Start, NodeKind::Custom("X".into()));
    gb = gb.add_edge(NodeKind::Custom("X".into()), NodeKind::End);

    let app = gb.compile().unwrap();
    let mut runner = AppRunner::builder()
        .app(app)
        .checkpointer(CheckpointerType::InMemory)
        .build()
        .await;
    let initial_state = state_with_user("hello");

    assert!(matches!(
        runner
            .create_session("err_sess".into(), initial_state)
            .await
            .unwrap(),
        SessionInit::Fresh
    ));

    let res = runner.run_step("err_sess", StepOptions::default()).await;
    assert!(res.is_err());

    let sess = runner.get_session("err_sess").unwrap();
    let errors_snapshot = sess.state.errors.snapshot();
    assert!(
        !errors_snapshot.is_empty(),
        "expected errors to be present in errors channel"
    );

    let error_event = &errors_snapshot[0];
    assert!(matches!(
        error_event.scope,
        weavegraph::channels::errors::ErrorScope::Node { .. }
    ));
    if let weavegraph::channels::errors::ErrorScope::Node { kind, step } = &error_event.scope {
        assert_eq!(kind, "Custom:X");
        assert_eq!(step, &1);
    }
}
