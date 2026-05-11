//! Feature-gated [`RuntimeObserver`] implementation using the [`metrics`] crate facade.
//!
//! Enable with `features = ["metrics"]`. This module emits standard counters and
//! histograms that any `metrics`-compatible recorder (e.g. `metrics-exporter-prometheus`)
//! can capture.
//!
//! # Metric inventory
//!
//! | Metric | Kind | Labels | Description |
//! |--------|------|--------|-------------|
//! | `weavegraph.node.invocations` | counter | `node`, `outcome` | Completed node executions |
//! | `weavegraph.node.step_duration_ms` | histogram | `node` | Superstep duration (shared across parallel nodes) |
//! | `weavegraph.invocation.count` | counter | `outcome` | Completed workflow invocations |
//! | `weavegraph.invocation.duration_ms` | histogram | (none) | Invocation wall-clock duration |
//! | `weavegraph.checkpoint.saves` | counter | `backend` | Successful checkpoint saves |
//! | `weavegraph.checkpoint.save_duration_ms` | histogram | `backend` | Checkpoint save duration |
//! | `weavegraph.checkpoint.loads` | counter | `backend` | Sessions resumed from a checkpoint |
//! | `weavegraph.event_bus.emits` | counter | `scope` | Events emitted through the event bus |
//!
//! # Cardinality note
//!
//! Labels are kept conservative by default. `session_id` and `invocation_id` are
//! intentionally **not** included as labels to avoid unbounded cardinality in
//! long-running services. The `node` label uses the node kind's string encoding
//! (e.g. `"features"`, `"strategy"`).
//!
//! # Usage
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use weavegraph::runtimes::{AppRunner, metrics_observer::MetricsObserver};
//! # use weavegraph::app::App;
//!
//! # async fn example(app: App) {
//! let runner = AppRunner::builder()
//!     .app(app)
//!     .observer(Arc::new(MetricsObserver))
//!     .build()
//!     .await;
//! # }
//! ```

use std::panic::RefUnwindSafe;

use crate::runtimes::observer::{
    CheckpointLoadMeta, CheckpointSaveMeta, EventBusEmitMeta, InvocationFinishMeta,
    InvocationStartMeta, NodeFinishMeta, NodeOutcome, RuntimeObserver,
};

/// A [`RuntimeObserver`] that emits metrics via the [`metrics`] crate facade.
///
/// Install a compatible recorder (e.g. `metrics-exporter-prometheus`) before
/// starting the runner to have these metrics exported to your observability stack.
///
/// See the [module documentation](self) for the full metric inventory.
#[derive(Debug, Clone, Copy)]
pub struct MetricsObserver;

// MetricsObserver holds no interior mutability and all metrics calls are
// thread-safe through the global recorder, so RefUnwindSafe is safe to assert.
impl RefUnwindSafe for MetricsObserver {}

impl RuntimeObserver for MetricsObserver {
    fn on_invocation_start(&self, _meta: &InvocationStartMeta<'_>) {
        // Nothing to emit at start — counts and durations are emitted on finish.
    }

    fn on_invocation_finish(&self, meta: &InvocationFinishMeta<'_>) {
        let outcome = match meta.outcome {
            crate::runtimes::observer::InvocationOutcome::Completed => "completed",
            crate::runtimes::observer::InvocationOutcome::Error => "error",
        };
        metrics::counter!("weavegraph.invocation.count", "outcome" => outcome).increment(1);
        metrics::histogram!("weavegraph.invocation.duration_ms").record(meta.duration_ms as f64);
    }

    fn on_node_finish(&self, meta: &NodeFinishMeta<'_>) {
        let node = meta.node_kind.encode().to_string();
        let outcome = match meta.outcome {
            NodeOutcome::Completed => "completed",
            NodeOutcome::Error => "error",
            NodeOutcome::Skipped => "skipped",
        };
        metrics::counter!(
            "weavegraph.node.invocations",
            "node" => node.clone(),
            "outcome" => outcome
        )
        .increment(1);
        if meta.outcome != NodeOutcome::Skipped {
            metrics::histogram!("weavegraph.node.step_duration_ms", "node" => node)
                .record(meta.step_duration_ms as f64);
        }
    }

    fn on_checkpoint_load(&self, meta: &CheckpointLoadMeta<'_>) {
        metrics::counter!(
            "weavegraph.checkpoint.loads",
            "backend" => meta.backend.to_string()
        )
        .increment(1);
    }

    fn on_checkpoint_save(&self, meta: &CheckpointSaveMeta<'_>) {
        let backend = meta.backend.to_string();
        metrics::counter!("weavegraph.checkpoint.saves", "backend" => backend.clone()).increment(1);
        metrics::histogram!(
            "weavegraph.checkpoint.save_duration_ms",
            "backend" => backend
        )
        .record(meta.duration_ms as f64);
    }

    fn on_event_bus_emit(&self, meta: &EventBusEmitMeta<'_>) {
        metrics::counter!(
            "weavegraph.event_bus.emits",
            "scope" => meta.scope.to_string()
        )
        .increment(1);
    }
}
