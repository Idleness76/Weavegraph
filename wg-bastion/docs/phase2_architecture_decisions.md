# Phase 2 Architecture Decision Records (ADRs)

**Last Updated:** February 2026  
**Status:** Pre-Implementation  
**Context:** wg-bastion Phase 2 (Prompt & Injection Security)

These ADRs document critical architectural decisions and their rationale.

---

## ADR-001: Transform Outcome Propagation Semantics

**Status:** ✅ ACCEPTED (with required implementation fix)

**Context:**
The pipeline needs a mechanism to propagate content modifications from one stage to the next. For example, `NormalizationStage` returns `StageOutcome::Transform` with HTML-stripped content, and `InjectionStage` should analyze the *cleaned* version, not the original input.

**Decision:**
`PipelineExecutor::run()` maintains a `current_content: Content` variable that is updated whenever a stage returns `Transform`. Subsequent stages receive `&current_content` instead of the original input.

**Consequences:**
- ✅ Normalization is effective — injection detection runs on canonicalized input
- ✅ Stages can transform content in sequence (e.g., Normalize → Redact → Sanitize)
- ⚠️ Requires careful cloning (use `Cow<'_, str>` where possible to minimize allocations)
- ⚠️ Final outcome must include the fully transformed content

**Implementation:**
```rust
pub async fn run(&self, content: &Content, ctx: &SecurityContext) -> Result<PipelineResult, ExecutorError> {
    let mut current_content = content.clone();
    
    for stage in &self.stages {
        let result = stage.evaluate(&current_content, ctx).await;
        
        if let Ok(StageOutcome::Transform { content: new_content, .. }) = result {
            current_content = new_content;  // Propagate mutation
        }
    }
    
    // Final outcome includes fully transformed content
}
```

**Alternatives Considered:**
- **Immutable pipeline** (all stages see original) → Rejected: defeats purpose of normalization
- **Explicit transform chain** (stages declare dependencies) → Rejected: too complex for Phase 2

---

## ADR-002: RefusalPolicy vs FailMode Interaction

**Status:** ✅ ACCEPTED

**Context:**
Two mechanisms control blocking behavior:
1. `RefusalPolicy` (per-stage): What to do when a threat is detected
2. `FailMode` (global): Whether to enforce blocking decisions

This creates ambiguity: if `RefusalPolicy::Block` is used but `FailMode::Open` is set, which wins?

**Decision:**
**Hierarchy:** RefusalPolicy determines the *remediation strategy*, FailMode determines *enforcement*.

| RefusalPolicy | Returns | FailMode Impact |
|--------------|---------|----------------|
| `Block` | `StageOutcome::Block` | ✅ Respects FailMode (may override to Allow) |
| `Redact` | `StageOutcome::Transform` | ❌ Bypasses FailMode (remediation, not block) |
| `SafeResponse` | `StageOutcome::Transform` | ❌ Bypasses FailMode (remediation, not block) |
| `Escalate` | `StageOutcome::Escalate` | ✅ Respects FailMode (may override to Allow) |

**Rationale:**
- `Transform` outcomes represent *safe alternatives* — the threat has been remediated
- `Block` and `Escalate` are *terminal decisions* that FailMode can override for testing/debugging
- This allows "fail-open" testing without disabling security features entirely

**Consequences:**
- ✅ `FailMode::Open` doesn't disable PII redaction — it only overrides hard blocks
- ✅ Clear mental model: "Transform = fixed, Block = stopped"
- ⚠️ Must document this carefully to avoid user confusion

**Example:**
```rust
// TemplateScanner detects a leaked API key
let outcome = RefusalPolicy::Redact { placeholder: "[REDACTED]" }.apply("API key found");
// outcome = Transform { content: "[REDACTED]", ... }

// PipelineExecutor sees Transform → allows request with redacted content
// Even if FailMode::Closed, the Transform bypasses the gate because it's a remediation
```

---

## ADR-003: Spotlight as InjectionStage Sub-Component

**Status:** ✅ ACCEPTED

**Context:**
Spotlight detection marks RAG content boundaries to prevent indirect injection. The plan proposes `input/spotlight.rs` as a separate top-level module with `priority = 45`, running just before `InjectionStage` (priority 50).

