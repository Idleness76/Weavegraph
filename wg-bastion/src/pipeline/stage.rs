//! The [`GuardrailStage`] trait — the primary evaluation interface for the
//! new pipeline framework.
//!
//! A guardrail stage receives a [`Content`] value and a [`SecurityContext`],
//! and returns a [`StageOutcome`].  Stages are composed into a
//! [`PipelineExecutor`](super::executor::PipelineExecutor) which orchestrates
//! execution order, fail-mode, caching, and metrics.
//!
//! # Implementing a stage
//!
//! ```rust,ignore
//! use wg_bastion::pipeline::{
//!     content::Content,
//!     outcome::{StageOutcome, StageError},
//!     stage::{GuardrailStage, SecurityContext},
//! };
//!
//! struct MyDetector;
//!
//! #[async_trait::async_trait]
//! impl GuardrailStage for MyDetector {
//!     fn id(&self) -> &str { "my_detector" }
//!
//!     async fn evaluate(
//!         &self,
//!         content: &Content,
//!         _ctx: &SecurityContext,
//!     ) -> Result<StageOutcome, StageError> {
//!         // inspection logic …
//!         Ok(StageOutcome::allow(1.0))
//!     }
//! }
//! ```

use super::content::Content;
use super::outcome::{StageError, StageOutcome};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Maximum depth for delegation parent chains. Beyond this limit,
/// `SecurityContext::child()` will omit the parent link to prevent
/// unbounded memory growth from deeply nested agent delegations.
const MAX_DELEGATION_DEPTH: usize = 64;

// ── SecurityContext ────────────────────────────────────────────────────

/// Contextual information passed to every guardrail stage.
///
/// Carries session identity, accumulated risk scoring, and a parent chain
/// for agent delegation tracking.  The context is **immutable** from a
/// stage's perspective — stages communicate downstream via their
/// [`StageOutcome`] and the pipeline's metadata aggregation.
///
/// ```rust
/// use wg_bastion::pipeline::stage::SecurityContext;
///
/// let ctx = SecurityContext::builder()
///     .session_id("sess-001")
///     .user_id("user-42")
///     .risk_score(0.0)
///     .build();
///
/// assert_eq!(ctx.session_id(), "sess-001");
/// ```
#[derive(Debug, Clone)]
pub struct SecurityContext {
    session_id: String,
    user_id: Option<String>,
    risk_score: f32,
    metadata: HashMap<String, serde_json::Value>,
    parent: Option<Arc<SecurityContext>>,
}

impl Default for SecurityContext {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            user_id: None,
            risk_score: 0.0,
            metadata: HashMap::new(),
            parent: None,
        }
    }
}

impl SecurityContext {
    /// Start building a context.
    #[must_use]
    pub fn builder() -> SecurityContextBuilder {
        SecurityContextBuilder::default()
    }

    /// The session identifier.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The user identifier, if authenticated.
    #[must_use]
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Accumulated risk score across the pipeline (0.0 = safe, 1.0 = certain threat).
    #[must_use]
    pub fn risk_score(&self) -> f32 {
        self.risk_score
    }

    /// Arbitrary metadata attached to this request.
    #[must_use]
    pub fn metadata(&self) -> &HashMap<String, serde_json::Value> {
        &self.metadata
    }

    /// Get a single metadata value.
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// The parent context in a delegation chain (for agent-to-agent flows).
    #[must_use]
    pub fn parent(&self) -> Option<&Arc<SecurityContext>> {
        self.parent.as_ref()
    }

    /// Walk the delegation chain and return the depth (0 = no parent).
    #[must_use]
    pub fn delegation_depth(&self) -> usize {
        let mut depth = 0;
        let mut current = self.parent.as_ref();
        while let Some(p) = current {
            depth += 1;
            current = p.parent.as_ref();
        }
        depth
    }

