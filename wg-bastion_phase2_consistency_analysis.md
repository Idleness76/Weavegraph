# wg-bastion Phase 2 Pattern Consistency Analysis

**Analysis Date:** 2026-02-XX  
**Scope:** Consistency check of Phase 2 proposed types against Phase 1 established patterns  
**Phase 1 Reference Files:** `content.rs`, `outcome.rs`, `stage.rs`, `executor.rs`, `compat.rs`, `config/mod.rs`

---

## Executive Summary

**Overall Pattern Consistency: 8.5/10** ✅ **GOOD**

The Phase 2 plan demonstrates **strong adherence** to Phase 1 established patterns with minor naming and structural inconsistencies identified. The proposed types largely follow the builder pattern, predicate methods, documentation style, and error handling conventions established in Phase 1.

**Critical Issues:** 0  
**Major Issues:** 3  
**Minor Issues:** 5  
**Recommendations:** 8

---

## 1. Config Naming Pattern Consistency ✅

### Phase 1 Pattern
- ✅ No config types yet (base `SecurityPolicy` only)
- ✅ Error enum: `ConfigError` (struct variants with `thiserror`)

### Phase 2 Proposed Types
**From plan lines 361-365, 708-711, 738-739, 998-1015:**

| Proposed Type | Pattern Match | Notes |
|--------------|---------------|-------|
| `SecureTemplate` | ✅ | Struct name (no suffix) |
| `HoneytokenStore` | ✅ | Store suffix follows Rust convention |
| `TemplateScanner` | ✅ | Scanner suffix follows tool/detector pattern |
| `RefusalPolicy` | ✅ | Policy suffix consistent with `SecurityPolicy` |
| `NormalizationStage` | ✅ | Stage suffix matches `GuardrailStage` trait |
| `InjectionStage` | ✅ | Stage suffix matches `GuardrailStage` trait |
| `EnsembleStrategy` | ✅ | Strategy suffix for enum is idiomatic |

### ❌ ISSUE 1.1: Missing `*Config` Types (Minor)

**Expected Phase 1 pattern:**
```rust
// Phase 1: SecurityPolicy struct exists, but no per-module configs yet
pub struct SecurityPolicy {
    pub version: String,
    pub enabled: bool,
    pub fail_mode: FailMode,
}
```

**Phase 2 plan mentions configs but doesn't define them:**
- `ScannerConfig` (mentioned in plan but not detailed)
- `HoneytokenConfig` (mentioned but not defined)
- `IsolationConfig` (mentioned but not defined)
- `RefusalConfig` (implied by `RefusalPolicy` but not defined)
- `NormalizationConfig` (mentioned but not defined)
- `InjectionConfig` (mentioned but not defined)
- `SpotlightConfig` (mentioned but not defined)

**Recommendation:**
Add explicit config struct definitions following Phase 1 pattern:
```rust
/// Configuration for template scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerConfig {
    pub enabled: bool,
    pub entropy_threshold: f32,
    pub patterns: Vec<SecretPattern>,
}

/// Configuration for honeytoken detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoneytokenConfig {
    pub enabled: bool,
    pub rotation_interval: Duration,
    pub encryption_key_source: KeySource,
}
```

---

## 2. Error Type Consistency ✅ Strong

### Phase 1 Pattern
```rust
/// Phase 1: StageError uses thiserror with struct variants
#[derive(Debug, Error)]
pub enum StageError {
    #[error("backend unavailable for stage '{stage}': {reason}")]
    BackendUnavailable { stage: String, reason: String },
    
    #[error("invalid content for stage '{stage}': {reason}")]
    InvalidContent { stage: String, reason: String },
    
    #[error("internal error in stage '{stage}': {source}")]
    Internal {
        stage: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
```

### Phase 2 Proposed Error Types

**From plan - `TemplateError` mentioned but not defined**

