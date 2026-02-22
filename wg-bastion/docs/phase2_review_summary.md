# Phase 2 Architecture Review — Executive Summary

**Status:** ✅ APPROVED WITH REQUIRED CHANGES  
**Overall Assessment:** STRONG design with 5 critical fixes needed

---

## Quick Answers to Your 10 Questions

### 1. Module Decomposition
✅ **STRONG** — Clean separation between prompt/ (compile-time) and input/ (runtime). Minor issue: `spotlight.rs` should be nested under `input/injection/` rather than top-level.

### 2. GuardrailStage Scoping
✅ **EXCELLENT** — Each stage has single responsibility. All correctly implement the pattern.

### 3. Feature Gate Strategy
✅ **SOUND** — Zero-dependency default (`heuristics`) is great. Recommend adding pure-Rust crypto alternative for `honeytoken` feature.

### 4. Pattern Library Abstraction
⚠️ **NEEDS DECISION** — Recommend **hybrid approach**: static patterns (zero-cost default) + optional runtime overrides (feature-gated). Best of both worlds.

### 5. StructuralAnalyzer/EnsembleScorer Visibility
⚠️ **USE SEALED TRAIT** — Make types public but prevent external implementations. Users configure, don't implement.

### 6. RefusalPolicy vs. FailMode
🔴 **CRITICAL CONFLICT** — Two overlapping policies. Need clear hierarchy: RefusalPolicy = remediation strategy, FailMode = enforcement override. Transform outcomes bypass FailMode.

### 7. HoneytokenStore as Stage or Utility
✅ **CORRECT AS UTILITY** — Used by SecureTemplate (injection) and EgressScanner (detection). Not a stage itself.

### 8. Extension Points for Phase 3
✅ **WELL-POSITIONED** — ML classifiers slot in cleanly. Recommend detector registry pattern for cleaner integration.

### 9. Priority Ordering
⚠️ **NEEDS CLARIFICATION** — Spotlight (45) should merge into InjectionStage (40). Document priority bands: 0-19 preprocessing, 20-39 enrichment, 40-59 detection, 60-79 post-processing, 80-99 audit.

### 10. Transform Propagation
🔴 **CRITICAL BUG** — Current PipelineExecutor doesn't propagate transformed content to subsequent stages. NormalizationStage's output never reaches InjectionStage! Must fix in `executor.rs`.

---

## Critical Issues (Must Fix Before Implementation)

### 🔴 Issue #1: Transform Propagation Bug
**File:** `wg-bastion/src/pipeline/executor.rs`  
**Problem:** Stages see original content, not transformed versions from previous stages.

**Fix:**
```rust
pub async fn run(&self, content: &Content, ctx: &SecurityContext) -> Result<PipelineResult, ExecutorError> {
    let mut current_content = content.clone();  // Track mutations
    
    for stage in &self.stages {
        let result = stage.evaluate(&current_content, ctx).await;  // Pass current, not original
        
        match result {
            Ok(StageOutcome::Transform { content: new_content, description }) => {
                current_content = new_content;  // Propagate to next stage
                final_outcome = StageOutcome::Transform { content: current_content.clone(), description };
            }
            // ...
        }
    }
}
```

**Impact:** Without this, HTML normalization is pointless — InjectionStage still sees `<script>` tags.

---

### 🔴 Issue #2: RefusalPolicy/FailMode Conflict
**Files:** `prompt/refusal.rs`, `pipeline/executor.rs`

**Problem:** Two mechanisms for blocking — which wins?

**Solution:**
```
RefusalPolicy = What to do when threat detected
  - Block → StageOutcome::Block (respects FailMode)
  - Redact → StageOutcome::Transform (bypasses FailMode)
  - SafeResponse → StageOutcome::Transform (bypasses FailMode)
  - Escalate → StageOutcome::Escalate (respects FailMode)

FailMode = Policy override (only affects Block/Escalate)
  - Closed: Enforce all blocks
  - Open: Override blocks to Allow
  - LogOnly: Override blocks to Allow (log only)

Key: Transform outcomes are remediations that make content safe.
They bypass FailMode because they're not blocking — they're fixing.
```

---

### 🔴 Issue #3: Spotlight Over-Abstraction
**File:** `input/spotlight.rs` → `input/injection/spotlight.rs`

**Problem:** Spotlight is a 5-line module that only makes sense as part of InjectionStage.

**Fix:**
```rust
// input/injection/mod.rs
pub mod heuristic;
pub mod structural;
pub mod spotlight;  // Nested, not top-level

pub struct InjectionStage {
    heuristic: HeuristicDetector,
    structural: StructuralAnalyzer,
    spotlight: Option<SpotlightDetector>,  // Only for Content::RetrievedChunks
    ensemble: EnsembleScorer,
}

impl InjectionStage {
    fn priority(&self) -> u32 { 40 }  // Single priority, not 45 + 50
}
```

---

## High-Priority Recommendations

### Pattern Library Strategy
Implement **hybrid approach** in `input/patterns.rs`:
- Static patterns compiled in (zero runtime cost)
- Optional runtime overrides via JSON (feature-gated)
- Static patterns searched first (cache locality)

### Detector Encapsulation
Use **sealed trait pattern**:
- `pub trait Detector: sealed::Sealed { ... }`
- Users can configure detectors, not implement them
- Prevents breaking changes when adding ML classifiers

### Priority Band Documentation
Document standard priority ranges:
```
0-19:   Preprocessing (NormalizationStage = 10)
20-39:  Enrichment (MultimodalStage = 20)
40-59:  Detection (InjectionStage = 40, PIIStage = 45)
60-79:  Post-processing (reserved)
80-99:  Audit (AuditStage = 90)
```

---

## Implementation Checklist

### Before Starting WS4/WS5:
- [ ] Fix Transform propagation in executor.rs (Issue #1)
- [ ] Define RefusalPolicy/FailMode hierarchy (Issue #2)
- [ ] Move spotlight.rs under input/injection/ (Issue #3)
- [ ] Add integration test for Transform chaining
- [ ] Document FailMode/RefusalPolicy interaction

### During Phase 2:
- [ ] Implement hybrid pattern library (static + runtime)
- [ ] Add sealed Detector trait
- [ ] Document priority bands in architecture.md
- [ ] Add pure-Rust crypto alternative (honeytoken-pure feature)

### Phase 2 Acceptance:
- [ ] Transform propagation test passes
- [ ] RefusalPolicy/FailMode behavior documented
- [ ] >90% injection detection rate
- [ ] <5% false positive rate
- [ ] Fuzz tests pass without panics

---

## Architecture Strengths

1. **Consistent Pattern Application** — All stages correctly implement GuardrailStage
2. **Zero-Dependency Default** — `heuristics` feature has no heavy deps
3. **Clear Threat Mapping** — Every module maps to specific OWASP categories
4. **Good Extension Points** — ML classifiers will integrate cleanly
5. **Proper Encapsulation** — HoneytokenStore correctly used as utility, not stage

---

## Final Verdict

**APPROVED** pending resolution of 3 critical issues. Once fixed, the architecture is production-ready.

The design demonstrates strong separation of concerns, consistent trait implementation, and thoughtful extensibility. The Transform propagation bug is the only code-breaking issue — the other concerns are about API clarity and documentation.

**Estimated Impact:**
- Critical fixes: 4-6 hours
- High-priority items: 2-3 days
- No architectural rework needed

**Confidence Level:** High. This is a solid design with well-understood fixes.

---

**Full Review:** See `phase2_architecture_review.md` for detailed analysis of all 10 questions.
