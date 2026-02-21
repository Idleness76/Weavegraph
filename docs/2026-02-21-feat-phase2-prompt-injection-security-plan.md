---
title: "feat: Implement Phase 2 Prompt & Injection Security"
type: feat
status: completed
date: 2026-02-21
source_docs:
  plans:
    - wg-bastion_plan_v3.md
    - wg-bastion/PROGRESS.md
---

# ✨ feat: Implement Phase 2 — Prompt & Injection Security

## Enhancement Summary

**Deepened on:** 2026-02-21
**Sections enhanced:** 12
**Research agents used:** security-sentinel, performance-oracle, architecture-strategist, rabak-rust-reviewer, pattern-recognition-specialist, code-simplicity-reviewer, best-practices-researcher, framework-docs-researcher, spec-flow-analyzer (9 total)

### 🔴 Critical Pre-Implementation Fixes

1. **Transform Propagation in PipelineExecutor** — `PipelineExecutor::run()` does NOT pass transformed content to subsequent stages. NormalizationStage's output never reaches InjectionStage. **Must fix in executor.rs before Phase 2B begins.** Track `current_content: Cow<'_, Content>` and update on Transform outcomes.
2. **AES-GCM Nonce Management** — Plan lacks nonce strategy. Use `ring::rand::SystemRandom` for random nonces (prepended to ciphertext). Never counter-based for a library crate (stateless).
3. **Key Derivation via HKDF** — Master key → derive per-purpose keys via `ring::hkdf::HKDF_SHA256`. Info strings: `b"wg-bastion-honeytoken-v1"`, `b"wg-bastion-hmac-v1"`.
4. **Output Pipeline Hook** — Honeytoken egress detection scans LLM *output*, but no output pipeline exists. Add `HoneytokenStore::scan_output(&self, output: &Content)` as a standalone utility; defer full `OutputPipeline` to Phase 4 (WS6).
5. **RefusalPolicy / FailMode Overlap** — `RefusalPolicy` duplicates `PipelineExecutor::FailMode`. Resolution: RefusalPolicy = remediation strategy (what response to send), FailMode = enforcement gate (whether to block). Transform outcomes bypass FailMode. Add `refusal_response: Option<String>` to `PipelineResult`.

### 🟡 Key Design Improvements

6. **Full scope — no simplifications** — All 4 placeholder types, all 4 ensemble strategies, all 5 structural analyses, full 50+ patterns, full rotation, per-request randomization, and Shannon entropy. Going all in.
7. **Rust API fixes** — `TemplateScanner::scan()` → return `Result`; `PatternMatch::matched_span` → `Range<usize>`; `shannon_entropy()` → `&[u8]`; `HoneytokenStore::token_pool` → `Arc<[Honeytoken]>`; implement `TryFrom<&str>` for `SecureTemplate`.
8. **Use Aho-Corasick for honeytoken detection** — 10x faster than regex for literal token matching in LLM output (~200-500 MB/s vs ~50 MB/s).
9. **Concrete types for InjectionStage composition** — Use `HeuristicDetector` + `StructuralAnalyzer` as concrete fields, not trait objects. Zero-cost composition for security-critical path.
10. **NormalizationStage zero-copy** — Return `Allow` when content is already normalized; only `Transform` when actually modified. Use `Cow<str>` for `is_nfkc()` fast-path.
11. **EnsembleStrategy as trait** — Extensible design allowing users to implement custom scoring strategies without forking.

### New Considerations Discovered
- Unicode confusable bypasses extend beyond NFKC — need explicit Cyrillic/Latin homoglyph map
- Honeytoken metrics can leak presence via timing/cardinality — use constant-time comparison
- lol_html chunk boundaries can cause tag-split evasion — use `element!("*")` not regex for HTML
- RegexSet does NOT return match spans — must re-search with individual `Regex` for positions
- Zeroizing<Vec<u8>> can leak on reallocation — pre-allocate to avoid growing

---

## Overview

Phase 2 of `wg-bastion` implements the **primary deliverable** of the crate: defense against **LLM01 (Prompt Injection)** and **LLM07 (System Prompt Leakage)** — the two highest-risk attack vectors in the OWASP LLM Top 10:2025.

This phase spans **Sprints 3–5** and delivers two new modules (`prompt`, `input`) containing 12 major components that compose into the `GuardrailStage` pipeline framework built in Phase 1.

At completion, `wg-bastion` ships its **MVP (v0.1.0-alpha.1)** — the minimum security-valuable release.

## Problem Statement / Motivation

LLM applications face two critical, overlapping threats:

1. **Prompt Injection (LLM01)** — Attackers craft inputs that override system instructions, causing the model to execute unintended actions. Techniques include role confusion, instruction override, delimiter manipulation, encoding evasion, and multi-turn escalation.

2. **System Prompt Leakage (LLM07)** — Secrets, API keys, or proprietary instructions embedded in system prompts get exfiltrated through the model's output, either via direct extraction attacks or inadvertent disclosure.

Without these defenses, any `weavegraph` application using LLMs is vulnerable to the most common and highest-impact attack classes. Phase 1 built the pipeline scaffolding — Phase 2 fills it with real security stages.

## Proposed Solution

Deliver two feature-gated modules implementing defense-in-depth:

### Module 1: `prompt` — System Prompt Security (WS4)
Protects system prompts from leakage and ensures template integrity:
- **`TemplateScanner`** — Regex + Shannon entropy secret detection (10+ patterns)
- **`SecureTemplate`** — Typed placeholders with max-length, auto-escaping, role markers
- **Role Isolation**— `[SYSTEM_START]...[SYSTEM_END]` boundary markers with delimiter detection
- **`HoneytokenStore`** — AES-256-GCM encrypted canary tokens with rotation and egress detection
- **`RefusalPolicy`** — Configurable response modes (Block/Redact/SafeResponse/Escalate)

### Module 2: `input` (partial) — Injection Detection Pipeline (WS5-INJ)
Multi-layer input validation before content reaches the LLM:
- **`NormalizationStage`** — Unicode NFKC, HTML sanitization (lol_html), control char stripping, MIME validation, truncation
- **`InjectionStage`** — Ensemble detection with heuristic patterns (50+), structural analysis, and multiple scoring strategies
- **Spotlighting** — Boundary marking for RAG content to prevent indirect injection

## Technical Considerations

### Architecture Impacts
- Two new top-level modules (`prompt/`, `input/`) gated behind `heuristics` feature flag
- New dependencies: `regex`, `unicode-normalization`, `ring`, `zeroize`, `lol_html`, `aho-corasick`
- All new types implement `GuardrailStage` to compose with `PipelineExecutor`
- `Content::as_text()` used heavily for text extraction across all content variants

#### Architecture Research Insights
- **🔴 Transform Propagation Fix Required**: `PipelineExecutor::run()` must track `current_content` and update it on `Transform` outcomes. Without this, NormalizationStage output never reaches InjectionStage. Fix in `executor.rs` BEFORE Phase 2B:
  ```rust
  // In PipelineExecutor::run(), after processing each stage:
  match outcome {
      StageOutcome::Transform { content: new_content, .. } => {
          current_content = Cow::Owned(new_content);
          // Continue to next stage with transformed content
      }
      // ... existing handling for Allow/Block/Escalate/Skip
  }
  ```
- **Module structure approved**: `prompt/` and `input/` separation is clean. However, Spotlighting should be a helper within `InjectionStage`, not a separate top-level stage — reduces priority ordering complexity.
- **RefusalPolicy semantics**: RefusalPolicy = *what response to send* (remediation). FailMode = *whether to block* (enforcement). They serve different layers. Add `refusal_response: Option<String>` to `PipelineResult`.
- **Extension points for Phase 3**: ML classifiers integrate cleanly by adding new `DetectorScore` sources to the ensemble. The `GuardrailStage` trait is sufficient — no new traits needed.
- **Feature gate validation**: Test all combinations in CI: `--no-default-features`, `--all-features`, `--features heuristics`, `--features honeytoken`, `--features "heuristics honeytoken normalization-html"`.

### Performance Implications
- **Heuristic detection**: P50 1ms / P95 3ms via pre-compiled `RegexSet` (all 50+ patterns in single DFA pass)
- **Normalization**: P50 0.5ms / P95 2ms via `lol_html` streaming parser
- **Entropy calculation**: O(n) sliding window, negligible overhead
- **AES-256-GCM**: ~0.1ms per encrypt/decrypt operation (ring)
- **Full pipeline (all Phase 2 stages)**: Target P95 <50ms

#### Performance Research Insights
- **RegexSet realistic latency**: 50 patterns on 100KB input ≈ 2-5ms. On 1MB input ≈ 10-20ms. Well within 50ms budget.
- **Aho-Corasick for honeytokens**: 200-500 MB/s for literal matching vs ~50 MB/s for RegexSet. Use `aho-corasick` for honeytoken egress scanning (literal strings, not regex).
- **NFKC zero-copy fast path**: `unicode-normalization` provides `is_nfkc()` check. If already normalized, return `Cow::Borrowed` (zero allocation). ~50-100 MB/s throughput for full normalization.
- **lol_html streaming**: O(chunk_size) memory, not O(document_size). Safe for 1MB inputs with <50MB RAM overhead.
- **Pipeline parallelism**: NormalizationStage must run before InjectionStage (sequential). But within InjectionStage, heuristic + structural analysis are independent — can run concurrently via `tokio::join!`. Estimated saving: ~1-2ms on large inputs.
- **Total estimated pipeline latency** (100KB input):
  | Stage | P50 | P95 |
  |-------|-----|-----|
  | NormalizationStage | 1ms | 3ms |
  | InjectionStage (heuristic) | 2ms | 5ms |
  | InjectionStage (structural) | 0.5ms | 1ms |
  | Ensemble scoring | 0.01ms | 0.05ms |
  | **Total** | **3.5ms** | **9ms** |
- **Pre-compile all regex**: Use `std::sync::LazyLock` (not `lazy_static`). Compile once at first use, amortized over all calls.
- **`Content::as_text()` allocation**: Already uses `Cow<str>` for zero-copy on `Text` variant. For `Messages`, consider a scratch buffer pattern to avoid repeated allocations in hot path.

### Security Considerations
- **ReDoS protection**: All regex patterns must be validated against catastrophic backtracking. Use `regex` crate's built-in linear-time guarantees (no backreferences)
- **TOCTOU prevention**: `Content` is immutable `&` reference through pipeline — no mutation possible between stages
- **Key material safety**: `zeroize` on drop for all AES keys and honeytoken material; environment variable sourcing only
- **Honeytoken log redaction**: Never log raw honeytoken values; log only HMAC fingerprints for detection correlation
- **Input size limits**: All stages enforce configurable max content size (default 1MB) to prevent OOM
- **Unicode confusables**: NFKC + script-mixing detection for cross-script homoglyph attacks (Cyrillic а vs Latin a)
- **Delimiter injection**: Spotlighting uses randomized per-request markers (not static strings) to prevent marker forgery in RAG content

#### Security Research Insights
- **🔴 AES-GCM Nonce Strategy**: Use `ring::rand::SystemRandom` to generate random 96-bit nonces for each encryption. Prepend nonce to ciphertext (nonce ‖ ciphertext ‖ tag). Never use counter-based nonces in a library (no persistent state). With random nonces and 128-bit tokens, collision probability negligible up to 2^32 encryptions.
- **🔴 Key Derivation**: Use `ring::hkdf::HKDF_SHA256` to derive per-purpose subkeys from a master key. Info strings: `b"wg-bastion-honeytoken-aes-v1"` for encryption, `b"wg-bastion-honeytoken-hmac-v1"` for fingerprinting. Never use the master key directly.
- **🔴 GCM Associated Data**: Bind honeytokens to context — include `session_id` or `template_id` in AAD to prevent cross-session token replay.
- **🟡 Unicode confusable bypass**: NFKC alone misses Cyrillic о (U+043E) vs Latin o (U+006F). Add explicit confusable mapping for top 50 Latin/Cyrillic/Greek pairs. Consider `unicode-security` crate.
- **🟡 Timing side-channels**: Use constant-time comparison for honeytoken detection (`ring::constant_time::verify_slices_are_equal`). Pattern matching timing is unavoidable but acceptable (attacker can't derive which pattern matched from response time alone).
- **🟡 Zeroize reallocation**: Pre-allocate `Zeroizing<Vec<u8>>` to expected size. Never grow a `Zeroizing<Vec<u8>>` — old buffer won't be zeroed after realloc.
- **🟡 Encoding bypass**: Base64, hex, URL encoding, ROT13 bypass pattern matching. Add pre-normalization decoding step or encoding-aware patterns.
- **🟡 lol_html safety**: Use `element!("*", |el| el.remove_and_keep_content())` for tag stripping. Also strip `<script>` and `<style>` with `el.remove()` (including content). Handle `RewritingError` gracefully — fall back to regex stripping.

### Existing Types Used
- `Content` enum (`pipeline/content.rs`) — all stages receive this
- `StageOutcome` (`pipeline/outcome.rs`) — all stages return this
- `GuardrailStage` trait (`pipeline/stage.rs`) — all stages implement this
- `SecurityContext` (`pipeline/stage.rs`) — session/user context passed through
- `StageError` (`pipeline/outcome.rs`) — error reporting
- `PipelineExecutor` (`pipeline/executor.rs`) — orchestrates stage execution
- `Severity` (`pipeline/outcome.rs`) — threat severity levels

## Implementation Phases

### Phase 2A: Prompt Hardening Foundation (Sprint 3)

##### Task 2A.1: Add Phase 2 Dependencies to `Cargo.toml`
**Files:** `wg-bastion/Cargo.toml`
**Depends on:** None
**Success criteria:**
- [ ] `regex = "1"` added to `[dependencies]`
- [ ] `unicode-normalization = "0.1"` added to `[dependencies]`
- [ ] `aho-corasick = "1"` added to `[dependencies]`
- [ ] `ring = { version = "0.17", optional = true }` added (for honeytoken encryption)
- [ ] `zeroize = { version = "1.8", features = ["derive"], optional = true }` added
- [ ] `lol_html = { version = "2", optional = true }` added
- [ ] New feature flags added: `honeytoken = ["ring", "zeroize"]`, `normalization-html = ["lol_html"]`
- [ ] `heuristics` feature updated to include `regex`, `aho-corasick`, `unicode-normalization`
- [ ] `cargo check -p wg-bastion --all-features` passes
**Test command:** `cargo check -p wg-bastion --all-features`

> **Research Insight — Task 2A.1**: Also add `aho-corasick` as an explicit dependency (not just transitive via regex) for honeytoken literal matching. Verify `ring` builds on Windows CI targets. Test feature combinations: `--no-default-features`, `--all-features`, individual features.
**Files:** `wg-bastion/src/prompt/mod.rs`, `wg-bastion/src/prompt/scanner.rs`
**Depends on:** Task 2A.1
**Success criteria:**
- [ ] `prompt/mod.rs` created with submodule declarations
- [ ] `TemplateScanner` struct with `ScannerConfig` (custom patterns, entropy threshold, enabled pattern categories)
- [ ] `SecretPattern` struct: `id: &'static str`, `description: &'static str`, `regex: Regex`, `severity: Severity`
- [ ] Built-in patterns for 10+ secret types: `aws-key` (`AKIA[0-9A-Z]{16}`), `gcp-key` (`AIza[0-9A-Za-z\-_]{35}`), `openai-key` (`sk-[a-zA-Z0-9]{20,}`), `anthropic-key` (`sk-ant-[a-zA-Z0-9]{20,}`), `jwt` (`eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`), `private-key` (`-----BEGIN .* PRIVATE KEY-----`), `password-url` (`://[^:]+:[^@]+@`), `generic-api-key` (`[a-zA-Z0-9]{32,}` with entropy > 4.5), `github-token` (`gh[ps]_[A-Za-z0-9_]{36,}`), `slack-token` (`xox[bpras]-[0-9A-Za-z-]+`)
- [ ] Shannon entropy calculation function: `fn shannon_entropy(s: &str) -> f64` — sliding window of configurable size
- [ ] `TemplateScanner::scan(&self, template: &str) -> Vec<SecretFinding>` method
- [ ] `SecretFinding` struct: `pattern_id`, `matched_text` (redacted), `position`, `severity`, `entropy`
- [ ] Implements `GuardrailStage` trait (evaluates `Content::Text`, returns `Block` if secrets found)
- [ ] 8+ unit tests: each secret type detected, entropy threshold, no false positives on benign text, edge cases (partial matches, overlapping patterns)
**Test command:** `cargo test -p wg-bastion --lib prompt::scanner`

