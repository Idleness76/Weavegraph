//! PostgreSQL checkpointer integration tests.
//!
//! These tests require a running PostgreSQL instance. Set the environment variable
//! `WEAVEGRAPH_POSTGRES_TEST_URL` to point to your test database, e.g.:
//!
//! ```bash
//! export WEAVEGRAPH_POSTGRES_TEST_URL="postgresql://weavegraph:weavegraph@localhost/weavegraph_test"
//! docker-compose up -d postgres
//! cargo test --features postgres-migrations runtimes_persistence_postgres
//! ```
//!
//! Each test uses transaction-based isolation with rollback to ensure test independence.

#![cfg(feature = "postgres")]

use chrono::Utc;
use rustc_hash::FxHashMap;
use weavegraph::channels::Channel;
use weavegraph::channels::errors::{ErrorEvent, LadderError};
use weavegraph::runtimes::{Checkpoint, Checkpointer, PostgresCheckpointer, PgStepQuery};
use weavegraph::types::NodeKind;

mod common;
use common::*;

/// Get the test database URL from environment or use default docker-compose URL.
fn get_test_db_url() -> String {
    std::env::var("WEAVEGRAPH_POSTGRES_TEST_URL")
        .unwrap_or_else(|_| "postgresql://weavegraph:weavegraph@localhost/weavegraph_test".into())
}

/// Helper to generate unique session IDs for test isolation.
fn unique_session_id(prefix: &str) -> String {
    format!("{}_{}", prefix, uuid::Uuid::new_v4())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_postgres_checkpointer_roundtrip() {
    let db_url = get_test_db_url();
    let cp = match PostgresCheckpointer::connect(&db_url).await {
        Ok(cp) => cp,
        Err(e) => {
            eprintln!("Skipping test - cannot connect to Postgres: {e}");
            eprintln!("Start Postgres with: docker-compose up -d postgres");
            return;
        }
    };

    let session_id = unique_session_id("roundtrip");
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
        session_id: session_id.clone(),
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
        .load_latest(&session_id)
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
    let db_url = get_test_db_url();
    let cp = match PostgresCheckpointer::connect(&db_url).await {
        Ok(cp) => cp,
        Err(e) => {
            eprintln!("Skipping test - cannot connect to Postgres: {e}");
            return;
        }
    };

    // Use unique session IDs for this test
    let prefix = format!("list_test_{}", uuid::Uuid::new_v4());
    let session_ids: Vec<String> = (0..3).map(|i| format!("{prefix}_s{i}")).collect();

    for s_id in &session_ids {
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

    let all_sessions = cp.list_sessions().await.unwrap();
    // Check that our test sessions are in the list
    for s_id in &session_ids {
        assert!(
            all_sessions.contains(s_id),
            "Session {s_id} should be in list"
        );
    }

    // Test loading nonexistent session
    let nonexistent = format!("nonexistent_{}", uuid::Uuid::new_v4());
    let res = cp.load_latest(&nonexistent).await.unwrap();
    assert!(res.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_step_execution_metadata_query_and_pagination() {
    let db_url = get_test_db_url();
    let cp = match PostgresCheckpointer::connect(&db_url).await {
        Ok(cp) => cp,
        Err(e) => {
            eprintln!("Skipping test - cannot connect to Postgres: {e}");
            return;
        }
    };

    let session_id = unique_session_id("paginate");

    for step in 1..=5 {
        let state = state_with_user(&format!("step {step}"));
        let checkpoint = Checkpoint {
            session_id: session_id.clone(),
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
            &session_id,
            PgStepQuery {
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
    let db_url = get_test_db_url();
    let cp = match PostgresCheckpointer::connect(&db_url).await {
        Ok(cp) => cp,
        Err(e) => {
            eprintln!("Skipping test - cannot connect to Postgres: {e}");
            return;
        }
    };

    let session_id = unique_session_id("err");
    let mut state = state_with_user("err");

    let err = ErrorEvent::app(LadderError::msg("boom"))
        .with_tag("t")
        .with_context(serde_json::json!({"a":1}));

    state.errors.get_mut().push(err.clone());
    let checkpoint = Checkpoint {
        session_id: session_id.clone(),
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
    let loaded = cp.load_latest(&session_id).await.unwrap().unwrap();
    let errors = loaded.state.errors.snapshot();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].error.message, "boom");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_idempotent_save() {
    let db_url = get_test_db_url();
    let cp = match PostgresCheckpointer::connect(&db_url).await {
        Ok(cp) => cp,
        Err(e) => {
            eprintln!("Skipping test - cannot connect to Postgres: {e}");
            return;
        }
    };

    let session_id = unique_session_id("idempotent");
    let state = state_with_user("test");

    let checkpoint = Checkpoint {
        session_id: session_id.clone(),
        step: 1,
        state: state.clone(),
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 1,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![],
        updated_channels: vec![],
    };

    // Save the same checkpoint twice - should not fail (upsert behavior)
    cp.save(checkpoint.clone()).await.expect("first save");
    cp.save(checkpoint).await.expect("second save (idempotent)");

    let loaded = cp.load_latest(&session_id).await.unwrap().unwrap();
    assert_eq!(loaded.step, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_concurrency_check() {
    let db_url = get_test_db_url();
    let cp = match PostgresCheckpointer::connect(&db_url).await {
        Ok(cp) => cp,
        Err(e) => {
            eprintln!("Skipping test - cannot connect to Postgres: {e}");
            return;
        }
    };

    let session_id = unique_session_id("concurrency");
    let state = state_with_user("test");

    // Save step 1
    let checkpoint1 = Checkpoint {
        session_id: session_id.clone(),
        step: 1,
        state: state.clone(),
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 1,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![],
        updated_channels: vec![],
    };
    cp.save(checkpoint1).await.expect("save step 1");

    // Try to save step 2 with correct expected_last_step
    let checkpoint2 = Checkpoint {
        session_id: session_id.clone(),
        step: 2,
        state: state.clone(),
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 1,
        created_at: Utc::now(),
        ran_nodes: vec![],
        skipped_nodes: vec![],
        updated_channels: vec![],
    };
    cp.save_with_concurrency_check(checkpoint2.clone(), Some(1))
        .await
        .expect("save step 2 with correct check");

    // Try to save step 3 with wrong expected_last_step (should fail)
    let checkpoint3 = Checkpoint {
        session_id: session_id.clone(),
        step: 3,
        state,
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 1,
        created_at: Utc::now(),
        ran_nodes: vec![],
        skipped_nodes: vec![],
        updated_channels: vec![],
    };
    let result = cp
        .save_with_concurrency_check(checkpoint3, Some(1))
        .await;
    assert!(result.is_err(), "should fail with wrong expected step");
}
