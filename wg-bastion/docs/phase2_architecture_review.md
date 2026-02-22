# wg-bastion Phase 2 Architecture Review
## Prompt & Injection Security Module Design

**Date:** February 2026  
**Reviewer:** System Architecture Expert  
**Scope:** Phase 2 (WS4 Prompt Hardening + WS5 Injection Detection)  
**Status:** Pre-Implementation Design Review

---

## Executive Summary

This review evaluates the Phase 2 architectural design against established patterns in the Phase 1 implementation (Content, StageOutcome, GuardrailStage, PipelineExecutor, SecurityContext) for pattern compliance, extensibility, and design integrity.

**Overall Assessment: STRONG with 5 CRITICAL recommendations**

The design demonstrates excellent separation of concerns, consistent application of the GuardrailStage pattern, and thoughtful feature gating. However, there are architectural tensions around RefusalPolicy/FailMode interaction, the static pattern library approach, and Transform propagation semantics that must be resolved before implementation.

---

## 1. Module Decomposition & Separation of Concerns

### Assessment: ✅ STRONG

**Strengths:**
- Clear separation between **compile-time concerns** (prompt/) and **runtime concerns** (input/)
- Each module has a well-defined responsibility aligned with OWASP threat categories:
  - `prompt/template.rs` → LLM07 (system prompt construction)
  - `prompt/scanner.rs` → LLM02/LLM07 (secret detection)
  - `prompt/isolation.rs` → LLM01 (boundary enforcement)
  - `prompt/honeytoken.rs` → LLM02/LLM07 (exfiltration detection)
  - `input/normalization.rs` → LLM01/LLM05 (canonicalization)
  - `input/injection.rs` → LLM01 (threat detection)

**Concerns:**
- `prompt/refusal.rs` overlaps with `PipelineExecutor`'s FailMode logic (see §6)
- `input/spotlight.rs` as a separate module feels like over-abstraction — it's specific to InjectionStage's RAG handling

### Recommendation:

**R1.1 [HIGH]:** Keep `spotlight.rs` as a **sub-module of `injection.rs`** rather than a top-level file:
```rust
// input/injection/mod.rs
pub mod heuristic;
pub mod structural;
pub mod ensemble;
pub mod spotlight;  // RAG-specific injection detection

pub struct InjectionStage {
    heuristic: HeuristicDetector,
    structural: StructuralAnalyzer,
    spotlight: Option<SpotlightDetector>,  // Only when Content::RetrievedChunks
    // ...
}
```

**Rationale:** Spotlight is a specialized detection mode for RAG content, not a general-purpose stage. Nesting it under `injection` makes the relationship explicit and avoids confusion about when to use it.

---

## 2. GuardrailStage Implementation Scope

### Assessment: ✅ EXCELLENT

Each stage has a **single, well-defined responsibility** that maps cleanly to the GuardrailStage contract:

| Stage | Single Responsibility | Appropriate Scope? |
|-------|----------------------|-------------------|
| `TemplateScanner` | Detect secrets/honeytokens in prompts | ✅ Yes |
| `RoleIsolation` | Validate delimiter boundaries | ✅ Yes |
| `NormalizationStage` | Canonicalize input | ✅ Yes |
| `InjectionStage` | Detect injection attempts | ✅ Yes |
| `SpotlightStage` | Mark RAG boundaries | ⚠️ See R1.1 |

**Design Pattern Consistency:**

All stages correctly implement the established pattern:
```rust
#[async_trait]
impl GuardrailStage for NormalizationStage {
    fn id(&self) -> &str { "normalization" }
    fn priority(&self) -> u32 { 10 }  // Runs first
    fn degradable(&self) -> bool { false }  // Critical for security
    
    async fn evaluate(&self, content: &Content, ctx: &SecurityContext) 
        -> Result<StageOutcome, StageError> 
    {
        match content {
            Content::Text(s) => {
                let normalized = self.normalize(s)?;
                Ok(StageOutcome::Transform {
                    content: Content::Text(normalized),
                    description: "Unicode NFKC + HTML stripped".into()
                })
            }
            _ => Ok(StageOutcome::Skip { reason: "text only".into() })
        }
    }
}
```

### Recommendation:

