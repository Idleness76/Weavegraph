# `wg-security` Standalone Crate Master Plan

`wg-security` will become a sibling crate to `weavegraph` and `wg-ragsmith`, delivering a comprehensive, opt-in security suite for graph-driven LLM applications. This plan expands every component with threat context, framework alignment, and implementation detail so two senior engineers working part-time can execute predictably.

---

## 0. Security Framework Anchors

### OWASP LLM Top 10 (2023)
- Community-driven catalog of the ten most critical risks for LLM applications (prompt injection, insecure output handling, model denial-of-service, excessive agency, data leakage, supply chain weaknesses, etc.).
- Provides concrete control recommendations and testing playbooks per risk.
- We map every module/task to one or more OWASP risks; this ensures coverage against known industry incidents (e.g., LangChain tool hijacking, retriever poisoning).

### NIST AI Risk Management Framework – Generative AI Profile (AI 600-1, draft 2023)
- Defines governance outcomes across the AI lifecycle: Govern, Map, Measure, Manage.
- Emphasizes documentation, transparency, monitoring, and continuous improvement.
- We adopt AI RMF outcomes as acceptance criteria for observability, incident response, and red-team activities.

### NIST Secure Software Development Framework (SSDF) SP 800-218A
- General secure SDLC guidance (prepare, protect, produce, respond) updated for AI contexts.
- Supplies requirements for dependency hygiene, release processes, logging, and vulnerability response.
- Guides WS11 (supply chain) and the PR checklist to ensure the security crate itself is produced securely.

### Mapping Approach
- For each workstream/task we capture “Threat Coverage” referencing OWASP LLM-XX IDs, AI RMF outcomes, and SSDF practices.
- A control matrix (WS1-02 deliverable) keeps traceability current as the implementation evolves.

---

## 1. Threat Landscape & Actors

| Threat Actor | Profile | Motivations | Primary Attack Surfaces | Mitigations (modules) |
|--------------|---------|-------------|--------------------------|------------------------|
| **Malicious end-users** | External users interacting with the agent UI or API. | Data exfiltration, bypass guardrails, disruptive prompts, abuse of tools. | User prompts, file uploads, tool requests, response rendering. | `prompt`, `input`, `output`, `abuse`, `telemetry`. |
| **Compromised retrieval sources** | Poisoned web pages or documents ingested into RAG corpora, potentially inserted by adversaries. | Indirect prompt injection, misinformation, secret harvesting. | Ingestion pipeline, retrieval results, context assembly. | `input.normalization`, `rag.ingestion`, `rag.provenance`, `rag.grounding`. |
| **Confused deputy via tools/MCP** | Third-party services or compromised agents reusing credentials. | Execute unintended actions, pivot to internal networks. | MCP tool calls, API tokens, session management. | `tools.policy`, `tools.mcp`, `abuse.rate_limiting`, `telemetry.incident`. |
| **Insiders / rogue integrators** | Developers or operators misconfiguring templates, exposing secrets, disabling controls. | Shortcut security measures, leak system prompts, misuse honeytokens. | Templates, config overrides, logs, release process. | `config`, `prompt.scanners`, `telemetry.log_retention`, `supply_chain`. |
| **Automated scaling adversaries** | Bots creating high-volume requests to exhaust budget or induce DoS. | Resource exhaustion, cost abuse. | Prompt API, tool execution, background jobs. | `abuse.rate_limiting`, `abuse.cost`, `abuse.circuit_breakers`. |
| **Supply chain attackers** | Actors attempting to inject malicious dependencies, tamper with releases. | Compromise dev pipeline, insert backdoors. | Crate dependencies, release artifacts, auxiliary Docker services. | `supply_chain`, WS11 tasks, signed releases. |

Threat modeling (WS1-01) elaborates on assets, trust boundaries, attack trees, and maps each actor to mitigations, detection, and response playbooks.

---

## 2. Vision, Scope, and Constraints

### 2.1 Goal
Deliver an extensible Rust crate that provides production-grade guardrails for weavegraph applications, minimizing friction and protecting against the dominant LLM threats (OWASP LLM-01…10) while aligning with NIST governance expectations.

### 2.2 Scope
- Prompt/system template enforcement (role isolation, canaries, refusal policy).
- Input gating pipelines (moderation, PII, injection checks, retrieval normalization).
- Output validation (schema enforcement, HTML/terminal sanitization, secret scanning).
- Tool/MCP policy enforcement (allowlists, scope minimization, honeytokens).
- RAG hygiene (ingestion sanitization, provenance, grounded answering rails).
- Abuse controls (rate limiting, recursion guards, cost alerts).
- Observability, telemetry, automated incident response, audit trail retention.
- Testing/red-teaming harnesses, adversarial corpora, continuous validation.
- Supply-chain hygiene, signed releases, documentation, and developer tooling.

