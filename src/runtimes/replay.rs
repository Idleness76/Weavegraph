//! Replay conformance helpers for comparing workflow runs.
//!
//! These helpers are intentionally small and test-friendly. They normalize common
//! nondeterministic fields, compare final state and event streams, and return
//! human-readable differences that can be used in ordinary assertions.

use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    channels::Channel,
    event_bus::Event,
    state::{StateKey, StateLifecycle, VersionedState},
};

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
#[non_exhaustive]
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

// ============================================================================
// Normalization profiles (WG-006)
// ============================================================================

/// A filter profile for [`normalize_state_with`] and [`compare_final_state_with`].
///
/// A profile lists extra-map keys that should be excluded from normalized state
/// output. This is the primary mechanism for separating durable state from
/// per-invocation scratch values during replay comparison and resume assertions.
///
/// # Conflict detection
///
/// When a key is added via [`ignore_key`](Self::ignore_key), the profile records
/// the key's [`StateLifecycle`] annotation. If the same storage key is later
/// registered with a **different** lifecycle annotation, the method panics with a
/// clear message. This prevents subtle bugs from defining the same slot constant
/// twice with conflicting policies.
///
/// Raw-string keys added via [`ignore_extra_keys`](Self::ignore_extra_keys) carry
/// no lifecycle annotation and do not trigger conflict detection.
///
/// # Examples
///
/// ```rust
/// use weavegraph::runtimes::replay::{StateNormalizeProfile, normalize_state_with};
/// use weavegraph::state::{StateKey, StateLifecycle};
/// use weavegraph::state::VersionedState;
///
/// const TICK_EVENT: StateKey<u64> = StateKey::new("wq", "event", 1).invocation_scoped();
///
/// let profile = StateNormalizeProfile::new().ignore_key(TICK_EVENT);
///
/// let state = VersionedState::new_with_user_message("hello");
/// let _normalized = normalize_state_with(&state, &profile);
/// ```
#[derive(Debug, Default, Clone)]
pub struct StateNormalizeProfile {
    /// (storage_key, optional lifecycle annotation).
    /// `None` = added via raw string; `Some(lc)` = added via typed StateKey.
    ignored: Vec<(String, Option<StateLifecycle>)>,
}

impl StateNormalizeProfile {
    /// Create an empty profile (no keys ignored; equivalent to `normalize_state`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ignore the given raw storage key strings during normalization.
    ///
    /// Use this for quick ad-hoc ignores. Prefer [`ignore_key`](Self::ignore_key)
    /// when you have a typed `StateKey` constant, as it also validates lifecycle
    /// consistency.
    #[must_use]
    pub fn ignore_extra_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for k in keys {
            self.add_raw(k.into(), None);
        }
        self
    }

    /// Ignore the storage slot identified by `key` during normalization.
    ///
    /// The key's [`StateLifecycle`] annotation is recorded. If the same storage
    /// key has previously been registered with a different lifecycle annotation,
    /// this method **panics** — this is intentional: it surfaces a configuration
    /// mistake at test/startup time rather than silently producing wrong results.
    #[must_use]
    pub fn ignore_key<T>(mut self, key: StateKey<T>) -> Self {
        self.add_raw(key.storage_key(), Some(key.lifecycle()));
        self
    }

    fn add_raw(&mut self, storage_key: String, lifecycle: Option<StateLifecycle>) {
        if let Some((_, existing_lc)) = self.ignored.iter().find(|(k, _)| k == &storage_key) {
            match (existing_lc, &lifecycle) {
                (Some(a), Some(b)) if a != b => {
                    panic!(
                        "StateNormalizeProfile: conflicting lifecycle annotations for key {:?}: \
                         already registered as {:?}, attempted to re-register as {:?}. \
                         Ensure the same StateKey constant is used throughout.",
                        storage_key, a, b
                    );
                }
                _ => {} // duplicate or compatible — idempotent
            }
            return;
        }
        self.ignored.push((storage_key, lifecycle));
    }

    /// Iterate over the concrete storage key strings this profile ignores.
    pub fn ignored_keys(&self) -> impl Iterator<Item = &str> {
        self.ignored.iter().map(|(k, _)| k.as_str())
    }
}

/// Normalize a final state into a JSON value, excluding keys listed in `profile`.
///
/// Identical to [`normalize_state`] except the caller can suppress named keys
/// from the `extra` map. Use this to compare only durable state when some extra
/// entries are per-invocation scratch that should not influence the comparison.
///
/// # Examples
///
/// ```rust
/// use weavegraph::runtimes::replay::{StateNormalizeProfile, normalize_state_with};
/// use weavegraph::state::{StateKey, VersionedState};
///
/// const TICK: StateKey<u64> = StateKey::new("wq", "tick", 1).invocation_scoped();
///
/// let profile = StateNormalizeProfile::new().ignore_key(TICK);
/// let state = VersionedState::new_with_user_message("hello");
/// let _value = normalize_state_with(&state, &profile);
/// ```
#[must_use]
pub fn normalize_state_with(state: &VersionedState, profile: &StateNormalizeProfile) -> Value {
    let mut extra = state.extra.snapshot();
    for key in profile.ignored_keys() {
        extra.remove(key);
    }
    json!({
        "messages": state.messages.snapshot(),
        "messages_version": state.messages.version(),
        "extra": extra,
        "extra_version": state.extra.version(),
        "errors": state.errors.snapshot(),
        "errors_version": state.errors.version(),
    })
}

/// Compare two final states using a caller-provided normalization profile.
///
/// Equivalent to [`compare_final_state`] but filters the `extra` map through
/// `profile` before comparing. Use this to assert that durable state matches
/// while ignoring known per-invocation scratch keys.
#[must_use]
pub fn compare_final_state_with(
    left: &VersionedState,
    right: &VersionedState,
    profile: &StateNormalizeProfile,
) -> ReplayComparison {
    let left_value = normalize_state_with(left, profile);
    let right_value = normalize_state_with(right, profile);
    if left_value == right_value {
        ReplayComparison::matched()
    } else {
        ReplayComparison::with_differences(vec![format!(
            "final state differs: left={left_value} right={right_value}"
        )])
    }
}

/// Compare two captured runs using a state profile and a caller-provided event normalizer.
///
/// Combines [`compare_final_state_with`] and [`compare_event_sequences_with`] into
/// a single assertion. Use this as the single call in iterative resume tests that
/// need both durable-state filtering and custom event normalization.
#[must_use]
pub fn compare_replay_runs_with_profile<F>(
    left: &ReplayRun,
    right: &ReplayRun,
    state_profile: &StateNormalizeProfile,
    event_normalizer: F,
) -> ReplayComparison
where
    F: Fn(&Event) -> Value,
{
    let mut differences = Vec::new();

    let state_comparison =
        compare_final_state_with(&left.final_state, &right.final_state, state_profile);
    differences.extend(state_comparison.differences().iter().cloned());

    let event_comparison =
        compare_event_sequences_with(&left.events, &right.events, event_normalizer);
    differences.extend(event_comparison.differences().iter().cloned());

    ReplayComparison::with_differences(differences)
}