**✅ Recommendation: Follow Phase 1 pattern:**
```rust
/// Errors encountered during template operations
#[derive(Debug, Error)]
pub enum TemplateError {
    /// Template placeholder validation failed
    #[error("invalid placeholder '{name}' in template '{template_id}': {reason}")]
    InvalidPlaceholder {
        template_id: String,
        name: String,
        reason: String,
    },
    
    /// Maximum length exceeded during rendering
    #[error("template '{template_id}' exceeded max length {max_len} (got {actual_len})")]
    MaxLengthExceeded {
        template_id: String,
        max_len: usize,
        actual_len: usize,
    },
    
    /// Encryption/decryption failure for honeytokens
    #[error("cryptographic operation failed: {source}")]
    CryptographicFailure {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
```

**❌ ISSUE 2.1: Missing Error Type Definitions (Major)**

Phase 2 plan references errors but doesn't define them:
- `TemplateError` (implied but not defined)
- `ScannerError` (implied but not defined)
- `HoneytokenError` (implied but not defined)
- `RefusalError` (implied but not defined)
- `NormalizationError` (implied but not defined)
- `InjectionError` (implied but not defined)

---

## 3. Builder Pattern Consistency ✅ Excellent

### Phase 1 Pattern
```rust
// Phase 1: Builder with fluent API, #[must_use], returns Self (not Result)
impl SecurityContextBuilder {
    #[must_use]
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = id.into();
        self
    }
    
    #[must_use]
    pub fn build(self) -> SecurityContext { /* ... */ }
}
```

### Phase 2 Proposed Builders

**From plan - `TemplateBuilder` mentioned:**

**✅ Expected consistency:**
```rust
/// Builder for SecureTemplate
#[derive(Debug, Default)]
pub struct TemplateBuilder {
    template_id: String,
    content: String,
    placeholders: Vec<Placeholder>,
    max_length: Option<usize>,
}

impl TemplateBuilder {
    #[must_use]
    pub fn template_id(mut self, id: impl Into<String>) -> Self {
        self.template_id = id.into();
        self
    }
    
    #[must_use]
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }
    
    #[must_use]
    pub fn placeholder(mut self, placeholder: Placeholder) -> Self {
        self.placeholders.push(placeholder);
        self
    }
    
    #[must_use]
    pub fn max_length(mut self, max_len: usize) -> Self {
        self.max_length = Some(max_len);
        self
    }
    
    /// Build the template (validates placeholders, applies defaults)
    #[must_use]
    pub fn build(self) -> SecureTemplate {
        SecureTemplate {
            template_id: self.template_id,
            content: self.content,
            placeholders: self.placeholders,
            max_length: self.max_length.unwrap_or(4096),
        }
    }
}
```

**✅ Phase 2 consistency expected:** All builders should:
1. Return `Self` (not `Result`) from setter methods
2. Mark all methods `#[must_use]`
3. Use fluent API pattern
4. `build()` returns the type directly (validation in `build()` if needed)

---

## 4. Predicate Method Consistency ✅ Excellent

### Phase 1 Pattern
```rust
// Phase 1: is_* predicates for enum variants
impl StageOutcome {
    #[must_use]
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
    
    #[must_use]
    pub fn is_block(&self) -> bool {
        matches!(self, Self::Block { .. })
    }
    
    #[must_use]
    pub fn is_skip(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }
}
```

### Phase 2 Expected Predicates

**For `RefusalMode` (mentioned in plan):**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RefusalMode {
    Block,
    Redact,
    SafeResponse,
    Escalate,
}

impl RefusalMode {
    #[must_use]
    pub fn is_block(&self) -> bool {
        matches!(self, Self::Block)
    }
    
    #[must_use]
    pub fn is_redact(&self) -> bool {
        matches!(self, Self::Redact)
    }
    
    #[must_use]
    pub fn is_safe_response(&self) -> bool {
        matches!(self, Self::SafeResponse)
    }
    
    #[must_use]
    pub fn is_escalate(&self) -> bool {
        matches!(self, Self::Escalate)
    }
}
```

**✅ GOOD:** Plan implies these predicates will be needed.

---

## 5. `#[must_use]` Annotation Consistency ✅