### 2.3 Non-Goals
- End-user authentication/authorization or billing enforcement.
- Proprietary vendor integrations requiring licenses beyond OSS/free tiers.
- Guaranteeing compliance for specific regulated industries without customization (we provide hooks).

### 2.4 Constraints
- **Technical**: Rust 1.89/1.91 MSRV, optional GPU acceleration (must run on CPU-only), keep dependency graph optional for base `weavegraph`.
- **Operational**: degrade gracefully when external services (Presidio, TruffleHog, remote classifiers) unavailable, with deterministic logging and policy fallback.
- **Performance**: maintain <50ms added latency per prompt in default configuration; provide tuning/benchmark tools.
- **Security**: treat crate as security-sensitive; require code reviews, fuzzing, and signed releases per SSDF.

---

## 3. Architecture & Project Layout

```
graft/
├── Cargo.toml (workspace root)
├── weavegraph/              (orchestration runtime)
├── wg-ragsmith/             (RAG utilities)
├── wg-security/             (this crate)
│   ├── Cargo.toml
│   ├── README.md
│   ├── src/
│   │   ├── lib.rs           (crate entry; feature gates, exports)
│   │   ├── config/          (policy schema + loaders)
│   │   ├── policy/          (pipeline executor + cache)
│   │   ├── prompt/          (templates, canaries, scanners)
│   │   ├── input/           (moderation, PII, injection, normalization)
│   │   ├── output/          (validation, sanitization, scanning)
│   │   ├── tools/           (tool/MCP governance)
│   │   ├── rag/             (retrieval hardening)
│   │   ├── abuse/           (rate limiting & cost controls)
│   │   ├── telemetry/       (events, exporters, incident automation)
│   │   ├── testing/         (attack harness, fixtures)
│   │   ├── supply_chain/    (SBOM, audit utilities)
│   │   └── cli/             (developer CLI commands)
│   ├── examples/            (graph demos with guardrails)
│   ├── benches/             (latency benchmarks)
│   ├── tests/               (integration suites)
│   ├── xtask/               (custom automation CLI)
│   └── docs/                (module-level docs, red-team playbook)
└── docs/
    └── wg-security_plan.md  (strategic plan)
```

Design notes:
- `wg-security` exports minimal traits/stubs to avoid leaking heavy deps upstream; consumers opt-in via feature flags (e.g., `pii-presidio`, `moderation-onnx`, `telemetry-otlp`).
- Example graphs demonstrate typical integration patterns (prompt gating, tool approvals, RAG checks).
- `xtask` provides developer utilities (policy init, canary rotation, running scanners) to encourage consistent operational practices.

---

## 4. Module Deep Dive

### 4.1 `config`
- **Responsibilities**: Define `SecurityPolicy`, layered overrides (global → graph → node → channel), environment-based configuration loading, JSON schema export for validation.
- **Key components**:
  - `SecurityPolicy`: top-level struct with sections (`prompt`, `input`, `output`, `tools`, etc.).
  - `PolicyBuilder`: merges defaults, files, runtime overrides.
  - `PolicyValidator`: ensures selections comply with mandatory safeguards (e.g., cannot completely disable sanitization without audit flag).
- **Threat coverage**: Prevents misconfiguration (OWASP LLM-09 – insecure defaults). Supports AI RMF Govern outcomes by documenting controls.
- **Interactions**: `PolicyHandle` consumed by `policy` executor; CLI can generate example configs.

### 4.2 `policy`
- **Responsibilities**: Execute guardrail stages in deterministic order, combine results, manage cache/timeouts/circuit breakers, expose telemetry hooks.
- **Key components**:
  - `GuardrailStage` trait with async run method returning `StageOutcome`.
  - `PipelineExecutor` orchestrates stage graph (pre-prompt, pre-tool, pre-output, etc.).
  - `StageCache` and `CircuitBreaker` (Tower middleware) to throttle heavy operations.
- **Threat coverage**: Ensures controls actually run and fail closed; mitigates bypass attempts (OWASP LLM-01/02/08).
- **Interactions**: Called by weavegraph runner via `pre_node` and `post_node` hooks; stages register metrics with `telemetry`.