**Decision:**
Spotlight is **not a separate stage**. It is a **detector within InjectionStage**, activated only when `Content::RetrievedChunks` is encountered.

**Module Structure:**
```
input/
  injection/
    mod.rs              (InjectionStage)
    heuristic.rs        (HeuristicDetector)
    structural.rs       (StructuralAnalyzer)
    spotlight.rs        (SpotlightDetector)
    ensemble.rs         (EnsembleScorer)
  normalization.rs
  patterns.rs
```

**Rationale:**
- Spotlight is **content-variant-specific** — it only applies to `RetrievedChunks`
- Running it as a separate stage creates artificial priority gaps (45 vs. 50)
- Ensemble scoring should include spotlight as a signal, not run it separately

**Implementation:**
```rust
impl GuardrailStage for InjectionStage {
    fn priority(&self) -> u32 { 40 }
    
    async fn evaluate(&self, content: &Content, ctx: &SecurityContext) -> Result<StageOutcome, StageError> {
        let mut detectors: Vec<&dyn Detector> = vec![&self.heuristic, &self.structural];
        
        // Conditionally add spotlight for RAG content
        let mut spotlight_temp = None;
        if matches!(content, Content::RetrievedChunks(_)) {
            spotlight_temp = Some(SpotlightDetector::new());
        }
        if let Some(ref s) = spotlight_temp {
            detectors.push(s);
        }
        
        // Ensemble scoring across all applicable detectors
        let scores: Vec<f32> = detectors.iter().map(|d| d.score(content)).collect();
        self.ensemble.aggregate(&scores)
    }
}
```

**Consequences:**
- ✅ Simpler priority ordering (no 45 vs. 50 confusion)
- ✅ Spotlight naturally participates in ensemble voting
- ✅ Easier to add future detectors (multimodal, ML classifier)

---

## ADR-004: Pattern Library as Hybrid Static/Runtime

**Status:** ✅ ACCEPTED

**Context:**
The pattern library contains 50+ regex patterns for injection detection. Should these be:
- **Static** (compiled in, zero runtime cost)
- **Runtime** (loaded from JSON, hot-reloadable)

**Decision:**
**Hybrid approach** — static by default, runtime overrides optional.

**Feature Gates:**
```toml
[features]
patterns-static = []  # Default: compiled-in patterns
patterns-dynamic = ["serde_json"]  # Optional: load from JSON
```

**API:**
```rust
// Default: zero-config, zero overhead
let patterns = PatternLibrary::builtin();

// Power users: add custom patterns
#[cfg(feature = "patterns-dynamic")]
let patterns = PatternLibrary::builtin()
    .with_overrides("custom_patterns.json")?;
```

**Rationale:**
- **Phase 2 MVP:** Static patterns are sufficient for 90% of users
- **Production scale:** Enterprises want tenant-specific patterns without recompiling
- **Performance:** Static patterns live in `.rodata`, no deserialization cost on hot path
- **Extension point:** Phase 4 ML classifiers can add learned patterns dynamically

