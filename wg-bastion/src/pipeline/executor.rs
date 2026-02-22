//! The [`PipelineExecutor`] — orchestrates multi-stage guardrail execution.
//!
//! # Execution model
//!
//! 1. Stages are sorted by [`GuardrailStage::priority`] (ascending).
//! 2. Each stage is evaluated sequentially against the supplied [`Content`].
//! 3. A [`Block`](StageOutcome::Block) or [`Escalate`](StageOutcome::Escalate)
//!    outcome short-circuits the remaining stages.
//! 4. If a stage returns `Err` and [`GuardrailStage::degradable`] is `true`, the
//!    pipeline records it as a degraded run and continues.  Non-degradable errors
//!    propagate immediately.
//! 5. The [`FailMode`] policy knob controls whether a
//!    blocking outcome actually blocks the request, logs it only, or lets it
//!    through.
//!
//! # Example
//!
//! ```rust,ignore
//! use wg_bastion::pipeline::executor::PipelineExecutor;
//! use wg_bastion::pipeline::content::Content;
//! use wg_bastion::pipeline::stage::SecurityContext;
//! use wg_bastion::config::FailMode;
//!
//! let executor = PipelineExecutor::builder()
//!     .add_stage(my_stage)
//!     .fail_mode(FailMode::Closed)
//!     .build();
//!
//! let result = executor.run(&Content::Text("hi".into()), &ctx).await;
//! ```

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Instant;

use crate::config::FailMode;

use super::content::Content;
use super::outcome::{StageError, StageOutcome};
use super::stage::{GuardrailStage, SecurityContext, StageMetrics};

/// Outcome label for stages that ran in degraded mode (returned `Err` but `degradable`).
const OUTCOME_DEGRADED: &str = "degraded";
/// Outcome label for stages that failed fatally.
const OUTCOME_ERROR: &str = "error";

// ── PipelineResult ─────────────────────────────────────────────────────

/// The outcome of a full pipeline run.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// The final merged outcome.
    pub outcome: StageOutcome,
    /// Per-stage execution metrics, in execution order.
    pub stage_metrics: Vec<StageMetrics>,
    /// Stages that ran in degraded mode (returned `Err` but `degradable`).
    pub degraded_stages: Vec<String>,
    /// Whether the pipeline was forced to allow by [`FailMode::Open`] or
    /// [`FailMode::LogOnly`] despite a blocking stage.
    pub overridden: bool,
    /// Optional refusal response text for `RefusalPolicy` integration.
    pub refusal_response: Option<String>,
}

impl PipelineResult {
    /// Convenience: true when the final outcome is [`StageOutcome::Allow`].
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        self.outcome.is_allow()
    }

    /// True if any stage ran in degraded mode.
    #[must_use]
    pub fn has_degraded(&self) -> bool {
        !self.degraded_stages.is_empty()
    }

    /// Total wall-clock time across all stages.
    #[must_use]
    pub fn total_duration(&self) -> std::time::Duration {
        self.stage_metrics.iter().map(|m| m.duration).sum()
    }
}

// ── ExecutorError ──────────────────────────────────────────────────────

/// Errors that can prevent the pipeline from completing.
///
/// Distinct from [`StageError`] which represents per-stage failures that
/// may be recoverable via `degradable`.
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    /// A non-degradable stage returned an error.
    #[error("critical stage '{stage_id}' failed: {source}")]
    CriticalStageFailure {
        /// The stage that failed.
        stage_id: String,
        /// Underlying error.
        source: StageError,
    },

    /// The pipeline has no stages configured.
    #[error("pipeline has no stages")]
    Empty,
}

// ── PipelineExecutor ───────────────────────────────────────────────────

/// Orchestrates evaluation of a sequence of [`GuardrailStage`] instances.
///
/// Created via [`ExecutorBuilder`].
pub struct PipelineExecutor {
    /// Stages sorted by priority (ascending — lower priority value runs first).
    stages: Vec<Arc<dyn GuardrailStage>>,
    /// Fail mode from the [`SecurityPolicy`](crate::config::SecurityPolicy).
    fail_mode: FailMode,
}

impl PipelineExecutor {
    /// Start building an executor.
    #[must_use]
    pub fn builder() -> ExecutorBuilder {
        ExecutorBuilder::default()
    }