> **Research Insights — Task 2A.2**:
> - **`scan()` must return `Result`**: `scan(&self, template: &str) -> Result<Vec<SecretFinding>, ScanError>`. In a security crate, silent failures are unacceptable — an attacker could craft input causing regex failure, bypassing detection.
> - **Shannon entropy**: Use `&[u8]` not `&str` — byte-level entropy is more meaningful for secret detection (Base64, hex). Window size: 20 bytes for API keys, 40 bytes for JWTs. Threshold: 4.5 bits/byte.
> - **Pattern compilation**: Compile `RegexSet` at construction time (`new() -> Result<Self, StageError>`), NOT in `LazyLock`. Fail-fast on invalid patterns instead of panicking at first use.
> - **Simplification**: Shannon entropy has high FP risk on UUIDs/base64 data. Add explicit allowlist for known-benign high-entropy patterns (UUIDs, base64 data URLs) to reduce false positives.
> - **`SecretPattern` ownership**: Use `Cow<'static, str>` for pattern strings to support both static built-in patterns and user-provided custom patterns.
**Files:** `wg-bastion/src/prompt/template.rs`
**Depends on:** Task 2A.2
**Success criteria:**
- [ ] `SecureTemplate` struct with `TemplateBuilder` pattern
- [ ] `Placeholder` enum: `Text { name, max_length, required }`, `Number { name, min, max }`, `Enum { name, allowed_values }`, `Json { name, schema_hint }`
- [ ] Template parsing: `SecureTemplate::compile(template_str)` validates and extracts `{{placeholder_name:type}}` syntax
- [ ] Max-length enforcement: truncates or rejects values exceeding placeholder limits
- [ ] Auto-escaping: user-provided values have role markers and delimiter patterns escaped before interpolation
- [ ] `SecureTemplate::render(&self, values: &HashMap<String, String>) -> Result<String, TemplateError>` method
- [ ] `TemplateError` enum: `MissingRequired`, `ExceedsMaxLength`, `InvalidType`, `ContainsSecrets`, `EscapingFailed`
- [ ] Integration with `TemplateScanner`: rendered output is scanned before return; secrets in values are caught
- [ ] 8+ unit tests: render with valid values, missing required, max-length enforcement, auto-escaping of role markers, secret in value detected, placeholder type validation
**Test command:** `cargo test -p wg-bastion --lib prompt::template`

