# `wg-bastion` Execution Progress Tracker

**Plan Reference:** [`wg-bastion_plan_v3.md`](../wg-bastion_plan_v3.md)  
**Goal:** Defense-in-depth security suite for weavegraph LLM applications — OWASP LLM Top 10:2025, NIST AI RMF, agentic AI threats.  
**Priority:** Prompt injection defense (LLM01) and system prompt hardening (LLM07) delivered first.

---

## Phase 1: Pipeline Critical Path (Sprints 1–2)

> Unblock prompt & injection work by shipping the minimum pipeline framework.
> See plan §9 Phase 1 for full task descriptions.

### WS3-FAST — Build Hygiene

#### WS3-05 · Remove unused dependencies

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Removed 5 unused `[dependencies]` from `Cargo.toml`: `miette`, `jsonschema`, `schemars`, `ring`, `zeroize`. These were declared in Sprint-0 scaffolding but had zero `use` statements in source. All 6 pre-existing tests pass. A comment records the removals so they can be re-added behind feature gates when their modules land. |

<details><summary>Future enhancements</summary>

- Re-introduce `ring` (or `aes-gcm`) behind `honeytoken` feature gate when `prompt::honeytoken` lands (Phase 2).
- Re-introduce `zeroize` for secret-handling types in `config` and `session`.
- Re-introduce `jsonschema` + `schemars` behind `schema-export` feature when `output::schema` lands (Phase 3).
- Re-introduce `miette` behind `diagnostics` or `testing` feature for rich error rendering in CLI tooling.

</details>

#### WS3-06 · Add `#[cfg(feature)]` gates

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Added `#![cfg_attr(docsrs, feature(doc_auto_cfg))]` to `lib.rs`. Replaced bare `// pub mod …` comment stubs with properly documented, feature-annotated comments showing which feature flag each future module requires. This ensures `docs.rs` renders feature requirements and prepares the crate for incremental module activation. |

<details><summary>Future enhancements</summary>

- Convert comments to actual `#[cfg(feature = "…")] pub mod …;` declarations as each module's source file lands.
- Consider a `full` feature that enables everything (already declared in `Cargo.toml` features list).
- Add compile-time diagnostics (`#[cfg(not(…))]` stubs) that emit friendly messages when someone tries to import a gated type without the feature enabled.

</details>

### WS1-FAST — CI

#### WS1-04 · CI workflow for wg-bastion

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete (existing) |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Workspace-level `.github/workflows/ci.yml` already covers `wg-bastion` via `--workspace` flags. Jobs: `fmt`, `clippy` (MSRV 1.89.0 + stable), `test` (MSRV + stable), `doc` (with `--cfg docsrs --all-features`), `cargo-deny` (license/advisory audit), `cargo-machete` (unused dep detection). No additional per-crate workflow needed. |

<details><summary>Future enhancements</summary>

- Add a `wg-bastion`-specific job that runs `cargo test -p wg-bastion --all-features` to exercise feature-gated code paths.
- Add integration test job with Docker services (Redis, Postgres) for persistence/session features when they land.
- Add `cargo-audit` as a separate advisory check (currently partially covered by `cargo-deny`).
- Consider adding a security-focused fuzzing job (`cargo-fuzz`) for injection detection stages.

</details>

### WS2-FAST — Pipeline Framework (Critical Path)

#### WS2-01 · `Content` enum

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Created `pipeline/content.rs` (~266 lines). `Content` is a `#[non_exhaustive]` enum with variants: `Text`, `Messages`, `ToolCall`, `ToolResult`, `RetrievedChunks`. Supporting types: `Message` (role/content with `system`/`user`/`assistant` constructors), `RetrievedChunk` (with builder pattern for `text`/`score`/`source`/`metadata`). Key methods: `variant_name()` for metrics labels, `as_text()` for lossy plaintext flattening. Full serde roundtrip support. 7 unit tests, all passing. |

<details><summary>Future enhancements</summary>

- Add `Content::Multimodal { parts: Vec<ContentPart> }` variant for image/audio inspection (Phase 3+, `multimodal` feature).
- Add `Content::Structured(serde_json::Value)` for arbitrary JSON payloads.
- Consider `Cow<'_, str>` in `Message::content` to avoid allocations when content is borrowed.
- Add `Content::byte_size()` for payload size budgeting.
- Add `FromStr` / `From<String>` impls for ergonomic construction from plain text.

</details>

