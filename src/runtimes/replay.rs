//! Replay conformance helpers for comparing workflow runs.
//!
//! These helpers are intentionally small and test-friendly. They normalize common
//! nondeterministic fields, compare final state and event streams, and return
//! human-readable differences that can be used in ordinary assertions.

use serde_json::{Value, json};
use thiserror::Error;

use crate::{channels::Channel, event_bus::Event, state::VersionedState};

/// Captured output from one workflow run.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReplayRun {
    /// Final workflow state produced by the run.
    pub final_state: VersionedState,
    /// Events captured during the run.
    pub events: Vec<Event>,
}

impl ReplayRun {
    /// Create a replay run from final state and captured events.
    #[must_use]
    pub fn new(final_state: VersionedState, events: Vec<Event>) -> Self {
        Self {
            final_state,
            events,
        }
    }
}

/// Result of comparing two replay artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayComparison {
    differences: Vec<String>,
}

impl ReplayComparison {
    /// Create a successful comparison with no differences.
    #[must_use]
    pub fn matched() -> Self {
        Self {
            differences: Vec::new(),
        }
    }

    /// Create a comparison with the supplied differences.
    #[must_use]
    pub fn with_differences(differences: Vec<String>) -> Self {
        Self { differences }
    }

    /// Return true when no differences were found.
    #[must_use]
    pub fn is_match(&self) -> bool {
        self.differences.is_empty()
    }

    /// Return the differences found during comparison.
    #[must_use]
    pub fn differences(&self) -> &[String] {
        &self.differences
    }

    /// Convert this report into a `Result` suitable for test assertions.
    pub fn assert_matches(self) -> Result<(), ReplayConformanceError> {
        if self.is_match() {
            Ok(())
        } else {
            Err(ReplayConformanceError::Mismatch {
                differences: self.differences,
            })
        }
    }
}

/// Errors returned by replay conformance helpers.
#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum ReplayConformanceError {
    /// The compared runs were not equivalent.
    #[error("replay conformance mismatch: {differences:?}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::replay::mismatch))
    )]
    Mismatch {
        /// Human-readable differences.
        differences: Vec<String>,
    },
}

/// Normalize an event for replay comparison.
///
/// The default normalizer uses Weavegraph's JSON event shape and removes the
/// top-level timestamp, which is normally wall-clock dependent.
#[must_use]
pub fn normalize_event(event: &Event) -> Value {
    let mut value = event.to_json_value();
    if let Value::Object(object) = &mut value {
        object.remove("timestamp");
    }
    value
}

/// Normalize a final state into a JSON value for stable comparison and diffs.
#[must_use]
pub fn normalize_state(state: &VersionedState) -> Value {
    json!({
        "messages": state.messages.snapshot(),
        "messages_version": state.messages.version(),
        "extra": state.extra.snapshot(),
        "extra_version": state.extra.version(),
        "errors": state.errors.snapshot(),
        "errors_version": state.errors.version(),
    })
}

/// Compare two final states with default normalization.
#[must_use]
pub fn compare_final_state(left: &VersionedState, right: &VersionedState) -> ReplayComparison {
    let left_value = normalize_state(left);
    let right_value = normalize_state(right);
    if left_value == right_value {
        ReplayComparison::matched()
    } else {
        ReplayComparison::with_differences(vec![format!(
            "final state differs: left={left_value} right={right_value}"
        )])
    }
}

/// Compare two event streams with the default event normalizer.
#[must_use]
pub fn compare_event_sequences(left: &[Event], right: &[Event]) -> ReplayComparison {
    compare_event_sequences_with(left, right, normalize_event)
}

/// Compare two event streams with a caller-provided normalizer.
///
/// Use this when domain events contain timestamps, generated IDs, or other
/// values that should be compared semantically rather than byte-for-byte.
#[must_use]
pub fn compare_event_sequences_with<F>(
    left: &[Event],
    right: &[Event],
    normalizer: F,
) -> ReplayComparison
where
    F: Fn(&Event) -> Value,
{
    let left_values: Vec<Value> = left.iter().map(&normalizer).collect();
    let right_values: Vec<Value> = right.iter().map(&normalizer).collect();

    if left_values == right_values {
        return ReplayComparison::matched();
    }

    let mut differences = Vec::new();
    if left_values.len() != right_values.len() {
        differences.push(format!(
            "event count differs: left={} right={}",
            left_values.len(),
            right_values.len()
        ));
    }

    let shared_len = left_values.len().min(right_values.len());
    for index in 0..shared_len {
        if left_values[index] != right_values[index] {
            differences.push(format!(
                "event {index} differs: left={} right={}",
                left_values[index], right_values[index]
            ));
            break;
        }
    }

    ReplayComparison::with_differences(differences)
}

/// Compare two captured runs with default state and event normalization.
#[must_use]
pub fn compare_replay_runs(left: &ReplayRun, right: &ReplayRun) -> ReplayComparison {
    compare_replay_runs_with(left, right, normalize_event)
}

/// Compare two captured runs with a caller-provided event normalizer.
#[must_use]
pub fn compare_replay_runs_with<F>(
    left: &ReplayRun,
    right: &ReplayRun,
    event_normalizer: F,
) -> ReplayComparison
where
    F: Fn(&Event) -> Value,
{
    let mut differences = Vec::new();

    let state_comparison = compare_final_state(&left.final_state, &right.final_state);
    differences.extend(state_comparison.differences().iter().cloned());

    let event_comparison =
        compare_event_sequences_with(&left.events, &right.events, event_normalizer);
    differences.extend(event_comparison.differences().iter().cloned());

    ReplayComparison::with_differences(differences)
}