    /// Derive a child context for agent delegation, creating a parent link.
    ///
    /// If the delegation chain has reached [`MAX_DELEGATION_DEPTH`], the
    /// parent link is **silently omitted** to prevent unbounded memory
    /// growth.  The child is still created with inherited identity and
    /// risk score — only the ancestry chain is truncated.
    #[must_use]
    pub fn child(&self, session_id: impl Into<String>) -> Self {
        // Cap the parent chain to avoid unbounded memory from deep delegation.
        let parent = if self.delegation_depth() >= MAX_DELEGATION_DEPTH {
            None
        } else {
            Some(Arc::new(self.clone()))
        };

        Self {
            session_id: session_id.into(),
            user_id: self.user_id.clone(),
            risk_score: self.risk_score,
            metadata: HashMap::new(),
            parent,
        }
    }

    /// Create a copy with an updated risk score.
    #[must_use]
    pub fn with_risk_score(mut self, score: f32) -> Self {
        self.risk_score = score;
        self
    }

    /// Create a copy with additional metadata merged in.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

// ── SecurityContextBuilder ─────────────────────────────────────────────

/// Builder for [`SecurityContext`].
#[derive(Debug, Default)]
pub struct SecurityContextBuilder {
    session_id: String,
    user_id: Option<String>,
    risk_score: f32,
    metadata: HashMap<String, serde_json::Value>,
    parent: Option<Arc<SecurityContext>>,
}

impl SecurityContextBuilder {
    /// Set the session identifier.
    #[must_use]
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = id.into();
        self
    }

    /// Set the user identifier.
    #[must_use]
    pub fn user_id(mut self, id: impl Into<String>) -> Self {
        self.user_id = Some(id.into());
        self
    }

    /// Set the initial risk score.
    ///
    /// # Panics (debug only)
    ///
    /// Debug-asserts that `score` is in \[0.0, 1.0\].
    #[must_use]
    pub fn risk_score(mut self, score: f32) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&score),
            "risk_score must be in [0.0, 1.0], got {score}",
        );
        self.risk_score = score;
        self
    }

    /// Add a metadata entry.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Link to a parent context (for delegation chains).
    #[must_use]
    pub fn parent(mut self, parent: Arc<SecurityContext>) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Build the context.
    #[must_use]
    pub fn build(self) -> SecurityContext {
        SecurityContext {
            session_id: self.session_id,
            user_id: self.user_id,
            risk_score: self.risk_score,
            metadata: self.metadata,
            parent: self.parent,
        }
    }
}

// ── GuardrailStage trait ───────────────────────────────────────────────

/// A single composable security check in the pipeline.
///
/// Implementations are expected to be cheap to clone (or internally
/// `Arc`-wrapped) and safe to share across Tokio tasks.
///
/// # Contract
///
/// - [`evaluate`](Self::evaluate) must be **pure** with respect to `self` —
///   it must not mutate internal state between calls.
/// - If the stage encounters an internal error, return `Err(StageError)`.
///   The pipeline will consult [`degradable`](Self::degradable) to decide
///   whether to skip the stage or abort.
/// - Stages should complete within a few milliseconds.  Long-running
///   backends (remote APIs, heavy ML models) should use the circuit
///   breaker provided by the pipeline layer.
#[async_trait]
pub trait GuardrailStage: Send + Sync {
    /// Unique identifier for this stage (e.g. `"injection_heuristic"`).
    ///
    /// Used for logging, metrics labels, and deduplication.
    fn id(&self) -> &str;

    /// Evaluate the given content against this guardrail.
    ///
    /// # Errors
    ///
    /// Returns [`StageError`] if the evaluation cannot complete (backend
    /// unavailable, invalid content format, internal bug).
    async fn evaluate(
        &self,
        content: &Content,
        ctx: &SecurityContext,
    ) -> Result<StageOutcome, StageError>;

    /// Whether the pipeline may skip this stage on error instead of aborting.
    ///
    /// Defaults to `true` (graceful degradation).  Override to `false` for
    /// stages that are critical and must never be skipped.
    fn degradable(&self) -> bool {
        true
    }

