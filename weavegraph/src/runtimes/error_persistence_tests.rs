/*!
Comprehensive error persistence tests for SQLite checkpointer.

This module verifies that error events are properly persisted through the
SQLite checkpointer and can be restored correctly, including complex error
chains, metadata, and integration with the new pagination and concurrency
control features.
*/

use crate::channels::errors::{ErrorEvent, ErrorScope, LadderError};
use crate::channels::Channel;
use crate::runtimes::checkpointer::{Checkpoint, Checkpointer};
use crate::runtimes::checkpointer_sqlite::{SQLiteCheckpointer, StepQuery};
use crate::runtimes::runner::{SessionState, StateVersions, StepReport};
use crate::schedulers::{Scheduler, SchedulerState};
use crate::state::VersionedState;
use crate::types::NodeKind;
use chrono::Utc;
use rustc_hash::FxHashMap;

/// Test basic error persistence roundtrip with the enhanced checkpoint structure
#[tokio::test]
async fn test_error_persistence_basic_roundtrip() {
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    let mut state = VersionedState::new_with_user_message("test persistence");

    // Create a test error event
    let error_event = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Node {
            kind: "test_node".to_string(),
            step: 42,
        },
        error: LadderError::msg("Node processing failed"),
        tags: vec!["critical".to_string()],
        context: serde_json::json!({"input_data": "corrupted"}),
    };

    state.errors.get_mut().push(error_event.clone());

    // Create checkpoint with enhanced structure
    let checkpoint = Checkpoint {
        session_id: "error_test_session".to_string(),
        step: 1,
        state,
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 2,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![],
        updated_channels: vec!["messages".to_string(), "errors".to_string()],
    };

    // Save checkpoint
    checkpointer
        .save(checkpoint)
        .await
        .expect("Failed to save checkpoint");

    // Load checkpoint
    let loaded = checkpointer
        .load_latest("error_test_session")
        .await
        .expect("Failed to load checkpoint")
        .expect("Checkpoint not found");

    // Verify error was preserved
    let errors = loaded.state.errors.snapshot();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].error.message, "Node processing failed");
    assert_eq!(errors[0].context["input_data"], "corrupted");
}

/// Test error persistence with step report integration
#[tokio::test]
async fn test_error_persistence_with_step_report() {
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    let mut state = VersionedState::new_with_user_message("step report test");

    // Create errors that would be generated during step execution
    let execution_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Scheduler { step: 5 },
        error: LadderError::msg("Concurrency limit exceeded during execution"),
        tags: vec!["scheduler".to_string(), "limit".to_string()],
        context: serde_json::json!({"active_tasks": 10, "limit": 8}),
    };

    let node_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Node {
            kind: "ProcessingNode".to_string(),
            step: 5,
        },
        error: LadderError::msg("Data validation failed")
            .with_cause(LadderError::msg("Invalid JSON structure")),
        tags: vec!["validation".to_string(), "retryable".to_string()],
        context: serde_json::json!({"field": "user_data", "line": 42}),
    };

    state
        .errors
        .get_mut()
        .extend(vec![execution_error, node_error]);

    // Create session state
    let session_state = SessionState {
        state,
        step: 5,
        frontier: vec![NodeKind::Other("ProcessingNode".to_string())],
        scheduler: Scheduler::new(8),
        scheduler_state: SchedulerState::default(),
    };

    // Create step report (simulating what would come from actual execution)
    let step_report = StepReport {
        step: 5,
        ran_nodes: vec![
            NodeKind::Start,
            NodeKind::Other("ProcessingNode".to_string()),
        ],
        skipped_nodes: vec![NodeKind::End],
        updated_channels: vec!["messages", "errors", "extra"],
        next_frontier: vec![NodeKind::Other("ProcessingNode".to_string())],
        state_versions: StateVersions {
            messages_version: 2,
            extra_version: 1,
        },
        completed: false,
    };

    // Create checkpoint from step report
    let checkpoint =
        Checkpoint::from_step_report("step_report_session", &session_state, &step_report);

    // Verify checkpoint includes execution metadata
    assert_eq!(checkpoint.ran_nodes.len(), 2);
    assert_eq!(checkpoint.skipped_nodes, vec![NodeKind::End]);
    assert_eq!(checkpoint.updated_channels.len(), 3);

    // Save checkpoint
    checkpointer
        .save(checkpoint)
        .await
        .expect("Failed to save checkpoint");

    // Use query_steps to get full checkpoint with execution metadata
    let query = StepQuery {
        limit: Some(10),
        ..Default::default()
    };

    let result = checkpointer
        .query_steps("step_report_session", query)
        .await
        .expect("Failed to query steps");

    assert_eq!(result.checkpoints.len(), 1);
    let loaded_checkpoint = &result.checkpoints[0];

    // Verify execution metadata is preserved
    assert_eq!(loaded_checkpoint.ran_nodes.len(), 2);
    assert!(loaded_checkpoint.ran_nodes.contains(&NodeKind::Start));
    assert!(loaded_checkpoint
        .ran_nodes
        .contains(&NodeKind::Other("ProcessingNode".to_string())));
    assert_eq!(loaded_checkpoint.skipped_nodes, vec![NodeKind::End]);
    assert_eq!(loaded_checkpoint.updated_channels.len(), 3);

    // Verify errors are preserved
    let errors = loaded_checkpoint.state.errors.snapshot();
    assert_eq!(errors.len(), 2);

    // Check scheduler error
    let scheduler_error = errors
        .iter()
        .find(|e| matches!(e.scope, ErrorScope::Scheduler { .. }))
        .expect("Should have scheduler error");
    assert_eq!(
        scheduler_error.error.message,
        "Concurrency limit exceeded during execution"
    );
    assert_eq!(scheduler_error.context["active_tasks"], 10);

    // Check node error with cause chain
    let node_error = errors
        .iter()
        .find(|e| matches!(e.scope, ErrorScope::Node { .. }))
        .expect("Should have node error");
    assert_eq!(node_error.error.message, "Data validation failed");
    assert!(node_error.error.cause.is_some());
    if let Some(cause) = &node_error.error.cause {
        assert_eq!(cause.message, "Invalid JSON structure");
    }
}

