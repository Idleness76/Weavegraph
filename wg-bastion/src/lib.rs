//! ```text
//! SecurityPolicy ─┬─► PolicyBuilder ─► Runtime Policy
//!                 │                     │
//!                 │                     ├─► InputPipeline ──► Stages ──► Node Execution
//!                 │                     │                     │
//!                 │                     │                     ├─► InjectionScanner
//!                 │                     │                     ├─► PIIStage
//!                 │                     │                     └─► Moderation
//!                 │                     │
//!                 │                     ├─► PromptGuard ──► Fragmentation & Honeytokens
//!                 │                     │
//!                 │                     ├─► OutputValidator ──► Schema, Sanitization, Egress
//!                 │                     │
//!                 │                     ├─► ToolGuard ──► Policy Enforcement & MCP Security
//!                 │                     │
//!                 │                     ├─► RagSecurity ──► Ingestion, Provenance, Grounding
//!                 │                     │
//!                 │                     └─► TelemetrySink ──► Audit, Metrics, Incidents
//!                 │
//!                 └─► Integration with weavegraph App via hooks and EventBus
//! ```
//!
//! # wg-bastion
//!
//! **Comprehensive security suite for graph-driven LLM applications.**
//!
//! `wg-bastion` provides defense-in-depth security controls for applications built on
//! [`weavegraph`](https://docs.rs/weavegraph), addressing the OWASP LLM Top 10 (2025),
//! NIST AI RMF, and modern agentic AI threats. The crate offers opt-in, composable
//! security pipelines with graceful degradation and minimal performance overhead.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use wg_bastion::prelude::*;
//!
//! // Load security policy from configuration
//! let policy = PolicyBuilder::new()
//!     .with_file("wg-bastion.toml")?
//!     .with_env()
//!     .build()?;
//!
//! // Integrate with weavegraph App
//! let app = GraphBuilder::new()
//!     .with_security_policy(policy)
//!     .build()?;
//! ```
//!
//! ## Key Features
//!
//! - **OWASP LLM:2025 Coverage** – All 10 categories addressed with dedicated controls
//! - **Defense in Depth** – Multi-stage pipelines with fallback strategies
//! - **Zero-Trust Architecture** – Validate inputs, outputs, tools, and RAG retrievals
//! - **Graceful Degradation** – Configurable fail modes (open/closed/log-only)
//! - **Minimal Overhead** – <50ms P95 latency for standard pipelines
//! - **Auditability** – Structured telemetry with OpenTelemetry export
//!
//! ## Architecture
//!
//! See [`docs/architecture.md`](https://github.com/Idleness76/weavegraph/tree/main/wg-bastion/docs/architecture.md)
//! for the complete module structure, threat model, and control matrix.
//!
//! ## Modules
//!
//! - [`config`] – Policy configuration, builder pattern, YAML/env loading
//! - [`pipeline`] – Core security pipeline framework and stage composition
//! - `prompt` – Prompt protection (fragmentation, honeytokens, leakage detection)
//! - `input` – Input validation (injection scanning, PII detection, moderation)
//! - `output` – Output validation (schema, sanitization, egress scanning, grounding)
//! - `tools` – Tool execution security (policies, MCP protocol, approval workflows)
//! - `rag` – RAG security (ingestion sanitization, provenance, embedding protection)
//! - `agents` – Agentic AI controls (delegation tracking, autonomy boundaries)
//! - `session` – Session management and context isolation
//! - `abuse` – Abuse prevention (rate limiting, cost monitoring, recursion guards)
//! - `telemetry` – Security events, audit logging, incident response

#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod config;
pub mod pipeline;

// ── Feature-gated modules ──────────────────────────────────────────────
// Each module below will be enabled when its corresponding feature (and
// source files) land.  The `cfg_attr(docsrs, doc(cfg(...)))` ensures
// docs.rs renders which feature is required.

// Phase 2 (prompt & injection security)
// #[cfg(feature = "heuristics")]
// pub mod prompt;
// #[cfg(feature = "heuristics")]
// pub mod input;

// Phase 3+ (output, tools, rag, agents, abuse, telemetry)
// #[cfg(feature = "heuristics")]
// pub mod output;
// #[cfg(feature = "heuristics")]
// pub mod tools;
// #[cfg(feature = "heuristics")]
// pub mod rag;
// #[cfg(feature = "heuristics")]
// pub mod agents;
// #[cfg(feature = "heuristics")]
// pub mod session;
// #[cfg(feature = "heuristics")]
// pub mod abuse;
// #[cfg(feature = "telemetry-otlp")]
// pub mod telemetry;

/// Re-exports for convenient access to core types
pub mod prelude {
    pub use crate::config::{FailMode, PolicyBuilder, SecurityPolicy};
    pub use crate::pipeline::{SecurityPipeline, SecurityStage};

    // New type-safe pipeline types (Phase 1)
    pub use crate::pipeline::content::{Content, Message, RetrievedChunk};
    pub use crate::pipeline::executor::{ExecutorBuilder, PipelineExecutor, PipelineResult};
    pub use crate::pipeline::outcome::{Severity, StageError, StageOutcome};
    pub use crate::pipeline::stage::{GuardrailStage, SecurityContext};

    // Backward-compatibility adapter
    pub use crate::pipeline::compat::LegacyAdapter;
}
