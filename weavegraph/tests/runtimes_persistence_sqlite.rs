#[cfg(feature = "sqlite")]
use chrono::Utc;
use rustc_hash::FxHashMap;
use weavegraph::channels::errors::{ErrorEvent, ErrorScope, LadderError};
use weavegraph::channels::Channel;
use weavegraph::runtimes::{Checkpoint, Checkpointer, SQLiteCheckpointer, StepQuery};
use weavegraph::types::NodeKind;

mod common;
use common::*;

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

    cp.save(cp_struct.clone()).await.expect("save");

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
    assert_extra_has(&loaded.state, "k");
    assert_eq!(
        loaded.state.extra.snapshot().get("k"),
        Some(&serde_json::json!(42))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_list_sessions_and_empty_load() {
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

    let cp2 = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect");
    let res = cp2.load_latest("nope").await.unwrap();
    assert!(res.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_step_execution_metadata_query_and_pagination() {
    let cp = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect");

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

    let result = cp
        .query_steps(
            "paginate_session",
            StepQuery {
                limit: Some(2),
                offset: Some(0),
                ..Default::default()
            },
        )
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_error_persistence_roundtrip() {
    let cp = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("connect");
    let mut state = state_with_user("err");
    let err = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::App,
        error: LadderError::msg("boom"),
        tags: vec!["t".into()],
        context: serde_json::json!({"a":1}),
    };
    state.errors.get_mut().push(err.clone());
    let checkpoint = Checkpoint {
        session_id: "err_sess".into(),
        step: 1,
        state,
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 1,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![],
        updated_channels: vec!["errors".into()],
    };
    cp.save(checkpoint).await.unwrap();
    let loaded = cp.load_latest("err_sess").await.unwrap().unwrap();
    let errors = loaded.state.errors.snapshot();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].error.message, "boom");
}