**R2.1 [LOW]:** Document the **variant-matching pattern** in `docs/architecture.md` as a convention:
```rust
// CONVENTION: Stages should match on Content variants and Skip when not applicable
match content {
    Content::Text(_) | Content::Messages(_) => { /* analyze */ }
    Content::ToolCall { .. } | Content::ToolResult { .. } => {
        Ok(StageOutcome::skip("user-facing content only"))
    }
    // Don't use catch-all `_` — be explicit about what's skipped
}
```

---

## 3. Feature Gate Strategy

### Assessment: ✅ SOUND with minor improvements needed

**Current Strategy:**
```toml
[features]
default = ["heuristics"]
heuristics = []  # Pattern-based detection (no ML)
honeytoken = ["ring", "zeroize"]  # Crypto for honeytoken encryption
normalization-html = ["lol_html"]  # HTML sanitization
```

**Strengths:**
- `heuristics` is a **zero-dependency default** — excellent for adoption
- Crypto dependencies (`ring`, `zeroize`) isolated behind `honeytoken` gate
- `lol_html` gated separately since HTML normalization is optional

**Concerns:**
1. `honeytoken` feature requires `ring` (native crypto) — may not compile on all targets
2. No feature flag for pattern library source (static vs. runtime-configurable)
3. Feature combinations not validated (e.g., what if user enables `honeytoken` but disables `heuristics`?)

### Recommendations:

**R3.1 [MEDIUM]:** Add **pure-Rust crypto alternative**:
```toml
honeytoken = []  # Meta-feature
honeytoken-ring = ["honeytoken", "ring", "zeroize"]  # Fast native crypto
honeytoken-pure = ["honeytoken", "aes-gcm", "chacha20poly1305"]  # Pure Rust fallback
```

**R3.2 [LOW]:** Add **pattern source feature**:
```toml
patterns-static = []  # Compile-time patterns (current approach)
patterns-dynamic = ["serde_json"]  # Load patterns from JSON at runtime
```

**R3.3 [MEDIUM]:** Add **feature validation** in `build.rs`:
```rust
// build.rs
fn main() {
    #[cfg(all(feature = "honeytoken", not(any(feature = "honeytoken-ring", feature = "honeytoken-pure"))))]
    compile_error!("honeytoken requires either honeytoken-ring or honeytoken-pure");
}
```

---

## 4. Pattern Library: Static vs. Configurable

### Assessment: ⚠️ ARCHITECTURAL DECISION REQUIRED

**Current Plan:** Static pattern library in `input/patterns.rs`:
```rust
pub struct PatternLibrary {
    categories: HashMap<&'static str, Vec<Pattern>>,
}

impl PatternLibrary {
    pub fn default() -> Self {
        // 50+ patterns compiled in
    }
}
```

**Trade-off Analysis:**

| Approach | Pros | Cons |
|----------|------|------|
| **Static** | Zero runtime overhead, type-safe, can't be misconfigured | Requires recompile to update patterns, large binary size |
| **Runtime** | Hot-reload patterns, A/B testing, per-tenant patterns | Deserialization overhead, schema validation needed |
| **Hybrid** | Defaults static, user can override at runtime | Best of both worlds | Complexity in merge semantics |

### Recommendation:

**R4.1 [CRITICAL]:** Implement **hybrid approach** for Phase 2:

```rust
// input/patterns.rs
#[derive(Clone)]
pub struct PatternLibrary {
    static_patterns: &'static [Pattern],
    runtime_overrides: Vec<Pattern>,
}

impl PatternLibrary {
    /// Built-in patterns (zero-cost)
    pub fn builtin() -> Self {
        Self {
            static_patterns: &BUILTIN_PATTERNS,
            runtime_overrides: Vec::new(),
        }
    }
    
    /// Load additional patterns from JSON (behind feature flag)
    #[cfg(feature = "patterns-dynamic")]
    pub fn with_overrides(mut self, path: impl AsRef<Path>) -> Result<Self, PatternError> {
        let json = std::fs::read_to_string(path)?;
        let patterns: Vec<Pattern> = serde_json::from_str(&json)?;
        self.runtime_overrides = patterns;
        Ok(self)
    }
    
    /// Search with static first (for cache locality), then overrides
    pub fn matches(&self, text: &str) -> Vec<Match> {
        let mut matches = Vec::new();
        
        // Static patterns (hot path)
        for pattern in self.static_patterns {
            if let Some(m) = pattern.find(text) {
                matches.push(m);
            }
        }
        
        // Runtime overrides
        for pattern in &self.runtime_overrides {
            if let Some(m) = pattern.find(text) {
                matches.push(m);
            }
        }
        
        matches
    }
}
```

