use super::scheduler::{Scheduler, SchedulerState, StepRunResult};
use crate::event_bus::EventBus;
use crate::node::{Node, NodeError};
use crate::types::NodeKind;
use crate::utils::testing::{
    create_test_snapshot, make_delayed_registry, make_test_registry, FailingNode,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;

#[tokio::test]
async fn test_superstep_propagates_node_error() {
    let sched = Scheduler::new(4);
    let mut state = SchedulerState::default();
    let mut nodes: FxHashMap<NodeKind, Arc<dyn Node>> = FxHashMap::default();
    nodes.insert(
        NodeKind::Other("FAIL".into()),
        Arc::new(FailingNode::default()),
    );
    let frontier = vec![NodeKind::Other("FAIL".into())];
    let snap = create_test_snapshot(1, 1);

    let event_bus = EventBus::default();
    let res = sched
        .superstep(
            &mut state,
            &nodes,
            frontier,
            snap,
            1,
            event_bus.get_sender(),
        )
        .await;
    match res {
        Err(super::scheduler::SchedulerError::NodeRun {
            source: NodeError::MissingInput { what },
            ..
        }) => {
            assert_eq!(what, "test_key");
        }
        other => panic!(
            "expected SchedulerError::NodeRun(MissingInput), got: {:?}",
            other
        ),
    }
}

#[test]
fn test_should_run_and_record_seen() {
    let sched = Scheduler::new(4);
    let mut state = SchedulerState::default();
    let id = "Other(\"A\")"; // format!("{:?}", NodeKind::Other("A".into()))

    // No record -> should run
    let snap1 = create_test_snapshot(1, 1);
    assert!(sched.should_run(&state, id, &snap1));

    // Record seen -> no change -> should not run
    sched.record_seen(&mut state, id, &snap1);
    assert!(!sched.should_run(&state, id, &snap1));

    // Version bump on messages -> should run
    let snap2 = create_test_snapshot(2, 1);
    assert!(sched.should_run(&state, id, &snap2));

    // Record bump, then only extra increases -> should run
    sched.record_seen(&mut state, id, &snap2);
    let snap3 = create_test_snapshot(2, 3);
    assert!(sched.should_run(&state, id, &snap3));
}

#[tokio::test]
async fn test_superstep_skips_end_and_nochange() {
    let sched = Scheduler::new(8);
    let mut state = SchedulerState::default();
    let nodes = make_test_registry();
    let frontier = vec![
        NodeKind::Other("A".into()),
        NodeKind::End,
        NodeKind::Other("B".into()),
    ];
    let event_bus = EventBus::default();

    // First run: nothing recorded, both A and B should run; End skipped.
    let snap = create_test_snapshot(1, 1);
    let res1: StepRunResult = sched
        .superstep(
            &mut state,
            &nodes,
            frontier.clone(),
            snap.clone(),
            1,
            event_bus.get_sender(),
        )
        .await
        .unwrap();

    // All ran except End
    let ran1: std::collections::HashSet<_> = res1.ran_nodes.iter().cloned().collect();
    assert!(ran1.contains(&NodeKind::Other("A".into())));
    assert!(ran1.contains(&NodeKind::Other("B".into())));
    assert!(!ran1.contains(&NodeKind::End));
    assert!(res1.skipped_nodes.contains(&NodeKind::End));
    assert_eq!(res1.outputs.len(), 2);

    // Record_seen happened inside superstep; with same snapshot, nothing should run now.
    let res2 = sched
        .superstep(
            &mut state,
            &nodes,
            frontier.clone(),
            snap.clone(),
            2,
            event_bus.get_sender(),
        )
        .await
        .unwrap();
    assert!(res2.ran_nodes.is_empty());

    // Both A and B plus End appear in skipped (version-gated or End)
    let skipped2: std::collections::HashSet<_> = res2.skipped_nodes.iter().cloned().collect();
    assert!(skipped2.contains(&NodeKind::Other("A".into())));
    assert!(skipped2.contains(&NodeKind::Other("B".into())));
    assert!(skipped2.contains(&NodeKind::End));
    assert!(res2.outputs.is_empty());

    // Increase messages version -> A and B should run again
    let snap_bump = create_test_snapshot(2, 1);
    let res3 = sched
        .superstep(
            &mut state,
            &nodes,
            frontier.clone(),
            snap_bump,
            3,
            event_bus.get_sender(),
        )
        .await
        .unwrap();
    let ran3: std::collections::HashSet<_> = res3.ran_nodes.iter().cloned().collect();
    assert!(ran3.contains(&NodeKind::Other("A".into())));
    assert!(ran3.contains(&NodeKind::Other("B".into())));
    assert_eq!(res3.outputs.len(), 2);
}

#[tokio::test]
async fn test_superstep_outputs_order_agnostic() {
    // Build two nodes with different delays to encourage out-of-order completion.
    let nodes = make_delayed_registry();

    let frontier = vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())];
    let snap = create_test_snapshot(1, 1);
    let sched = Scheduler::new(2);
    let mut state = SchedulerState::default();
    let event_bus = EventBus::default();
    let res = sched
        .superstep(
            &mut state,
            &nodes,
            frontier.clone(),
            snap,
            1,
            event_bus.get_sender(),
        )
        .await
        .unwrap();

    // ran_nodes preserves scheduling order (frontier order, after gating)
    assert_eq!(
        res.ran_nodes,
        vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())]
    );

    // outputs may arrive in any order; validate by ID set, not sequence
    let ids: std::collections::HashSet<_> = res.outputs.iter().map(|(id, _)| id.clone()).collect();
    let expected: std::collections::HashSet<_> =
        [NodeKind::Other("A".into()), NodeKind::Other("B".into())]
            .into_iter()
            .collect();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn test_superstep_serialized_with_limit_1() {
    // Two nodes with different delays, but concurrency limit 1 forces serial execution.
    let nodes = make_delayed_registry();

    let frontier = vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())];
    let snap = create_test_snapshot(1, 1);
    let sched = Scheduler::new(1); // force serial execution
    let mut state = SchedulerState::default();
    let event_bus = EventBus::default();
    let res = sched
        .superstep(
            &mut state,
            &nodes,
            frontier.clone(),
            snap,
            1,
            event_bus.get_sender(),
        )
        .await
        .unwrap();

    // ran_nodes preserves scheduling order
    assert_eq!(
        res.ran_nodes,
        vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())]
    );

    // outputs must be in the same order as ran_nodes
    let output_ids: Vec<_> = res.outputs.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(output_ids, res.ran_nodes);
}
