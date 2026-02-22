# Phase 2 Implementation Checklist

**Target:** Sprint 3-5 (Weeks 5-10)  
**Goal:** Complete prompt hardening and injection detection with >90% detection rate

---

## Pre-Sprint 3: Critical Fixes (Must Complete First)

### 🔴 BLOCKER: Transform Propagation Bug
**File:** `wg-bastion/src/pipeline/executor.rs`  
**Issue:** Stages see original content, not transformed versions

- [ ] Modify `PipelineExecutor::run()` signature to track `current_content`
- [ ] Update `for stage in &self.stages` loop to pass `&current_content`
- [ ] Handle `StageOutcome::Transform` to update `current_content`
- [ ] Update `PipelineResult` to include final transformed content
- [ ] Add integration test: `test_transform_propagates_to_next_stage()`
- [ ] Verify normalization → injection pipeline works correctly
- [ ] Document propagation semantics in `executor.rs` doc comments

**Acceptance:** Test passes showing InjectionStage analyzes normalized content, not raw.

---

### 🔴 BLOCKER: RefusalPolicy/FailMode Clarification
**Files:** `prompt/refusal.rs`, `pipeline/executor.rs`, `docs/architecture.md`

- [ ] Define clear semantics in `prompt/refusal.rs` doc comments
- [ ] Update `RefusalPolicy::apply()` to return correct outcomes:
  - [ ] `Block` → `StageOutcome::Block`
  - [ ] `Redact` → `StageOutcome::Transform`
  - [ ] `SafeResponse` → `StageOutcome::Transform`
  - [ ] `Escalate` → `StageOutcome::Escalate`
- [ ] Update `PipelineExecutor::apply_fail_mode()` to skip Transform outcomes
- [ ] Add doc comment: "Transform outcomes bypass FailMode (they're remediations)"
- [ ] Document interaction in `docs/architecture.md` with decision matrix
- [ ] Add test: `test_fail_mode_does_not_override_transform_outcomes()`
- [ ] Add test: `test_fail_mode_open_allows_safe_responses()`

**Acceptance:** FailMode::Open overrides Block but not Transform. Documentation clear.

---

### 🟡 HIGH: Spotlight Module Reorganization
**Action:** Move `input/spotlight.rs` → `input/injection/spotlight.rs`

- [ ] Create `input/injection/` directory
- [ ] Move existing `input/injection.rs` → `input/injection/mod.rs`
- [ ] Create `input/injection/heuristic.rs` for HeuristicDetector
- [ ] Create `input/injection/structural.rs` for StructuralAnalyzer
- [ ] Create `input/injection/spotlight.rs` for SpotlightDetector
- [ ] Create `input/injection/ensemble.rs` for EnsembleScorer
- [ ] Update imports in `input/mod.rs`
- [ ] Update `InjectionStage` to compose detectors internally
- [ ] Remove separate `SpotlightStage` from plan (merge into InjectionStage)
- [ ] Update priority: InjectionStage = 40 (not 45 + 50)

**Acceptance:** Single InjectionStage at priority 40, spotlight is an internal detector.

---

## Sprint 3 (Weeks 5-6): WS4 Prompt Hardening

### WS4-04: TemplateScanner
**Dependencies:** None  
**Feature Flag:** `heuristics` (default)

- [ ] Create `prompt/scanner.rs`
- [ ] Define `SecretPattern` struct with regex + entropy threshold
- [ ] Implement 10+ patterns:
  - [ ] AWS keys (`AKIA...`)
  - [ ] GCP keys (`AIza...`)
  - [ ] OpenAI keys (`sk-...`)
  - [ ] Anthropic keys (`sk-ant-...`)
  - [ ] JWT tokens (`eyJ...`)
  - [ ] RSA/EC private keys (`-----BEGIN...KEY-----`)
  - [ ] Passwords in URLs (`://user:pass@`)
  - [ ] Generic high-entropy strings (>4.5 bits/char)
  - [ ] UUIDs in prompts (potential honeytokens)
  - [ ] Base64-encoded secrets
- [ ] Implement `TemplateScanner` struct
- [ ] Implement `GuardrailStage` for TemplateScanner
  - [ ] `id()` → `"template_scanner"`
  - [ ] `priority()` → `30`
  - [ ] `degradable()` → `false` (critical)
  - [ ] `evaluate()` → scan for secrets
- [ ] Add `RefusalPolicy` field to TemplateScanner
- [ ] Add unit tests for each pattern
- [ ] Add test: scanner detects secrets in `Content::Text`
- [ ] Add test: scanner skips non-text content variants
- [ ] Benchmark: <5ms for 1KB prompt scan