**Rationale:**
- **MVP (Phase 2):** Static patterns ship compiled-in, zero config needed
- **Production (Phase 3+):** Power users can add custom patterns without recompiling
- **Performance:** Static patterns stay in `.rodata`, no deserialization cost
- **Extension Point:** Clear path for Phase 4 ML classifiers to add learned patterns

---

## 5. StructuralAnalyzer & EnsembleScorer: Public API or Implementation Detail?

### Assessment: ⚠️ ENCAPSULATION DECISION NEEDED

**Current Plan:**
```
input/
  injection.rs         (InjectionStage)
  structural.rs        (StructuralAnalyzer — pub?)
  ensemble.rs          (EnsembleScorer — pub?)
  patterns.rs          (HeuristicDetector — pub?)
```

**Options:**

| Approach | Visibility | Rationale |
|----------|-----------|-----------|
| **A: All public** | `pub mod structural; pub mod ensemble;` | Users can compose custom detectors |
| **B: Internal only** | `mod structural; mod ensemble;` (private) | InjectionStage is the only API surface |
| **C: Sealed traits** | Public types, sealed trait prevents external impls | Discoverable but controlled |

### Recommendation:

**R5.1 [HIGH]:** Use **sealed trait pattern** for extensibility without instability:

```rust
// input/injection/mod.rs
pub use self::structural::StructuralAnalyzer;
pub use self::ensemble::{EnsembleScorer, EnsembleStrategy};

mod sealed {
    pub trait Sealed {}
}

/// Detector that can contribute to ensemble scoring.
/// 
/// This trait is sealed — you cannot implement it for your own types,
/// but you can configure the built-in detectors.
pub trait Detector: sealed::Sealed {
    fn score(&self, content: &Content) -> f32;
}

// Only our internal types can implement Detector
impl sealed::Sealed for HeuristicDetector {}
impl sealed::Sealed for StructuralAnalyzer {}
impl sealed::Sealed for SpotlightDetector {}

impl Detector for HeuristicDetector {
    fn score(&self, content: &Content) -> f32 { /* ... */ }
}
```

**Rationale:**
- **Phase 2:** Users configure `InjectionStage` with ensemble strategy, but don't implement custom detectors
- **Phase 3+:** When ML classifiers land, we can unseal the trait if needed
- **API Stability:** Sealed trait prevents users from depending on unstable internals

**Alternative for Power Users:**
```rust
// InjectionStage builder exposes configuration, not types
let stage = InjectionStage::builder()
    .enable_heuristics(true)
    .enable_structural(true)
    .enable_spotlight(true)  // When Content::RetrievedChunks
    .ensemble_strategy(EnsembleStrategy::WeightedAverage {
        weights: [
            ("heuristic", 0.4),
            ("structural", 0.4),
            ("spotlight", 0.2),
        ].into()
    })
    .build();
```

---

## 6. RefusalPolicy vs. FailMode Interaction

### Assessment: ⚠️ **CRITICAL ARCHITECTURAL CONFLICT**

**The Problem:**

Two overlapping policies control blocking behavior:

1. **FailMode** (global, in `PipelineExecutor`):
   - `Closed`: Block → actually blocks
   - `Open`: Block → allow (log only)
   - `LogOnly`: Block → allow (log only)

2. **RefusalPolicy** (per-stage, in `prompt/refusal.rs`):
   - `Block`: Return hard block
   - `Redact`: Remove sensitive parts
   - `SafeResponse`: Replace with canned text
   - `Escalate`: Wait for human approval

**Conflict Scenarios:**

```rust
// Scenario 1: What happens here?
let executor = PipelineExecutor::builder()
    .fail_mode(FailMode::Open)  // "Allow everything"
    .add_stage(TemplateScanner {
        refusal: RefusalPolicy::Block  // "Block on secrets"
    })
    .build();

// Does the block get overridden? If so, RefusalPolicy is meaningless.
```