### 4.3 `prompt`
- **Responsibilities**: Harden system templates, enforce role isolation, insert canaries, detect secrets at build-time, implement refusal rewrites.
- **Key components**:
  - `SecureTemplate`: wrapper over templating library with static typing for slots.
  - `HoneytokenStore`: rotates tokens, persists encrypted in local store, logs access.
  - `TemplateScanner`: uses heuristics + entropy detection to flag secrets.
  - `RefusalPolicy`: server-side fallback responses when leakage detected.
- **Threat coverage**: OWASP LLM-01 (prompt injection), LLM-06 (data leakage). Aligns with AI RMF Manage (mitigate) by ensuring policy enforcement outside model.

### 4.4 `input`
- **Responsibilities**: Gating pipeline for user prompts, retrieval chunks, tool suggestions.
- **Key components**:
  - `ModerationStage`: ONNX-powered classifier (Llama Guard 2) with CPU fallback heuristics.
  - `PIIStage`: hybrid approach (regex/dictionary + optional Presidio connector).
  - `InjectionStage`: heuristics (directive detection, suspicious characters), optional classifier (Prompt Shields), spotlighting for RAG chunks.
  - `NormalizationStage`: streaming HTML → text converter with script stripping, MIME allowlist.
- **Threat coverage**: OWASP LLM-01/02/04/06; AI RMF Measure (monitoring gate results); SSDF requirement for input validation.

### 4.5 `output`
- **Responsibilities**: Validate, sanitize, and scan model outputs before delivery/logging.
- **Key components**:
  - `SchemaValidator`: guardrails DSL (type checking, regex, enum enforcement).
  - `Sanitizer`: wrappers for `ammonia` (HTML) and terminal escapes.
  - `CodeGuard`: policy toggle for blocking code suggestions or requiring sandbox.
  - `EgressScanner`: adapters to TruffleHog/detect-secrets invoked on responses/logs.
  - `Transformers`: safe conversions (Markdown → HTML, JSON patches).
- **Threat coverage**: OWASP LLM-02 (insecure output handling) and LLM-06 (data leakage).

### 4.6 `tools`
- **Responsibilities**: Apply least privilege to tools/MCP, mitigate confused deputy attacks, manage approvals.
- **Key components**:
  - `ToolPolicy`: config of allowlists, scopes, quotas, risk levels.
  - `ExecutionGuard`: wraps MCP/tool invocations to enforce binding to session tokens, idempotency keys.
  - `FetchSanitizer`: sanitize HTTP responses, block script execution, enforce allowlists.
  - `ApprovalFlow`: integrates with human or secondary model approval for high-risk actions; logs state transitions.
- **Threat coverage**: OWASP LLM-08 (excessive agency), LLM-03 (training data poisoning via tool ingestion), LLM-05 (supply chain) when combined with honeytokens.

### 4.7 `rag`
- **Responsibilities**: Secure ingestion, annotate provenance, enforce grounded answering.
- **Key components**:
  - `SanitizedIngestion`: ensures inputs come from allowlisted domains, hashed, signed, stored with metadata.
  - `ProvenanceTagger`: attaches domain hashes, crawl timestamps, injection verdicts.
  - `GroundedRails`: rule engine verifying claims are supported by retrieved facts, invoking cross-encoder scoring if necessary.
  - `CorpusScanner`: CLI to check stored documents for PII/secrets.
- **Threat coverage**: OWASP LLM-01 (indirect prompt injection), LLM-02 (hallucination), LLM-06 (data leakage), LLM-07 (supply chain).

### 4.8 `abuse`
- **Responsibilities**: Prevent DoS and runaway behaviors.
- **Key components**:
  - `RateLimiter`: multi-tenant token buckets (per user, per tool, per session).
  - `RecursionGuard`: track tool hop depth/breadth; enforce max thresholds.
  - `CostMonitor`: compute token usage, external call costs; emit alerts.
  - `CircuitBreaker`: degrade or pause guardrails when dependent service fails.
- **Threat coverage**: OWASP LLM-04 (DoS), LLM-08 (excessive agency), LLM-09 (insecure defaults).

### 4.9 `telemetry`
- **Responsibilities**: Provide observability, event schemas, automated incident response.
- **Key components**:
  - `EventSchema`: extend existing docs schemas with guardrail metadata.
  - `OtelExporter`: OTLP/HTTP exporters, tracing spans, metrics (counts, latency, risk scores).
  - `IncidentOrchestrator`: triggers token revocation/quarantine, notifies on-call (PagerDuty, Slack).
  - `DashboardTemplates`: Grafana JSON for dashboards mapping to AI RMF Manage outcomes.