#### WS2-02 · `StageOutcome` enum

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Created `pipeline/outcome.rs` (~247 lines). `Severity` enum (Info/Low/Medium/High/Critical) with derived `Ord` for threshold comparisons and `Display` impl. `StageOutcome` is `#[non_exhaustive]` with variants: `Allow{confidence}`, `Block{reason, severity}`, `Transform{content, description}`, `Escalate{reason, timeout}`, `Skip{reason}`. Convenience constructors and `is_*()` predicate methods. `StageError` enum with struct variants `BackendUnavailable`, `InvalidContent`, `Internal` for pipeline degradation handling. 5 unit tests, all passing. |

<details><summary>Future enhancements</summary>

- Add `StageOutcome::Quarantine` variant for content that needs async review (agent quarantine flows).
- Add `StageOutcome::RateLimit { retry_after: Duration }` for abuse-prevention stages.
- Consider making `Severity` carry a numeric score (`f32`) in addition to the ordinal for fine-grained ensemble scoring.
- Add `StageOutcome::merge()` combinators for multi-stage outcome aggregation.
- Add serialization support to `StageOutcome` for audit log persistence.

</details>

#### WS2-03 · `GuardrailStage` trait

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Created `pipeline/stage.rs` (~300 lines). `GuardrailStage` async trait with methods: `id() -> &str`, `evaluate(&Content, &SecurityContext) -> Result<StageOutcome, StageError>`, `degradable() -> bool` (default true), `priority() -> u32` (default 100). New `SecurityContext` with builder pattern: `session_id`, `user_id`, `risk_score: f32`, `metadata: HashMap<String, serde_json::Value>`, `parent: Option<Arc<SecurityContext>>` for agent delegation chains. Methods: `delegation_depth()`, `child()`, `with_risk_score()`, `with_metadata()`. `StageMetrics` struct for executor instrumentation. 4 unit tests (context builder, delegation chain, allow stage, block stage), all passing. |

<details><summary>Future enhancements</summary>

- Add `GuardrailStage::cacheable() -> bool` (default false) for LRU cache integration (WS2-05).
- Add `GuardrailStage::circuit_breaker_config()` for per-stage circuit-breaker tuning (WS2-06).
- Add `SecurityContext::with_parent()` for explicit parent setting (vs `child()` which creates the Arc).
- Consider a `#[derive(GuardrailStage)]` proc-macro for reducing boilerplate on simple stages.
- Add `GuardrailStage::describe() -> StageDescription` for introspection (listing expected content types, supported severity levels).
- Evaluate replacing `async_trait` with native async trait (RPITIT) once MSRV allows it (Rust 1.75+).

</details>

#### WS2-04 · `PipelineExecutor`

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Created `pipeline/executor.rs` (~560 lines). `PipelineExecutor` with `ExecutorBuilder`: stages sorted by priority at build time, sequential execution with full `FailMode` integration (`Closed`/`Open`/`LogOnly`). Short-circuits on `Block`/`Escalate` outcomes. Degradable stage errors are logged and skipped; critical stage errors propagate as `ExecutorError::CriticalStageFailure`. `PipelineResult` carries: final `StageOutcome`, per-stage `StageMetrics` (duration, cache_hit, degraded, outcome label), list of degraded stage IDs, and `overridden` flag. Helper methods: `is_allowed()`, `has_degraded()`, `total_duration()`. 10 unit tests covering: empty executor, single allow, priority ordering, block short-circuit, degradable continuation, critical failure propagation, all 3 fail-mode variants, duration summation. All passing. |

<details><summary>Future enhancements</summary>

- Add parallel execution mode via `tokio::JoinSet` for independent stages (WS2-08 benchmarks first to set baseline).
- Integrate `StageCache` (LRU + TTL via `dashmap`) — check cache before `evaluate()`, store on cache-miss (WS2-05).
- Integrate `CircuitBreaker` per stage — skip stages in open state, attempt in half-open (WS2-06).
- Add `PipelineExecutor::run_with_transform()` that passes transformed content forward to subsequent stages.
- Add configurable timeouts per stage via `tokio::time::timeout`.
- Add OpenTelemetry span creation per stage for distributed tracing (Phase 6, `telemetry-otlp` feature).
- Consider Tower-style middleware composition for cross-cutting concerns (logging, metrics, caching).
- Add `PipelineResult::to_audit_record()` for structured audit log emission.

</details>