### Phase 1 Pattern
- ✅ All constructors marked `#[must_use]`
- ✅ All builder methods marked `#[must_use]`
- ✅ All getter methods marked `#[must_use]`
- ✅ All predicate methods marked `#[must_use]`

### Phase 2 Expected

**All Phase 2 types should follow:**
```rust
impl SecureTemplate {
    /// Create a new template builder
    #[must_use]
    pub fn builder() -> TemplateBuilder {
        TemplateBuilder::default()
    }
    
    /// Render the template with provided values
    #[must_use]
    pub fn render(&self, values: &HashMap<String, String>) -> Result<String, TemplateError> {
        // ...
    }
    
    /// Get the template ID
    #[must_use]
    pub fn template_id(&self) -> &str {
        &self.template_id
    }
}
```

**✅ Recommendation:** Ensure all Phase 2 types follow this pattern.

---

## 6. `#[non_exhaustive]` on Public Enums ✅

### Phase 1 Pattern
```rust
// Phase 1: All public enums marked #[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Content { /* ... */ }

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StageOutcome { /* ... */ }
```

### Phase 2 Expected Enums

**From plan:**
- `RefusalMode` - ✅ Should be `#[non_exhaustive]`
- `EnsembleStrategy` - ✅ Should be `#[non_exhaustive]`
- `KeySource` (mentioned for honeytoken key management) - ✅ Should be `#[non_exhaustive]`

**❌ ISSUE 6.1: Plan doesn't explicitly state `#[non_exhaustive]` (Minor)**

Add to plan:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]  // ← Explicitly add this
pub enum RefusalMode {
    Block,
    Redact,
    SafeResponse,
    Escalate,
}
```

---

## 7. Stage `id()` Method Consistency ✅

### Phase 1 Pattern
```rust
// Phase 1: GuardrailStage::id() returns &str (not &'static str yet)
#[async_trait]
pub trait GuardrailStage: Send + Sync {
    fn id(&self) -> &str;
    // ...
}
```

### Phase 2 Stages

**From plan:**
- `NormalizationStage` - ✅ Should implement `id() -> &str`
- `InjectionStage` - ✅ Should implement `id() -> &str`

**Expected implementation:**
```rust
pub struct NormalizationStage {
    config: NormalizationConfig,
}

#[async_trait]
impl GuardrailStage for NormalizationStage {
    fn id(&self) -> &str {
        "normalization"
    }
    
    async fn evaluate(
        &self,
        content: &Content,
        ctx: &SecurityContext,
    ) -> Result<StageOutcome, StageError> {
        // ...
    }
    
    fn priority(&self) -> u32 {
        10  // Run early in pipeline
    }
}
```

**✅ GOOD:** Plan implies this pattern.

---

## 8. Metric Labels / `variant_name()` Pattern ✅

### Phase 1 Pattern
```rust
// Phase 1: variant_name() for enum metric labels
impl Content {
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Messages(_) => "messages",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolResult { .. } => "tool_result",
            Self::RetrievedChunks(_) => "retrieved_chunks",
        }
    }
}

impl StageOutcome {
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Allow { .. } => "allow",
            Self::Block { .. } => "block",
            Self::Transform { .. } => "transform",
            Self::Escalate { .. } => "escalate",
            Self::Skip { .. } => "skip",
        }
    }
}
```

### Phase 2 Expected Pattern

**For `RefusalMode`:**
```rust
impl RefusalMode {
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Redact => "redact",
            Self::SafeResponse => "safe_response",
            Self::Escalate => "escalate",
        }
    }
}
```

**For `EnsembleStrategy`:**
```rust
impl EnsembleStrategy {
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::AnyAboveThreshold => "any_above_threshold",
            Self::WeightedAverage { .. } => "weighted_average",
            Self::MajorityVote => "majority_vote",
            Self::MaxScore => "max_score",
        }
    }
}
```

**✅ Recommendation:** Add `variant_name()` to all public enums for telemetry.

---

## 9. Serde Consistency ✅

### Phase 1 Pattern
```rust
// Phase 1: Serde with snake_case renaming
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Content { /* ... */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}
```

### Phase 2 Expected Pattern

**All config types and enums should use serde:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerConfig {
    pub enabled: bool,
    pub entropy_threshold: f32,
    #[serde(default)]
    pub patterns: Vec<SecretPattern>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RefusalMode {
    Block,
    Redact,
    SafeResponse,
    Escalate,
}
```