/// Test error persistence with pagination and filtering
#[tokio::test]
async fn test_error_persistence_with_pagination() {
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    // Create multiple checkpoints with different error patterns
    for step in 1..=10 {
        let mut state = VersionedState::new_with_user_message(&format!("step {step}"));

        // Add different types of errors to different steps
        if step % 3 == 0 {
            // Add critical errors every 3rd step
            let critical_error = ErrorEvent {
                when: Utc::now(),
                scope: ErrorScope::Node {
                    kind: "CriticalNode".to_string(),
                    step,
                },
                error: LadderError::msg("Critical system failure"),
                tags: vec!["critical".to_string(), "system".to_string()],
                context: serde_json::json!({"severity": "high", "step": step}),
            };
            state.errors.get_mut().push(critical_error);
        }

        if step % 2 == 0 {
            // Add warning errors on even steps
            let warning_error = ErrorEvent {
                when: Utc::now(),
                scope: ErrorScope::App,
                error: LadderError::msg("Performance degradation detected"),
                tags: vec!["warning".to_string(), "performance".to_string()],
                context: serde_json::json!({"response_time_ms": step * 100}),
            };
            state.errors.get_mut().push(warning_error);
        }

        let checkpoint = Checkpoint {
            session_id: "pagination_test_session".to_string(),
            step,
            state,
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 4,
            created_at: Utc::now(),
            ran_nodes: if step <= 5 {
                vec![NodeKind::Start]
            } else {
                vec![NodeKind::Other("TestNode".to_string())]
            },
            skipped_nodes: vec![NodeKind::End],
            updated_channels: vec!["messages".to_string(), "errors".to_string()],
        };

        checkpointer
            .save(checkpoint)
            .await
            .expect("Failed to save checkpoint");
    }

    // Test pagination: Get first 3 steps
    let query = StepQuery {
        limit: Some(3),
        offset: Some(0),
        ..Default::default()
    };

    let result = checkpointer
        .query_steps("pagination_test_session", query)
        .await
        .expect("Failed to query steps");

    assert_eq!(result.page_info.total_count, 10);
    assert_eq!(result.page_info.page_size, 3);
    assert!(result.page_info.has_next_page);
    assert_eq!(result.checkpoints.len(), 3);

    // Results should be in descending order (newest first)
    assert_eq!(result.checkpoints[0].step, 10);
    assert_eq!(result.checkpoints[1].step, 9);
    assert_eq!(result.checkpoints[2].step, 8);

    // Test filtering by step range
    let query = StepQuery {
        min_step: Some(3),
        max_step: Some(6),
        ..Default::default()
    };

    let result = checkpointer
        .query_steps("pagination_test_session", query)
        .await
        .expect("Failed to query steps");

    assert_eq!(result.page_info.total_count, 4); // steps 3, 4, 5, 6
    assert_eq!(result.checkpoints.len(), 4);

    // Verify errors are present in the filtered results
    let step_6_checkpoint = result
        .checkpoints
        .iter()
        .find(|cp| cp.step == 6)
        .expect("Should have step 6");

    let step_6_errors = step_6_checkpoint.state.errors.snapshot();
    assert_eq!(step_6_errors.len(), 2); // Both critical (step % 3 == 0) and warning (step % 2 == 0)

    // Find the critical error
    let critical_error = step_6_errors
        .iter()
        .find(|e| e.tags.contains(&"critical".to_string()))
        .expect("Should have critical error");
    assert_eq!(critical_error.context["step"], 6);

    // Find the warning error
    let warning_error = step_6_errors
        .iter()
        .find(|e| e.tags.contains(&"warning".to_string()))
        .expect("Should have warning error");
    assert_eq!(warning_error.context["response_time_ms"], 600);
}