```rust
// Scenario 2: Redundant configuration?
let executor = PipelineExecutor::builder()
    .fail_mode(FailMode::Closed)
    .add_stage(TemplateScanner {
        refusal: RefusalPolicy::SafeResponse("Sorry, I can't help with that.")
    })
    .build();

// TemplateScanner returns Transform (safe response)
// But FailMode::Closed expects Block for hard stops
// Should SafeResponse be treated as Allow or Block?
```

### Recommendation:

**R6.1 [CRITICAL]:** **Clarify the hierarchy** — FailMode is a **policy override**, RefusalPolicy is a **remediation strategy**:

```rust
// DESIGN PRINCIPLE:
// - RefusalPolicy determines WHAT to do when a threat is detected
// - FailMode determines WHETHER that decision is enforced

impl PipelineExecutor {
    fn apply_fail_mode(&self, outcome: StageOutcome, overridden: &mut bool) -> StageOutcome {
        match (&self.fail_mode, &outcome) {
            // Block outcomes respect FailMode
            (FailMode::Open | FailMode::LogOnly, StageOutcome::Block { .. }) => {
                *overridden = true;
                StageOutcome::allow(0.0)
            }
            
            // Transform outcomes BYPASS FailMode (they're remediations, not blocks)
            (_, StageOutcome::Transform { .. }) => outcome,
            
            // SafeResponse is a Transform, not a Block
            (_, StageOutcome::Block { reason, .. }) if reason.contains("safe_response:") => {
                // This should never happen — stages using RefusalPolicy::SafeResponse
                // should return Transform, not Block
                panic!("Stage returned Block when it should use Transform for safe responses");
            }
            
            // Escalate outcomes respect FailMode timeout but not the escalation itself
            (FailMode::Open, StageOutcome::Escalate { .. }) => {
                *overridden = true;
                StageOutcome::allow(0.0)
            }
            
            _ => outcome
        }
    }
}
```

**R6.2 [CRITICAL]:** **Redefine RefusalPolicy semantics**:

```rust
// prompt/refusal.rs
pub enum RefusalPolicy {
    /// Return StageOutcome::Block (respects FailMode)
    Block { severity: Severity },
    
    /// Return StageOutcome::Transform with redacted content (bypasses FailMode)
    Redact { placeholder: String },
    
    /// Return StageOutcome::Transform with canned response (bypasses FailMode)
    SafeResponse { template: String },
    
    /// Return StageOutcome::Escalate (respects FailMode for timeout)
    Escalate { timeout: Duration },
}

impl RefusalPolicy {
    pub fn apply(&self, reason: String) -> StageOutcome {
        match self {
            Self::Block { severity } => StageOutcome::Block { reason, severity: *severity },
            Self::Redact { placeholder } => StageOutcome::Transform {
                content: Content::Text(placeholder.clone()),
                description: format!("Redacted: {reason}"),
            },
            Self::SafeResponse { template } => StageOutcome::Transform {
                content: Content::Text(template.clone()),
                description: format!("Safe response for: {reason}"),
            },
            Self::Escalate { timeout } => StageOutcome::Escalate { reason, timeout: *timeout },
        }
    }
}
```

**R6.3 [HIGH]:** **Document the interaction** in architecture docs:

```markdown
## FailMode vs. RefusalPolicy

**Hierarchy:**
1. Stage detects threat
2. RefusalPolicy determines remediation (Block/Redact/SafeResponse/Escalate)
3. PipelineExecutor applies FailMode to Block/Escalate outcomes only
4. Transform outcomes (Redact, SafeResponse) bypass FailMode

**Decision Matrix:**

| RefusalPolicy | Stage Outcome | FailMode::Closed | FailMode::Open | FailMode::LogOnly |
|--------------|---------------|------------------|----------------|-------------------|
| Block        | Block         | Request blocked  | Log + Allow    | Log + Allow       |
| Redact       | Transform     | Request proceeds with redacted content (bypasses FailMode) |  |  |
| SafeResponse | Transform     | Request proceeds with canned response (bypasses FailMode) |  |  |
| Escalate     | Escalate      | Wait for approval | Log + Allow   | Log + Allow       |

**Key Insight:** Transform outcomes are **remediations that make unsafe content safe**.
FailMode only gates **terminal blocking decisions**, not remediations.
```

---

## 7. HoneytokenStore: Stage vs. Utility