- **Threat coverage**: AI RMF Measure and Manage (monitor, respond); SSDF respond practices.

### 4.10 `testing`
- **Responsibilities**: Keep guardrails effective via adversarial datasets and automated harnesses.
- **Key components**:
  - `AdversarialCorpus`: curated prompts (Rebuff, AdvPromptSet, custom honeytokens).
  - `AttackHarness`: orchestrates input/output/tool pipelines under malicious scenarios; outputs metrics.
  - `RegressionSuites`: ensures stage ordering, fallback semantics, degrade modes remain intact.
- **Threat coverage**: Ensures continuous assurance (AI RMF Measure; SSDF produce/respond).

### 4.11 `supply_chain`
- **Responsibilities**: Generate SBOM, run dependency audits, verify release integrity.
- **Key components**:
  - `SbomGenerator`: CycloneDX output integrated with CI.
  - `AuditRunner`: wrappers for `cargo audit`, `cargo deny`, license scanning.
- **Threat coverage**: OWASP LLM-10 (supply chain), SSDF produce/respond.

### 4.12 `cli`
- **Responsibilities**: Developer tooling through `cargo xtask security`.
- **Key components**:
  - Commands: `init-policy`, `scan-templates`, `rotate-honeytokens`, `run-attack-suite`, `export-sbom`.
- **Threat coverage**: Reduces operator error (AI RMF Govern).

---

## 5. Workstreams & Detailed Tasks

Each workstream (WS) groups cohesive milestones. For every task we list purpose, deliverable, dependencies, and threat coverage.

### WS1 – Foundations & Governance
- **Context**: Establish governance per AI RMF “Govern” outcome and SSDF “Prepare”. Prevent scope drift and ensure traceability.
- **Tasks**:
  1. **WS1-01 – Threat Model & Attack Tree**
     - Deliverables: `docs/threat_model.md`, diagram (draw.io/mermaid).
     - Includes data flow diagrams, trust boundaries, attacker capability analysis, mapping to OWASP/NIST.
     - Threat coverage: baseline for all; required before coding.
  2. **WS1-02 – Control Matrix**
     - Deliverables: `docs/control_matrix.md` (CSV + narrative).
     - Maps each OWASP LLM risk and NIST control to module/stage/test.
     - Dependency: WS1-01. Ensures auditability.
  3. **WS1-03 – Governance Docs**
     - Deliverables: `wg-security/README.md`, `docs/architecture.md`, PR/security review checklist.
     - Defines coding standards (linting, doc comments, unsafe usage policies).
     - Threat coverage: mitigates insider misconfiguration (OWASP LLM-09).

### WS2 – Crate Scaffolding & Policy Runtime
- **Context**: Provide runnable crate with policy engine to orchestrate future modules.
- **Tasks**:
  1. **WS2-01 – Project Scaffold**
     - Deliverables: new crate, features, baseline CI (fmt, clippy, test).
     - Includes doc comment skeleton, MSRV check, default features minimal.
  2. **WS2-02 – Policy Schema**
     - Implement `SecurityPolicy`, overlay logic, JSON schema export.
     - Output: schema tests, sample config file.
     - Threat coverage: ensures safe defaults (OWASP LLM-09).
  3. **WS2-03 – GuardrailStage Trait & Pipeline Executor**
     - Build asynchronous pipeline engine with typed outcomes (Allow, Block, Transform, Escalate).
     - Provide instrumentation hooks for metrics.
  4. **WS2-04 – Cache & Circuit Breakers**
     - Use `tower` + `dashmap` to memoise stage results, set timeouts/retries, enforce fail-closed.
     - Provide DSL for degrade behavior (e.g., fallback to heuristics).
  5. **WS2-05 – Weavegraph Integration**
     - Add `PolicyHandle` exposed via feature flag to weavegraph; sample showing `pre_prompt`/`post_prompt` hooking.
     - Provide minimal example test.

### WS3 – Prompt Hardening Module
- **Context**: Address OWASP LLM-01/06 early; essential for MVP.
- **Tasks**:
  1. **WS3-01 – Secure Template Engine**
     - Implement typed placeholders, automatic escaping, multi-role separation (system/user/assistant).
     - Fuzz/fuzz-like tests for injection (e.g., `{{ role="system" }}` attempt).
  2. **WS3-02 – Honeytoken Manager**
     - Create encrypted store (AES-GCM via `ring` or `orion`), rotation schedule, detection events.
     - Integrate with telemetry for alerting.
  3. **WS3-03 – Template Secret Scanner**
     - Static analyzer using regex + entropy detection for API keys/URLs.
     - CLI command `cargo xtask security scan-templates`.
  4. **WS3-04 – Refusal Policy**
     - Implement policy to strip/replace disallowed data from outputs if canary triggered; maintain audit logs.
     - Provide fallback message templating.