    /// Run all stages against the given content.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutorError::CriticalStageFailure`] if a non-degradable
    /// stage fails, or [`ExecutorError::Empty`] if no stages are registered.
    pub async fn run(
        &self,
        content: &Content,
        ctx: &SecurityContext,
    ) -> Result<PipelineResult, ExecutorError> {
        if self.stages.is_empty() {
            return Err(ExecutorError::Empty);
        }

        let mut stage_metrics = Vec::with_capacity(self.stages.len());
        let mut degraded_stages = Vec::new();
        let mut final_outcome = StageOutcome::allow(1.0);
        let mut overridden = false;
        let mut current_content = Cow::Borrowed(content);

        for stage in &self.stages {
            let start = Instant::now();
            let result = stage.evaluate(current_content.as_ref(), ctx).await;
            let duration = start.elapsed();

            match result {
                Ok(outcome) => {
                    let outcome_name = outcome.variant_name().to_owned();

                    stage_metrics.push(StageMetrics {
                        stage_id: stage.id().to_owned(),
                        duration,
                        cache_hit: false,
                        degraded: false,
                        outcome: outcome_name,
                    });

                    // Short-circuit on terminal outcomes.
                    if outcome.is_block() || outcome.is_escalate() {
                        final_outcome = self.apply_fail_mode(outcome, &mut overridden);
                        // Even if fail_mode overrides to Allow, we stop — the
                        // violation was recorded.
                        break;
                    }

                    match outcome {
                        // For Allow, keep the lowest confidence seen so far.
                        StageOutcome::Allow { confidence } => {
                            if let StageOutcome::Allow {
                                confidence: ref mut prev,
                            } = final_outcome
                            {
                                *prev = prev.min(confidence);
                            }
                        }
                        // Propagate transformed content to subsequent stages.
                        StageOutcome::Transform {
                            content: new_content,
                            ..
                        } => {
                            current_content = Cow::Owned(new_content);
                            final_outcome = StageOutcome::allow(1.0);
                        }
                        // For Skip and future variants, keep the latest outcome.
                        other => {
                            final_outcome = other;
                        }
                    }
                }
                Err(err) => {
                    if stage.degradable() {
                        tracing::warn!(
                            stage = stage.id(),
                            error = %err,
                            "degradable stage failed — skipping",
                        );

                        degraded_stages.push(stage.id().to_owned());

                        stage_metrics.push(StageMetrics {
                            stage_id: stage.id().to_owned(),
                            duration,
                            cache_hit: false,
                            degraded: true,
                            outcome: OUTCOME_DEGRADED.to_owned(),
                        });
                    } else {
                        // Record partial metrics before propagating.
                        stage_metrics.push(StageMetrics {
                            stage_id: stage.id().to_owned(),
                            duration,
                            cache_hit: false,
                            degraded: false,
                            outcome: OUTCOME_ERROR.to_owned(),
                        });

                        return Err(ExecutorError::CriticalStageFailure {
                            stage_id: stage.id().to_owned(),
                            source: err,
                        });
                    }
                }
            }
        }

        Ok(PipelineResult {
            outcome: final_outcome,
            stage_metrics,
            degraded_stages,
            overridden,
            refusal_response: None,
        })
    }

    /// Apply [`FailMode`] policy to a terminal outcome.
    fn apply_fail_mode(&self, outcome: StageOutcome, overridden: &mut bool) -> StageOutcome {
        match self.fail_mode {
            FailMode::Closed => outcome,
            FailMode::Open => {
                tracing::warn!(
                    fail_mode = "open",
                    "blocking outcome overridden to allow by FailMode::Open"
                );
                *overridden = true;
                StageOutcome::allow(0.0)
            }
            FailMode::LogOnly => {
                tracing::info!(
                    fail_mode = "log_only",
                    "blocking outcome logged but allowed by FailMode::LogOnly"
                );
                *overridden = true;
                StageOutcome::allow(0.0)
            }
        }
    }
}

// ── ExecutorBuilder ────────────────────────────────────────────────────

/// Builder for [`PipelineExecutor`].
///
/// Stages are added in any order; the builder sorts them by
/// [`GuardrailStage::priority`] at `.build()` time.
#[derive(Default)]
pub struct ExecutorBuilder {
    stages: Vec<Arc<dyn GuardrailStage>>,
    fail_mode: FailMode,
}

impl ExecutorBuilder {
    /// Add a stage (will be sorted by priority at build time).
    #[must_use]
    pub fn add_stage(mut self, stage: impl GuardrailStage + 'static) -> Self {
        self.stages.push(Arc::new(stage));
        self
    }

    /// Add a pre-wrapped `Arc<dyn GuardrailStage>`.
    #[must_use]
    pub fn add_shared_stage(mut self, stage: Arc<dyn GuardrailStage>) -> Self {
        self.stages.push(stage);
        self
    }

    /// Set the [`FailMode`] (defaults to [`FailMode::Closed`]).
    #[must_use]
    pub fn fail_mode(mut self, mode: FailMode) -> Self {
        self.fail_mode = mode;
        self
    }

