//! Core security pipeline framework.
//!
//! This module provides the foundational abstractions for composing multi-stage
//! security pipelines with support for:
//!
//! - Async execution with Tokio
//! - Stage ordering and dependencies
//! - Metadata propagation across stages
//! - Graceful degradation on stage failures
//! - Conditional stage execution
//!
//! ## Architecture
//!
//! ```text
//! SecurityPipeline
//!   ├─► Stage 1 (Injection Scanner)
//!   ├─► Stage 2 (PII Detection)      ◄── Conditional
//!   ├─► Stage 3 (Moderation)         ◄── Optional
//!   └─► Stage N (...)
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use wg_bastion::pipeline::{SecurityPipeline, SecurityStage};
//!
//! let pipeline = SecurityPipeline::builder()
//!     .add_stage(InjectionScanner::new())
//!     .add_stage(PIIDetector::new())
//!     .build();
//!
//! let result = pipeline.execute(&input, &ctx).await?;
//! ```

// ── New type-safe pipeline submodules ──────────────────────────────────
pub mod compat;
pub mod content;
pub mod executor;
pub mod outcome;
pub mod stage;

use async_trait::async_trait;
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during pipeline execution
#[derive(Debug, Error)]
pub enum PipelineError {
    /// A security stage detected a threat
    #[error("Security threat detected in stage '{stage}': {reason}")]
    ThreatDetected {
        /// Name of the stage that detected the threat
        stage: String,
        /// Description of the threat
        reason: String,
    },

    /// A stage failed to execute
    #[error("Stage '{stage}' failed: {source}")]
    StageFailure {
        /// Name of the failed stage
        stage: String,
        /// Underlying error
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Pipeline configuration is invalid
    #[error("Invalid pipeline configuration: {0}")]
    Configuration(String),
}

/// Context passed through pipeline stages
#[derive(Debug, Clone, Default)]
pub struct SecurityContext {
    /// Arbitrary metadata for stages to share information
    pub metadata: HashMap<String, String>,

    /// Session identifier for correlation
    pub session_id: Option<String>,

    /// User identifier (if authenticated)
    pub user_id: Option<String>,
}

impl SecurityContext {
    /// Create a new empty security context
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add metadata to the context
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set the session ID
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }
}

/// Result of a security stage execution
#[derive(Debug, Clone)]
pub struct StageResult {
    /// Whether the stage passed (true) or detected a threat (false)
    pub passed: bool,

    /// Optional message explaining the result
    pub message: Option<String>,

    /// Metadata to propagate to subsequent stages
    pub metadata: HashMap<String, String>,
}

impl StageResult {
    /// Create a passing result
    #[must_use]
    pub fn pass() -> Self {
        Self {
            passed: true,
            message: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a failing result with a reason
    #[must_use]
    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            passed: false,
            message: Some(message.into()),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the result
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Trait for security pipeline stages
#[async_trait]
pub trait SecurityStage: Send + Sync {
    /// Name of the stage for logging and error reporting
    fn name(&self) -> &str;

    /// Execute the stage on the given input
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if the stage execution fails
    async fn execute(
        &self,
        input: &str,
        ctx: &SecurityContext,
    ) -> Result<StageResult, PipelineError>;

    /// Whether this stage should run based on context (default: always)
    fn should_run(&self, _ctx: &SecurityContext) -> bool {
        true
    }
}

/// Multi-stage security pipeline
#[derive(Default)]
pub struct SecurityPipeline {
    stages: Vec<Box<dyn SecurityStage>>,
}

impl SecurityPipeline {
    /// Create a new pipeline builder
    #[must_use]
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// Execute the pipeline on the given input
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if any stage fails or detects a threat
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SecurityContext,
    ) -> Result<(), PipelineError> {
        for stage in &self.stages {
            if !stage.should_run(ctx) {
                tracing::debug!(stage = stage.name(), "Skipping conditional stage");
                continue;
            }

            tracing::trace!(stage = stage.name(), "Executing security stage");

            let result = stage.execute(input, ctx).await?;

            // Propagate metadata
            ctx.metadata.extend(result.metadata);

            if !result.passed {
                return Err(PipelineError::ThreatDetected {
                    stage: stage.name().to_string(),
                    reason: result
                        .message
                        .unwrap_or_else(|| "Unknown threat".to_string()),
                });
            }
        }

        Ok(())
    }
}

/// Builder for constructing security pipelines
#[derive(Default)]
pub struct PipelineBuilder {
    stages: Vec<Box<dyn SecurityStage>>,
}

impl PipelineBuilder {
    /// Create a new pipeline builder
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a stage to the pipeline
    #[must_use]
    pub fn add_stage(mut self, stage: impl SecurityStage + 'static) -> Self {
        self.stages.push(Box::new(stage));
        self
    }

    /// Build the final pipeline
    #[must_use]
    pub fn build(self) -> SecurityPipeline {
        SecurityPipeline {
            stages: self.stages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PassingStage;

    #[async_trait]
    impl SecurityStage for PassingStage {
        fn name(&self) -> &'static str {
            "passing_stage"
        }

        async fn execute(
            &self,
            _input: &str,
            _ctx: &SecurityContext,
        ) -> Result<StageResult, PipelineError> {
            Ok(StageResult::pass())
        }
    }

    struct FailingStage;

    #[async_trait]
    impl SecurityStage for FailingStage {
        fn name(&self) -> &'static str {
            "failing_stage"
        }

        async fn execute(
            &self,
            _input: &str,
            _ctx: &SecurityContext,
        ) -> Result<StageResult, PipelineError> {
            Ok(StageResult::fail("Test threat"))
        }
    }

    #[tokio::test]
    async fn test_passing_pipeline() {
        let pipeline = SecurityPipeline::builder().add_stage(PassingStage).build();

        let mut ctx = SecurityContext::new();
        let result = pipeline.execute("test input", &mut ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_failing_pipeline() {
        let pipeline = SecurityPipeline::builder()
            .add_stage(PassingStage)
            .add_stage(FailingStage)
            .build();

        let mut ctx = SecurityContext::new();
        let result = pipeline.execute("test input", &mut ctx).await;
        assert!(result.is_err());

        if let Err(PipelineError::ThreatDetected { stage, reason }) = result {
            assert_eq!(stage, "failing_stage");
            assert_eq!(reason, "Test threat");
        }
    }

    #[test]
    fn test_security_context_builder() {
        let ctx = SecurityContext::new()
            .with_session_id("session123")
            .with_metadata("key", "value");

        assert_eq!(ctx.session_id, Some("session123".to_string()));
        assert_eq!(ctx.metadata.get("key"), Some(&"value".to_string()));
    }
}
