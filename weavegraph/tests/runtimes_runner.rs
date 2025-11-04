use async_trait::async_trait;
#[cfg(feature = "sqlite")]
use weavegraph::channels::Channel;
use weavegraph::graphs::{EdgePredicate, GraphBuilder};
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{
    AppRunner, CheckpointerType, PausedReason, RuntimeConfig, SessionInit, StepOptions, StepResult,
};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;
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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;

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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
    let initial_state = state_with_user("hello");

    let result = runner
        .create_session("test_session".into(), initial_state)
        .await
        .unwrap();
    assert_eq!(result, SessionInit::Fresh);
    assert!(runner.get_session("test_session").is_some());
}

#[tokio::test]
async fn test_run_step_basic() {
    let app = make_test_app();
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
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
        Ok(NodePartial::new().with_messages(vec![Message::assistant("worker-run")]))
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

    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
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
            Some(CheckpointerType::SQLite),
            Some(db_path.display().to_string()),
        ))
        .compile()
        .unwrap();

    let mut runner1 = AppRunner::new(app.clone(), CheckpointerType::SQLite).await;
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

    let mut runner2 = AppRunner::new(app, CheckpointerType::SQLite).await;
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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;

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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;

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
    let mut runner = AppRunner::new(app, CheckpointerType::InMemory).await;
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