    /// Build the executor.  Stages are sorted by priority ascending.
    #[must_use]
    pub fn build(mut self) -> PipelineExecutor {
        self.stages.sort_by_key(|s| s.priority());
        PipelineExecutor {
            stages: self.stages,
            fail_mode: self.fail_mode,
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unnecessary_literal_bound)]
mod tests {
    use super::*;
    use crate::pipeline::outcome::Severity;

    // ── Test stages ────────────────────────────────────────────────────

    struct AllowStage {
        id: &'static str,
        priority: u32,
    }

    #[async_trait::async_trait]
    impl GuardrailStage for AllowStage {
        fn id(&self) -> &str {
            self.id
        }
        async fn evaluate(
            &self,
            _content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            Ok(StageOutcome::allow(0.95))
        }
        fn priority(&self) -> u32 {
            self.priority
        }
    }

    struct BlockStage;

    #[async_trait::async_trait]
    impl GuardrailStage for BlockStage {
        fn id(&self) -> &str {
            "blocker"
        }
        async fn evaluate(
            &self,
            _content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            Ok(StageOutcome::block("threat", Severity::High))
        }
        fn degradable(&self) -> bool {
            false
        }
    }

    struct FailingDegradable;

    #[async_trait::async_trait]
    impl GuardrailStage for FailingDegradable {
        fn id(&self) -> &str {
            "degradable_fail"
        }
        async fn evaluate(
            &self,
            _content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            Err(StageError::BackendUnavailable {
                stage: "degradable_fail".into(),
                reason: "test".into(),
            })
        }
        fn degradable(&self) -> bool {
            true
        }
    }

    struct FailingCritical;

    #[async_trait::async_trait]
    impl GuardrailStage for FailingCritical {
        fn id(&self) -> &str {
            "critical_fail"
        }
        async fn evaluate(
            &self,
            _content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            Err(StageError::Internal {
                stage: "critical_fail".into(),
                source: "critical bug".into(),
            })
        }
        fn degradable(&self) -> bool {
            false
        }
    }

    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }

    fn ctx() -> SecurityContext {
        SecurityContext::default()
    }