**✅ GOOD:** Plan implies this.

---

## 10. Display Implementations ✅

### Phase 1 Pattern
```rust
// Phase 1: Display for enums used in logging/metrics
impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}
```

### Phase 2 Expected Pattern

**For `RefusalMode`:**
```rust
impl std::fmt::Display for RefusalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block => write!(f, "block"),
            Self::Redact => write!(f, "redact"),
            Self::SafeResponse => write!(f, "safe_response"),
            Self::Escalate => write!(f, "escalate"),
        }
    }
}
```

**✅ Recommendation:** Add Display for all enums used in logging.

---

## 11. Module Documentation Pattern ✅

### Phase 1 Pattern
```rust
//! Content types flowing through the security pipeline.
//!
//! [`Content`] is the core unit of inspection — every guardrail stage receives
//! a `Content` value and evaluates it against its rules.
//!
//! # Design rationale
//!
//! The previous `SecurityStage::execute(&str, …)` API accepted only plain text.
//! Real LLM applications pass structured messages, tool calls, RAG chunks, and
//! multimodal blobs.  `Content` captures all of these while remaining
//! `Clone + Debug + Send + Sync` for safe async pipeline usage.
```

### Phase 2 Expected Pattern

**For `prompt/mod.rs`:**
```rust
//! System prompt security and template management.
//!
//! This module provides controls to prevent **LLM01:2025 (Prompt Injection)**
//! and **LLM07:2025 (System Prompt Leakage)** attacks through:
//!
//! - [`SecureTemplate`] — typed placeholders, length limits, auto-escaping
//! - [`HoneytokenStore`] — canary trap detection for prompt exfiltration
//! - [`TemplateScanner`] — secret pattern detection (API keys, JWTs, etc.)
//! - [`RefusalPolicy`] — configurable refusal strategies
//!
//! # Design rationale
//!
//! String concatenation for prompt assembly is vulnerable to injection attacks.
//! [`SecureTemplate`] enforces structured composition with validation at
//! template construction time, not runtime.
//!
//! # Example
//!
//! ```rust
//! use wg_bastion::prompt::{SecureTemplate, Placeholder};
//!
//! let template = SecureTemplate::builder()
//!     .template_id("system_prompt")
//!     .content("You are {role}. User query: {query}")
//!     .placeholder(Placeholder::new("role").max_length(50))
//!     .placeholder(Placeholder::new("query").max_length(1000))
//!     .build();
//!
//! let rendered = template.render(&values)?;
//! ```
```

**✅ GOOD:** Follow Phase 1 module rustdoc pattern.

---

## 12. Test Module Pattern ✅

### Phase 1 Pattern
```rust
// Phase 1: Test modules at bottom with helper functions
#[cfg(test)]
mod tests {
    use super::*;
    
    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }
    
    fn ctx() -> SecurityContext {
        SecurityContext::default()
    }
    
    #[test]
    fn text_variant_name() {
        let c = Content::Text("hello".into());
        assert_eq!(c.variant_name(), "text");
    }
    
    #[tokio::test]
    async fn always_allow_stage() {
        let stage = AlwaysAllow;
        let content = Content::Text("hello".into());
        let ctx = SecurityContext::default();
        let outcome = stage.evaluate(&content, &ctx).await.unwrap();
        assert!(outcome.is_allow());
    }
}
```

### Phase 2 Expected Pattern

**For `prompt/template.rs`:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    fn basic_template() -> SecureTemplate {
        SecureTemplate::builder()
            .template_id("test")
            .content("Hello {name}")
            .placeholder(Placeholder::new("name").max_length(50))
            .build()
    }
    
    fn values(name: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("name".to_string(), name.to_string());
        map
    }
    
    #[test]
    fn template_renders_with_valid_placeholder() {
        let template = basic_template();
        let rendered = template.render(&values("Alice")).unwrap();
        assert_eq!(rendered, "Hello Alice");
    }
    
    #[test]
    fn template_rejects_overlength_placeholder() {
        let template = basic_template();
        let long_name = "x".repeat(100);
        let result = template.render(&values(&long_name));
        assert!(result.is_err());
    }
    
    #[test]
    fn template_round_trips_json() {
        let original = basic_template();
        let json = serde_json::to_string(&original).unwrap();
        let restored: SecureTemplate = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.template_id(), "test");
    }
}
```

