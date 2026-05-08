use proptest::prelude::*;
use serde_json::json;
use weavegraph::event_bus::Event;
use weavegraph::runtimes::{
    ReplayComparison, ReplayConformanceError, ReplayRun, compare_event_sequences,
    compare_event_sequences_with, compare_final_state, compare_replay_runs,
    compare_replay_runs_with, normalize_event, normalize_state,
};
use weavegraph::state::VersionedState;

#[test]
fn replay_event_normalization_removes_runtime_timestamp() {
    let left = Event::node_message_with_meta("router", 1, "route", "selected");
    let right = Event::node_message_with_meta("router", 1, "route", "selected");

    assert_eq!(normalize_event(&left), normalize_event(&right));
    assert!(compare_event_sequences(&[left], &[right]).is_match());
}

#[test]
fn replay_event_comparison_reports_mismatch() {
    let comparison = compare_event_sequences(
        &[Event::node_message("route", "selected-a")],
        &[Event::node_message("route", "selected-b")],
    );

    assert!(!comparison.is_match());
    assert_eq!(comparison.differences().len(), 1);
}

#[test]
fn replay_event_comparison_supports_custom_normalizer() {
    let comparison = compare_event_sequences_with(
        &[Event::node_message("route", "selected-a")],
        &[Event::node_message("route", "selected-b")],
        |event| json!({ "scope": event.scope_label(), "message": "ignored" }),
    );

    assert!(comparison.is_match());
}

#[test]
fn replay_run_comparison_checks_state_and_events() {
    let left_state = VersionedState::builder()
        .with_extra("value", json!(1))
        .build();
    let right_state = VersionedState::builder()
        .with_extra("value", json!(1))
        .build();
    let different_state = VersionedState::builder()
        .with_extra("value", json!(2))
        .build();

    let left = ReplayRun::new(left_state, vec![Event::diagnostic("run", "done")]);
    let right = ReplayRun::new(right_state, vec![Event::diagnostic("run", "done")]);
    let different = ReplayRun::new(different_state, vec![Event::diagnostic("run", "done")]);

    assert!(compare_replay_runs(&left, &right).is_match());

    let mismatch = compare_replay_runs(&left, &different);
    assert!(!mismatch.is_match());
    assert!(matches!(
        mismatch.assert_matches(),
        Err(ReplayConformanceError::Mismatch { .. })
    ));
}

#[test]
fn replay_comparison_constructors_and_assertion_errors_preserve_differences() {
    assert!(ReplayComparison::matched().assert_matches().is_ok());

    let comparison = ReplayComparison::with_differences(vec!["first".into(), "second".into()]);
    assert!(!comparison.is_match());
    assert_eq!(comparison.differences(), &["first", "second"]);

    match comparison.assert_matches() {
        Err(ReplayConformanceError::Mismatch { differences }) => {
            assert_eq!(differences, vec!["first", "second"]);
        }
        other => panic!("expected mismatch error, got {other:?}"),
    }
}

#[test]
fn replay_event_comparison_reports_count_mismatch_when_shared_prefix_matches() {
    let left = vec![
        Event::diagnostic("run", "one"),
        Event::diagnostic("run", "two"),
    ];
    let right = vec![Event::diagnostic("run", "one")];

    let comparison = compare_event_sequences(&left, &right);

    assert!(!comparison.is_match());
    assert_eq!(comparison.differences().len(), 1);
    assert!(comparison.differences()[0].contains("event count differs"));
}

#[test]
fn replay_event_comparison_empty_sequences_match() {
    assert!(compare_event_sequences(&[], &[]).is_match());
}

#[test]
fn replay_final_state_normalization_includes_versions_and_extra() {
    let state = VersionedState::builder()
        .with_user_message("hello")
        .with_extra("answer", json!(42))
        .build();
    let normalized = normalize_state(&state);

    assert_eq!(normalized["messages_version"], 1);
    assert_eq!(normalized["extra_version"], 1);
    assert_eq!(normalized["errors_version"], 1);
    assert_eq!(normalized["extra"]["answer"], 42);
}

#[test]
fn replay_final_state_comparison_reports_state_mismatch() {
    let left = VersionedState::builder()
        .with_extra("value", json!(1))
        .build();
    let right = VersionedState::builder()
        .with_extra("value", json!(2))
        .build();

    let comparison = compare_final_state(&left, &right);

    assert!(!comparison.is_match());
    assert!(comparison.differences()[0].contains("final state differs"));
}

#[test]
fn replay_run_comparison_aggregates_state_and_event_differences() {
    let left = ReplayRun::new(
        VersionedState::builder()
            .with_extra("value", json!(1))
            .build(),
        vec![Event::diagnostic("run", "left")],
    );
    let right = ReplayRun::new(
        VersionedState::builder()
            .with_extra("value", json!(2))
            .build(),
        vec![Event::diagnostic("run", "right")],
    );

    let comparison = compare_replay_runs(&left, &right);

    assert!(!comparison.is_match());
    assert_eq!(comparison.differences().len(), 2);
    assert!(comparison.differences()[0].contains("final state differs"));
    assert!(comparison.differences()[1].contains("event 0 differs"));
}

#[test]
fn replay_run_custom_event_normalizer_can_ignore_event_differences() {
    let left = ReplayRun::new(
        VersionedState::builder()
            .with_extra("value", json!(1))
            .build(),
        vec![Event::diagnostic("run", "left")],
    );
    let right = ReplayRun::new(
        VersionedState::builder()
            .with_extra("value", json!(1))
            .build(),
        vec![Event::diagnostic("run", "right")],
    );

    let comparison = compare_replay_runs_with(&left, &right, |_| json!({ "event": "ignored" }));

    assert!(comparison.is_match());
}

proptest! {
    #[test]
    fn prop_replay_event_custom_normalizer_matches_same_length_sequences(
        left in prop::collection::vec("[A-Za-z0-9 _-]{0,24}", 0..12),
        right in prop::collection::vec("[A-Za-z0-9 _-]{0,24}", 0..12),
    ) {
        let len = left.len().min(right.len());
        let left_events: Vec<Event> = left.into_iter().take(len).map(|message| Event::diagnostic("scope", message)).collect();
        let right_events: Vec<Event> = right.into_iter().take(len).map(|message| Event::diagnostic("scope", message)).collect();

        prop_assert!(compare_event_sequences_with(&left_events, &right_events, |_| json!("ignored")).is_match());
    }
}
