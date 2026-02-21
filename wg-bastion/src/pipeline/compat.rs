//! Backward-compatibility adapter for the legacy [`SecurityStage`] trait.
//!
//! The original Sprint-1 `SecurityStage` trait operates on `&str` inputs and
//! returns [`StageResult`] (pass/fail).  The new [`GuardrailStage`] trait uses
//! the richer [`Content`] enum and [`StageOutcome`].
//!
//! [`LegacyAdapter`] wraps any `SecurityStage` implementation and presents it
//! as a `GuardrailStage`, enabling existing stages to participate in the new
//! [`PipelineExecutor`](super::executor::PipelineExecutor) without rewrite.
//!
//! # Example
//!
//! ```rust,ignore
//! use wg_bastion::pipeline::compat::LegacyAdapter;
//! use wg_bastion::pipeline::executor::PipelineExecutor;
//!
//! // `MyOldStage` implements the legacy `SecurityStage` trait.
//! let adapted = LegacyAdapter::new(MyOldStage);
//!
//! let executor = PipelineExecutor::builder()
//!     .add_stage(adapted)
//!     .build();
//! ```
//!
//! # Limitations
//!
//! - Only the [`Content::Text`] variant is forwarded to the legacy stage.
//!   Non-text content returns [`StageOutcome::Skip`].
//! - The legacy `StageResult::metadata` is not propagated into `StageOutcome`
//!   (this may be revisited if needed for specific migration paths).

use async_trait::async_trait;

use super::content::Content;
use super::outcome::{Severity, StageError, StageOutcome};
use super::stage::{GuardrailStage, SecurityContext};
use super::{SecurityStage, StageResult};

// ── LegacyAdapter ──────────────────────────────────────────────────────

/// Wraps a legacy [`SecurityStage`] to implement [`GuardrailStage`].
///
/// See the [module-level docs](self) for usage and limitations.
pub struct LegacyAdapter<S> {
    inner: S,
}

impl<S: SecurityStage> LegacyAdapter<S> {
    /// Wrap a legacy stage.
    #[must_use]
    pub fn new(stage: S) -> Self {
        Self { inner: stage }
    }
}

#[async_trait]
impl<S: SecurityStage + 'static> GuardrailStage for LegacyAdapter<S> {
    fn id(&self) -> &str {
        self.inner.name()
    }

    async fn evaluate(
        &self,
        content: &Content,
        ctx: &SecurityContext,
    ) -> Result<StageOutcome, StageError> {
        // Only text content is supported by the legacy API.
        let text = match content {
            Content::Text(t) => t.as_str(),
            other => {
                return Ok(StageOutcome::skip(format!(
                    "legacy stage '{}' only supports text; got {}",
                    self.inner.name(),
                    other.variant_name(),
                )));
            }
        };

        // Build a legacy SecurityContext from the new one.
        let mut legacy_ctx = super::SecurityContext::new().with_session_id(ctx.session_id());
        if let Some(uid) = ctx.user_id() {
            legacy_ctx = legacy_ctx.with_metadata("user_id", uid);
        }

        // Delegate to the legacy stage.
        let result =
            self.inner
                .execute(text, &legacy_ctx)
                .await
                .map_err(|e| StageError::Internal {
                    stage: self.inner.name().to_owned(),
                    source: Box::new(e),
                })?;

        Ok(translate_result(result))
    }

    /// Legacy stages are always degradable to preserve existing behavior.
    fn degradable(&self) -> bool {
        true
    }
}

/// Convert a legacy [`StageResult`] into a [`StageOutcome`].
fn translate_result(result: StageResult) -> StageOutcome {
    if result.passed {
        StageOutcome::allow(1.0)
    } else {
        let reason = result
            .message
            .unwrap_or_else(|| "legacy stage blocked".into());
        StageOutcome::block(reason, Severity::Medium)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{PipelineError, SecurityStage, StageResult};

    // Alias the *legacy* SecurityContext to avoid confusion with the
    // new `stage::SecurityContext` imported via `super::*`.
    type LegacyCtx = crate::pipeline::SecurityContext;

    struct LegacyPass;

    #[async_trait]
    impl SecurityStage for LegacyPass {
        fn name(&self) -> &str {
            "legacy_pass"
        }
        async fn execute(
            &self,
            _input: &str,
            _ctx: &LegacyCtx,
        ) -> Result<StageResult, PipelineError> {
            Ok(StageResult::pass())
        }
    }

    struct LegacyFail;

    #[async_trait]
    impl SecurityStage for LegacyFail {
        fn name(&self) -> &str {
            "legacy_fail"
        }
        async fn execute(
            &self,
            _input: &str,
            _ctx: &LegacyCtx,
        ) -> Result<StageResult, PipelineError> {
            Ok(StageResult::fail("bad content"))
        }
    }

    #[tokio::test]
    async fn adapted_pass_becomes_allow() {
        let adapted = LegacyAdapter::new(LegacyPass);
        let content = Content::Text("hello".into());
        let ctx = SecurityContext::default();

        let outcome = adapted.evaluate(&content, &ctx).await.unwrap();
        assert!(outcome.is_allow());
    }

    #[tokio::test]
    async fn adapted_fail_becomes_block() {
        let adapted = LegacyAdapter::new(LegacyFail);
        let content = Content::Text("evil".into());
        let ctx = SecurityContext::default();

        let outcome = adapted.evaluate(&content, &ctx).await.unwrap();
        assert!(outcome.is_block());
    }

    #[tokio::test]
    async fn non_text_content_skips() {
        let adapted = LegacyAdapter::new(LegacyPass);
        let content = Content::ToolCall {
            tool_name: "fn".into(),
            arguments: serde_json::json!({}),
        };
        let ctx = SecurityContext::default();

        let outcome = adapted.evaluate(&content, &ctx).await.unwrap();
        assert!(outcome.is_skip());
    }

    #[tokio::test]
    async fn adapter_works_with_executor() {
        use crate::pipeline::executor::PipelineExecutor;

        let exec = PipelineExecutor::builder()
            .add_stage(LegacyAdapter::new(LegacyPass))
            .build();

        let content = Content::Text("safe".into());
        let ctx = SecurityContext::default();

        let result = exec.run(&content, &ctx).await.unwrap();
        assert!(result.is_allowed());
    }
}