### Assessment: ✅ CORRECTLY DESIGNED AS UTILITY

**Current Plan:** `HoneytokenStore` is a **utility** used by stages, not a stage itself.

**Rationale:**
- Honeytokens are **injected** by `SecureTemplate` (at prompt construction time)
- Honeytokens are **detected** by `EgressScanner` (output validation stage, Phase 4)
- `HoneytokenStore` provides the **shared state** between injection and detection

**Architecture:**

```rust
// prompt/honeytoken.rs
pub struct HoneytokenStore {
    store: Arc<RwLock<HashMap<String, Honeytoken>>>,  // session_id -> token
    cipher: Aes256Gcm,
}

impl HoneytokenStore {
    /// Generate a honeytoken for a session
    pub fn generate(&self, session_id: &str) -> String { /* ... */ }
    
    /// Check if text contains any known honeytokens
    pub fn detect(&self, text: &str) -> Option<HoneytokenHit> { /* ... */ }
    
    /// Rotate all tokens older than TTL
    pub fn rotate(&self, ttl: Duration) { /* ... */ }
}

// Usage in SecureTemplate
let template = SecureTemplate::builder()
    .template("You are a helpful assistant. Secret token: {{__honeytoken__}}")
    .honeytoken_store(store.clone())  // Inject at render time
    .build();

// Usage in EgressScanner (Phase 4)
impl GuardrailStage for EgressScanner {
    async fn evaluate(&self, content: &Content, ctx: &SecurityContext) 
        -> Result<StageOutcome, StageError> 
    {
        if let Some(hit) = self.honeytoken_store.detect(&content.as_text()) {
            // Honeytoken leaked! Trigger incident.
            self.incident_orchestrator.trigger(IncidentType::HoneytokenExfiltration, ctx).await?;
            return Ok(StageOutcome::block("Honeytoken detected in output", Severity::Critical));
        }
        Ok(StageOutcome::allow(1.0))
    }
}
```

### Recommendation:

**R7.1 [LOW]:** Keep as utility, but add **builder pattern** for testability:

```rust
// Test helper
impl HoneytokenStore {
    /// Create a store with deterministic tokens (for testing only)
    #[cfg(test)]
    pub fn with_static_tokens(tokens: HashMap<String, String>) -> Self {
        Self {
            store: Arc::new(RwLock::new(
                tokens.into_iter()
                    .map(|(k, v)| (k, Honeytoken { value: v, created_at: Instant::now() }))
                    .collect()
            )),
            cipher: Aes256Gcm::new_from_slice(&[0u8; 32]).unwrap(),
        }
    }
}
```

---

## 8. Extension Points for Phase 3 (ML Classifiers, PII Detection)

### Assessment: ✅ WELL-POSITIONED with recommendations

**Extension Points Identified:**

1. **ML Classifiers** (Phase 3):
   - `input/injection.rs` → Add `MLDetector` as a fourth detector
   - Ensemble scorer already supports weighted averaging
   - Feature flag: `ml-onnx` or `ml-candle`

2. **PII Detection** (Phase 3):
   - `input/pii.rs` → New GuardrailStage
   - Backends: local regex (default) + Presidio HTTP connector
   - Feature flag: `pii-presidio`

**Current Design Supports:**

```rust
// Phase 2: Heuristic + Structural + Spotlight
let injection_stage = InjectionStage {
    heuristic: HeuristicDetector::new(patterns),
    structural: StructuralAnalyzer::new(),
    spotlight: None,
    ensemble: EnsembleScorer::new(EnsembleStrategy::WeightedAverage {
        weights: [("heuristic", 0.5), ("structural", 0.5)].into()
    }),
};

// Phase 3: Add ML classifier
#[cfg(feature = "ml-onnx")]
let injection_stage = InjectionStage {
    heuristic: HeuristicDetector::new(patterns),
    structural: StructuralAnalyzer::new(),
    ml_classifier: Some(MLClassifier::from_onnx("models/injection.onnx")?),
    spotlight: None,
    ensemble: EnsembleScorer::new(EnsembleStrategy::WeightedAverage {
        weights: [("heuristic", 0.3), ("structural", 0.3), ("ml", 0.4)].into()
    }),
};
```

### Recommendations:

**R8.1 [MEDIUM]:** Add **detector registry pattern** for extensibility:

```rust
// input/injection/mod.rs
pub struct InjectionStage {
    detectors: Vec<Box<dyn Detector>>,
    ensemble: EnsembleScorer,
}

impl InjectionStage {
    pub fn builder() -> InjectionStageBuilder {
        InjectionStageBuilder::default()
            .with_heuristic_detector()  // Always included
            .with_structural_detector()  // Always included
    }
}

pub struct InjectionStageBuilder {
    detectors: Vec<Box<dyn Detector>>,
    ensemble: EnsembleStrategy,
}

impl InjectionStageBuilder {
    pub fn with_heuristic_detector(mut self) -> Self {
        self.detectors.push(Box::new(HeuristicDetector::default()));
        self
    }
    
    #[cfg(feature = "ml-onnx")]
    pub fn with_ml_detector(mut self, model_path: impl AsRef<Path>) -> Result<Self, StageError> {
        self.detectors.push(Box::new(MLDetector::from_onnx(model_path)?));
        Ok(self)
    }
    
    pub fn with_spotlight(mut self) -> Self {
        self.detectors.push(Box::new(SpotlightDetector::default()));
        self
    }
    
    pub fn ensemble_strategy(mut self, strategy: EnsembleStrategy) -> Self {
        self.ensemble = strategy;
        self
    }
    
    pub fn build(self) -> InjectionStage {
        InjectionStage {
            detectors: self.detectors,
            ensemble: EnsembleScorer::new(self.ensemble),
        }
    }
}
```

**R8.2 [LOW]:** Add **detector metadata trait** for introspection:

```rust
pub trait Detector: sealed::Sealed {
    fn score(&self, content: &Content) -> f32;
    
    /// Human-readable detector name for logging/metrics
    fn name(&self) -> &str;
    
    /// Whether this detector requires network/disk (for latency budgeting)
    fn is_expensive(&self) -> bool { false }
}
```

---

## 9. Priority Ordering: Normalization=10, Spotlight=45, Injection=50

### Assessment: ⚠️ NEEDS CLARIFICATION

**Current Plan:**
```rust
impl GuardrailStage for NormalizationStage {
    fn priority(&self) -> u32 { 10 }  // Run first
}

impl GuardrailStage for SpotlightStage {
    fn priority(&self) -> u32 { 45 }  // Before injection
}

impl GuardrailStage for InjectionStage {
    fn priority(&self) -> u32 { 50 }  // After spotlight
}
```

**Questions:**
1. Why is Spotlight (45) separate from Injection (50)?
2. What happens if a user adds a custom stage with priority 20? Does it run on normalized or raw content?

### Recommendation:

**R9.1 [HIGH]:** **Document the priority bands** and merge Spotlight into InjectionStage:

```rust
// Priority Band Allocation (in docs/architecture.md)

// BAND 0-19: Pre-processing (always runs first)
// 10: NormalizationStage (canonicalization, HTML stripping)
// 15: [Reserved for future preprocessing]

// BAND 20-39: Content enrichment (RAG, multimodal)
// 20: MultimodalStage (OCR text extraction)
// 25: [Reserved for future enrichment]

// BAND 40-59: Threat detection (main security checks)
// 40: InjectionStage (unified: heuristic + structural + spotlight)
// 45: PIIStage (personal data detection)
// 50: ModerationStage (harmful content)

// BAND 60-79: Post-detection processing
// 60: [Reserved for future stages]

// BAND 80-99: Telemetry and auditing
// 90: AuditStage (always runs last)
```

**R9.2 [CRITICAL]:** **Merge Spotlight into InjectionStage** as a detector:

```rust
// InjectionStage internally handles spotlight based on Content variant
impl GuardrailStage for InjectionStage {
    fn priority(&self) -> u32 { 40 }
    
    async fn evaluate(&self, content: &Content, ctx: &SecurityContext) 
        -> Result<StageOutcome, StageError> 
    {
        let detectors: Vec<&dyn Detector> = vec![
            &self.heuristic,
            &self.structural,
        ];
        
        // Add spotlight detector only for RAG content
        let mut spotlight_opt = None;
        if matches!(content, Content::RetrievedChunks(_)) {
            spotlight_opt = Some(SpotlightDetector::default());
        }
        if let Some(ref spotlight) = spotlight_opt {
            detectors.push(spotlight);
        }
        
        // Ensemble scoring
        let scores: Vec<f32> = detectors.iter().map(|d| d.score(content)).collect();
        let final_score = self.ensemble.aggregate(&scores);
        
        if final_score > self.threshold {
            Ok(StageOutcome::block("Injection detected", Severity::High))
        } else {
            Ok(StageOutcome::allow(1.0 - final_score))
        }
    }
}
```