    // ── Actual tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_executor_errors() {
        let exec = PipelineExecutor::builder().build();
        let result = exec.run(&text("hi"), &ctx()).await;
        assert!(matches!(result, Err(ExecutorError::Empty)));
    }

    #[tokio::test]
    async fn single_allow_stage() {
        let exec = PipelineExecutor::builder()
            .add_stage(AllowStage {
                id: "a",
                priority: 100,
            })
            .build();

        let result = exec.run(&text("hi"), &ctx()).await.unwrap();
        assert!(result.is_allowed());
        assert_eq!(result.stage_metrics.len(), 1);
        assert!(!result.has_degraded());
        assert!(!result.overridden);
    }

    #[tokio::test]
    async fn multiple_stages_execute_in_priority_order() {
        let exec = PipelineExecutor::builder()
            .add_stage(AllowStage {
                id: "low",
                priority: 200,
            })
            .add_stage(AllowStage {
                id: "high",
                priority: 10,
            })
            .build();

        let result = exec.run(&text("x"), &ctx()).await.unwrap();
        assert!(result.is_allowed());
        assert_eq!(result.stage_metrics[0].stage_id, "high");
        assert_eq!(result.stage_metrics[1].stage_id, "low");
    }

    #[tokio::test]
    async fn block_short_circuits() {
        let exec = PipelineExecutor::builder()
            .add_stage(AllowStage {
                id: "first",
                priority: 10,
            })
            .add_stage(BlockStage)
            .add_stage(AllowStage {
                id: "never",
                priority: 300,
            })
            .build();

        let result = exec.run(&text("bad"), &ctx()).await.unwrap();
        assert!(result.outcome.is_block());
        // Only 2 stages ran (allow + block); the third was skipped.
        assert_eq!(result.stage_metrics.len(), 2);
    }

    #[tokio::test]
    async fn degradable_stage_continues() {
        let exec = PipelineExecutor::builder()
            .add_stage(FailingDegradable)
            .add_stage(AllowStage {
                id: "after",
                priority: 200,
            })
            .build();

        let result = exec.run(&text("x"), &ctx()).await.unwrap();
        assert!(result.is_allowed());
        assert!(result.has_degraded());
        assert_eq!(result.degraded_stages, vec!["degradable_fail"]);
        assert_eq!(result.stage_metrics.len(), 2);
        assert!(result.stage_metrics[0].degraded);
    }

    #[tokio::test]
    async fn critical_failure_propagates() {
        let exec = PipelineExecutor::builder()
            .add_stage(FailingCritical)
            .add_stage(AllowStage {
                id: "unreachable",
                priority: 200,
            })
            .build();

        let err = exec.run(&text("x"), &ctx()).await.unwrap_err();
        assert!(matches!(err, ExecutorError::CriticalStageFailure { .. }));
    }

    #[tokio::test]
    async fn fail_mode_open_overrides_block() {
        let exec = PipelineExecutor::builder()
            .add_stage(BlockStage)
            .fail_mode(FailMode::Open)
            .build();

        let result = exec.run(&text("bad"), &ctx()).await.unwrap();
        assert!(result.is_allowed());
        assert!(result.overridden);
    }

    #[tokio::test]
    async fn fail_mode_log_only_overrides_block() {
        let exec = PipelineExecutor::builder()
            .add_stage(BlockStage)
            .fail_mode(FailMode::LogOnly)
            .build();

        let result = exec.run(&text("bad"), &ctx()).await.unwrap();
        assert!(result.is_allowed());
        assert!(result.overridden);
    }

    #[tokio::test]
    async fn fail_mode_closed_preserves_block() {
        let exec = PipelineExecutor::builder()
            .add_stage(BlockStage)
            .fail_mode(FailMode::Closed)
            .build();

        let result = exec.run(&text("bad"), &ctx()).await.unwrap();
        assert!(result.outcome.is_block());
        assert!(!result.overridden);
    }

    #[test]
    fn total_duration_sums_stages() {
        use std::time::Duration;

        let result = PipelineResult {
            outcome: StageOutcome::allow(1.0),
            stage_metrics: vec![
                StageMetrics {
                    stage_id: "a".into(),
                    duration: Duration::from_millis(10),
                    cache_hit: false,
                    degraded: false,
                    outcome: "allow".into(),
                },
                StageMetrics {
                    stage_id: "b".into(),
                    duration: Duration::from_millis(20),
                    cache_hit: false,
                    degraded: false,
                    outcome: "allow".into(),
                },
            ],
            degraded_stages: vec![],
            overridden: false,
            refusal_response: None,
        };

        assert_eq!(result.total_duration(), Duration::from_millis(30));
    }

    // ── Transform propagation tests ────────────────────────────────

    /// A stage that transforms "raw" → "normalized".
    struct TransformStage {
        id: &'static str,
        from: &'static str,
        to: &'static str,
        priority: u32,
    }

    #[async_trait::async_trait]
    impl GuardrailStage for TransformStage {
        fn id(&self) -> &str {
            self.id
        }
        async fn evaluate(
            &self,
            content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            if let Content::Text(s) = content
                && s == self.from
            {
                return Ok(StageOutcome::transform(
                    Content::Text(self.to.into()),
                    format!("transformed '{}' → '{}'", self.from, self.to),
                ));
            }
            Ok(StageOutcome::allow(1.0))
        }
        fn priority(&self) -> u32 {
            self.priority
        }
    }

    /// A stage that asserts it receives specific content.
    struct InspectStage {
        expected_text: &'static str,
        priority: u32,
    }

    #[async_trait::async_trait]
    impl GuardrailStage for InspectStage {
        fn id(&self) -> &str {
            "inspector"
        }
        async fn evaluate(
            &self,
            content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            match content {
                Content::Text(s) if s == self.expected_text => Ok(StageOutcome::allow(1.0)),
                other => Ok(StageOutcome::block(
                    format!("expected text '{}', got {:?}", self.expected_text, other),
                    Severity::High,
                )),
            }
        }
        fn priority(&self) -> u32 {
            self.priority
        }
    }

    #[tokio::test]
    async fn transform_propagates_to_next_stage() {
        let exec = PipelineExecutor::builder()
            .add_stage(TransformStage {
                id: "normalizer",
                from: "raw",
                to: "normalized",
                priority: 10,
            })
            .add_stage(InspectStage {
                expected_text: "normalized",
                priority: 20,
            })
            .build();

        let result = exec.run(&text("raw"), &ctx()).await.unwrap();
        // InspectStage would block if it received "raw" instead of "normalized".
        assert!(
            result.is_allowed(),
            "expected Allow, got {:?} — transform was not propagated",
            result.outcome
        );
        assert_eq!(result.stage_metrics.len(), 2);
    }

    #[tokio::test]
    async fn multiple_transforms_chain() {
        let exec = PipelineExecutor::builder()
            .add_stage(TransformStage {
                id: "step1",
                from: "a",
                to: "b",
                priority: 10,
            })
            .add_stage(TransformStage {
                id: "step2",
                from: "b",
                to: "c",
                priority: 20,
            })
            .add_stage(InspectStage {
                expected_text: "c",
                priority: 30,
            })
            .build();

        let result = exec.run(&text("a"), &ctx()).await.unwrap();
        assert!(
            result.is_allowed(),
            "expected Allow after chained transforms, got {:?}",
            result.outcome
        );
        assert_eq!(result.stage_metrics.len(), 3);
    }
}