    /// Execution priority — lower values run first.
    ///
    /// The [`PipelineExecutor`](super::executor::PipelineExecutor) sorts
    /// stages by priority before execution.  Default is `100`.
    fn priority(&self) -> u32 {
        100
    }
}

// ── StageMetrics ───────────────────────────────────────────────────────

/// Metrics captured for a single stage execution.
///
/// Produced by the [`PipelineExecutor`](super::executor::PipelineExecutor)
/// for each stage and aggregated in the pipeline result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageMetrics {
    /// Stage identifier.
    pub stage_id: String,
    /// Wall-clock execution time.
    pub duration: std::time::Duration,
    /// Whether the stage returned from cache.
    pub cache_hit: bool,
    /// Whether the stage ran in degraded mode due to an error.
    pub degraded: bool,
    /// The outcome variant name (e.g. `"allow"`, `"block"`).
    pub outcome: String,
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::outcome::Severity;

    struct AlwaysAllow;

    #[async_trait]
    impl GuardrailStage for AlwaysAllow {
        fn id(&self) -> &str {
            "always_allow"
        }

        async fn evaluate(
            &self,
            _content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            Ok(StageOutcome::allow(1.0))
        }
    }

    struct AlwaysBlock;

    #[async_trait]
    impl GuardrailStage for AlwaysBlock {
        fn id(&self) -> &str {
            "always_block"
        }

        async fn evaluate(
            &self,
            _content: &Content,
            _ctx: &SecurityContext,
        ) -> Result<StageOutcome, StageError> {
            Ok(StageOutcome::block("threat detected", Severity::High))
        }

        fn degradable(&self) -> bool {
            false
        }

        fn priority(&self) -> u32 {
            10
        }
    }

    #[test]
    fn context_builder() {
        let ctx = SecurityContext::builder()
            .session_id("s1")
            .user_id("u1")
            .risk_score(0.5)
            .metadata("source", serde_json::json!("test"))
            .build();

        assert_eq!(ctx.session_id(), "s1");
        assert_eq!(ctx.user_id(), Some("u1"));
        assert!((ctx.risk_score() - 0.5).abs() < f32::EPSILON);
        assert_eq!(ctx.get_metadata("source"), Some(&serde_json::json!("test")));
    }

    #[test]
    fn delegation_chain() {
        let root = SecurityContext::builder().session_id("root").build();
        let child = root.child("child-1");
        let grandchild = child.child("child-2");

        assert_eq!(root.delegation_depth(), 0);
        assert_eq!(child.delegation_depth(), 1);
        assert_eq!(grandchild.delegation_depth(), 2);
        assert_eq!(grandchild.parent().unwrap().session_id(), "child-1");
    }

    #[test]
    fn test_delegation_depth_normal() {
        let root = SecurityContext::builder().session_id("root").build();
        let c1 = root.child("c1");
        let c2 = c1.child("c2");
        let c3 = c2.child("c3");

        assert_eq!(c3.delegation_depth(), 3);
    }

    #[test]
    fn test_delegation_depth_limit() {
        let mut ctx = SecurityContext::builder().session_id("d-0").build();
        for i in 1..=MAX_DELEGATION_DEPTH + 1 {
            ctx = ctx.child(format!("d-{i}"));
        }
        // Depth must never exceed the configured maximum.
        assert!(ctx.delegation_depth() <= MAX_DELEGATION_DEPTH);
    }

    #[tokio::test]
    async fn always_allow_stage() {
        let stage = AlwaysAllow;
        let content = Content::Text("hello".into());
        let ctx = SecurityContext::default();
        let outcome = stage.evaluate(&content, &ctx).await.unwrap();
        assert!(outcome.is_allow());
    }

    #[tokio::test]
    async fn always_block_stage() {
        let stage = AlwaysBlock;
        assert_eq!(stage.priority(), 10);
        assert!(!stage.degradable());

        let content = Content::Text("malicious".into());
        let ctx = SecurityContext::default();
        let outcome = stage.evaluate(&content, &ctx).await.unwrap();
        assert!(outcome.is_block());
    }
}