/// Test error persistence with concurrency control
#[tokio::test]
async fn test_error_persistence_with_concurrency_control() {
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    let mut state = VersionedState::new_with_user_message("concurrency test");

    // Add an error that simulates a concurrency issue
    let concurrency_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Runner {
            session: "concurrent_session".to_string(),
            step: 1,
        },
        error: LadderError::msg("Checkpoint conflict detected").with_details(serde_json::json!({
            "expected_step": 0,
            "actual_step": 1,
            "conflict_resolution": "retry_with_backoff"
        })),
        tags: vec!["concurrency".to_string(), "checkpoint".to_string()],
        context: serde_json::json!({
            "thread_id": "worker_1",
            "attempt": 2
        }),
    };

    state.errors.get_mut().push(concurrency_error);

    // Create initial checkpoint
    let checkpoint0 = Checkpoint {
        session_id: "concurrent_session".to_string(),
        step: 0,
        state: state.clone(),
        frontier: vec![NodeKind::Start],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 2,
        created_at: Utc::now(),
        ran_nodes: vec![],
        skipped_nodes: vec![],
        updated_channels: vec![],
    };

    // Save with concurrency check
    checkpointer
        .save_with_concurrency_check(checkpoint0, None)
        .await
        .expect("Failed to save initial checkpoint");

    // Create checkpoint for step 1
    let checkpoint1 = Checkpoint {
        session_id: "concurrent_session".to_string(),
        step: 1,
        state,
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 2,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![],
        updated_channels: vec!["messages".to_string(), "errors".to_string()],
    };

    // Save with correct expectation
    checkpointer
        .save_with_concurrency_check(checkpoint1, Some(0))
        .await
        .expect("Failed to save checkpoint with concurrency check");

    // Try to save conflicting checkpoint (should fail)
    let checkpoint2 = Checkpoint {
        session_id: "concurrent_session".to_string(),
        step: 2,
        state: VersionedState::new_with_user_message("conflict"),
        frontier: vec![NodeKind::End],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 2,
        created_at: Utc::now(),
        ran_nodes: vec![NodeKind::Start],
        skipped_nodes: vec![],
        updated_channels: vec!["messages".to_string()],
    };

    // This should fail due to concurrency conflict
    let result = checkpointer
        .save_with_concurrency_check(checkpoint2, Some(0))
        .await;

    assert!(result.is_err());
    if let Err(error) = result {
        assert!(error.to_string().contains("concurrency conflict"));
    }

    // Verify the error from step 1 is still preserved
    let loaded = checkpointer
        .load_latest("concurrent_session")
        .await
        .expect("Failed to load checkpoint")
        .expect("Checkpoint not found");

    let errors = loaded.state.errors.snapshot();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].error.message, "Checkpoint conflict detected");
    assert_eq!(errors[0].error.details["expected_step"], 0);
    assert_eq!(errors[0].context["thread_id"], "worker_1");
}