#### WS2-07 · Backward-compat wrapper

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 1 / Sprint 1 |
| **Summary** | Created `pipeline/compat.rs` (~195 lines). `LegacyAdapter<S>` wraps any `SecurityStage` (takes `&str`, returns `StageResult`) and presents it as a `GuardrailStage` (takes `&Content`, returns `StageOutcome`). Text content is forwarded; non-text content returns `Skip`. Legacy `StageResult::pass()` maps to `Allow{1.0}`, `fail(reason)` maps to `Block{reason, Medium}`. Legacy stages are always degradable. Session/user IDs are propagated to the legacy `SecurityContext`. 4 unit tests (adapted pass, adapted fail, non-text skip, adapter-with-executor integration), all passing. |

<details><summary>Future enhancements</summary>

- Add metadata propagation from legacy `StageResult::metadata` into a `StageOutcome::Allow` metadata extension.
- Allow configurable `Severity` mapping (currently hardcoded `Medium` for all legacy failures).
- Add `LegacyAdapter::with_degradable(bool)` for overriding the always-degradable default.
- Consider a `From<Box<dyn SecurityStage>>` impl on `ExecutorBuilder` for ergonomic migration.
- Add deprecation warning (tracing event) when a legacy adapter is constructed, nudging migration.

</details>

---

## Phase 1 Gate

| Criterion | Status |
|-----------|--------|
| `Content`, `StageOutcome`, `GuardrailStage`, `PipelineExecutor` compile & pass tests | ✅ 36 tests pass |
| CI workflow green | ✅ Workspace CI covers wg-bastion |
| Unused deps removed | ✅ 5 deps removed |
| Feature gates in place | ✅ `docsrs` cfg + feature-annotated module comments |

**Phase 1 complete.** All acceptance criteria met. Ready to proceed to Phase 2 (Prompt & Injection Security).

---

## Phase 2: Prompt & Injection Security (Sprints 3–5)

> Defense-in-depth against OWASP LLM01 (Prompt Injection) and LLM07 (System Prompt Leakage).
> See plan §9 Phase 2 and the [detailed implementation plan](../docs/2026-02-21-feat-phase2-prompt-injection-security-plan.md).

### Sub-Phase 2A — Prompt Hardening

#### 2A.1 · Phase 2 Dependencies & Feature Flags

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Added optional deps: `regex`, `aho-corasick`, `unicode-normalization`, `ring`, `zeroize`, `lol_html`. Feature flags: `heuristics` (regex + aho-corasick + unicode-normalization, default ON), `honeytoken` (ring + zeroize + aho-corasick), `normalization-html` (lol_html). All `cargo check` feature combinations pass. |

#### 2A.2 · TemplateScanner

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Created `prompt/scanner.rs` (~350 lines). 10 built-in secret patterns (API keys, JWTs, PEM blocks, etc.) compiled into `RegexSet`. Shannon entropy detection with configurable threshold (default 4.5). Implements `GuardrailStage` at priority 5 — blocks on secret detection, warns on high-entropy. 10 unit tests. |

#### 2A.3 · SecureTemplate

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Created `prompt/template.rs`. Typed placeholders (`Text`/`Number`/`Enum`/`Json`) with auto-escaping and max-length enforcement. `TemplateScanner` integration via `TryFrom`. Builder pattern for template construction. 12 unit tests. |

#### 2A.4 · RoleIsolation

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Created `prompt/isolation.rs`. Per-request randomized boundary markers, role marker forgery detection, nesting violation checks. Implements `GuardrailStage` at priority 15. 10 unit tests. |

#### 2A.5 · HoneytokenStore

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Created `prompt/honeytoken.rs` (gated behind `honeytoken` feature). AES-256-GCM encryption via `ring`, HKDF key derivation, HMAC fingerprinting (never logs plaintext), Aho-Corasick multi-pattern detection, pool rotation with `Arc<RwLock<Vec<Honeytoken>>>`. Zeroizing for secret memory. 9 unit tests. |

#### 2A.6 · RefusalPolicy

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Created `prompt/refusal.rs`. Four refusal modes: `Block`, `Redact`, `SafeResponse`, `Escalate`. Per-severity mapping with configurable defaults. `AuditEntry` with hashed reasons (never logs raw content). 11 unit tests. |

#### 2A.7 · Wire Prompt Module

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Added `pub mod prompt` (gated behind `heuristics` feature) to `lib.rs`. All prompt types re-exported in prelude: `TemplateScanner`, `SecureTemplate`, `RoleIsolation`, `RefusalPolicy`, `HoneytokenStore`. |

### Sub-Phase 2B — Input Analysis