**Rationale:** Having two separate stages (Spotlight @ 45, Injection @ 50) with a 5-point gap serves no purpose. Spotlight is just a detection mode within InjectionStage.

---

## 10. Transform Propagation Semantics

### Assessment: ⚠️ **CRITICAL DESIGN GAP**

**The Problem:**

`NormalizationStage` returns `StageOutcome::Transform` with normalized content. How does this propagate to subsequent stages?

```rust
// Step 1: NormalizationStage normalizes input
let outcome = normalization_stage.evaluate(
    &Content::Text("Hello<script>alert(1)</script>".into()), 
    &ctx
).await?;

// outcome = Transform { 
//     content: Content::Text("Helloalert(1)"),  // HTML stripped
//     description: "HTML stripped" 
// }

// Step 2: InjectionStage runs next. Does it see:
// A) Original content: "Hello<script>alert(1)</script>"  ❌ defeats normalization
// B) Transformed content: "Helloalert(1)"  ✅ sees normalized input
```

**Current PipelineExecutor Code (from executor.rs:168-183):**

```rust
match outcome {
    StageOutcome::Allow { confidence } => {
        // Updates confidence, but doesn't propagate content changes
        if let StageOutcome::Allow { confidence: ref mut prev } = final_outcome {
            *prev = prev.min(confidence);
        }
    }
    other => {
        final_outcome = other;  // Replaces final_outcome, but doesn't update `content`
    }
}
```

**Bug:** Transformed content is stored in `final_outcome`, but subsequent stages still receive the **original** `content` parameter!

### Recommendation:

**R10.1 [CRITICAL]:** **Modify `PipelineExecutor::run` to propagate Transform mutations**:

```rust
pub async fn run(
    &self,
    content: &Content,
    ctx: &SecurityContext,
) -> Result<PipelineResult, ExecutorError> {
    // ...
    
    let mut current_content = content.clone();  // Start with original
    let mut final_outcome = StageOutcome::allow(1.0);
    
    for stage in &self.stages {
        let start = Instant::now();
        
        // Pass current_content (which may be transformed)
        let result = stage.evaluate(&current_content, ctx).await;
        let duration = start.elapsed();
        
        match result {
            Ok(outcome) => {
                let outcome_name = outcome.variant_name().to_owned();
                
                stage_metrics.push(StageMetrics { /* ... */ });
                
                // Short-circuit on terminal outcomes
                if outcome.is_block() || outcome.is_escalate() {
                    final_outcome = self.apply_fail_mode(outcome, &mut overridden);
                    break;
                }
                
                match outcome {
                    StageOutcome::Allow { confidence } => {
                        if let StageOutcome::Allow { confidence: ref mut prev } = final_outcome {
                            *prev = prev.min(confidence);
                        }
                    }
                    
                    // KEY FIX: Propagate transformed content to next stage
                    StageOutcome::Transform { content: new_content, description } => {
                        current_content = new_content;  // Next stage sees transformed version
                        final_outcome = StageOutcome::Transform { 
                            content: current_content.clone(), 
                            description 
                        };
                    }
                    
                    other => {
                        final_outcome = other;
                    }
                }
            }
            // ... error handling
        }
    }
    
    Ok(PipelineResult {
        outcome: final_outcome,
        stage_metrics,
        degraded_stages,
        overridden,
    })
}
```

**R10.2 [HIGH]:** **Add integration test** for Transform chaining:

```rust
#[tokio::test]
async fn transform_propagates_to_next_stage() {
    struct NormalizingStage;
    
    #[async_trait]
    impl GuardrailStage for NormalizingStage {
        fn id(&self) -> &str { "normalizer" }
        fn priority(&self) -> u32 { 10 }
        
        async fn evaluate(&self, content: &Content, _ctx: &SecurityContext) 
            -> Result<StageOutcome, StageError> 
        {
            let text = content.as_text().replace("<script>", "");
            Ok(StageOutcome::transform(
                Content::Text(text),
                "Stripped <script>"
            ))
        }
    }
    
    struct InspectorStage {
        saw_script_tag: Arc<AtomicBool>,
    }
    
    #[async_trait]
    impl GuardrailStage for InspectorStage {
        fn id(&self) -> &str { "inspector" }
        fn priority(&self) -> u32 { 20 }
        
        async fn evaluate(&self, content: &Content, _ctx: &SecurityContext) 
            -> Result<StageOutcome, StageError> 
        {
            let text = content.as_text();
            if text.contains("<script>") {
                self.saw_script_tag.store(true, Ordering::SeqCst);
            }
            Ok(StageOutcome::allow(1.0))
        }
    }
    
    let saw_script = Arc::new(AtomicBool::new(false));
    
    let executor = PipelineExecutor::builder()
        .add_stage(NormalizingStage)
        .add_stage(InspectorStage { saw_script_tag: saw_script.clone() })
        .build();
    
    let result = executor.run(
        &Content::Text("Hello<script>alert(1)</script>".into()),
        &SecurityContext::default()
    ).await.unwrap();
    
    // Inspector should NOT have seen <script> because normalizer removed it
    assert!(!saw_script.load(Ordering::SeqCst), 
        "Transform did not propagate — InspectorStage saw original content!");
    
    // Final outcome should be Transform (from NormalizingStage)
    assert!(result.outcome.is_transform());
}
```

---

## Summary of Recommendations

### Critical (Must Address Before Phase 2 Start)

| ID | Recommendation | Impact |
|----|---------------|---------|
| R6.1 | Clarify FailMode vs. RefusalPolicy hierarchy | Prevents conflicting blocking logic |
| R6.2 | Redefine RefusalPolicy semantics (Transform vs. Block) | API clarity |
| R9.2 | Merge Spotlight into InjectionStage as a detector | Eliminates unnecessary stage |
| R10.1 | Fix Transform propagation in PipelineExecutor | Security bug — normalization bypassed |
| R10.2 | Add integration test for Transform chaining | Prevents regression |

### High Priority (Address During Phase 2)

| ID | Recommendation | Impact |
|----|---------------|--------|
| R1.1 | Nest spotlight under input/injection/ | Better module organization |
| R4.1 | Implement hybrid pattern library (static + runtime) | Extensibility |
| R5.1 | Use sealed trait pattern for Detector | API stability |
| R6.3 | Document FailMode/RefusalPolicy interaction | Prevents user confusion |
| R9.1 | Document priority bands | Clear extension points |

### Medium Priority (Can Defer to Phase 3)

| ID | Recommendation | Impact |
|----|---------------|--------|
| R3.1 | Add pure-Rust crypto alternative for honeytoken | Portability |
| R3.3 | Add feature validation in build.rs | Prevents invalid configurations |
| R8.1 | Add detector registry pattern | Cleaner ML integration |

### Low Priority (Nice-to-Have)

| ID | Recommendation | Impact |
|----|---------------|--------|
| R2.1 | Document Content variant-matching pattern | Convention clarity |
| R3.2 | Add patterns-dynamic feature flag | Future flexibility |
| R7.1 | Add HoneytokenStore builder for testing | Test ergonomics |
| R8.2 | Add Detector metadata trait | Better introspection |

---

## Conclusion

The Phase 2 design is **architecturally sound** with excellent separation of concerns and consistent application of the GuardrailStage pattern. The critical issues are:

1. **Transform propagation bug** (R10.1) — must be fixed or normalized content won't reach subsequent stages
2. **FailMode/RefusalPolicy conflict** (R6.1, R6.2) — needs clear hierarchy definition
3. **Spotlight over-abstraction** (R9.2) — should be merged into InjectionStage

Once these are addressed, the design is production-ready. The feature gating strategy is sound, the pattern library hybrid approach provides the right balance, and the extension points for Phase 3 are well-positioned.

**Approval Status:** ✅ **APPROVED WITH REQUIRED CHANGES**  
**Recommended Action:** Address all CRITICAL recommendations before starting WS4/WS5 implementation.

---

**Reviewed by:** System Architecture Expert  
**Date:** February 2026  
**Next Review:** End of Sprint 5 (Phase 2 completion)