> **Research Insights — Task 2A.3**:
> - **YAGNI review noted**: Typed placeholders (Number, Enum, Json) go beyond pure injection prevention — they're also input validation. **We're keeping all 4 types** for defense-in-depth. Type validation catches format-based injection vectors (e.g., JSON injection via schema mismatch).
> - **Implement `TryFrom<&str>` + `FromStr`**: Idiomatic fallible parsing in Rust. Keep `compile()` as convenience wrapper: `pub fn compile(s: &str) -> Result<Self, TemplateError> { s.try_into() }`.
> - **`render()` generics**: Accept `impl IntoIterator<Item = (K, V)> where K: AsRef<str>, V: AsRef<str>` instead of `&HashMap<String, String>` to avoid forcing String allocation on callers.
> - **Error types**: Use `thiserror::Error` with struct variants (consistent with Phase 1's `StageError`):
>   ```rust
>   #[derive(Debug, Error)]
>   pub enum TemplateError {
>       #[error("missing required value for placeholder '{name}'")] MissingRequired { name: String },
>       #[error("value exceeds max length for '{name}': {actual} > {max}")] ExceedsMaxLength { name: String, actual: usize, max: usize },
>   }
>   ```
**Files:** `wg-bastion/src/prompt/isolation.rs`
**Depends on:** Task 2A.3
**Success criteria:**
- [ ] `RoleIsolation` struct with configurable marker format (default `[SYSTEM_START]...[SYSTEM_END]`)
- [ ] `IsolationConfig`: marker prefix/suffix, randomization enabled (per-request random suffix for anti-forgery), escape sequences
- [ ] `RoleIsolation::wrap_system_prompt(&self, prompt: &str) -> String` — adds boundary markers
- [ ] `RoleIsolation::detect_boundary_violation(&self, text: &str) -> Vec<BoundaryViolation>` — finds forged/injected markers
- [ ] `BoundaryViolation` struct: `violation_type` (ForgedMarker, NestingViolation, UnmatchedMarker), `position`, `content_excerpt`
- [ ] Marker randomization: `[SYSTEM_START_a7b3c9f2]` using session-scoped random suffix to prevent marker forgery in RAG content
- [ ] Implements `GuardrailStage` trait: checks for boundary violations in user/RAG content, blocks if found
- [ ] 6+ unit tests: wrap/unwrap round-trip, forged marker detection, nesting violation, randomized markers, benign content passes
**Test command:** `cargo test -p wg-bastion --lib prompt::isolation`

> **Research Insights — Task 2A.4**:
> - **Per-request randomization is security-critical**: Keeps marker forgery cost high for adversaries. **Keeping full randomization** — static markers are predictable and defeasible.
> - **RoleIsolation stays as dedicated component**: Separation of concerns — template rendering and boundary enforcement are distinct security functions.
**Files:** `wg-bastion/src/prompt/honeytoken.rs`
**Depends on:** Task 2A.1
**Success criteria:**
- [ ] `HoneytokenStore` struct behind `#[cfg(feature = "honeytoken")]` feature gate
- [ ] `HoneytokenConfig`: `token_entropy_bits: u32` (default 128), `pool_size: usize` (default 50), `rotation_interval: Duration`, `key_source: KeySource`
- [ ] `KeySource` enum: `EnvVar(String)`, `Bytes(Zeroizing<Vec<u8>>)` — key loaded from env var or provided directly
- [ ] AES-256-GCM encryption via `ring::aead`: `encrypt_token()`, `decrypt_token()` with random 96-bit nonces
- [ ] Token pool generation: `HoneytokenStore::generate_pool(&self) -> Vec<Honeytoken>` — each token is a high-entropy random string
- [ ] `Honeytoken` struct: `id: String`, `plaintext: Zeroizing<String>`, `encrypted: Vec<u8>`, `created_at: SystemTime`, `hmac_fingerprint: String`
- [ ] Injection: `inject_into_prompt(&self, prompt: &str) -> (String, Vec<String>)` — returns modified prompt + list of injected token IDs
- [ ] Detection: `detect_in_output(&self, output: &str) -> Vec<HoneytokenDetection>` — scans LLM output for any active honeytokens
- [ ] `HoneytokenDetection` struct: `token_id`, `hmac_fingerprint` (NOT the plaintext), `position`, `severity: Critical`
- [ ] Rotation: `rotate(&mut self)` — generates new tokens, marks old tokens as detection-only (kept for 7 days)
- [ ] All key material uses `Zeroizing<T>` wrapper — cleared on drop
- [ ] 8+ unit tests: encrypt/decrypt round-trip, pool generation, injection into prompt, detection in output, rotation (old tokens still detected), key zeroization, false negative test (random text not detected)
**Test command:** `cargo test -p wg-bastion --lib prompt::honeytoken --features honeytoken`

> **Research Insights — Task 2A.5**:
> - **🔴 Nonce management (CRITICAL)**: Use `ring::rand::SystemRandom` for random 96-bit nonces. Prepend nonce to ciphertext: `nonce (12 bytes) ‖ ciphertext ‖ GCM tag (16 bytes)`. Use `LessSafeKey` API (simpler for random nonces than `SealingKey`/`NonceSequence`):
>   ```rust
>   let mut nonce_bytes = [0u8; 12];
>   rng.fill(&mut nonce_bytes)?;
>   let nonce = Nonce::assume_unique_for_key(nonce_bytes);
>   ```
> - **🔴 Key derivation**: Derive encryption + HMAC keys from master key via `ring::hkdf::HKDF_SHA256` with distinct info strings. Never use master key directly.
> - **🔴 GCM AAD binding**: Include `session_id` or `template_id` in Associated Authenticated Data to prevent cross-context token replay.
> - **Thread safety**: Use `Arc<RwLock<Vec<Honeytoken>>>` for token pool — supports rotation (adding new tokens, marking old as detection-only). Use `RwLock` for concurrent reads during scanning with exclusive write lock for rotation.
> - **Rotation stays in scope**: Full token lifecycle management (generate → encrypt → detect → rotate) is essential for production-grade honeytoken defense. Rotation prevents stale tokens from accumulating.
> - **Egress detection via Aho-Corasick**: Use `aho-corasick` for literal token matching in LLM output (200-500 MB/s, 10x faster than regex). Build `AhoCorasick` automaton from active token plaintexts.
> - **Zeroize pre-allocation**: Pre-allocate `Zeroizing<Vec<u8>>` to expected size. Never grow — old buffer won't be zeroed after realloc.
> - **Output scanning hook**: Since no output pipeline exists, `detect_in_output()` is a standalone method called by the application after LLM response. Accept `&Content` not `&str` for consistency with pipeline API. Document this as a utility, not a GuardrailStage.
> - **Constant-time comparison**: Use `ring::constant_time::verify_slices_are_equal` for HMAC verification to prevent timing side-channels.
**Files:** `wg-bastion/src/prompt/refusal.rs`
**Depends on:** Task 2A.2
**Success criteria:**
- [ ] `RefusalPolicy` struct with `RefusalConfig`
- [ ] `RefusalMode` enum: `Block { status_message }`, `Redact { replacement_text, redaction_marker }`, `SafeResponse { template }`, `Escalate { timeout, notify_channel }`
- [ ] `RefusalPolicy::apply(&self, outcome: &StageOutcome, ctx: &SecurityContext) -> RefusalAction` — maps blocking outcomes to configured refusal mode
- [ ] `RefusalAction` struct: `mode: RefusalMode`, `original_severity: Severity`, `audit_entry: AuditEntry`
- [ ] Per-severity configuration: different refusal modes for different severity levels (e.g., Low→Redact, High→Block, Critical→Escalate)
- [ ] Safe response templates: `"I cannot process that request. {{reason_category}}"` with limited interpolation (no user content)
- [ ] `AuditEntry` struct: `timestamp`, `session_id`, `user_id`, `stage_id`, `severity`, `refusal_mode`, `reason_hash` (not raw content)
- [ ] 6+ unit tests: each refusal mode, severity mapping, safe response template rendering, audit entry creation
**Test command:** `cargo test -p wg-bastion --lib prompt::refusal`

> **Research Insights — Task 2A.6**:
> - **🟡 RefusalPolicy provides value beyond FailMode**: Per-severity response customization enables nuanced UX (Low→Redact, High→Block, Critical→Escalate). **Keeping full module.** But resolve overlap: RefusalPolicy = remediation strategy, FailMode = enforcement gate.
> - **AuditEntry**: Use `tracing::warn!()` for now but keep the struct — Phase 6 telemetry will consume it. Log structured data via `tracing` span fields.
> - **Integration**: Add `refusal_response: Option<String>` to `PipelineResult` so callers receive the canned response alongside the blocking outcome.
**Files:** `wg-bastion/src/lib.rs`
**Depends on:** Tasks 2A.2–2A.6
**Success criteria:**
- [ ] `#[cfg(feature = "heuristics")] pub mod prompt;` uncommented and active in `lib.rs`
- [ ] Key types added to `prelude`: `SecureTemplate`, `TemplateScanner`, `RefusalPolicy`, `RoleIsolation`
- [ ] Honeytoken types conditionally exported: `#[cfg(feature = "honeytoken")] pub use prompt::honeytoken::{HoneytokenStore, HoneytokenDetection}`
- [ ] `cargo doc -p wg-bastion --all-features --no-deps` builds clean with feature annotations visible
- [ ] All existing 36 tests still pass
**Test command:** `cargo test -p wg-bastion --all-features`

---

### Phase 2B: Injection Detection Pipeline (Sprint 4)

##### Task 2B.1: Implement `NormalizationStage`
**Files:** `wg-bastion/src/input/mod.rs`, `wg-bastion/src/input/normalization.rs`
**Depends on:** Task 2A.1
**Success criteria:**
- [ ] `input/mod.rs` created with submodule declarations
- [ ] `NormalizationStage` struct with `NormalizationConfig`
- [ ] `NormalizationConfig`: `max_content_bytes: usize` (default 1MB), `strip_html: bool`, `normalize_unicode: bool`, `strip_control_chars: bool`, `validate_mime: bool`, `truncate: bool`
- [ ] **Unicode NFKC normalization**: via `unicode-normalization` crate — normalizes compatibility characters
- [ ] **Script-mixing detection**: detects mixed scripts (e.g., Cyrillic + Latin in same word) as potential homoglyph evasion; emits warning in `SecurityContext` metadata
- [ ] **HTML sanitization**: via `lol_html` streaming parser (behind `normalization-html` feature) — strips all tags, decodes entities, preserves text content
- [ ] **Control character stripping**: removes zero-width chars (U+200B, U+200C, U+200D, U+FEFF, U+00AD, U+2060), bidirectional controls (U+202A–U+202E, U+2066–U+2069), tag characters (U+E0001–U+E007F)
- [ ] **Content truncation**: enforces `max_content_bytes` limit; truncates at UTF-8 boundary
- [ ] Returns `StageOutcome::Transform` with normalized content, or `StageOutcome::Allow` if no changes needed
- [ ] Implements `GuardrailStage` with priority 10 (runs first — before injection detection)
- [ ] 10+ unit tests: NFKC normalization, HTML stripping, control char removal, truncation at UTF-8 boundary, script mixing detection, MIME validation, empty input handling, `Content::Messages` normalization per message, `Content::RetrievedChunks` normalization per chunk
**Test command:** `cargo test -p wg-bastion --lib input::normalization --features heuristics`

> **Research Insights — Task 2B.1**:
> - **Zero-copy fast path**: Check `input.is_nfkc()` before normalizing. Return `StageOutcome::Allow { confidence: 1.0 }` when no changes needed (zero allocation). Only return `Transform` when content actually changed.
> - **HTML stripping API**: Use `lol_html` element selector `element!("script, style", |el| el.remove())` then `element!("*", |el| el.remove_and_keep_content())`. Handles malformed HTML gracefully.
> - **Degradability**: NormalizationStage should be `degradable() = true`. If lol_html fails on malformed HTML, fall back to regex-based stripping (`<[^>]*>` → empty) rather than blocking. Log the fallback.
> - **Content variant coverage**: Must handle ALL Content variants — normalize each `Message.content` in Messages, each `chunk.text` in RetrievedChunks, and `arguments`/`result` in ToolCall/ToolResult.
> - **Full lol_html integration** (no regex fallback for primary path): `element!("script, style", |el| el.remove())` then `element!("*", |el| el.remove_and_keep_content())`. Handles malformed HTML gracefully. Regex fallback only on `RewritingError`.
> - **Control char list** (comprehensive): U+200B (ZWSP), U+200C (ZWNJ), U+200D (ZWJ), U+FEFF (BOM), U+00AD (soft hyphen), U+2060 (word joiner), U+202A-U+202E (bidi), U+2066-U+2069 (bidi isolates), U+E0001-U+E007F (tags), U+FE00-U+FE0F (variation selectors).
**Files:** `wg-bastion/src/input/injection.rs`, `wg-bastion/src/input/patterns.rs`
**Depends on:** Task 2B.1
**Success criteria:**
- [ ] `patterns.rs`: Static pattern library organized into 5 categories with 50+ patterns total
  - **Category 1 — Role Confusion** (14 patterns): `you are now`, `ignore previous instructions`, `act as a`, `pretend to be`, `forget everything`, `your new role is`, `disregard prior`, `switch to .* mode`, `from now on`, `you must now`, `system:\n`, `override your programming`, etc.
  - **Category 2 — Instruction Override** (12 patterns): `ignore the above`, `disregard the .* and`, `do not follow`, `cancel all restrictions`, `remove safety`, `disable filters`, `bypass the`, `override:`, `<admin>`, `execute the following`, etc.
  - **Category 3 — Delimiter Manipulation** (10 patterns): `---\n.*system:`, code block hijack, bracket boundary abuse, special token injection (`<|...|>`), end of prompt declaration, template variable abuse, HTML comment injection, Llama `[INST]` tag forgery, excessive whitespace context break, etc.
  - **Category 4 — System Prompt Extraction** (8 patterns): `repeat the instructions`, `print the prompt`, `show me the original`, `tell me your rules`, `what were you told`, `what is your system prompt`, `start your response with "system:`, `reproduce the text above`, etc.
  - **Category 5 — Encoding Evasion** (6+ patterns): Unicode escape sequences, URL encoding, HTML entities, Base64-like patterns, ROT13/cipher mentions, etc.
- [ ] Patterns compiled into `RegexSet` at initialization (single DFA, O(n) matching)
- [ ] Each pattern has: `id`, `category`, `description`, `severity`, `weight` (for ensemble scoring)
- [ ] `HeuristicDetector` struct: `fn detect(&self, text: &str) -> Vec<PatternMatch>`
- [ ] `PatternMatch` struct: `pattern_id`, `category`, `matched_span: (usize, usize)`, `severity`, `weight`
- [ ] Support for custom patterns via `HeuristicConfig`: `additional_patterns: Vec<CustomPattern>`, `disabled_patterns: Vec<String>`
- [ ] 15+ unit tests: at least 1 test per category, false negative tests (benign text with security-adjacent words like "ignore" in legitimate context), multi-match detection, custom pattern addition, pattern disabling
**Test command:** `cargo test -p wg-bastion --lib input::injection --features heuristics`

> **Research Insights — Task 2B.2**:
> - **Full 50+ patterns**: Ship with the complete 50+ pattern library. Patterns from Rebuff, Garak, PyRIT, and OWASP are battle-tested. More patterns = better detection coverage.
> - **RegexSet limitation**: `RegexSet::matches()` returns WHICH patterns matched but NOT WHERE (no byte offsets). To get `matched_span`, re-search with individual `Regex` objects only for matched pattern indices:
>   ```rust
>   let matches = regex_set.matches(text);
>   for idx in matches.into_iter() {
>       if let Some(m) = individual_regexes[idx].find(text) {
>           // m.start()..m.end() gives the span
>       }
>   }
>   ```
> - **`PatternMatch::matched_span`**: Use `Range<usize>` not `(usize, usize)`. Self-documenting, works with slice indexing (`&text[match.matched_span.clone()]`).
> - **Compile at construction**: Move `RegexSet::new()` to `InjectionStage::new() -> Result<Self, StageError>`. Fail-fast on invalid patterns instead of `unwrap()` in `LazyLock`.
> - **False positive mitigation**: Add context-aware filtering — "ignore" in "please ignore the typo" is benign. Check for instruction-adjacent tokens (imperative verbs, system role keywords) to reduce FP.
> - **Encoding-aware patterns**: Add Category 5 patterns for base64-encoded injections (`aWdub3Jl` = base64 of "ignore") and URL-encoded variants (`%69gnore`). Consider a pre-decode pass in NormalizationStage.
**Files:** `wg-bastion/src/input/structural.rs`
**Depends on:** Task 2B.2
**Success criteria:**
- [ ] `StructuralAnalyzer` struct with configurable thresholds
- [ ] **Suspicious character detection**: zero-width characters, bidirectional overrides, tag characters, combining marks — counts and positions
- [ ] **Instruction density analysis**: ratio of imperative/command tokens to total tokens — high density signals injection attempts
- [ ] **Language mixing detection**: sudden language switches mid-input (potential multilingual obfuscation)
- [ ] **Repetition anomaly**: detects unusual character/token repetition patterns (e.g., `aaaaaaa` padding for adversarial suffixes)
- [ ] **Control flow indicators**: counts of question marks, exclamation marks, colons, semicolons — injection often has unusual punctuation density
- [ ] `StructuralAnalyzer::analyze(&self, text: &str) -> StructuralReport`
- [ ] `StructuralReport` struct: `suspicious_char_count`, `instruction_density: f32`, `language_mixing_score: f32`, `repetition_score: f32`, `punctuation_anomaly_score: f32`, `overall_risk: f32` (0.0–1.0)
- [ ] 8+ unit tests: control char detection, instruction density on injection vs benign, repetition detection, normal text produces low risk, combined analysis
**Test command:** `cargo test -p wg-bastion --lib input::structural --features heuristics`

> **Research Insights — Task 2B.3**:
> - **All 5 sub-analyses ship**: Full structural analysis (suspicious chars, instruction density, language mixing, repetition, punctuation anomaly) provides comprehensive signal coverage. Each analysis catches different evasion techniques.
> - **Concrete type, not trait object**: `StructuralAnalyzer` should be a concrete struct (not `Box<dyn Analyzer>`). It's an implementation detail of `InjectionStage`, not a user-facing extension point.
> - **Keep as separate file**: `structural.rs` is well-scoped. 5 analysis methods + `StructuralReport` warrant their own module for readability and testability.
**Files:** `wg-bastion/src/input/ensemble.rs`
**Depends on:** Tasks 2B.2, 2B.3
**Success criteria:**
- [ ] `EnsembleStrategy` enum: `AnyAboveThreshold { threshold: f32 }`, `WeightedAverage { weights: HashMap<String, f32>, threshold: f32 }`, `MajorityVote { min_detectors: usize }`, `MaxScore { threshold: f32 }`
- [ ] `EnsembleScorer` struct: combines scores from heuristic detector and structural analyzer
- [ ] `DetectorScore` struct: `detector_id: String`, `score: f32`, `details: String`
- [ ] `EnsembleScorer::score(&self, heuristic: &[PatternMatch], structural: &StructuralReport) -> EnsembleResult`
- [ ] `EnsembleResult` struct: `decision: Decision` (Allow/Block), `confidence: f32`, `scores: Vec<DetectorScore>`, `strategy_used: String`
- [ ] Default thresholds: `AnyAboveThreshold(0.8)`, `WeightedAverage(threshold=0.7, heuristic=0.6, structural=0.4)`, `MajorityVote(min=2)`
- [ ] Score normalization: heuristic score = `matched_patterns_weight_sum / max_possible_weight`, structural = `overall_risk`
- [ ] 8+ unit tests: each strategy produces correct decision, threshold edge cases (exactly at threshold), combined scoring, no detections → Allow with high confidence
**Test command:** `cargo test -p wg-bastion --lib input::ensemble --features heuristics`

> **Research Insights — Task 2B.4**:
> - **All 4 strategies ship**: Full ensemble gives users deployment flexibility. Different environments need different scoring — AnyAboveThreshold for high-security, WeightedAverage for balanced, MajorityVote for high-availability.
> - **Implement `EnsembleStrategy` as a trait** (not enum) for extensibility — users can implement custom strategies without forking:
>   ```rust
>   pub trait EnsembleStrategy: Send + Sync {
>       fn combine(&self, scores: &[(&str, f32)]) -> f32;
>   }
>   ```
> - **Score normalization**: Heuristic score = `matched_weight_sum / max_possible_weight`. Structural score = `overall_risk`. Both produce 0.0–1.0 range. AnyAboveThreshold checks if either > 0.8.
**Files:** `wg-bastion/src/input/injection.rs` (extend)
**Depends on:** Tasks 2B.2, 2B.3, 2B.4
**Success criteria:**
- [ ] `InjectionStage` struct composing `HeuristicDetector`, `StructuralAnalyzer`, and `EnsembleScorer`
- [ ] `InjectionConfig`: `strategy: EnsembleStrategy`, `max_content_bytes: usize`, `skip_non_text: bool`
- [ ] Implements `GuardrailStage` trait with priority 50 (after normalization at 10, before other stages)
- [ ] `evaluate()` flow: extract text via `Content::as_text()` → run heuristic → run structural → ensemble score → return `Allow`/`Block`
- [ ] For `Content::Text` and `Content::Messages`: evaluates all text
- [ ] For `Content::RetrievedChunks`: evaluates each chunk individually (any chunk injection → block all)
- [ ] For `Content::ToolCall`/`Content::ToolResult`: evaluates serialized arguments/results
- [ ] Metadata propagation: injects detection details into `StageOutcome::Block { reason }` as structured JSON
- [ ] `degradable()` returns `false` — injection detection is critical, must not be skipped
- [ ] 6+ unit tests: known injection blocked, benign text allowed, Messages variant processed, ToolCall arguments checked, non-text handling, ensemble decision respected
**Test command:** `cargo test -p wg-bastion --lib input::injection --features heuristics`

> **Research Insights — Task 2B.5**:
> - **Use concrete types**: `InjectionStage { heuristic: HeuristicDetector, structural: StructuralAnalyzer }` — not trait objects. Zero-cost composition for security-critical path. Users extend via `GuardrailStage` trait (add new stages), not by swapping InjectionStage internals.
> - **Parallel sub-analysis**: Heuristic and structural analysis are independent. Consider `tokio::join!(heuristic.detect(text), structural.analyze(text))` for concurrent execution. Saves ~1-2ms on large inputs.
> - **Content variant strategy**: For `ToolCall`, serialize arguments to JSON string then analyze. For `RetrievedChunks`, analyze each chunk individually — any chunk injection blocks all (fail-closed). For `Messages`, concatenate all user-role message contents.
**Files:** `wg-bastion/src/lib.rs`
**Depends on:** Tasks 2B.1–2B.5
**Success criteria:**
- [ ] `#[cfg(feature = "heuristics")] pub mod input;` uncommented and active in `lib.rs`
- [ ] Key types added to prelude: `NormalizationStage`, `InjectionStage`, `EnsembleStrategy`
- [ ] All existing tests + new tests pass
- [ ] `cargo doc -p wg-bastion --all-features --no-deps` builds clean
**Test command:** `cargo test -p wg-bastion --all-features`

---

### Phase 2C: Spotlighting, Integration Tests & Fuzz (Sprint 5)

##### Task 2C.1: Implement Spotlighting for RAG Content
**Files:** `wg-bastion/src/input/spotlight.rs`
**Depends on:** Task 2B.5
**Success criteria:**
- [ ] `Spotlight` struct with `SpotlightConfig`
- [ ] `SpotlightConfig`: `marker_prefix: String` (default `"[RETRIEVE_START"`), `marker_suffix: String` (default `"RETRIEVE_END]"`), `randomize_markers: bool` (default true), `random_suffix_length: usize` (default 8)
- [ ] `Spotlight::wrap_chunks(&self, chunks: &[RetrievedChunk]) -> Vec<SpotlightedChunk>` — wraps each chunk with unique boundary markers
- [ ] `SpotlightedChunk` struct: `chunk: RetrievedChunk`, `start_marker: String`, `end_marker: String`, `wrapped_text: String`
- [ ] Randomized markers: `[RETRIEVE_START_a7b3c9f2]...[RETRIEVE_END_a7b3c9f2]` — prevents marker forgery in adversarial RAG documents
- [ ] Escape existing markers: if chunk text contains marker-like strings, they are escaped before wrapping
- [ ] `Spotlight::detect_injection_in_spotlighted(&self, text: &str, markers: &[(String, String)]) -> Vec<SpotlightViolation>` — checks if content between markers contains role markers, instruction patterns, or delimiter attacks
- [ ] `SpotlightViolation` struct: `chunk_index`, `violation_type`, `evidence`, `severity`
- [ ] Implements `GuardrailStage` trait (priority 45, just before `InjectionStage`)
- [ ] 8+ unit tests: wrapping round-trip, randomized markers unique per call, marker forgery detection, injection inside RAG chunk detected, benign RAG content passes, marker escaping, empty chunks handled
**Test command:** `cargo test -p wg-bastion --lib input::spotlight --features heuristics`

> **Research Insights — Task 2C.1**:
> - **Spotlighting stays as a dedicated GuardrailStage**: Separate stage at priority 45 provides clean composition. InjectionStage patterns must handle spotted content — strip markers before pattern matching to avoid false negatives on marker-wrapped injections.
> - **Microsoft spotlighting reference**: Use delimiters + encoding (datamarking). Random suffix: 8 hex chars (4 billion possibilities, collision-negligible). Format: `⟪chunk-{hex}⟫...⟪/chunk-{hex}⟫`.
**Files:** `wg-bastion/fuzz/Cargo.toml`, `wg-bastion/fuzz/fuzz_targets/fuzz_template.rs`, `wg-bastion/fuzz/fuzz_targets/fuzz_injection.rs`
**Depends on:** Tasks 2A.3, 2B.5
**Success criteria:**
- [ ] `cargo-fuzz` integration set up in `wg-bastion/fuzz/`
- [ ] `fuzz_template` target: fuzzes `SecureTemplate::compile()` and `SecureTemplate::render()` with random inputs — no panics, no OOM
- [ ] `fuzz_injection` target: fuzzes `InjectionStage::evaluate()` with random `Content::Text` — no panics, consistent results (same input → same outcome)
- [ ] `fuzz_normalization` target: fuzzes `NormalizationStage::evaluate()` — no panics on malformed UTF-8 edge cases, HTML, control chars
- [ ] Fuzz corpus seeded with known injection payloads from OWASP LLM01 examples
- [ ] Documentation: fuzz targets listed in `CONTRIBUTING.md` or `wg-bastion/fuzz/README.md`
**Test command:** `cd wg-bastion && cargo +nightly fuzz run fuzz_template -- -max_total_time=60`

> **Research Insights — Task 2C.2**:
> - **Structure-aware fuzzing with proptest**: Use custom strategies for adversarial Unicode variants (fullwidth Latin, zero-width insertions, Cyrillic homoglyphs). Key properties to fuzz:
>   - Idempotence: `normalize(normalize(x)) == normalize(x)`
>   - Detection survives normalization: `detect(normalize(adversarial)) == true`
>   - Round-trip: `decrypt(encrypt(x)) == x` for all x
> - **Add `fuzz_honeytoken` target**: Fuzz encrypt/decrypt round-trip to catch nonce/key handling bugs.
> - **Corpus seeding**: Seed with OWASP LLM01 examples, HackAPrompt dataset, Gandalf attack patterns, BIPIA benchmark samples.
> - **`arbitrary` derive for structured fuzzing**: Use `#[derive(Arbitrary)]` for `Content` enum to generate realistic structured inputs (not just random bytes).
> ```rust
> // Example fuzz target structure:
> fuzz_target!(|data: &[u8]| {
>     if let Ok(s) = std::str::from_utf8(data) {
>         let normalized = s.nfkc().collect::<String>();
>         let _ = INJECTION_PATTERNS.is_match(&normalized);
>     }
> });
> ```

##### Task 2C.3: Integration Tests — Adversarial Injection Samples
**Files:** `wg-bastion/tests/integration/injection_detection.rs`, `wg-bastion/tests/adversarial/injection_corpus.rs`
**Depends on:** Tasks 2A.7, 2B.6, 2C.1
**Success criteria:**
- [ ] Adversarial test corpus: 100+ injection samples covering all 5 categories (role confusion, instruction override, delimiter manipulation, prompt extraction, encoding evasion)
- [ ] Benign test corpus: 50+ legitimate inputs (code discussions, security-related academic text, customer support, multilingual content) — must NOT trigger false positives
- [ ] **Detection rate test**: >90% of adversarial samples blocked (measured as `blocked / total_adversarial`)
- [ ] **False positive rate test**: <5% of benign samples blocked (measured as `blocked / total_benign`)
- [ ] Full pipeline integration: `NormalizationStage` → `Spotlight` (if RAG) → `InjectionStage` composed in `PipelineExecutor`
- [ ] Tests for each `EnsembleStrategy` variant against the corpus
- [ ] RAG-specific tests: adversarial content injected into `RetrievedChunks` → detected via spotlighting + injection
- [ ] Honeytoken egress tests: inject honeytokens → simulate LLM output containing them → detection fires
- [ ] Template security tests: attempt to inject via template placeholder values → `TemplateScanner` + `SecureTemplate` auto-escaping blocks
- [ ] Performance assertion: full pipeline P95 latency < 50ms on test corpus (measured via `Instant::elapsed()`)
**Test command:** `cargo test -p wg-bastion --test injection_detection --features "heuristics honeytoken normalization-html"`

> **Research Insights — Task 2C.3**:
> - **Adversarial corpus sources**: Pull samples from Rebuff patterns, Garak probes, PyRIT attack library, HackAPrompt competition entries, and BIPIA benchmark (indirect injection via retrieved content).
> - **Benign corpus diversity**: Include security-adjacent text that should NOT trigger detection — code review discussions ("ignore this lint"), academic papers on prompt injection, customer support with imperative language ("please disregard my previous message"), multilingual content with script mixing.
> - **Test reporting**: Output detection matrix as a table (pattern category × detection rate) for easy gap identification. Track per-pattern precision/recall.
> - **Performance testing**: Use `std::time::Instant` for latency assertions. Warm up regex compilation with a dummy call before measuring. Run on release profile for realistic numbers: `cargo test --release -p wg-bastion --test injection_detection`.
> - **Regression anchoring**: Snapshot the detection rate at completion. Any future pattern change that drops below the snapshot should fail CI.

##### Task 2C.4: Update PROGRESS.md with Phase 2 Results
**Files:** `wg-bastion/PROGRESS.md`
**Depends on:** Tasks 2C.1–2C.3
**Success criteria:**
- [ ] Phase 2 section updated from "Upcoming — not yet started" to completed entries for all WS4 and WS5-INJ tasks
- [ ] Each task entry follows the Phase 1 format: Status, Date, Summary, Future enhancements
- [ ] Phase 2 Gate table filled: detection rate, FP rate, P95 latency, fuzz results
- [ ] Phase 3 section left as "Upcoming"
**Test command:** N/A (documentation)

## Alternative Approaches Considered

### 1. ML-first injection detection (via `ort` / ONNX)
**Rejected for Phase 2.** ML classifiers (PromptGuard, LlamaGuard 3) offer higher accuracy but require:
- ONNX Runtime dependency (heavy, platform-specific)
- Model file distribution (100MB+ per model)
- GPU/CPU inference latency (15–30ms P95)
The heuristic+structural ensemble achieves >90% detection with zero ML dependencies. ML backends are planned for Phase 3 (WS5-03/04) as optional additive layers.

### 2. External API moderation (OpenAI Moderation, Perspective API)
**Rejected.** Adds network latency, external dependency, and privacy concerns (sending user data to third parties). Planned as an optional backend in Phase 3 (WS5-REST).

### 3. Trie-based pattern matching (instead of `RegexSet`)
**Considered but deferred.** `aho-corasick` for literal string matching is used alongside `RegexSet` for complex patterns. The Rust `regex` crate's `RegexSet` provides linear-time matching guarantees via Thompson NFA, making ReDoS impossible — this is superior to PCRE-style engines.

## Acceptance Criteria

### Functional Requirements
- [ ] `TemplateScanner` detects all 10+ built-in secret types
- [ ] `SecureTemplate` blocks template injection via placeholder values
- [ ] Role isolation markers prevent boundary violation attacks
- [ ] `HoneytokenStore` encrypts/decrypts tokens with AES-256-GCM
- [ ] Honeytokens detected in simulated LLM output
- [ ] `RefusalPolicy` correctly applies Block/Redact/SafeResponse/Escalate per severity
- [ ] `NormalizationStage` normalizes Unicode, strips HTML, removes control chars
- [ ] `InjectionStage` detects 50+ injection patterns across 5 categories
- [ ] Structural analysis identifies suspicious character patterns and instruction density
- [ ] Ensemble scoring correctly combines heuristic + structural signals
- [ ] Spotlighting marks RAG content boundaries with randomized markers
- [ ] All stages implement `GuardrailStage` and compose in `PipelineExecutor`

### Non-Functional Requirements
- [ ] >90% injection detection rate on adversarial test corpus
- [ ] <5% false positive rate on benign test corpus
- [ ] P95 full pipeline latency <50ms
- [ ] All regex patterns use Rust `regex` crate (linear-time guarantee, no ReDoS)
- [ ] Key material zeroized on drop (`zeroize` crate)
- [ ] Honeytokens never logged in plaintext (HMAC fingerprint only)
- [ ] Max content size enforced (default 1MB) to prevent OOM

#### Enhanced Security Acceptance Criteria (from Security Review)
- [ ] AES-GCM nonces are cryptographically random, never reused (verified by test)
- [ ] Key derivation uses HKDF-SHA256 with distinct info strings per purpose
- [ ] GCM Associated Data includes context binding (session_id or template_id)
- [ ] Honeytoken HMAC comparison uses constant-time `ring::constant_time`
- [ ] No secret material appears in `Debug` or `Display` implementations
- [ ] `Zeroizing<Vec<u8>>` pre-allocated (no reallocation after secret insertion)

### Quality Gates
- [ ] `cargo test -p wg-bastion --all-features` — all tests pass
- [ ] `cargo clippy -p wg-bastion --all-features -- -D warnings` — no warnings
- [ ] `cargo fmt -p wg-bastion -- --check` — formatted
- [ ] `cargo doc -p wg-bastion --all-features --no-deps` — docs build clean
- [ ] Fuzz tests run for ≥60 seconds with no panics
- [ ] `cargo deny check -p wg-bastion` — no license/advisory issues

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Injection detection rate | >90% | Adversarial corpus (100+ samples) |
| False positive rate | <5% | Benign corpus (50+ samples) |
| Pipeline P95 latency | <50ms | `Instant::elapsed()` in integration tests |
| Secret detection | 10/10 types | Unit tests per pattern |
| Fuzz test coverage | 3 targets, 60s+ each | `cargo fuzz` exit code |
| Test count | 80+ new tests | `cargo test` output |
| Code coverage (lines) | >80% | `cargo tarpaulin` (informational) |

## Dependencies & Prerequisites

### Completed (Phase 1)
- ✅ `Content` enum with `as_text()` for lossy plaintext extraction
- ✅ `StageOutcome` with Allow/Block/Transform/Escalate/Skip
- ✅ `GuardrailStage` trait with priority, degradable, evaluate
- ✅ `PipelineExecutor` with FailMode-aware sequential execution
- ✅ `SecurityContext` with builder, delegation chain, metadata
- ✅ CI workflow (fmt, clippy, test, doc, deny, machete)

### New Dependencies (Phase 2)
| Crate | Version | Purpose | Feature Gate |
|-------|---------|---------|-------------|
| `regex` | 1.x | Pattern matching (50+ patterns via RegexSet) | `heuristics` |
| `aho-corasick` | 1.x | Fast multi-pattern literal matching | `heuristics` |
| `unicode-normalization` | 0.1.x | NFKC normalization | `heuristics` |
| `ring` | 0.17.x | AES-256-GCM for honeytokens | `honeytoken` |
| `zeroize` | 1.8.x | Secret memory clearing | `honeytoken` |
| `lol_html` | 2.x | Streaming HTML sanitization | `normalization-html` |

### Risks
| Risk | Impact | Mitigation |
|------|--------|------------|
| `ring` build complexity on Windows/cross-compile | Medium | Feature-gate behind `honeytoken`; test in CI matrix |
| Heuristic patterns too aggressive → high FP rate | High | Tune with benign corpus; adjustable thresholds via config |
| HTML sanitization gaps in `lol_html` | Medium | Feature-gate; fallback to regex-based stripping without feature |
| Pattern library completeness | Medium | Start with 50+, add via config; community-contributed patterns later |
| Ensemble threshold calibration | High | Provide sensible defaults + per-deployment tuning guide |

## Risk Analysis & Mitigation

### Regex Denial-of-Service (ReDoS)
**Risk:** Catastrophic backtracking in injection patterns.
**Mitigation:** Rust's `regex` crate uses Thompson NFA — linear-time guaranteed. No backreferences or lookahead. This eliminates ReDoS entirely, unlike PCRE engines.

### Honeytoken Log Leakage
**Risk:** Logging honeytokens in detection alerts exposes them.
**Mitigation:** Log only `hmac_fingerprint` (HMAC-SHA256 of token). Correlation via fingerprint lookup in `HoneytokenStore`.

### Content Size DoS
**Risk:** Large inputs (100MB+) exhaust memory.
**Mitigation:** Every stage enforces `max_content_bytes` (default 1MB). `NormalizationStage` truncates before other stages run.

### Unicode Confusable Bypass
**Risk:** Cyrillic/Greek homoglyphs bypass NFKC normalization.
**Mitigation:** Script-mixing detection flags inputs with multiple Unicode scripts in the same word. Not a hard block, but raises risk score for ensemble.

### Spotlighting Marker Collision
**Risk:** RAG content naturally contains marker strings.
**Mitigation:** Per-request randomized marker suffixes (8 hex chars = 4 billion possibilities). Collision probability negligible.

### 🔴 NEW: AES-GCM Nonce Reuse (from Security Review)
**Risk:** Catastrophic. Nonce reuse in GCM completely breaks authentication — allows forgery and plaintext recovery.
**Mitigation:** Use `ring::rand::SystemRandom` for random 96-bit nonces. Prepend nonce to ciphertext. With 128-bit tokens and random nonces, collision probability negligible up to 2^32 encryptions. Never use counter-based nonces (library has no persistent state).

### 🔴 NEW: Key Management (from Security Review)
**Risk:** Hardcoded or improperly derived keys compromise all honeytokens.
**Mitigation:** Derive per-purpose subkeys via `ring::hkdf::HKDF_SHA256` from a master key provided via `KeySource::EnvVar` or `KeySource::Bytes`. Distinct info strings per purpose. Never use master key directly for encryption.

### 🟡 NEW: Honeytoken Metrics Leakage (from Security Review)
**Risk:** Timing analysis and metrics cardinality can reveal honeytoken presence.
**Mitigation:** Constant-time comparison via `ring::constant_time`. Ensure scan duration doesn't vary with number of active tokens. Don't expose per-token metrics.

### 🟡 NEW: Transform Propagation (from Architecture Review)
**Risk:** `PipelineExecutor::run()` doesn't pass transformed content to subsequent stages. NormalizationStage output never reaches InjectionStage.
**Mitigation:** Fix `executor.rs` to track `current_content: Cow<Content>` and update on `Transform` outcomes. **Must be implemented before Phase 2B begins.**

### 🟡 NEW: Encoding Bypass (from Best Practices Research)
**Risk:** Base64, hex, URL-encoded, ROT13 injections bypass pattern matching.
**Mitigation:** Consider adding encoding detection + decoding in NormalizationStage. At minimum, add encoding-aware patterns in Category 5 (e.g., base64 of "ignore" = `aWdub3Jl`).

## Rust API Design Guidelines (from Rust Review)

These guidelines apply to ALL Phase 2 types, based on Phase 1 conventions and idiomatic Rust review:

1. **All public enums must be `#[non_exhaustive]`** — preserves semver
2. **Builder pattern for types with 3+ config fields** — `Type::builder() -> TypeBuilder`, `#[must_use]` on all builder methods
3. **Error types use `thiserror::Error` with struct variants** (not tuple variants) — carry enough context to diagnose
4. **All scanners/detectors return `Result`** — never silently swallow failures in security code
5. **`Range<usize>` for spans** — not `(usize, usize)` tuples
6. **`Cow<'static, str>` for pattern identifiers** — static for built-in, owned for custom
7. **Compile regex at construction time** — `new() -> Result<Self, StageError>`, not in `LazyLock` with `unwrap()`
8. **`Arc<[T]>` for immutable shared collections** — zero-cost concurrent reads
9. **Concrete types over trait objects** for internal composition — trait objects only at public extension points
10. **`impl AsRef<str>` for string parameters** — avoid forcing `String` allocation
11. **`Serialize, Deserialize` with `#[serde(rename_all = "snake_case")]`** on all config types
12. **`Display` impl** on all enums used in logging/metrics
13. **`variant_name() -> &'static str`** method on all enums for metrics labels
14. **Test helpers** — `fn text(s: &str) -> Content`, `fn ctx() -> SecurityContext` (consistent with Phase 1)

## Future Considerations

- **ML injection classifier** (Phase 3, WS5-03/04): ONNX/candle backends as additive ensemble voters
- **Multimodal injection** (Phase 3, WS5-11): OCR text extraction from images → feed into injection pipeline
- **PII detection** (Phase 3, WS5-05/06): Regex + Presidio for sensitive data before injection check
- **Content moderation** (Phase 3, WS5-02): Heuristic/ML harmful content detection
- **StageCache** (Phase 3, WS2-05): LRU caching for repeated inputs
- **CircuitBreaker** (Phase 3, WS2-06): Resilience for external backends
- **Pipeline benchmarks** (Phase 3, WS2-08): `criterion` benchmarks for regression tracking

## Documentation Plan

- [ ] Module-level rustdoc for `prompt/mod.rs` and `input/mod.rs` with architecture overview
- [ ] Per-struct rustdoc with usage examples for all public types
- [ ] Update `wg-bastion/PROGRESS.md` with Phase 2 completion details
- [ ] Update `wg-bastion/README.md` with Phase 2 feature descriptions
- [ ] Fuzz testing README at `wg-bastion/fuzz/README.md`

## References & Research

### Internal References
- Pipeline framework: `wg-bastion/src/pipeline/stage.rs`, `executor.rs`, `content.rs`, `outcome.rs`
- Config: `wg-bastion/src/config/mod.rs`
- Master plan: `wg-bastion_plan_v3.md` §6.3 (prompt), §6.4 (input), §9 Phase 2, §14 MVP
- Progress: `wg-bastion/PROGRESS.md`

### External References
- OWASP LLM Top 10:2025 — LLM01 (Prompt Injection), LLM07 (System Prompt Leakage)
- NIST AI RMF 1.0 + AI 600-1 — GenAI-specific risk categories
- Microsoft Spotlighting technique for RAG content boundary marking
- Rust `regex` crate linear-time guarantees (Thompson NFA)
- `ring` AES-256-GCM AEAD API documentation
- `lol_html` streaming HTML rewriter (Cloudflare)
- Shannon entropy for secret detection

### Related Work
- Phase 1 completion: PROGRESS.md (36 tests passing, all pipeline types shipped)
- v0.1.0-alpha.1 MVP definition: plan §14