**Consequences:**
- ✅ Zero-config default (no JSON files to manage)
- ✅ Extensibility for power users
- ⚠️ Need to define merge semantics (overrides replace or augment?)
- ⚠️ Runtime patterns need validation (invalid regex shouldn't panic)

**Merge Semantics:**
```rust
impl PatternLibrary {
    /// Search static patterns first, then runtime overrides
    pub fn matches(&self, text: &str) -> Vec<Match> {
        // Static (hot path, cache-friendly)
        let mut matches = self.static_patterns.iter()
            .filter_map(|p| p.find(text))
            .collect::<Vec<_>>();
        
        // Runtime overrides (cold path)
        matches.extend(
            self.runtime_overrides.iter()
                .filter_map(|p| p.find(text))
        );
        
        matches
    }
}
```

---

## ADR-005: Detector Trait as Sealed Pattern

**Status:** ✅ ACCEPTED

**Context:**
`InjectionStage` composes multiple detectors (heuristic, structural, spotlight, future ML). Should the `Detector` trait be:
- **Public and open** (users can implement custom detectors)
- **Private** (internal implementation detail)
- **Public but sealed** (visible but not implementable)

**Decision:**
Use the **sealed trait pattern** — `Detector` is public for documentation and type signatures, but users cannot implement it.

**Implementation:**
```rust
// input/injection/mod.rs
mod sealed {
    pub trait Sealed {}
}

/// Detector that contributes to ensemble scoring.
/// 
/// This trait is sealed — you cannot implement it for external types.
/// Configure the built-in detectors via `InjectionStageBuilder`.
pub trait Detector: sealed::Sealed {
    fn score(&self, content: &Content) -> f32;
    fn name(&self) -> &str;
    fn is_expensive(&self) -> bool { false }
}

// Only internal types can implement Detector
impl sealed::Sealed for HeuristicDetector {}
impl sealed::Sealed for StructuralAnalyzer {}
impl sealed::Sealed for SpotlightDetector {}

impl Detector for HeuristicDetector { /* ... */ }
```

**Rationale:**
- **Phase 2-3:** We don't know the stable API for detectors yet
- **Sealed trait** lets us refactor internals without breaking users
- **Discovery:** Types are visible in docs, users understand the architecture
- **Configuration over implementation:** Users tune weights, not write detectors

**User API:**
```rust
// Users configure, not implement
let stage = InjectionStage::builder()
    .with_heuristic_detector()
    .with_structural_detector()
    .ensemble_strategy(EnsembleStrategy::WeightedAverage {
        weights: [("heuristic", 0.4), ("structural", 0.6)].into()
    })
    .build();
```

**Consequences:**
- ✅ API stability — we can change detector internals freely
- ✅ Clear documentation — users see what detectors exist
- ⚠️ Cannot add custom detectors in Phase 2 (acceptable trade-off)
- ⚠️ May unseal in Phase 4 if ML integration requires it

---

## ADR-006: Priority Band Allocation Strategy

**Status:** ✅ ACCEPTED

**Context:**
Stages execute in priority order (lower = earlier). Current plan has ad-hoc priorities: Normalization=10, Spotlight=45, Injection=50. What's the long-term strategy?

**Decision:**
Define **priority bands** with reserved ranges for future expansion.

**Allocation:**
```
Band 0-19:   Preprocessing (always first)
  10: NormalizationStage
  15: [Reserved]

Band 20-39:  Content Enrichment (multimodal, RAG metadata)
  20: MultimodalStage (Phase 3)
  25: [Reserved]

Band 40-59:  Threat Detection (core security)
  40: InjectionStage
  45: PIIStage (Phase 3)
  50: ModerationStage (Phase 3)

Band 60-79:  Post-Detection Processing
  60: [Reserved for future]

Band 80-99:  Audit & Telemetry (always last)
  90: AuditStage (Phase 6)
```

**Rationale:**
- **Clear semantics:** Band number = execution phase
- **Room for growth:** 10-point gaps allow insertion without renumbering
- **Documentation:** Users know where custom stages should go

**Consequences:**
- ✅ No priority collisions across phases
- ✅ Custom stages have clear guidance ("preprocessing? use 10-19")
- ⚠️ Must document in `docs/architecture.md`

**Custom Stage Guidance:**
```rust
// User adding a custom deduplication stage
impl GuardrailStage for DeduplicationStage {
    fn priority(&self) -> u32 { 
        15  // Band 0-19 (preprocessing), after Normalization (10)
    }
}
```

---

## ADR-007: HoneytokenStore Encryption Strategy

**Status:** ✅ ACCEPTED (with feature flag alternatives)

**Context:**
Honeytokens must be encrypted at rest (AES-256-GCM). However, `ring` crate (native crypto) may not compile on all targets.

**Decision:**
Offer **two implementations** behind feature flags:

```toml
[features]
honeytoken = []  # Meta-feature (enables API)
honeytoken-ring = ["honeytoken", "ring", "zeroize"]  # Fast native crypto (default)
honeytoken-pure = ["honeytoken", "aes-gcm", "chacha20poly1305"]  # Pure Rust fallback

default = ["heuristics", "honeytoken-ring"]
```

**API:**
```rust
// Unified API regardless of backend
pub struct HoneytokenStore {
    #[cfg(feature = "honeytoken-ring")]
    cipher: ring::aead::Aes256Gcm,
    
    #[cfg(feature = "honeytoken-pure")]
    cipher: aes_gcm::Aes256Gcm,
    
    store: Arc<RwLock<HashMap<String, Honeytoken>>>,
}

impl HoneytokenStore {
    pub fn new(key: &[u8; 32]) -> Result<Self, CryptoError> {
        #[cfg(feature = "honeytoken-ring")]
        let cipher = ring::aead::Aes256Gcm::new(key)?;
        
        #[cfg(feature = "honeytoken-pure")]
        let cipher = aes_gcm::Aes256Gcm::new(key.into());
        
        Ok(Self { cipher, store: Default::default() })
    }
}
```

**Rationale:**
- **Default fast path:** Most users get native crypto via `ring`
- **Portability:** WASM/embedded targets can use pure Rust
- **API stability:** Users never see the backend choice

**Consequences:**
- ✅ Works on all Rust targets
- ✅ Performance: ring is ~5x faster than pure Rust AES
- ⚠️ Must keep both implementations in sync

---

## ADR-008: Content Variant Matching Convention

**Status:** ✅ ACCEPTED (convention, not enforcement)

**Context:**
Stages receive `&Content` and may only be applicable to certain variants. Should we enforce a pattern?

**Decision:**
**Convention:** Stages should explicitly match on applicable variants and return `Skip` for others. Avoid catch-all `_` patterns.

**Recommended Pattern:**
```rust
async fn evaluate(&self, content: &Content, ctx: &SecurityContext) -> Result<StageOutcome, StageError> {
    match content {
        // Explicitly handle applicable variants
        Content::Text(text) => self.analyze_text(text),
        Content::Messages(msgs) => self.analyze_messages(msgs),
        
        // Explicitly skip non-applicable variants
        Content::ToolCall { .. } => {
            Ok(StageOutcome::skip("user-facing content only"))
        }
        Content::ToolResult { .. } => {
            Ok(StageOutcome::skip("user-facing content only"))
        }
        Content::RetrievedChunks(_) => {
            Ok(StageOutcome::skip("not applicable to RAG content"))
        }
    }
}
```

**Anti-Pattern (avoid):**
```rust
match content {
    Content::Text(text) => { /* ... */ }
    _ => Ok(StageOutcome::skip("not applicable"))  // ❌ Catch-all hides mistakes
}
```

**Rationale:**
- **Explicitness:** Adding `Content::Multimodal` in Phase 3 won't silently skip existing stages
- **Compiler assistance:** Non-exhaustive match forces acknowledgment of new variants
- **Documentation:** Reading the match makes scope clear

**Consequences:**
- ✅ New content variants require explicit handling
- ⚠️ More verbose (but clippy can warn on catch-all in #![deny(clippy::wildcard_enum_match_arm)])

---

## Summary Table

| ADR | Decision | Status | Impact |
|-----|----------|--------|--------|
| 001 | Transform propagation via `current_content` | ✅ Required Fix | Critical for normalization |
| 002 | RefusalPolicy = remediation, FailMode = enforcement | ✅ Required | API clarity |
| 003 | Spotlight nested in InjectionStage | ✅ Required | Simplifies architecture |
| 004 | Hybrid pattern library (static + runtime) | ✅ Accepted | Extensibility |
| 005 | Sealed Detector trait | ✅ Accepted | API stability |
| 006 | Priority bands (0-19, 20-39, 40-59, etc.) | ✅ Accepted | Long-term organization |
| 007 | Dual crypto backends for honeytokens | ✅ Accepted | Portability |
| 008 | Explicit variant matching convention | ✅ Convention | Code quality |

---

**Next Steps:**
1. Implement ADR-001 (Transform propagation) in `executor.rs`
2. Document ADR-002 (FailMode/RefusalPolicy) in architecture docs
3. Reorganize spotlight per ADR-003 before starting WS5-10
4. Review all ADRs with team before Sprint 3

**Maintained by:** System Architecture Expert  
**Review Cadence:** End of each phase
