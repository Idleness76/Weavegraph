use chrono::Utc;
use rustc_hash::FxHashMap;
use weavegraph::channels::Channel;
use weavegraph::runtimes::checkpointer::{
    restore_session_state, Checkpoint, Checkpointer, InMemoryCheckpointer,
};
use weavegraph::runtimes::checkpointer_sqlite::{SQLiteCheckpointer, StepQuery};
use weavegraph::runtimes::runner::SessionState;
use weavegraph::schedulers::{Scheduler, SchedulerState};
use weavegraph::state::VersionedState;
use weavegraph::types::NodeKind;

mod common;
use common::*;

// Base Checkpointer trait tests
#[tokio::test]
async fn test_inmemory_checkpointer_save_and_load_roundtrip() {
    let cp_store = InMemoryCheckpointer::new();
    let mut session = SessionState {
        state: state_with_user("hi"),
        step: 3,
        frontier: vec![NodeKind::Start],
        scheduler: Scheduler::new(4),
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
async fn test_inmemory_checkpointer_list_sessions() {
    let cp_store = InMemoryCheckpointer::new();
    let session = SessionState {
        state: state_with_user("x"),
        step: 0,
        frontier: vec![NodeKind::Start],
        scheduler: Scheduler::new(1),
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

// SQLite Checkpointer tests
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sqlite_checkpointer_roundtrip() {
    let cp = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect sqlite memory");
    let mut state = state_with_user("hello");
    state
        .extra
        .get_mut()
        .insert("k".into(), serde_json::json!(42));

    let mut versions_seen: FxHashMap<String, FxHashMap<String, u64>> = FxHashMap::default();
    versions_seen.insert(
        "Start".into(),
        FxHashMap::from_iter([("messages".into(), 1_u64), ("extra".into(), 1_u64)]),
    );

    let cp_struct = Checkpoint {
        session_id: "sessX".into(),
        step: 1,
        state: state.clone(),
        frontier: vec![NodeKind::End],
        versions_seen: versions_seen.clone(),
        concurrency_limit: 4,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![],
        updated_channels: vec!["messages".to_string()],
    };

    // Save (async trait method)
    cp.save(cp_struct.clone()).await.expect("save");

    // Load
    let loaded = cp
        .load_latest("sessX")
        .await
        .expect("load_latest")
        .expect("Some checkpoint");
    assert_eq!(loaded.step, 1);
    assert_eq!(loaded.frontier, vec![NodeKind::End]);
    assert_eq!(
        loaded
            .versions_seen
            .get("Start")
            .and_then(|m| m.get("messages"))
            .copied(),
        Some(1)
    );
    assert_eq!(loaded.state.messages.snapshot()[0].role, "user");
    assert_eq!(
        loaded.state.extra.snapshot().get("k"),
        Some(&serde_json::json!(42))
    );

    // Restore session state utility compatibility
    let session_state = restore_session_state(&loaded);
    assert_eq!(session_state.step, 1);
    assert_eq!(session_state.frontier.len(), 1);
    assert_eq!(session_state.scheduler.concurrency_limit, 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sqlite_list_sessions() {
    let cp = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect");
    for i in 0..3 {
        let s_id = format!("s{i}");
        let state = state_with_user("x");
        let cp_struct = Checkpoint {
            session_id: s_id.clone(),
            step: 1,
            state: state.clone(),
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 1,
            created_at: Utc::now(),
            ran_nodes: vec![],
            skipped_nodes: vec![NodeKind::End],
            updated_channels: vec![],
        };
        cp.save(cp_struct).await.unwrap();
    }
    let mut sessions = cp.list_sessions().await.unwrap();
    sessions.sort();
    assert_eq!(sessions, vec!["s0", "s1", "s2"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sqlite_load_nonexistent() {
    let cp = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect");
    let res = cp.load_latest("nope").await.unwrap();
    assert!(res.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sqlite_step_execution_metadata() {
    let cp = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect sqlite memory");

    let state = state_with_user("test");
    let checkpoint = Checkpoint {
        session_id: "test_session".into(),
        step: 1,
        state,
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 2,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![NodeKind::End],
        updated_channels: vec!["messages".to_string()],
    };

    cp.save(checkpoint.clone()).await.expect("save checkpoint");

    // Query the step to verify execution metadata is preserved
    let query = StepQuery {
        limit: Some(10),
        ..Default::default()
    };

    let result = cp
        .query_steps("test_session", query)
        .await
        .expect("query steps");
    assert_eq!(result.checkpoints.len(), 1);

    let loaded = &result.checkpoints[0];
    assert_eq!(loaded.ran_nodes, vec![NodeKind::Start]);
    assert_eq!(loaded.skipped_nodes, vec![NodeKind::End]);
    assert_eq!(loaded.updated_channels, vec!["messages".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sqlite_query_steps_pagination() {
    let cp = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect sqlite memory");

    // Create multiple checkpoints
    for step in 1..=5 {
        let state = state_with_user(&format!("step {step}"));
        let checkpoint = Checkpoint {
            session_id: "paginate_session".into(),
            step,
            state,
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 1,
            created_at: Utc::now(),
            ran_nodes: if step % 2 == 0 {
                vec![NodeKind::Start]
            } else {
                vec![]
            },
            skipped_nodes: vec![NodeKind::End],
            updated_channels: vec!["messages".to_string()],
        };
        cp.save(checkpoint).await.expect("save checkpoint");
    }

    // Test pagination with limit
    let query = StepQuery {
        limit: Some(2),
        offset: Some(0),
        ..Default::default()
    };

    let result = cp
        .query_steps("paginate_session", query)
        .await
        .expect("query steps");
    assert_eq!(result.page_info.total_count, 5);
    assert_eq!(result.page_info.page_size, 2);
    assert_eq!(result.page_info.offset, 0);
    assert!(result.page_info.has_next_page);
    assert_eq!(result.checkpoints.len(), 2);
    assert_eq!(result.checkpoints[0].step, 5);
    assert_eq!(result.checkpoints[1].step, 4);
}