**Acceptance:** All 10+ patterns detected, RefusalPolicy applies correctly.

---

### WS4-01: SecureTemplate
**Dependencies:** WS2-01 (Content enum)  
**Feature Flag:** `heuristics` (default)

- [ ] Create `prompt/template.rs`
- [ ] Define `SecureTemplate` struct with builder pattern
- [ ] Implement typed placeholders:
  - [ ] `{{user_input}}` — auto-escaped
  - [ ] `{{system_instruction}}` — validated max length
  - [ ] `{{__honeytoken__}}` — special marker for honeytokens
- [ ] Add max-length enforcement per placeholder type
- [ ] Add auto-escaping for user-provided values (HTML entities, quotes)
- [ ] Implement `SecureTemplate::render()` → `Content`
- [ ] Add `#[must_use]` on builder methods
- [ ] Add doc examples showing usage
- [ ] Add test: template with undefined placeholder fails
- [ ] Add test: max-length enforcement truncates/errors
- [ ] Add test: special chars in user input get escaped

**Acceptance:** Templates render safely, undefined placeholders caught.

---

### WS4-02: Role Isolation
**Dependencies:** WS4-01  
**Feature Flag:** `heuristics` (default)

- [ ] Create `prompt/isolation.rs`
- [ ] Define delimiter markers:
  - [ ] `[SYSTEM_START]...[SYSTEM_END]`
  - [ ] `[USER_START]...[USER_END]`
  - [ ] `[ASSISTANT_START]...[ASSISTANT_END]`
- [ ] Implement randomized marker generation (UUID suffix)
- [ ] Implement `RoleIsolation` struct
- [ ] Implement `GuardrailStage` for RoleIsolation
  - [ ] `id()` → `"role_isolation"`
  - [ ] `priority()` → `35`
  - [ ] `degradable()` → `false`
  - [ ] `evaluate()` → detect delimiter violations
- [ ] Add detection logic for:
  - [ ] User input containing `[SYSTEM_START]`
  - [ ] Unbalanced delimiters
  - [ ] Nested role markers
- [ ] Integrate with `SecureTemplate` to auto-inject delimiters
- [ ] Add test: user cannot inject system markers
- [ ] Add test: balanced delimiters pass, unbalanced fail

**Acceptance:** Delimiter injection attempts blocked.

---

### WS4-03: HoneytokenStore
**Dependencies:** WS2-01  
**Feature Flags:** `honeytoken-ring` (default) or `honeytoken-pure`

- [ ] Create `prompt/honeytoken.rs`
- [ ] Implement `HoneytokenStore` struct with:
  - [ ] `store: Arc<RwLock<HashMap<String, Honeytoken>>>`
  - [ ] `cipher: Aes256Gcm` (conditional compilation)
- [ ] Add feature-gated crypto backends:
  - [ ] `#[cfg(feature = "honeytoken-ring")]` → use `ring`
  - [ ] `#[cfg(feature = "honeytoken-pure")]` → use `aes-gcm` crate
- [ ] Implement methods:
  - [ ] `new(key: &[u8; 32])` → initialize with encryption key
  - [ ] `generate(session_id: &str)` → create and encrypt honeytoken
  - [ ] `detect(text: &str)` → check for known honeytokens
  - [ ] `rotate(ttl: Duration)` → expire old tokens
- [ ] Add `zeroize` on drop for key material
- [ ] Integrate with `SecureTemplate` (inject on `{{__honeytoken__}}`)
- [ ] Add unit tests:
  - [ ] Generate token, detect it in text
  - [ ] Rotation removes expired tokens
  - [ ] Encrypted tokens don't appear in plaintext
- [ ] Add test helper: `with_static_tokens()` for deterministic tests

**Acceptance:** Honeytokens generated, encrypted, detected, rotated.

---

### WS4-05: RefusalPolicy
**Dependencies:** WS2-02 (StageOutcome)  
**Feature Flag:** `heuristics` (default)

- [ ] Create `prompt/refusal.rs`
- [ ] Define `RefusalPolicy` enum:
  - [ ] `Block { severity: Severity }`
  - [ ] `Redact { placeholder: String }`
  - [ ] `SafeResponse { template: String }`
  - [ ] `Escalate { timeout: Duration }`
- [ ] Implement `RefusalPolicy::apply(reason: String) -> StageOutcome`
- [ ] Add doc comments explaining Transform vs. Block semantics
- [ ] Add builder for safe response templates
- [ ] Add unit tests for each variant
- [ ] Update `TemplateScanner` to use `RefusalPolicy`
- [ ] Update `RoleIsolation` to use `RefusalPolicy`