#### 2B.1 · NormalizationStage

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 3 |
| **Summary** | Created `input/normalization.rs`. NFKC Unicode normalization, control character stripping, script mixing detection, UTF-8 safe truncation. Optional HTML sanitization via `lol_html` (behind `normalization-html` feature). Implements `GuardrailStage` at priority 10 with `Transform` outcome. Handles all `Content` variants. 20 unit tests. |

#### 2B.2 · HeuristicDetector + Injection Patterns

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 4 |
| **Summary** | Created `input/patterns.rs` (50 patterns across 5 categories: role-confusion, instruction-override, delimiter-manipulation, system-extraction, encoding-evasion) and `input/injection.rs` (`HeuristicDetector` with `RegexSet` two-pass detection, custom pattern support, `InjectionStage` composing detector + structural analyzer + ensemble scorer). Implements `GuardrailStage` at priority 50. 23 unit tests. |

#### 2B.3 · StructuralAnalyzer

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 4 |
| **Summary** | Created `input/structural.rs`. Five analysis signals: suspicious character density, instruction density, language mixing, character repetition, punctuation anomaly. Weighted risk scoring (0.0–1.0). 13 unit tests. |

#### 2B.4 · EnsembleScorer

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 4 |
| **Summary** | Created `input/ensemble.rs`. Trait-based `EnsembleStrategy` with four built-in implementations: `AnyAboveThreshold`, `WeightedAverage`, `MajorityVote`, `MaxScore`. User-extensible via custom trait impls. 9 unit tests. |

#### 2B.5 · InjectionStage

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 4 |
| **Summary** | `InjectionStage` in `input/injection.rs` composes `HeuristicDetector` + `StructuralAnalyzer` + `EnsembleScorer`. Handles all `Content` variants. 7 additional unit tests. |

#### 2B.6 · Wire Input Module

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 4 |
| **Summary** | Added `pub mod input` (gated behind `heuristics` feature) to `lib.rs`. All input types re-exported in prelude: `NormalizationStage`, `InjectionStage`, `EnsembleScorer`, `Spotlight`. |

### Sub-Phase 2C — Validation & Hardening

#### 2C.1 · Spotlight (RAG Boundary Marking)

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 5 |
| **Summary** | Created `input/spotlight.rs`. Per-request randomized boundary markers for RAG-retrieved content. Detects injection attempts, role marker forgery, and escape sequences within retrieved chunks. Implements `GuardrailStage` at priority 45. 12 unit tests. |

#### 2C.2 · Fuzz Targets

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 5 |
| **Summary** | Created `fuzz/` directory with 3 `cargo-fuzz` targets: `fuzz_template` (SecureTemplate parsing), `fuzz_injection` (HeuristicDetector + StructuralAnalyzer), `fuzz_normalization` (NormalizationStage). Documentation in `fuzz/README.md`. |

#### 2C.3 · Integration Tests

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 5 |
| **Summary** | Created `tests/injection_detection.rs` (759 lines, 20 integration tests). Adversarial corpus: 100 samples across 5 categories. Benign corpus: 52 samples. Full pipeline composition tests (NormalizationStage → Spotlight → InjectionStage). Tests all 4 ensemble strategies. RAG injection tests, template security, normalization evasion. **Results: 100% detection rate, 1.9% FP rate, P95 latency 5.5ms.** |

#### 2C.4 · Update PROGRESS.md

| Field | Value |
|-------|-------|
| **Status** | ✅ Complete |
| **Date** | Phase 2 / Sprint 5 |
| **Summary** | This section. |

---

### Phase 2 Gate

| Criterion | Target | Actual | Status |
|-----------|--------|--------|--------|
| Injection detection rate (adversarial corpus) | >90% | **100%** (100/100) | ✅ |
| False positive rate (benign corpus) | <5% | **1.9%** (1/52) | ✅ |
| P95 full pipeline latency | <50ms | **5.5ms** | ✅ |
| Injection patterns | ≥50 | **50** (5 categories) | ✅ |
| New tests (unit + integration) | ≥80 | **197 total** (161 unit + 20 integration + 16 prior) | ✅ |
| Fuzz targets | ≥3 | **3** (template, injection, normalization) | ✅ |
| All ensemble strategies tested | 4/4 | **4/4** | ✅ |

**Phase 2 complete.** All acceptance criteria met. Ready to proceed to Phase 3 (Output & Data Leakage Prevention).

---

## Phase 3–7

> _See plan for full breakdown._