/// Test complex error serialization with nested structures
#[tokio::test]
async fn test_complex_error_serialization() {
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    let mut state = VersionedState::new_with_user_message("complex serialization test");

    // Create a complex error with deeply nested structures
    let complex_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Node {
            kind: "DataProcessor".to_string(),
            step: 100,
        },
        error: LadderError::msg("Complex data processing failure")
            .with_details(serde_json::json!({
                "input_schema": {
                    "version": "2.1.0",
                    "fields": {
                        "user_data": {"type": "object", "required": true},
                        "metadata": {"type": "array", "items": "string"}
                    }
                },
                "validation_errors": [
                    {"field": "user_data.email", "error": "invalid_format"},
                    {"field": "metadata[2]", "error": "missing_value"}
                ],
                "processing_stats": {
                    "records_processed": 1250,
                    "errors_found": 47,
                    "success_rate": 0.9624
                }
            }))
            .with_cause(
                LadderError::msg("Schema validation failed")
                    .with_details(serde_json::json!({
                        "validator": "json_schema_v7",
                        "error_count": 2
                    }))
                    .with_cause(
                        LadderError::msg("Invalid email format in user_data").with_details(
                            serde_json::json!({
                                "field_path": "user_data.email",
                                "provided_value": "not-an-email",
                                "expected_pattern": "^[\\w\\.-]+@[\\w\\.-]+\\.[a-zA-Z]{2,}$"
                            }),
                        ),
                    ),
            ),
        tags: vec![
            "data_processing".to_string(),
            "validation".to_string(),
            "schema_error".to_string(),
            "retryable".to_string(),
        ],
        context: serde_json::json!({
            "batch_id": "batch_2025_09_30_001",
            "processing_node": "worker-node-03",
            "memory_usage_mb": 512.7,
            "processing_time_ms": 15750,
            "retry_count": 0,
            "max_retries": 3,
            "input_source": {
                "type": "file",
                "path": "/data/input/user_records.jsonl",
                "size_bytes": 2048576,
                "checksum": "sha256:abc123..."
            },
            "environment": {
                "version": "Weavegraph-1.0.0",
                "node_id": "node-03",
                "cluster": "production-us-east"
            }
        }),
    };

    state.errors.get_mut().push(complex_error);

    let checkpoint = Checkpoint {
        session_id: "complex_serialization_session".to_string(),
        step: 100,
        state,
        frontier: vec![NodeKind::Other("NextProcessor".to_string())],
        versions_seen: FxHashMap::default(),
        concurrency_limit: 4,
        created_at: Utc::now(),
        ran_nodes: vec![
            NodeKind::Start,
            NodeKind::Other("DataProcessor".to_string()),
        ],
        skipped_nodes: vec![],
        updated_channels: vec![
            "messages".to_string(),
            "errors".to_string(),
            "extra".to_string(),
        ],
    };

    // Save checkpoint
    checkpointer
        .save(checkpoint)
        .await
        .expect("Failed to save checkpoint");

    // Load using query_steps to get full metadata
    let query = StepQuery {
        min_step: Some(100),
        max_step: Some(100),
        ..Default::default()
    };

    let result = checkpointer
        .query_steps("complex_serialization_session", query)
        .await
        .expect("Failed to query steps");

    assert_eq!(result.checkpoints.len(), 1);
    let loaded_checkpoint = &result.checkpoints[0];

    // Verify execution metadata
    assert_eq!(loaded_checkpoint.step, 100);
    assert_eq!(loaded_checkpoint.ran_nodes.len(), 2);
    assert!(loaded_checkpoint
        .ran_nodes
        .contains(&NodeKind::Other("DataProcessor".to_string())));

    // Verify complex error was preserved
    let errors = loaded_checkpoint.state.errors.snapshot();
    assert_eq!(errors.len(), 1);
    let error = &errors[0];

    // Check main error
    assert_eq!(error.error.message, "Complex data processing failure");
    assert_eq!(error.tags.len(), 4);
    assert!(error.tags.contains(&"data_processing".to_string()));

    // Check deeply nested details
    assert_eq!(error.error.details["input_schema"]["version"], "2.1.0");
    assert_eq!(
        error.error.details["validation_errors"][0]["field"],
        "user_data.email"
    );
    assert_eq!(
        error.error.details["processing_stats"]["records_processed"],
        1250
    );
    assert_eq!(
        error.error.details["processing_stats"]["success_rate"],
        0.9624
    );

    // Check complex context
    assert_eq!(error.context["batch_id"], "batch_2025_09_30_001");
    assert_eq!(error.context["memory_usage_mb"], 512.7);
    assert_eq!(error.context["input_source"]["size_bytes"], 2048576);
    assert_eq!(
        error.context["environment"]["cluster"],
        "production-us-east"
    );

    // Check cause chain (3 levels deep)
    let cause1 = error.error.cause.as_ref().expect("Should have first cause");
    assert_eq!(cause1.message, "Schema validation failed");
    assert_eq!(cause1.details["validator"], "json_schema_v7");

    let cause2 = cause1.cause.as_ref().expect("Should have second cause");
    assert_eq!(cause2.message, "Invalid email format in user_data");
    assert_eq!(cause2.details["field_path"], "user_data.email");
    assert_eq!(cause2.details["provided_value"], "not-an-email");

    // Verify no third-level cause
    assert!(cause2.cause.is_none());
}