**Acceptance:** All 4 policy modes work, semantics clear in docs.

---

### WS4-06: Fuzz Tests
**Dependencies:** WS4-01  
**Tool:** `cargo-fuzz`

- [ ] Create `fuzz/` directory
- [ ] Add `cargo-fuzz` to dev dependencies
- [ ] Create fuzz target: `fuzz_template_injection.rs`
- [ ] Fuzz `SecureTemplate::render()` with:
  - [ ] Random placeholder names
  - [ ] Random input strings (UTF-8, invalid UTF-8, control chars)
  - [ ] Nested templates
  - [ ] Extremely long inputs
- [ ] Run fuzzer for 24 hours on CI
- [ ] Add any discovered crashes to regression suite
- [ ] Document findings in `docs/fuzzing_results.md`

**Acceptance:** No panics, UB, or crashes after 24-hour fuzz run.

---

## Sprint 4 (Weeks 7-8): WS5-INJ Part 1 (Normalization + Heuristics)

### WS5-01: NormalizationStage
**Dependencies:** WS2-03 (GuardrailStage)  
**Feature Flags:** `heuristics` (default), `normalization-html` (optional)

- [ ] Create `input/normalization.rs`
- [ ] Implement `NormalizationStage` struct with:
  - [ ] Unicode NFKC normalization (via `unicode-normalization`)
  - [ ] Control char stripping (except tab, newline)
  - [ ] Optional HTML stripping (feature-gated `lol_html`)
  - [ ] MIME validation (reject binary blobs disguised as text)
  - [ ] Length truncation (configurable max)
- [ ] Implement `GuardrailStage`:
  - [ ] `priority()` → `10` (always first)
  - [ ] `degradable()` → `false` (critical)
  - [ ] `evaluate()` → return `Transform` with normalized content
- [ ] Add feature gate for HTML:
  ```toml
  normalization-html = ["lol_html"]
  ```
- [ ] Add tests:
  - [ ] Unicode variants normalize to same form
  - [ ] HTML tags stripped when feature enabled
  - [ ] Control chars removed
  - [ ] Binary content detected and rejected
- [ ] Benchmark: <3ms for 10KB input

**Acceptance:** Normalization produces canonical form, Transform outcome.

---

### WS5-07: InjectionStage — Heuristic Detector
**Dependencies:** WS2-03, WS5-01  
**Feature Flag:** `heuristics` (default)

- [ ] Create `input/patterns.rs` (pattern library)
- [ ] Define `Pattern` struct: `{ id, regex, category, severity }`
- [ ] Implement 50+ patterns across 5 categories:
  - [ ] **Instruction injection** (10 patterns):
    - [ ] `Ignore previous instructions`
    - [ ] `Disregard all prior`
    - [ ] `New instructions:`
    - [ ] `Forget everything before`
    - [ ] `You are now`
    - [ ] etc.
  - [ ] **Role manipulation** (10 patterns):
    - [ ] `Act as if you are`
    - [ ] `Pretend to be`
    - [ ] `You're no longer`
    - [ ] etc.
  - [ ] **Context termination** (10 patterns):
    - [ ] `[SYSTEM_END]` (delimiter injection)
    - [ ] `</system>`
    - [ ] `-- End of system prompt --`
    - [ ] etc.
  - [ ] **Adversarial suffixes** (10 patterns):
    - [ ] `! ! ! ! ! !` (repetition)
    - [ ] Base64-encoded instructions
    - [ ] ROT13 obfuscation
    - [ ] etc.
  - [ ] **Multilingual obfuscation** (10 patterns):
    - [ ] Zero-width characters
    - [ ] Homoglyphs (Cyrillic lookalikes)
    - [ ] Unicode confusables
    - [ ] etc.
- [ ] Implement `PatternLibrary`:
  - [ ] `builtin()` → static patterns
  - [ ] `#[cfg(feature = "patterns-dynamic")] with_overrides()` → JSON load
  - [ ] `matches(text: &str) -> Vec<Match>`
- [ ] Create `input/injection/heuristic.rs`
- [ ] Implement `HeuristicDetector` using `PatternLibrary`
- [ ] Implement `Detector` trait (sealed):
  - [ ] `score(content: &Content) -> f32`
  - [ ] `name() -> &str` → `"heuristic"`
  - [ ] `is_expensive() -> bool` → `false`