### WS4 – Input Guard Pipelines
- **Context**: Protect against direct/indirect prompt injection and harmful content (OWASP LLM-01/04/06).
- **Tasks**:
  1. **WS4-01 – Moderation Stage**
     - Integrate Llama Guard 2 ONNX; support CPU fallback (quantized).
     - Provide batching and timeouts; generate evaluation metrics.
  2. **WS4-02 – PII Detection**
     - Implement regex/dictionary detection (SSN, credit cards, name/phone patterns).
     - Feature for Presidio HTTP connector; degrade to local heuristics.
     - Provide actions: mask, hash, reject with context.
  3. **WS4-03 – Prompt Injection Detection**
     - Implement heuristics (e.g., presence of “ignore previous instructions”), structural detection (anomaly scoring), optional classifier integration (Prompt Shields).
     - Provide scoring output for telemetry.
  4. **WS4-04 – Retrieval Normalization**
     - Streaming parser using `html5ever` or `lol_html`; enforce MIME allowlist, max size, script stripping, canonicalization.
  5. **WS4-05 – Pipeline Composition**
     - Compose stages with metadata propagation (verdict, reasons).
     - Provide configuration per channel (user prompts vs retrieved content).

### WS5 – Output Validation & Egress Safety
- **Context**: Guard outputs against injection, XSS, leakage (OWASP LLM-02/06).
- **Tasks**:
  1. **WS5-01 – Schema Validator**
     - Provide declarative DSL (similar to Guardrails) with type enforcement, `serde_json` integration.
  2. **WS5-02 – Sanitization Pipeline**
     - HTML/terminal sanitization with `ammonia`; streaming to handle long outputs.
  3. **WS5-03 – Code Guard**
     - Detect code fences / shell commands; require explicit policy to pass through; otherwise block or mark for approval.
  4. **WS5-04 – Secret/PII Egress Scanner**
     - Bridge to TruffleHog/detect-secrets via controlled subprocess; ensure sanitized temporary files; map severity → action.
  5. **WS5-05 – Transform Hooks**
     - Provide sanitized Markdown → HTML converter, JSON patcher; ensure deterministic output for caching.

### WS6 – Tool & MCP Security
- **Context**: Prevent excessive agency and session hijack (OWASP LLM-08).
- **Tasks**:
  1. **WS6-01 – Tool Policy Schema**
     - Define YAML/JSON structure for allowlists, rate limits, approval requirements, risk scoring formula.
  2. **WS6-02 – Execution Wrapper**
     - Implement guard around `weavegraph` tool invocations: check session tokens, enforce idempotency keys, audit logs.
  3. **WS6-03 – Fetch Sanitizer**
     - For HTTP tool outputs, sanitize HTML, enforce redirect policies, block script tags.
  4. **WS6-04 – Approval Workflow**
     - Provide integration with manual or secondary LLM approval; add state machine for request/approve/deny.
  5. **WS6-05 – Tool Honeytokens**
     - Inject honeytokens into tool responses; detect exfiltration attempts.

### WS7 – RAG Hardening
- **Context**: Address indirect prompt injection, misinformation (OWASP LLM-01/02/06).
- **Tasks**:
  1. **WS7-01 – Sanitized Ingestion**
     - Extend `wg-ragsmith` integration to hash/sign documents, enforce domain allowlist, MIME filters, size caps.
  2. **WS7-02 – Provenance Tagging**
     - Append metadata to chunks (domain hash, timestamp, injection score) for downstream gating.
  3. **WS7-03 – Grounded Answer Rails**
     - Build rule engine verifying each answer claim has supporting citation; integrate cross-encoder for validation.
  4. **WS7-04 – Corpus Scanner CLI**
     - CLI to run PII/secret scan across stored corpus; produce severity reports.
  5. **WS7-05 – Adversarial Chunk Tests**
     - Integration tests verifying highlight/downgrade of malicious chunks.