**✅ Recommendation:** Follow Phase 1 test helper pattern for all Phase 2 modules.

---

## 13. Convenience Constructor Consistency ✅

### Phase 1 Pattern
```rust
// Phase 1: Convenience constructors for common cases
impl StageOutcome {
    #[must_use]
    pub fn allow(confidence: f32) -> Self {
        debug_assert!((0.0..=1.0).contains(&confidence));
        Self::Allow { confidence }
    }
    
    #[must_use]
    pub fn block(reason: impl Into<String>, severity: Severity) -> Self {
        Self::Block {
            reason: reason.into(),
            severity,
        }
    }
    
    #[must_use]
    pub fn skip(reason: impl Into<String>) -> Self {
        Self::Skip {
            reason: reason.into(),
        }
    }
}
```

### Phase 2 Expected Pattern

**For `SecretPattern`:**
```rust
/// Represents a secret pattern for scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretPattern {
    pub id: String,
    pub name: String,
    pub regex: String,
    pub entropy_threshold: Option<f32>,
}

impl SecretPattern {
    /// AWS access key pattern
    #[must_use]
    pub fn aws_key() -> Self {
        Self {
            id: "aws-key".to_string(),
            name: "AWS Access Key".to_string(),
            regex: r"AKIA[0-9A-Z]{16}".to_string(),
            entropy_threshold: None,
        }
    }
    
    /// OpenAI API key pattern
    #[must_use]
    pub fn openai_key() -> Self {
        Self {
            id: "openai-key".to_string(),
            name: "OpenAI API Key".to_string(),
            regex: r"sk-[a-zA-Z0-9]{48}".to_string(),
            entropy_threshold: None,
        }
    }
    
    /// High-entropy string pattern
    #[must_use]
    pub fn high_entropy(threshold: f32) -> Self {
        Self {
            id: "high-entropy".to_string(),
            name: "High Entropy String".to_string(),
            regex: String::new(),
            entropy_threshold: Some(threshold),
        }
    }
}
```

**✅ Recommendation:** Add convenience constructors for common patterns.

---

## 14. Naming Inconsistencies Between Phases

### ❌ ISSUE 14.1: Config Type Naming Ambiguity (Minor)

**Phase 1 established:**
- `SecurityPolicy` (top-level policy struct)
- `FailMode` (enum without "Config" suffix)
- `PolicyBuilder` (builder for `SecurityPolicy`)

**Phase 2 proposed:**
- `RefusalPolicy` (policy struct - ✅ consistent)
- `RefusalMode` (enum - ✅ consistent with `FailMode`)
- `RefusalConfig` (mentioned but conflicts with `RefusalPolicy`)

**Recommendation:**
Choose one naming pattern:

**Option A: Policy + Mode pattern (cleaner)**
```rust
pub struct RefusalPolicy {
    pub mode: RefusalMode,
    pub safe_response_template: Option<String>,
    pub escalation_timeout: Duration,
}

pub enum RefusalMode {
    Block,
    Redact,
    SafeResponse,
    Escalate,
}
```