- [ ] Add tests for each pattern category
- [ ] Benchmark: <5ms for 1KB input with 50 patterns

**Acceptance:** 50+ patterns, <1% false positive on benign corpus.

---

### WS5-INJ-IT-1: Integration Tests (Normalization + Heuristic)
**Dependencies:** WS5-01, WS5-07

- [ ] Create `tests/integration/prompt_injection.rs`
- [ ] Add adversarial samples:
  - [ ] 20 known prompt injection payloads
  - [ ] 20 benign edge cases (should not trigger)
- [ ] Test pipeline: Normalization → InjectionStage (heuristic only)
- [ ] Verify:
  - [ ] >80% detection on adversarial set
  - [ ] <5% false positive on benign set
  - [ ] Normalized content reaches InjectionStage
- [ ] Document results in `docs/detection_baseline.md`

**Acceptance:** Pipeline works end-to-end, baseline metrics recorded.

---

## Sprint 5 (Weeks 9-10): WS5-INJ Part 2 (Structural + Ensemble + Spotlight)

### WS5-08: InjectionStage — Structural Analyzer
**Dependencies:** WS5-07  
**Feature Flag:** `heuristics` (default)

- [ ] Create `input/injection/structural.rs`
- [ ] Implement `StructuralAnalyzer` with analysis methods:
  - [ ] **Char frequency analysis**:
    - [ ] Punctuation density (! ? . ratios)
    - [ ] Digit-to-letter ratio
    - [ ] Special char concentration
  - [ ] **Instruction density**:
    - [ ] Imperative verb count per sentence
    - [ ] Capitalization patterns (ALL CAPS sections)
  - [ ] **Repetition detection**:
    - [ ] Token n-gram repetition
    - [ ] Char-level repetition (!!!, ...)
    - [ ] Phrase repetition across input
  - [ ] **Topic shift detection**:
    - [ ] Abrupt changes in vocabulary
    - [ ] Context boundary markers
- [ ] Implement scoring: `score(content: &Content) -> f32`
  - [ ] Combine all features into 0.0–1.0 score
  - [ ] Weight features by importance
- [ ] Implement `Detector` trait (sealed)
- [ ] Add unit tests for each feature
- [ ] Add test: benign text scores <0.3, adversarial >0.7
- [ ] Benchmark: <10ms for 1KB input

**Acceptance:** Structural features detect obfuscated injections missed by regex.

---

### WS5-09: InjectionStage — Ensemble Scorer
**Dependencies:** WS5-07, WS5-08  
**Feature Flag:** `heuristics` (default)

- [ ] Create `input/injection/ensemble.rs`
- [ ] Define `EnsembleStrategy` enum:
  - [ ] `AnyAboveThreshold { threshold: f32 }`
  - [ ] `WeightedAverage { weights: HashMap<String, f32> }`
  - [ ] `MajorityVote { threshold: f32 }`
  - [ ] `MaxScore`
- [ ] Implement `EnsembleScorer`:
  - [ ] `new(strategy: EnsembleStrategy)`
  - [ ] `aggregate(scores: &[f32]) -> f32`
- [ ] Update `InjectionStage` to compose:
  - [ ] `heuristic: HeuristicDetector`
  - [ ] `structural: StructuralAnalyzer`
  - [ ] `ensemble: EnsembleScorer`
- [ ] Implement `GuardrailStage` for InjectionStage:
  - [ ] `priority()` → `40`
  - [ ] `evaluate()` → run all detectors, ensemble score
  - [ ] Block if final score > threshold
- [ ] Add tests for each ensemble strategy
- [ ] Add test: ensemble improves detection over single detector

**Acceptance:** Ensemble achieves >90% detection, <5% FP.

---

### WS5-10: InjectionStage — Spotlight for RAG
**Dependencies:** WS5-09  
**Feature Flag:** `heuristics` (default)

- [ ] Create `input/injection/spotlight.rs`
- [ ] Implement boundary marker insertion:
  - [ ] Prefix: `[RAG_START source="{{url}}"]`
  - [ ] Suffix: `[RAG_END]`
- [ ] Implement `SpotlightDetector`:
  - [ ] Only activates for `Content::RetrievedChunks`
  - [ ] Detects boundary marker manipulation
  - [ ] Checks for injection patterns between markers
- [ ] Implement `Detector` trait (sealed)
- [ ] Update `InjectionStage::evaluate()` to conditionally add spotlight
- [ ] Add test: spotlight detects RAG-specific injection
- [ ] Add test: spotlight skips for non-RAG content

**Acceptance:** RAG boundary markers prevent indirect injection.

