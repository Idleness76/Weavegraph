//! Runtime observer trait and metadata types for workflow telemetry hooks.
//!
//! `RuntimeObserver` is an opt-in interface that receives structured callbacks
//! at key points during graph execution: invocation boundaries, per-node
//! completion, checkpoint operations, and event-bus emissions. All methods
//! have default no-op implementations, so implementors only override the hooks
//! they care about.
//!
//! # Usage
//!
//! Register an observer when building a runner:
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use weavegraph::runtimes::{AppRunner, observer::{RuntimeObserver, NodeFinishMeta}};
//! use weavegraph::app::App;
//!
//! #[derive(Debug)]
//! struct CountingObserver {
//!     count: std::sync::atomic::AtomicU64,
//! }
//!
//! impl RuntimeObserver for CountingObserver {
//!     fn on_node_finish(&self, meta: &NodeFinishMeta<'_>) {
//!         self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//!     }
//! }
//!
//! # async fn example(app: App) {
//! let observer = Arc::new(CountingObserver { count: Default::default() });
//! let runner = AppRunner::builder()
//!     .app(app)
//!     .observer(observer)
//!     .build()
//!     .await;
//! # }
//! ```
//!
//! # Performance contract
//!
//! Observer hooks are called **synchronously** on the execution thread. They must
//! be fast and non-blocking. Panics inside hooks are caught by the runner, which
//! logs a warning via [`tracing`] and continues execution — the graph is never
//! brought down by an observer failure.
//!
//! # Note on per-node timing in 0.6.0
//!
//! In this release, `step_duration_ms` in [`NodeFinishMeta`] reflects the elapsed
//! time for the **entire superstep** that contained the node, not the per-node
//! wall time. Nodes within the same superstep share the step's duration. Per-node
//! timing would require scheduler-level instrumentation and is planned for a
//! future release.

use std::fmt;
use std::panic::RefUnwindSafe;

use crate::types::NodeKind;

// ============================================================================
// Outcome enums
// ============================================================================

/// Outcome of a completed workflow invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvocationOutcome {
    /// The invocation ran to completion successfully.
    Completed,
    /// The invocation ended with a runtime error.
    Error,
}

/// Outcome of a completed node execution within a superstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeOutcome {
    /// The node ran and returned a `NodePartial`.
    Completed,
    /// The node returned a fatal `NodeError`.
    Error,
    /// The node was skipped (version-gated or terminal `End` node).
    Skipped,
}

// ============================================================================
// Metadata structs — all #[non_exhaustive] so fields can be added without
// breaking implementors that destructure them (though &-access is idiomatic).
// ============================================================================

/// Metadata supplied to [`RuntimeObserver::on_invocation_start`].
#[derive(Debug)]
#[non_exhaustive]
pub struct InvocationStartMeta<'a> {
    /// The session identifier for this invocation.
    pub session_id: &'a str,
    /// Stable fingerprint of the compiled graph definition.
    ///
    /// Computed by [`App::graph_definition_hash`](crate::app::App::graph_definition_hash).
    pub graph_id: &'a str,
}

/// Metadata supplied to [`RuntimeObserver::on_invocation_finish`].
#[derive(Debug)]
#[non_exhaustive]
pub struct InvocationFinishMeta<'a> {
    /// The session identifier.
    pub session_id: &'a str,
    /// Stable fingerprint of the compiled graph definition.
    pub graph_id: &'a str,
    /// Wall-clock elapsed time for the full invocation in milliseconds.
    pub duration_ms: u64,
    /// Outcome of the invocation.
    pub outcome: InvocationOutcome,
}

/// Metadata supplied to [`RuntimeObserver::on_node_finish`].
///
/// See [module-level note](self) on per-node timing in 0.6.0.
#[derive(Debug)]
#[non_exhaustive]
pub struct NodeFinishMeta<'a> {
    /// The node that completed.
    pub node_kind: &'a NodeKind,
    /// The session identifier.
    pub session_id: &'a str,
    /// The step number within which this node executed.
    pub step: u64,
    /// Elapsed time for the superstep containing this node, in milliseconds.
    ///
    /// All nodes in the same superstep share this value. Per-node timing
    /// is not available in 0.6.0.
    pub step_duration_ms: u64,
    /// Outcome of this node.
    pub outcome: NodeOutcome,
}

/// Metadata supplied to [`RuntimeObserver::on_checkpoint_load`].
#[derive(Debug)]
#[non_exhaustive]
pub struct CheckpointLoadMeta<'a> {
    /// The session identifier.
    pub session_id: &'a str,
    /// Human-readable backend name (e.g. `"sqlite"`, `"postgres"`, `"in-memory"`).
    pub backend: &'a str,
    /// The step number that was loaded from the checkpoint.
    pub step: u64,
}

/// Metadata supplied to [`RuntimeObserver::on_checkpoint_save`].
#[derive(Debug)]
#[non_exhaustive]
pub struct CheckpointSaveMeta<'a> {
    /// The session identifier.
    pub session_id: &'a str,
    /// Human-readable backend name.
    pub backend: &'a str,
    /// The step number that was saved.
    pub step: u64,
    /// Wall-clock duration of the save operation in milliseconds.
    pub duration_ms: u64,
}

/// Metadata supplied to [`RuntimeObserver::on_event_bus_emit`].
#[derive(Debug)]
#[non_exhaustive]
pub struct EventBusEmitMeta<'a> {
    /// The scope label of the emitted event (e.g. `"features"`, `"__weavegraph_stream_end__"`).
    pub scope: &'a str,
}

// ============================================================================
// RuntimeObserver trait
// ============================================================================

/// Observer interface for runtime telemetry hooks.
///
/// Register an implementation with
/// [`AppRunnerBuilder::observer`](crate::runtimes::runner::AppRunnerBuilder::observer).
/// All methods default to no-ops; implement only the callbacks you need.
///
/// # Safety contract
///
/// Implementations **must not panic** — panics are caught by the runner and
/// produce a `tracing::warn!` log entry. The supertrait bound [`RefUnwindSafe`]
/// is required to make this catch-and-continue safe without `AssertUnwindSafe`
/// wrappers at every callsite.
///
/// Implementations must be `Send + Sync` as the runner can share them across
/// async tasks.
pub trait RuntimeObserver: Send + Sync + fmt::Debug + RefUnwindSafe + 'static {
    /// Called immediately before a workflow invocation begins running.
    fn on_invocation_start(&self, _meta: &InvocationStartMeta<'_>) {}

    /// Called after a workflow invocation finishes (successfully or with an error).
    fn on_invocation_finish(&self, _meta: &InvocationFinishMeta<'_>) {}

    /// Called once for each node after the superstep containing it completes.
    ///
    /// In 0.6.0, `step_duration_ms` is the superstep duration shared by all
    /// nodes in the same parallel step. See the [module note](self).
    fn on_node_finish(&self, _meta: &NodeFinishMeta<'_>) {}

    /// Called after a checkpoint is successfully loaded during session creation.
    fn on_checkpoint_load(&self, _meta: &CheckpointLoadMeta<'_>) {}

    /// Called after a checkpoint is successfully saved.
    fn on_checkpoint_save(&self, _meta: &CheckpointSaveMeta<'_>) {}

    /// Called after each event is emitted through the event bus.
    fn on_event_bus_emit(&self, _meta: &EventBusEmitMeta<'_>) {}
}