**Option B: Config pattern (more explicit)**
```rust
pub struct RefusalConfig {
    pub mode: RefusalMode,
    pub safe_response_template: Option<String>,
    pub escalation_timeout: Duration,
}
```

**Recommendation:** Use **Option A** (Policy + Mode) to align with `SecurityPolicy` + `FailMode` pattern.

### ❌ ISSUE 14.2: Store vs Manager Naming (Minor)

**Phase 2 proposed:**
- `HoneytokenStore` - implies storage/persistence
- `TemplateScanner` - implies active processing

**Recommendation:** Be consistent:
- Use `Store` for persistence-focused types
- Use `Manager` for active lifecycle management
- Use `Scanner` for read-only analysis
- Use `Validator` for validation logic

```rust
pub struct HoneytokenStore {  // ✅ Correct - manages persistent state
    // ...
}

pub struct TemplateScanner {  // ✅ Correct - reads and analyzes
    // ...
}

pub struct SessionManager {  // If needed for lifecycle
    // ...
}
```

---

## 15. Structural Consistency with Phase 1

### Module Structure

**Phase 1 pattern:**
```
config/
  mod.rs        (types + builder + validation)

pipeline/
  mod.rs        (trait + basic types)
  content.rs    (Content enum + Message + RetrievedChunk)
  outcome.rs    (StageOutcome + Severity + StageError)
  stage.rs      (GuardrailStage trait + SecurityContext + StageMetrics)
  executor.rs   (PipelineExecutor + ExecutorBuilder + PipelineResult)
  compat.rs     (LegacyAdapter)
```

**Phase 2 expected (from plan lines 360-365):**
```
prompt/
  mod.rs
  template.rs       (SecureTemplate + TemplateBuilder)
  honeytoken.rs     (HoneytokenStore + Honeytoken + HoneytokenDetection)
  scanner.rs        (TemplateScanner + SecretPattern + SecretFinding)
  isolation.rs      (RoleIsolation + BoundaryViolation)
  refusal.rs        (RefusalPolicy + RefusalMode + RefusalAction)
```

**✅ GOOD:** Follows Phase 1 pattern of `mod.rs` + focused sub-modules.

---

## Summary of Issues

### Critical Issues
None.

### Major Issues

1. **ISSUE 2.1:** Missing error type definitions for all Phase 2 modules
   - Need: `TemplateError`, `ScannerError`, `HoneytokenError`, etc.
   - All should use `thiserror::Error` with struct variants

### Minor Issues

1. **ISSUE 1.1:** Missing `*Config` type definitions
2. **ISSUE 6.1:** Plan doesn't explicitly state `#[non_exhaustive]` on enums
3. **ISSUE 14.1:** Config vs Policy naming ambiguity (`RefusalPolicy` vs `RefusalConfig`)
4. **ISSUE 14.2:** Store vs Manager naming could be more consistent

---

## Recommendations

1. ✅ **Add explicit config struct definitions** following Phase 1 pattern
2. ✅ **Define all error enums** using `thiserror` with struct variants
3. ✅ **Mark all public enums `#[non_exhaustive]`** in plan and code
4. ✅ **Standardize on Policy + Mode pattern** (not Policy + Config)
5. ✅ **Add `variant_name()` methods** to all public enums for telemetry
6. ✅ **Add Display implementations** for enums used in logging
7. ✅ **Follow module rustdoc pattern** with architecture diagrams and examples
8. ✅ **Add test helper functions** for each module following Phase 1 pattern

---

## Conclusion

The Phase 2 plan demonstrates **strong consistency** with Phase 1 established patterns. The main gaps are:

1. Missing explicit definitions for config types
2. Missing error type definitions
3. Minor naming ambiguities that should be resolved early

**Overall Assessment: 8.5/10** - Very good consistency with room for improvement in completeness of type definitions.

**Readiness for Implementation:** ✅ **READY** with the above recommendations incorporated.

---

*End of Phase 2 Pattern Consistency Analysis*
