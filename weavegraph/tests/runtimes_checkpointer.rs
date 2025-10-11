use rustc_hash::FxHashMap;
use weavegraph::channels::Channel;
use weavegraph::runtimes::{Checkpoint, Checkpointer, InMemoryCheckpointer};
use weavegraph::schedulers::SchedulerState;
use weavegraph::state::VersionedState;
use weavegraph::types::NodeKind;

#[tokio::test]
async fn test_save_and_load_roundtrip() {
    let cp_store = InMemoryCheckpointer::new();
    let mut session = weavegraph::runtimes::SessionState {
        state: VersionedState::new_with_user_message("hi"),
        step: 3,
        frontier: vec![NodeKind::Start],
        scheduler: weavegraph::schedulers::Scheduler::new(4),
        scheduler_state: SchedulerState::default(),
    };
    session.scheduler_state.versions_seen.insert(
        "Start".into(),
        FxHashMap::from_iter([("messages".into(), 1_u64), ("extra".into(), 1_u64)]),
    );

    let cp = Checkpoint::from_session("sess1", &session);
    cp_store.save(cp.clone()).await.unwrap();

    let loaded = cp_store.load_latest("sess1").await.unwrap().unwrap();
    assert_eq!(loaded.step, 3);
    assert_eq!(loaded.frontier, vec![NodeKind::Start]);
    assert_eq!(
        loaded.versions_seen.get("Start").unwrap().get("messages"),
        Some(&1)
    );
    assert_eq!(
        loaded.state.messages.snapshot().len(),
        session.state.messages.snapshot().len()
    );
}

#[tokio::test]
async fn test_list_sessions() {
    let cp_store = InMemoryCheckpointer::new();
    let session = weavegraph::runtimes::SessionState {
        state: VersionedState::new_with_user_message("x"),
        step: 0,
        frontier: vec![NodeKind::Start],
        scheduler: weavegraph::schedulers::Scheduler::new(1),
        scheduler_state: SchedulerState::default(),
    };
    cp_store
        .save(Checkpoint::from_session("alpha", &session))
        .await
        .unwrap();
    cp_store
        .save(Checkpoint::from_session("beta", &session))
        .await
        .unwrap();
    let mut ids = cp_store.list_sessions().await.unwrap();
    ids.sort();
    assert_eq!(ids, vec!["alpha", "beta"]);
}