---

### WS5-INJ-IT-2: Full Integration Tests
**Dependencies:** WS5-10  

- [ ] Expand `tests/integration/prompt_injection.rs`
- [ ] Add 50 adversarial samples covering:
  - [ ] Direct instruction injection
  - [ ] Role manipulation
  - [ ] Context termination
  - [ ] Adversarial suffixes
  - [ ] Multilingual obfuscation
  - [ ] RAG indirect injection
  - [ ] Multimodal injection (OCR-extracted text)
- [ ] Add 100 benign samples:
  - [ ] Normal user queries
  - [ ] Technical content (code, JSON)
  - [ ] Non-English text
  - [ ] Long-form text
- [ ] Test full pipeline: Normalization → InjectionStage (all detectors)
- [ ] Measure:
  - [ ] Detection rate (target: >90%)
  - [ ] False positive rate (target: <5%)
  - [ ] Latency (target: <50ms P95)
- [ ] Document results in `docs/phase2_acceptance_report.md`

**Acceptance:** >90% detection, <5% FP, <50ms P95 latency.

---

## Phase 2 Acceptance Checklist

### Functional Requirements
- [ ] Template injection attempts blocked (TemplateScanner)
- [ ] Honeytoken leakage detected (HoneytokenStore + future EgressScanner)
- [ ] Scanner catches 10+ secret types (TemplateScanner)
- [ ] Delimiter injection blocked (RoleIsolation)
- [ ] >90% injection detection on adversarial set (InjectionStage)
- [ ] <5% false positive rate on benign corpus (InjectionStage)
- [ ] Fuzz tests pass without panics (WS4-06)

### Non-Functional Requirements
- [ ] <50ms P95 latency for full input pipeline
- [ ] Graceful degradation when features disabled
- [ ] Zero-dependency default (`heuristics` feature)
- [ ] All stages implement GuardrailStage correctly
- [ ] Transform propagation works (critical fix)
- [ ] RefusalPolicy/FailMode interaction documented
- [ ] Priority bands documented

### Documentation
- [ ] `docs/phase2_architecture_review.md` reviewed
- [ ] `docs/phase2_architecture_decisions.md` accepted
- [ ] `docs/architecture.md` updated with Phase 2 modules
- [ ] `README.md` status table updated
- [ ] Inline doc comments on all public APIs
- [ ] Examples added: `examples/prompt_hardening.rs`, `examples/injection_detection.rs`

### Testing
- [ ] All unit tests pass
- [ ] Integration tests pass (WS5-INJ-IT-2)
- [ ] Fuzz tests clean (WS4-06)
- [ ] Benchmark regression: <5ms executor overhead
- [ ] Property tests for Transform chaining (ADR-001)

### Quality Gates
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test --all-features` passes
- [ ] `cargo audit` clean
- [ ] `cargo deny check` passes
- [ ] CI workflow green

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|---------|------------|
| Transform propagation breaks existing tests | High | High | Verify all pipeline tests, add regression suite |
| RefusalPolicy/FailMode confusion causes prod issue | Medium | High | Extensive documentation, example code |
| 90% detection rate not achievable with heuristics | Medium | Critical | Plan fallback: lower threshold to 85%, defer to Phase 3 ML |
| Honeytoken encryption key management unclear | Medium | Medium | Document key derivation from session secret |
| Pattern library false positives too high | Low | High | Tune patterns iteratively, add override mechanism |
| Fuzz tests find crashes | Low | Critical | Fix immediately, add to regression suite |

---

## Sprint Tracking

### Sprint 3 Exit Criteria
- [ ] TemplateScanner, SecureTemplate, RoleIsolation, HoneytokenStore, RefusalPolicy implemented
- [ ] Unit tests pass
- [ ] Fuzz tests running (may not be complete)
- [ ] Doc comments on all public APIs

### Sprint 4 Exit Criteria
- [ ] NormalizationStage, InjectionStage (heuristic only) implemented
- [ ] Pattern library with 50+ patterns
- [ ] Integration tests show >80% baseline detection
- [ ] Transform propagation fix verified

### Sprint 5 Exit Criteria (Phase 2 Complete)
- [ ] InjectionStage (structural + ensemble + spotlight) complete
- [ ] Full integration tests pass with >90% detection, <5% FP
- [ ] All acceptance criteria met
- [ ] Documentation complete
- [ ] Ready for Phase 3 (output validation, PII, moderation)

---

**Maintained by:** Implementation Team  
**Last Updated:** Pre-Sprint 3  
**Next Review:** End of Sprint 3