### WS8 – Abuse & Availability Controls
- **Context**: Avoid DoS/cost blowouts (OWASP LLM-04/08/09).
- **Tasks**:
  1. **WS8-01 – Rate Limiter & Quotas**
     - Implement multi-dimensional rate limiting (user/session/tool) with persistent counters (Redis optional).
  2. **WS8-02 – Recursion Guard**
     - Track tool hop depth; cut off recursion with actionable errors.
  3. **WS8-03 – Cost Monitor**
     - Estimate token usage + external service cost; emit metrics/alerts when thresholds exceeded.
  4. **WS8-04 – Circuit Breakers**
     - Wrap guardrail stages and external services with breaker states; degrade gracefully with alerts.

### WS9 – Telemetry & Incident Response
- **Context**: Satisfy AI RMF Measure/Manage; respond to incidents quickly.
- **Tasks**:
  1. **WS9-01 – Event Schema Extensions**
     - Update JSON schemas with guardrail metadata, risk scores, provenance IDs.
  2. **WS9-02 – OTLP Exporter & Metrics**
     - Provide tracing spans, metrics (counts, latency, false positive rates).
  3. **WS9-03 – Incident Orchestrator**
     - Implement workflow (quarantine session, revoke tokens, notify on-call) triggered by high-severity events.
  4. **WS9-04 – Grafana Dashboards**
     - Prebuilt dashboards for prompt injections, PII detections, cost usage, honeytoken hits.
  5. **WS9-05 – Audit Log Retention**
     - Export encrypted JSONL logs, enforce retention/purge policies, support DSAR requests.

### WS10 – Testing & Red Team Tooling
- **Context**: Ensure controls remain effective as threats evolve.
- **Tasks**:
  1. **WS10-01 – Adversarial Corpus Curation**
     - Collect datasets, label per risk, document licensing, integrate into repo.
  2. **WS10-02 – Attack Harness**
     - Build CLI/test harness executing guardrail pipelines across adversarial corpora; produce coverage metrics.
  3. **WS10-03 – Regression Suites**
     - Integration tests verifying pipeline ordering, degrade modes, error handling.
  4. **WS10-04 – Red Team Playbook**
     - Document scenarios, roles, frequency, success metrics; align with UK/NCSC guidelines.
  5. **WS10-05 – Canary Leak Probe**
     - Scheduled job to exercise honeytoken detection end-to-end nightly.

### WS11 – Supply Chain & Release Hygiene
- **Context**: Prevent tampering, ensure traceability (OWASP LLM-10, SSDF).
- **Tasks**:
  1. **WS11-01 – SBOM Generation**
     - Integrate CycloneDX generation into CI; store artifacts.
  2. **WS11-02 – Dependency Audits**
     - Configure `cargo audit`, `cargo deny`, license checks, policy exceptions.
  3. **WS11-03 – Signed Releases**
     - Setup GitHub workflow signing git tags and container images (Cosign); crates.io publish with `cargo-release`.
  4. **WS11-04 – Hardened Aux Services**
     - Provide Docker Compose for Presidio, TruffleHog with non-root users, read-only FS, network policies; include security checklist.

### WS12 – Developer Experience & Adoption
- **Context**: Ensure teams can adopt without specialist knowledge.
- **Tasks**:
  1. **WS12-01 – Integration Guide**
     - Step-by-step instructions for weaving guardrails into existing graphs; include config examples, troubleshooting.
  2. **WS12-02 – Examples**
     - `security_pipeline.rs`, `rag_guarded.rs` demonstrating pipeline usage, metrics export.
  3. **WS12-03 – `xtask` CLI Enhancements**
     - Commands for policy init, scanner runs, canary rotation, running attack suite.
  4. **WS12-04 – FAQ/Troubleshooting**
     - Address latency tuning, false positives, degrade modes, integration with CI/CD.

---

## 6. External Dependencies & Service Strategy
- **Llama Guard 2 ONNX**: Open-source moderation model; host weights internally, run via ONNX Runtime. Provide instructions for CPU quantization to avoid GPU requirement.
- **Microsoft Prompt Shields**: optional classifier integration via REST API; provide offline fallback heuristics; note usage limits.
- **Microsoft Presidio**: recommend self-hosted container; integrate via HTTP API with authentication; include optional local-only PII detection.
- **TruffleHog / detect-secrets**: run through CLI wrappers; manage via sandboxed subprocess; maintain pinned versions.
- **Ammonia**: Rust HTML sanitizer maintained by Servo; ensures safe markup.
- **OpenTelemetry**: expose metrics/traces; allow OTLP collector integration.
- **Tower, DashMap, Tokio**: concurrency primitives for pipelines, caching, timeouts.
- **Redis (optional)**: persistent rate limiting/counters; provide feature flag.
- **Grafana**: dashboards; supply JSON exports and instructions.