/// Test error filtering by execution metadata
#[tokio::test]
async fn test_error_filtering_by_execution_metadata() {
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    // Create checkpoints with specific execution patterns
    for step in 1..=5 {
        let mut state = VersionedState::new_with_user_message(&format!("step {step}"));

        // Add errors related to specific nodes
        let node_name = if step <= 2 {
            "StartupNode"
        } else {
            "ProcessingNode"
        };

        let error = ErrorEvent {
            when: Utc::now(),
            scope: ErrorScope::Node {
                kind: node_name.to_string(),
                step,
            },
            error: LadderError::msg(format!("Error in {node_name} at step {step}")),
            tags: vec![node_name.to_lowercase()],
            context: serde_json::json!({"step": step, "node": node_name}),
        };

        state.errors.get_mut().push(error);

        let checkpoint = Checkpoint {
            session_id: "filtering_test_session".to_string(),
            step,
            state,
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 2,
            created_at: Utc::now(),
            ran_nodes: if step <= 2 {
                vec![NodeKind::Other("StartupNode".to_string())]
            } else {
                vec![NodeKind::Other("ProcessingNode".to_string())]
            },
            skipped_nodes: vec![NodeKind::End],
            updated_channels: vec!["messages".to_string(), "errors".to_string()],
        };

        checkpointer
            .save(checkpoint)
            .await
            .expect("Failed to save checkpoint");
    }

    // Filter by ran node (StartupNode - should get steps 1-2)
    let query = StepQuery {
        ran_node: Some(NodeKind::Other("StartupNode".to_string())),
        ..Default::default()
    };

    let result = checkpointer
        .query_steps("filtering_test_session", query)
        .await
        .expect("Failed to query steps");

    assert_eq!(result.page_info.total_count, 2);
    assert_eq!(result.checkpoints.len(), 2);

    // Verify the filtered checkpoints have the correct errors
    for checkpoint in &result.checkpoints {
        let errors = checkpoint.state.errors.snapshot();
        assert_eq!(errors.len(), 1);
        let error = &errors[0];

        if let ErrorScope::Node { kind, .. } = &error.scope {
            assert_eq!(kind, "StartupNode");
        }

        assert!(error.tags.contains(&"startupnode".to_string()));
    }

    // Filter by ran node (ProcessingNode - should get steps 3-5)
    let query = StepQuery {
        ran_node: Some(NodeKind::Other("ProcessingNode".to_string())),
        ..Default::default()
    };

    let result = checkpointer
        .query_steps("filtering_test_session", query)
        .await
        .expect("Failed to query steps");

    assert_eq!(result.page_info.total_count, 3);
    assert_eq!(result.checkpoints.len(), 3);

    // Verify the filtered checkpoints have the correct errors
    for checkpoint in &result.checkpoints {
        let errors = checkpoint.state.errors.snapshot();
        assert_eq!(errors.len(), 1);
        let error = &errors[0];

        if let ErrorScope::Node { kind, .. } = &error.scope {
            assert_eq!(kind, "ProcessingNode");
        }

        assert!(error.tags.contains(&"processingnode".to_string()));
    }
}