For each dependency we document licensing, configuration, degrade behaviour, and reference Docker Compose files for local development.

---

## 7. Risk Register & Mitigations
- **Latency Inflation**
  - *Impact*: guardrails make apps feel sluggish.
  - *Mitigation*: caching, asynchronous parallel stage execution, optional bypass for low-risk stages, publish benchmarks (benches/pipeline_bench.rs).
  - *Monitoring*: latency metrics per stage (WS9).
- **False Positives / User Friction**
  - *Impact*: legitimate prompts blocked; users bypass security.
  - *Mitigation*: configurable thresholds, approval workflows, telemetry to tune thresholds, policy override with audit logging.
  - *Monitoring*: false positive rates tracked via attack harness + real traffic metrics.
- **External Service Downtime**
  - *Impact*: guardrail pipeline fails; security coverage drops.
  - *Mitigation*: circuit breakers, degrade modes, local heuristics, incident alerts.
  - *Monitoring*: degrade mode telemetry, health checks.
- **Model Drift / Misconfiguration**
  - *Impact*: outdated moderation/injection models miss attacks.
  - *Mitigation*: store model IDs/checksums, add update checklist, schedule review (quarterly).
- **Secret Leakage in Logs**
  - *Impact*: telemetry becomes new attack surface.
  - *Mitigation*: logging macros with mandatory redaction, secret scanning of logs, retention policies.
- **Supply Chain Compromise**
  - *Impact*: malicious dependency/backdoor.
  - *Mitigation*: SBOM, signed releases, dependency auditing, freeze file for critical versions.
- **Team Bandwidth**
  - *Impact*: part-time effort delays features.
  - *Mitigation*: sprint roadmap, prioritize MVP, allow deferral of non-critical modules.

---

## 8. Rollout Strategy & Milestones
1. **Phase 0 – Governance Ready**: Complete WS1 deliverables, baseline threat model, control matrix, coding standards.
2. **Phase 1 – MVP Guardrails**: Deliver WS2 + WS3 + core of WS4 (moderation + injection). Enables system prompt hardening and user prompt sanitization.
3. **Phase 2 – Output & Tool Safety**: Add WS5 + WS6 core features (schema validation, tool policy wrappers).
4. **Phase 3 – RAG & Abuse Controls**: Integrate WS7 + WS8 to protect retrieval pipelines and budgets.
5. **Phase 4 – Observability & Testing**: Land WS9 + WS10 for telemetry, incident response, continuous validation.
6. **Phase 5 – Release & Adoption**: Finalize WS11 + WS12, run red team exercise, publish documentation, sign releases.

Milestones align with gating reviews and beta releases:
- **Beta 0 (MVP)**: after Phase 1.
- **Beta 1**: after Phases 2 & 3 (complete guardrails).
- **RC**: after Phase 4.
- **1.0 GA**: after Phase 5 sign-off.

---

## 9. Release Checklist (v1.0)
- **Governance**: Threat model/control matrix updated; security review sign-off logged.
- **Quality Gates**: CI runs `fmt`, `clippy`, `test`, adversarial harness, fuzz tests (where applicable), `cargo audit`, `cargo deny`.
- **Policy Coverage**: Default policy enforces prompt hardening, input gating, output validation, tool policy, RAG hygiene, abuse guard (can be tuned but not disabled without audit flag).
- **Observability**: OTLP exporter + dashboards verified; degrade mode alerts tested; honeytoken incident flow executed end-to-end.
- **Docs & Examples**: Integration guide, architecture doc, FAQ, and examples validated by fresh checkout instructions.
- **Testing**: Attack harness coverage report meets thresholds; red team playbook executed with findings resolved or accepted.
- **Supply Chain**: SBOM generated; dependencies pinned; release artifacts signed; CHANGELOG updated.
- **Operations**: CLI tools documented; support procedures (incident runbook, DSAR process) available.

---

## 10. Suggested Sprint Roadmap (2-Week Sprints, Part-Time Cadence)

### Sprint 1 – Governance & Scaffold
- **Goals**: WS1-01/02/03, WS2-01, partial WS2-02.
- **Deliverables**: threat model doc, control matrix draft, governance README, crate scaffold builds in CI, initial `SecurityPolicy` struct.
- **Threat coverage**: establishes compliance baseline; prevents mis-scoped implementation.
- **Risks**: time spent aligning frameworks; mitigate via cut-down initial docs with TODOs for deep dives.

### Sprint 2 – Policy Engine & Prompt Templating
- **Goals**: finish WS2-02/03/05, begin WS3-01.
- **Deliverables**: pipeline executor running placeholder stages, weavegraph integration example, secure template engine prototype.
- **Threat coverage**: enables hooking guardrails into graphs; early protection against role smuggling.
- **Risks**: concurrency complexity; mitigate with unit tests and instrumentation.

### Sprint 3 – Honeytokens & Refusal Policies
- **Goals**: WS3-02/03/04, CLI commands (WS12-03 subset).
- **Deliverables**: honeytoken rotation CLI, template scanner, refusal fallback flow, documentation updates.
- **Threat coverage**: addresses system prompt leakage detection (OWASP LLM-06).
- **Risks**: key management; mitigate with encryption best practices, secret storage doc.

### Sprint 4 – Input Moderation & Injection Heuristics
- **Goals**: WS4-01/03/05, basic rate limiting (WS8-01 minimal).
- **Deliverables**: user prompt pipeline (moderation + injection detection), heuristics, telemetry fields, simple rate limiter.
- **Threat coverage**: direct prompt injection, harmful content (LLM-01/04).
- **Risks**: classifier integration time; fallback heuristics ensure MVP progress.

### Sprint 5 – PII Handling & Retrieval Normalization
- **Goals**: WS4-02/04, metadata propagation, partial WS7-02.
- **Deliverables**: PII redaction module, HTML sanitizer for retrieved content, metadata tagging.
- **Threat coverage**: PII exposure prevention, RAG-level injection control.

### Sprint 6 – Output Guardrails Core
- **Goals**: WS5-01/02/04 baseline.
- **Deliverables**: schema validator, HTML/terminal sanitizer, secret scanner stub, integration tests.
- **Threat coverage**: LLM-02/06 output controls.

### Sprint 7 – Tool Policy Foundations
- **Goals**: WS6-01/02/03.
- **Deliverables**: tool policy schema, execution wrapper, HTTP response sanitizer, example integration.
- **Threat coverage**: excessive agency, confused deputy risk.

### Sprint 8 – RAG Hygiene Foundations
- **Goals**: WS7-01/02/04.
- **Deliverables**: sanitized ingestion plug-in, provenance tagging, corpus scanner CLI.
- **Threat coverage**: indirect prompt injection, data leakage from corpus.

### Sprint 9 – Observability MVP
- **Goals**: WS9-01/02/04.
- **Deliverables**: updated schemas, OTLP exporter, Grafana dashboards, instrumentation docs.
- **Threat coverage**: monitoring for all prior modules, AI RMF Measure outcomes satisfied.

### Sprint 10 – Abuse & Degrade Controls
- **Goals**: WS8-02/03/04, extend WS8-01.
- **Deliverables**: recursion guard, cost metrics, circuit breakers, telemetry integration.
- **Threat coverage**: DoS, runaway costs.

### Sprint 11 – Testing & Red Team Prep
- **Goals**: WS10-01/02/03/05.
- **Deliverables**: adversarial corpora, attack harness CI job, regression suite expansion, nightly canary probe job.
- **Threat coverage**: ensures ongoing effectiveness; ready for external review.

### Sprint 12 – Supply Chain & Release Prep
- **Goals**: WS11-01/02/03/04, WS12-01/02/04.
- **Deliverables**: SBOM automation, audit gates, signed release workflow, integration guide, examples refreshed, FAQ.
- **Threat coverage**: supply chain, adoption.

Include buffer sprints or interleaved hardening weeks if latency metrics or false positives require tuning.

---

## 11. MVP Cut – Prompt Hardening + User Prompt Sanitization

A minimal but production-useful release focusing on protecting system prompts and gating user input is achievable within the first **two sprints** if scope is constrained to:
- `SecurityPolicy`, `GuardrailStage` executor with limited stage types.
- Secure templating (WS3-01) and honeytoken detection (WS3-02 basic).
- Template scanner CLI (WS3-03).
- Moderation heuristics (WS4-01 lightweight) and injection heuristics (WS4-03) without external classifier.
- Simple rate limiting (WS8-01 minimal) to prevent immediate abuse.
- Basic telemetry logging (subset of WS9-01) for audit.

Deliverables for MVP:
- Example graph demonstrating sanitized user prompt flow and hardened system prompt template.
- Policy configuration enabling/disabling moderation heuristics.
- CLI commands for scanning templates and rotating honeytokens.
- Documentation describing integration steps and limitations (e.g., PII detection minimal, output validation pending).

This MVP establishes immediate value while the team incrementally adds deeper defenses (PII, output sanitization, tool security) in subsequent sprints.
