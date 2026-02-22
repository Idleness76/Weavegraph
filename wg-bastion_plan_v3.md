# `wg-bastion` Master Plan v3.0

**Last Updated:** February 2026  
**Status:** Strategic Implementation Plan  
**Target Rust Version:** 1.89+ (Edition 2024)  
**Supersedes:** `wg-security_plan.md` (v1.0, 2023), `wg-bastion_plan_v2.md` (v2.0, January 2026)

---

## Executive Summary

`wg-bastion` is a standalone, opt-in security crate within the Weavegraph workspace that provides defense-in-depth guardrails for graph-driven LLM applications. It sits alongside `weavegraph` (the core graph-execution runtime) and `wg-ragsmith` (RAG utilities), providing composable security pipelines that hook into weavegraph's node execution lifecycle, event bus, and reducer system.

This v3.0 plan is a ground-up reconciliation of the v1.0 and v2.0 plans against:

- **Actual codebase state** — Sprint 1 skeleton is complete (~564 LOC across 3 source files, plus extensive docs/threat model/control matrix)
- **OWASP LLM Top 10 (2025)** — Released November 2024, with new categories LLM07 (System Prompt Leakage) and LLM08 (Vector/Embedding Weaknesses)
- **OWASP Agentic AI Security Initiative** — Launched late 2025, addressing autonomous agent risks
- **NIST AI RMF 1.0 + AI 600-1** — Generative AI Profile (July 2024), 12 GenAI-specific risk categories
- **EU AI Act** — Regulation 2024/1689 with phased enforcement (February 2025 through August 2027)
- **MCP Protocol** — Specification revision 2025-03-26 with OAuth 2.1, Streamable HTTP transport, and session management
- **Modern guardrails architectures** — NeMo Guardrails v0.20.0, LlamaGuard 3, ShieldGemma, PromptGuard
- **Rust ML ecosystem** — `ort` 2.0.0-rc.11, `candle` 0.9.2, `burn` 0.16.x

The crate targets <50ms P95 latency overhead, graceful degradation when external services are unavailable, and fail-closed defaults. Every module maps to specific OWASP, NIST, EU AI Act, and MITRE ATLAS identifiers for full traceability.

---

## Table of Contents

0. [Current Implementation Status](#0-current-implementation-status)
1. [Security Framework Anchors](#1-security-framework-anchors)
2. [Threat Landscape & Actors](#2-threat-landscape--actors)
3. [Vision, Scope & Constraints](#3-vision-scope--constraints)
4. [Architecture & Project Layout](#4-architecture--project-layout)
5. [Core Traits & Types](#5-core-traits--types)
6. [Module Deep Dive](#6-module-deep-dive)
7. [Weavegraph Integration Architecture](#7-weavegraph-integration-architecture)
8. [wg-ragsmith Integration Architecture](#8-wg-ragsmith-integration-architecture)
9. [Workstreams & Execution Plan](#9-workstreams--execution-plan)
10. [Sprint Roadmap](#10-sprint-roadmap)
11. [External Dependencies](#11-external-dependencies)
12. [Risk Register](#12-risk-register)
13. [Rollout Strategy & Release Gates](#13-rollout-strategy--release-gates)
14. [MVP Definition](#14-mvp-definition)
15. [Release Checklist](#15-release-checklist)
16. [Grounding Resources](#16-grounding-resources)
17. [Appendices](#17-appendices)

---

## 0. Current Implementation Status

Before detailing the forward plan, this section documents what exists today so the execution plan builds on solid ground.

### 0.1 Implemented (Sprint 1 Complete)

| Component | File | LOC | Status |
|-----------|------|-----|--------|
| **Crate scaffold** | `wg-bastion/Cargo.toml` | 87 | ✅ Full — dependencies, feature flags, workspace member |
| **Crate entry** | `src/lib.rs` | 87 | ✅ Full — doc comments, module declarations, prelude |
| **Config module** | `src/config/mod.rs` | 224 | ✅ Full — `SecurityPolicy`, `PolicyBuilder`, `FailMode`, `ConfigError` |
| **Pipeline module** | `src/pipeline/mod.rs` | 253 | ✅ Full — `SecurityStage` trait, `SecurityPipeline`, `PipelineBuilder`, `SecurityContext`, `StageResult` |
| **README** | `README.md` | 246 | ✅ Full — overview, quick start, status table |
| **Architecture doc** | `docs/architecture.md` | ~350 | ✅ Full — module design, integration patterns |
| **Threat model** | `docs/threat_model.md` | 507 | ✅ Full — attack trees for all OWASP categories |
| **Control matrix (MD)** | `docs/control_matrix.md` | ~300 | ✅ Full — human-readable control mapping |
| **Control matrix (CSV)** | `docs/control_matrix.csv` | 70 rows | ✅ Full — 70 controls with status tracking |
| **Attack playbook** | `docs/attack_playbooks/llm01_prompt_injection.md` | 284 | ✅ Full — incident response for LLM01 |
| **Data flow diagram** | `docs/diagrams/data_flow.mmd` | 193 | ✅ Full — Mermaid flowchart |

### 0.2 Code Quality Assessment

**Strengths:**
- Excellent doc comments with examples on every public type
- `#![warn(missing_docs, clippy::pedantic)]` enforced
- Idiomatic Rust patterns: builder, `#[must_use]`, `thiserror`, `Default`
- Async-first design with `async_trait` and Tokio
- Serde serialization with `snake_case` renaming
- 6 unit tests passing

**Issues to Address:**
- `miette`, `jsonschema`, `schemars`, `ring`, `zeroize` declared but unused
- No `#[cfg(feature = "...")]` guards — feature flags have no compile-time effect
- `SecurityStage::execute` takes `&str` only — needs structured `Content` enum
- `SecurityPipeline` has no `FailMode` integration (always fail-closed)
- Sequential execution only — no parallel stage support
- `PipelineBuilder::build()` is infallible despite `Configuration` error variant
- No integration tests, property tests, or benchmarks

### 0.3 Missing Components

- **9 source modules** — prompt, input, output, tools, rag, agents, session, abuse, telemetry
- **9 attack playbooks** — LLM02 through LLM10
- **Zero weavegraph integration** — no hooks, no feature flag, no cross-crate dependency
- **No tests/ directory**, no examples/, no benches/
- **No CI workflow** for wg-bastion specifically

---

## 1. Security Framework Anchors

### 1.1 OWASP LLM Top 10 (2025 Edition)

Released November 2024 by the OWASP GenAI Security Project (600+ contributors). This is the primary risk taxonomy for wg-bastion.

| Risk ID | Vulnerability | Change from 2023 | wg-bastion Module(s) |
|---------|--------------|-------------------|---------------------|
| **LLM01:2025** | Prompt Injection | Retained #1. Expanded: multimodal injection, adversarial suffixes, multilingual obfuscation, instruction hierarchy attacks | `input.injection`, `prompt.isolation`, `input.multimodal` |
| **LLM02:2025** | Sensitive Information Disclosure | Renamed from "Data Leakage" (was LLM06); elevated to #2 | `input.pii`, `output.egress`, `prompt.honeytoken` |
| **LLM03:2025** | Supply Chain Vulnerabilities | Elevated from LLM05; expanded to model repos, AIBOM | `supply_chain.sbom`, `supply_chain.aibom` |
| **LLM04:2025** | Data and Model Poisoning | Expanded from "Training Data Poisoning"; adds embedding poisoning, sleeper agents | `rag.ingestion`, `rag.provenance`, `input.moderation` |
| **LLM05:2025** | Improper Output Handling | Renamed from "Insecure Output Handling" | `output.schema`, `output.sanitizer`, `output.code_guard` |
| **LLM06:2025** | Excessive Agency | Elevated from LLM08; critical for agentic systems | `tools.policy`, `tools.guard`, `tools.approval`, `agents.boundaries` |
| **LLM07:2025** | System Prompt Leakage | **NEW** — dedicated category for template exposure | `prompt.template`, `prompt.honeytoken`, `prompt.scanner`, `output.egress` |
| **LLM08:2025** | Vector and Embedding Weaknesses | **NEW** — RAG-specific security risks | `rag.embedding`, `rag.access_control`, `rag.provenance` |
| **LLM09:2025** | Misinformation | Expanded from "Overreliance" | `output.grounding`, `rag.grounding` |
| **LLM10:2025** | Unbounded Consumption | Renamed from "DoS"; includes wallet-draining, inference cost attacks | `abuse.rate_limit`, `abuse.cost`, `abuse.recursion`, `abuse.circuit` |

**Source:** [genai.owasp.org/llm-top-10](https://genai.owasp.org/llm-top-10/)

### 1.2 OWASP Agentic AI Security Initiative (2025)

Launched late 2025 under the OWASP GenAI project. Addresses autonomous agent systems using frameworks like LangGraph, AutoGPT, CrewAI, and weavegraph.

**Key Risk Categories:**

1. **Excessive Autonomy / Privilege Escalation** — Agents with broad tool access taking unauthorized actions
2. **Goal Manipulation / Misalignment** — Adversarial manipulation of agent objectives via indirect injection
3. **Memory/State Poisoning** — Corrupting persistent agent memory to influence future sessions
4. **Multi-Agent Trust Boundaries** — Cascading attacks through unverified agent-to-agent trust
5. **Tool Abuse / Confused Deputy** — Agents performing privileged operations on behalf of users
6. **Planning Hijacking** — Manipulating reasoning/planning loops to alter execution sequences
7. **Uncontrolled Delegation** — Agent-to-agent delegation without authorization chains
8. **Resource Exhaustion via Loops** — Infinite tool-call loops creating unbounded consumption
9. **Observation Manipulation** — Injecting false observations into agent perception
10. **Inadequate Human-in-the-Loop** — Missing or bypassable checkpoints for high-risk actions

**Source:** [genai.owasp.org/initiatives/agentic-security-initiative](https://genai.owasp.org/initiatives/agentic-security-initiative/)

### 1.3 NIST AI Risk Management Framework

#### AI RMF 1.0 (January 2023)

| Function | Purpose | wg-bastion Mapping |
|----------|---------|-------------------|
| **GOVERN** | Establish AI risk governance: policies, roles, accountability | `config` module, policy documentation, governance docs |
| **MAP** | Identify and contextualize AI risks | Threat model, control matrix, risk register |
| **MEASURE** | Analyze and assess AI risks with metrics | Attack harness, regression suites, telemetry metrics |
| **MANAGE** | Prioritize, respond to, monitor risks | Incident orchestrator, circuit breakers, audit trails |

#### AI 600-1: Generative AI Profile (July 2024)

Cross-sectoral profile addressing 12 GenAI-specific risk categories:

| # | Risk Category | wg-bastion Coverage |
|---|--------------|---------------------|
| 1 | CBRN Information | `input.moderation` (harmful content detection) |
| 2 | Confabulation | `output.grounding`, `rag.grounding` |
| 3 | Data Privacy | `input.pii`, `output.egress`, `telemetry.audit` |
| 4 | Environmental Impact | `abuse.cost` (resource monitoring) |
| 5 | Harmful Bias | `input.moderation`, `output.schema` |
| 6 | Homogenization | Documentation only (out of scope for runtime controls) |
| 7 | Human-AI Configuration | `tools.approval` (human-in-the-loop), `agents.boundaries` |
| 8 | Information Integrity | `output.grounding`, `prompt.honeytoken` |
| 9 | Information Security | All security modules |
| 10 | Intellectual Property | `rag.provenance`, `supply_chain.aibom` |
| 11 | Obscene/Degrading Content | `input.moderation`, `output.sanitizer` |
| 12 | Value Chain Integration | `supply_chain`, `config.validator` |

**Source:** [NIST AI 600-1](https://airc.nist.gov/Docs/1)

#### AI 100-2e2023: Adversarial ML Taxonomy

Attack/defense taxonomy covering data poisoning, model evasion, model extraction, and inference attacks. Informs our threat model and attack harness design.

### 1.4 EU AI Act (Regulation 2024/1689)

| Date | Milestone | Status (Feb 2026) |
|------|-----------|-------------------|
| **February 2, 2025** | Prohibited practices in effect | ✅ Active |
| **August 2, 2025** | GPAI model rules in effect | ✅ Active |
| **August 2, 2026** | High-risk AI systems; transparency obligations | ⏳ 6 months away |
| **August 2, 2027** | Remaining high-risk categories | ⏳ 18 months away |

**wg-bastion provides hooks for EU AI Act compliance:**
- Audit logging supports documentation requirements (Article 12)
- Human-in-the-loop approval flows support oversight requirements (Article 14)
- AIBOM generation supports transparency requirements (Article 52)
- Content provenance tracking supports AI-generated content labeling

### 1.5 MCP Protocol Security (Spec Revision 2025-03-26)

The Model Context Protocol (used by weavegraph's optional `llm` feature via `rmcp` 0.8) has critical security implications:

| Concern | MCP Spec Requirement | wg-bastion Control |
|---------|---------------------|-------------------|
| **Confused Deputy** | Agents acting on behalf of users can be tricked | `tools.mcp` — per-tool capability tokens, scope validation |
| **Session Management** | Cryptographically secure `Mcp-Session-Id` | `session.tokens` — UUID/JWT session IDs, lifecycle management |
| **Token Handling** | OAuth 2.1, Bearer tokens, PKCE required | `tools.mcp` — token validation middleware |
| **Transport Security** | Origin header validation, HTTPS required, localhost binding | `tools.fetch` — URL validation, transport enforcement |
| **Token Passthrough** | Never accept tokens not issued for the server | `tools.mcp` — token audience validation |
| **DNS Rebinding** | Local servers must validate Origin | Network security documentation |

**Source:** [modelcontextprotocol.io/specification/2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26/)

### 1.6 MITRE ATLAS Mapping

We map threats to the MITRE ATLAS framework (16 tactics, 155 techniques, 35 mitigations) for standardized threat intelligence:

| Technique ID | Name | wg-bastion Module |
|--------------|------|------------------|
| AML.T0051 | LLM Prompt Injection | `input.injection`, `prompt` |
| AML.T0054 | LLM Jailbreak | `input.moderation`, `output` |
| AML.T0057 | LLM Data Leakage | `output.egress`, `prompt.honeytoken` |
| AML.T0043 | Craft Adversarial Data | `rag.ingestion`, `input.normalization` |
| AML.T0048 | Embed Malware | `tools.fetch`, `rag.ingestion` |
| AML.T0040 | ML Model Inference API Access | `abuse.rate_limit`, `session` |

**Source:** [atlas.mitre.org](https://atlas.mitre.org/)

### 1.7 Mapping Approach

For every workstream task and module we capture:
- **OWASP LLM-XX:2025** risk identifiers
- **OWASP Agentic AI** risk categories (where applicable)
- **NIST AI RMF** function outcomes (Govern / Map / Measure / Manage)
- **NIST AI 600-1** GenAI risk categories
- **EU AI Act** article references (where applicable)
- **MITRE ATLAS** technique identifiers
- **SSDF** practice references

The living control matrix (`docs/control_matrix.csv`, 70 controls) maintains traceability. Each control has a unique ID (C001–C070) linking to specific code, tests, and documentation.

---

## 2. Threat Landscape & Actors

### 2.1 Threat Actor Profiles

| Threat Actor | Profile | Motivations | Primary Surfaces | Mitigating Modules |
|--------------|---------|-------------|------------------|-------------------|
| **Malicious End-Users** | External users via UI/API | Data exfiltration, guardrail bypass, tool manipulation | User prompts, file uploads, tool requests | `input`, `prompt`, `output`, `abuse`, `telemetry` |
| **Adversarial Retrievers** | Poisoned docs/web pages in RAG corpora | Indirect injection, misinformation, credential harvesting | Ingestion pipeline, vector stores, context assembly | `input.normalization`, `rag.ingestion`, `rag.provenance` |
| **MCP Confused Deputy** | Third-party MCP servers or compromised agents | Session hijacking, lateral movement, credential reuse | MCP tool calls, session tokens, authorization flows | `tools.mcp`, `tools.policy`, `session`, `telemetry.incident` |
| **Rogue Insiders** | Developers/operators with config access | Disable controls, leak prompts, misuse honeytokens | Templates, config overrides, logs, release pipeline | `config`, `prompt.scanner`, `telemetry.audit`, `supply_chain` |
| **Automated Adversaries** | Bots/scripts for high-volume attacks | Resource exhaustion, cost explosion, model extraction | Prompt API, tool execution, embedding endpoints | `abuse.rate_limit`, `abuse.cost`, `abuse.circuit` |
| **Supply Chain Attackers** | Actors targeting dependencies/releases | Pipeline compromise, backdoor insertion, model poisoning | Crate deps, model weights, Docker images | `supply_chain.sbom`, `supply_chain.aibom` |
| **Multi-Agent Exploiters** | Attackers leveraging agent-to-agent trust | Privilege escalation, trust boundary violations | Inter-agent communication, shared context, delegated actions | `agents.delegation`, `agents.communication`, `session.isolation` |
| **Embedding Inverters** | ML researchers with inversion techniques | Recover training data from embeddings, membership inference | Vector stores, embedding APIs, similarity search | `rag.embedding`, `rag.access_control` |

### 2.2 Attack Vector Evolution (2024–2026)

**Prompt Injection Advances:**
- Multimodal injection (text in images, audio commands, structured data payloads)
- Adversarial suffixes via GCG (Greedy Coordinate Gradient — Zou et al. 2023, refined through 2025)
- AutoDAN (genetic algorithm-based jailbreak generation)
- Crescendo attacks (multi-turn escalation)
- PAIR (Prompt Automatic Iterative Refinement using attacker LLM)
- Multilingual obfuscation exploiting translation inconsistencies
- Instruction hierarchy attacks (role boundary manipulation)

**MCP-Specific Threats:**
- Confused deputy via static client IDs and consent cookie exploitation
- Token passthrough anti-pattern (accepting tokens not issued for the server)
- Session hijacking through guessed session IDs
- DNS rebinding against local MCP servers binding to 0.0.0.0
- SSE stream manipulation when transport is unencrypted
- Tool enumeration for targeted attack crafting
- Authorization scope escalation via overly broad OAuth scopes

**RAG/Vector Store Threats (LLM08:2025):**
- Embedding inversion (demonstrated with ada-002 and similar models)
- Cross-context leakage in multi-tenant vector stores
- Retrieval poisoning (adversarial documents ranking highly for target queries)
- Federation conflicts from contradictory knowledge sources

**Agentic AI Threats:**
- Tool chain exploitation (chaining calls for unauthorized outcomes)
- Agent memory poisoning (corrupting persistent state across sessions)
- Goal hijacking via context injection
- Delegation attacks exploiting inter-agent trust
- Planning loop manipulation through observation injection

---

## 3. Vision, Scope & Constraints

### 3.1 Vision

Deliver an extensible, production-grade Rust security crate that provides defense-in-depth guardrails for weavegraph LLM applications. Protect against the OWASP LLM Top 10:2025 and agentic AI threats while maintaining developer ergonomics, minimal latency overhead, and alignment with NIST AI RMF governance.

### 3.2 Core Capabilities

| Layer | Capabilities |
|-------|-------------|
| **Input Security** | Prompt injection detection (heuristic + ML), content moderation, PII detection/masking, input normalization, multimodal validation |
| **Context Management** | System prompt isolation, role boundary enforcement, external content segregation, honeytoken injection/detection |
| **Output Security** | Schema validation, content moderation, egress scanning, sanitization, fact-checking/grounding hooks |
| **Tool/MCP Security** | Capability-based access control, tool allowlisting, execution sandboxing, human-in-the-loop approval, MCP protocol controls |
| **RAG Security** | Permission-aware retrieval, ingestion validation, provenance tracking, embedding access controls, corpus scanning |
| **Agentic Security** | Delegation chain tracking, autonomy boundaries, kill switches, agent memory protection, inter-agent authentication |
| **Abuse Prevention** | Multi-dimensional rate limiting, token/cost budgets, recursion guards, circuit breakers |
| **Observability** | Structured security events, OTLP export, incident orchestration, audit trails, AIBOM generation |
| **Testing** | Adversarial corpus, attack harness, regression suites, red team playbook |

### 3.3 Non-Goals

- End-user authentication/authorization (use dedicated auth libraries)
- Billing/metering enforcement
- Proprietary vendor integrations requiring paid licenses
- Industry certification guarantees without customization
- Model training security (focus is inference-time)

### 3.4 Technical Constraints

| Constraint | Requirement | Rationale |
|-----------|-------------|-----------|
| MSRV | 1.89+ (Edition 2024) | Workspace alignment with weavegraph |
| Async Runtime | Tokio-compatible | Match weavegraph's `tokio` 1.x runtime |
| GPU | Optional (CPU-only must work) | Deployment accessibility |
| Dependencies | Feature-gated heavy deps | Keep base weavegraph lightweight |
| Performance | <50ms P95 added latency (default config) | Production viability |
| Degradation | Graceful fallback when services unavailable | Operational resilience |
| Security Posture | Code review, fuzzing, signed releases | SSDF compliance |

---

## 4. Architecture & Project Layout

### 4.1 Workspace Structure

```
Weavegraph/
├── Cargo.toml                    (workspace root)
├── weavegraph/                   (core graph-execution runtime)
├── wg-ragsmith/                  (RAG ingestion/chunking/vector-store)
├── wg-bastion/                   (this crate — security suite)
│   ├── Cargo.toml
│   ├── README.md
│   ├── src/
│   │   ├── lib.rs                (crate entry; feature gates, re-exports)
│   │   │
│   │   ├── config/               (policy schema + configuration)
│   │   │   ├── mod.rs            ← EXISTS: PolicyBuilder, SecurityPolicy, FailMode
│   │   │   ├── policy.rs         (expanded SecurityPolicy with per-module sections)
│   │   │   ├── builder.rs        (enhanced PolicyBuilder with layered overrides)
│   │   │   ├── validator.rs      (policy validation rules)
│   │   │   └── schema.rs         (JSON schema export via schemars)
│   │   │
│   │   ├── pipeline/             (guardrail execution engine)
│   │   │   ├── mod.rs            ← EXISTS: SecurityStage, SecurityPipeline
│   │   │   ├── stage.rs          (enhanced GuardrailStage trait with Content enum)
│   │   │   ├── executor.rs       (PipelineExecutor with parallel support)
│   │   │   ├── outcome.rs        (StageOutcome: Allow/Block/Transform/Escalate)
│   │   │   ├── cache.rs          (LRU result caching with TTL)
│   │   │   └── circuit.rs        (CircuitBreaker state machine)
│   │   │
│   │   ├── prompt/               (system prompt security)
│   │   │   ├── mod.rs
│   │   │   ├── template.rs       (SecureTemplate with typed placeholders)
│   │   │   ├── honeytoken.rs     (HoneytokenStore with AES-256-GCM)
│   │   │   ├── scanner.rs        (TemplateScanner for secrets)
│   │   │   ├── isolation.rs      (role boundary enforcement)
│   │   │   └── refusal.rs        (RefusalPolicy)
│   │   │
│   │   ├── input/                (input validation pipeline)
│   │   │   ├── mod.rs
│   │   │   ├── moderation.rs     (ML/heuristic content moderation)
│   │   │   ├── pii.rs            (PII detection: local + Presidio)
│   │   │   ├── injection.rs      (ensemble injection detection)
│   │   │   ├── normalization.rs  (HTML/encoding/unicode normalization)
│   │   │   └── multimodal.rs     (image/audio/document validation)
│   │   │
│   │   ├── output/               (output validation pipeline)
│   │   │   ├── mod.rs
│   │   │   ├── schema.rs         (SchemaValidator with DSL + JSON Schema)
│   │   │   ├── sanitizer.rs      (HTML via ammonia, terminal escapes)
│   │   │   ├── code_guard.rs     (code output controls)
│   │   │   ├── egress.rs         (secret/PII egress scanning)
│   │   │   └── grounding.rs      (fact-checking hooks)
│   │   │
│   │   ├── tools/                (tool/MCP security)
│   │   │   ├── mod.rs
│   │   │   ├── policy.rs         (ToolPolicy: allowlists, scopes, risk scores)
│   │   │   ├── guard.rs          (ExecutionGuard wrapper)
│   │   │   ├── mcp.rs            (MCP-specific: OAuth 2.1, session binding)
│   │   │   ├── fetch.rs          (FetchSanitizer for HTTP responses)
│   │   │   ├── approval.rs       (ApprovalFlow state machine)
│   │   │   └── capability.rs     (capability-based access control)
│   │   │
│   │   ├── rag/                  (RAG security)
│   │   │   ├── mod.rs
│   │   │   ├── ingestion.rs      (SanitizedIngestion wrapping wg-ragsmith)
│   │   │   ├── provenance.rs     (ProvenanceTagger)
│   │   │   ├── grounding.rs      (GroundedRails)
│   │   │   ├── embedding.rs      (embedding security, inversion detection)
│   │   │   ├── access_control.rs (tenant isolation, permission-aware retrieval)
│   │   │   └── scanner.rs        (CorpusScanner CLI)
│   │   │
│   │   ├── agents/               (agentic AI security)
│   │   │   ├── mod.rs
│   │   │   ├── delegation.rs     (delegation chain tracking)
│   │   │   ├── boundaries.rs     (autonomy boundaries, kill switches)
│   │   │   ├── memory.rs         (agent memory protection, entry signing)
│   │   │   └── communication.rs  (inter-agent authentication)
│   │   │
│   │   ├── session/              (session management)
│   │   │   ├── mod.rs
│   │   │   ├── context.rs        (SecurityContext with parent chain)
│   │   │   ├── isolation.rs      (session isolation)
│   │   │   └── tokens.rs         (cryptographic session tokens)
│   │   │
│   │   ├── abuse/                (abuse prevention)
│   │   │   ├── mod.rs
│   │   │   ├── rate_limit.rs     (governor-based multi-dimensional limiting)
│   │   │   ├── recursion.rs      (RecursionGuard with cycle detection)
│   │   │   ├── cost.rs           (CostMonitor with budget enforcement)
│   │   │   └── circuit.rs        (CircuitBreaker for external services)
│   │   │
│   │   ├── telemetry/            (observability)
│   │   │   ├── mod.rs
│   │   │   ├── events.rs         (SecurityEvent structured types)
│   │   │   ├── exporter.rs       (OTLP exporter via opentelemetry)
│   │   │   ├── incident.rs       (IncidentOrchestrator)
│   │   │   ├── audit.rs          (encrypted audit logs, DSAR support)
│   │   │   └── dashboards.rs     (Grafana template exports)
│   │   │
│   │   ├── testing/              (security testing utilities)
│   │   │   ├── mod.rs
│   │   │   ├── corpus.rs         (AdversarialCorpus management)
│   │   │   ├── harness.rs        (AttackHarness with metrics)
│   │   │   └── regression.rs     (RegressionSuites)
│   │   │
│   │   ├── supply_chain/         (supply chain security)
│   │   │   ├── mod.rs
│   │   │   ├── sbom.rs           (CycloneDX SBOM generation)
│   │   │   ├── aibom.rs          (AI Bill of Materials)
│   │   │   └── audit.rs          (dependency auditing)
│   │   │
│   │   └── cli/                  (developer CLI)
│   │       ├── mod.rs
│   │       └── commands.rs       (xtask commands)
│   │
│   ├── examples/
│   │   ├── basic_guardrails.rs
│   │   ├── rag_secured.rs
│   │   ├── mcp_secured.rs
│   │   ├── agentic_boundaries.rs
│   │   └── full_pipeline.rs
│   │
│   ├── benches/
│   │   ├── pipeline_latency.rs
│   │   └── stage_throughput.rs
│   │
│   ├── tests/
│   │   ├── integration/
│   │   ├── adversarial/
│   │   └── regression/
│   │
│   └── docs/                     ← EXISTS (partially)
│       ├── architecture.md       ← EXISTS
│       ├── threat_model.md       ← EXISTS
│       ├── control_matrix.md     ← EXISTS
│       ├── control_matrix.csv    ← EXISTS
│       ├── integration_guide.md
│       ├── red_team_playbook.md
│       ├── attack_playbooks/     ← LLM01 EXISTS; LLM02-10 needed
│       └── diagrams/             ← data_flow.mmd EXISTS
```

### 4.2 Feature Flags

```toml
[features]
default = ["heuristics"]

# Core detection (lightweight, no ML)
heuristics = []

# Full feature bundle
full = ["moderation-onnx", "pii-presidio", "telemetry-otlp", "storage-sqlite"]

# ML inference backends
moderation-onnx = ["ort"]              # ONNX Runtime (LlamaGuard 3, PromptGuard)
moderation-candle = ["candle-core"]    # Pure Rust inference via candle
moderation-remote = ["reqwest"]        # Remote moderation APIs

# PII detection
pii-presidio = ["reqwest"]             # Microsoft Presidio connector
pii-local = []                         # Regex/dictionary only (included in default)

# Injection detection
injection-classifier = ["ort"]         # ML-based injection detection
injection-heuristics = []              # Pattern-based (always available)

# Secret scanning
secrets-builtin = []                   # Built-in entropy + regex (default)

# Telemetry
telemetry-otlp = ["opentelemetry", "opentelemetry_sdk", "opentelemetry-otlp"]
telemetry-json = []                    # JSON Lines logging

# Storage backends
storage-redis = ["redis"]              # Distributed state (rate limits, sessions)
storage-sqlite = ["sqlx"]              # Local persistent state

# RAG integration
rag-security = []                      # RAG hardening features

# Agent security
agent-security = []                    # Agentic AI controls

# MCP security
mcp-security = []                      # MCP protocol-specific controls

# Testing
testing = []                           # Expose test harness APIs
adversarial-corpus = ["testing"]       # Include adversarial datasets
```

### 4.3 Defense-in-Depth Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│                            User Request                                  │
├──────────────────────────────────────────────────────────────────────────┤
│  Layer 1: SESSION & RATE LIMITING                                        │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                     │
│  │ Session Mgmt │→│ Rate Limits  │→│ Cost Budget  │                     │
│  └──────────────┘ └──────────────┘ └──────────────┘                     │
├──────────────────────────────────────────────────────────────────────────┤
│  Layer 2: INPUT VALIDATION                                               │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ Normalization│→│ Moderation   │→│ PII Scan     │→│ Injection    │    │
│  │ & Encoding   │ │ Classifier   │ │ & Mask       │ │ Detection    │    │
│  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘    │
├──────────────────────────────────────────────────────────────────────────┤
│  Layer 3: CONTEXT MANAGEMENT                                             │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ System Prompt│→│ Role         │→│ External     │→│ Honeytoken   │    │
│  │ Isolation    │ │ Boundaries   │ │ Segregation  │ │ Injection    │    │
│  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘    │
├──────────────────────────────────────────────────────────────────────────┤
│  Layer 4: TOOL / MCP / AGENT EXECUTION                                   │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ Capability   │→│ Delegation   │→│ Human-in-    │→│ Recursion    │    │
│  │ Access Ctrl  │ │ Tracking     │ │ the-Loop     │ │ Guard        │    │
│  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘    │
├──────────────────────────────────────────────────────────────────────────┤
│  Layer 5: OUTPUT VALIDATION                                              │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ Schema       │→│ Content      │→│ PII/Secret   │→│ Grounding    │    │
│  │ Validation   │ │ Sanitization │ │ Egress Scan  │ │ Check        │    │
│  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘    │
├──────────────────────────────────────────────────────────────────────────┤
│  Layer 6: AUDIT & OBSERVABILITY (cross-cutting)                          │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ Event        │→│ Anomaly      │→│ Provenance   │→│ Incident     │    │
│  │ Logging      │ │ Detection    │ │ Tracking     │ │ Response     │    │
│  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘    │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 5. Core Traits & Types

These types form the backbone of the pipeline system. The existing `SecurityStage` trait in `pipeline/mod.rs` will be evolved to this richer model:

```rust
/// Content being evaluated by the pipeline
#[derive(Debug, Clone)]
pub enum Content {
    /// Plain text (user prompt, assistant response)
    Text(String),
    /// Structured messages (role + content)
    Messages(Vec<Message>),
    /// Tool invocation request
    ToolCall { tool_name: String, arguments: serde_json::Value },
    /// Tool execution result
    ToolResult { tool_name: String, result: serde_json::Value },
    /// Retrieved RAG chunks with provenance
    RetrievedChunks(Vec<RetrievedChunk>),
    /// Multimodal content
    Multimodal { modality: Modality, data: bytes::Bytes },
}

/// Outcome from a guardrail stage evaluation
#[derive(Debug, Clone)]
pub enum StageOutcome {
    /// Allow the content to proceed unchanged
    Allow { confidence: f32 },
    /// Block the content entirely
    Block { reason: String, severity: Severity },
    /// Transform the content (PII masking, sanitization)
    Transform { content: Content, description: String },
    /// Escalate for human review
    Escalate { reason: String, timeout: Duration },
    /// Skip (stage not applicable)
    Skip { reason: String },
}

/// Trait for guardrail stages (evolution of existing SecurityStage)
#[async_trait]
pub trait GuardrailStage: Send + Sync {
    /// Unique identifier for this stage
    fn id(&self) -> &'static str;
    /// Evaluate the content against this guardrail
    async fn evaluate(&self, content: &Content, ctx: &SecurityContext)
        -> Result<StageOutcome, StageError>;
    /// Whether this stage can degrade gracefully on failure
    fn degradable(&self) -> bool { true }
    /// Priority for ordering (lower = earlier)
    fn priority(&self) -> u32 { 100 }
    /// Metrics labels for telemetry
    fn metrics_labels(&self) -> Vec<(&'static str, String)> { vec![] }
}

/// Security context passed through the pipeline
#[derive(Debug, Clone)]
pub struct SecurityContext {
    pub session_id: String,
    pub user_id: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub risk_score: f32,
    pub parent_context: Option<Arc<SecurityContext>>,  // Agent delegation chain
}

/// Severity levels for security events
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}
```

**Migration from existing types:** The current `SecurityStage` (takes `&str`, returns `StageResult`) will be preserved as a compatibility wrapper. `GuardrailStage` is the new primary trait with richer `Content` enum and `StageOutcome` types. A blanket impl will allow existing `SecurityStage` implementations to work with the new `PipelineExecutor`.

---

## 6. Module Deep Dive

### 6.1 `config` — Policy Schema & Configuration

**Current state:** Basic `SecurityPolicy` with `version`, `enabled`, `fail_mode`. `PolicyBuilder` supports YAML/TOML/JSON files and env vars.

**Expansion needed:**
- Per-module sub-configurations (`InputPolicyConfig`, `OutputPolicyConfig`, etc.)
- Layered override hierarchy (global → graph → node → request)
- JSON Schema export via `schemars` (already declared as dependency)
- Policy validation rules (cannot disable all stages without audit flag)
- Hot-reload support for runtime policy changes

**Key design decisions:**
- Configuration is **declarative** — users describe desired security posture, not implementation details
- Secure-by-default — all security features ON; explicit opt-out required
- Audit-logged overrides — disabling protections requires `SECURITY_AUDIT_OVERRIDE=true` env var

**Override Hierarchy** (later overrides win):
1. Compiled defaults (secure by default)
2. Global config file (`wg-bastion.toml`)
3. Environment variables (`WG_BASTION_INPUT_MODERATION_ENABLED=false`)
4. Graph-level overrides (via `GraphBuilder`)
5. Node-level overrides (via node configuration)
6. Request-level overrides (with audit logging)

### 6.2 `pipeline` — Guardrail Execution Engine

**Current state:** Sequential `SecurityPipeline` with `&str` input, short-circuit on failure.

**Expansion needed:**
- Evolve to `PipelineExecutor` with `Content` enum support
- Parallel execution for independent stages via `tokio::join!`
- `StageCache` (LRU with TTL using `dashmap`)
- `CircuitBreaker` (closed/open/half-open state machine)
- `FailMode` integration — honor open/closed/log-only from policy
- `PipelineResult` with metrics (latency per stage, cache hits, degraded stages)
- Benchmark harness for <5ms orchestration overhead

**Execution Flow:**
```
Input Content
     │
     ▼
┌─────────────────────────────────────────────────────────────┐
│  For each stage (sorted by priority):                        │
│    1. Check circuit breaker state                            │
│    2. Check cache for existing result                        │
│    3. If cache miss: execute stage                           │
│    4. Record metrics (latency, outcome)                      │
│    5. Update cache (if cacheable)                            │
│    6. If Block/Escalate: short-circuit remaining stages      │
│    7. If error + degradable: apply fallback, continue        │
└─────────────────────────────────────────────────────────────┘
     │
     ▼
Pipeline Result (final_outcome, stage_results, metrics, degraded_stages)
```

### 6.3 `prompt` — System Prompt Security

Addresses **LLM01:2025**, **LLM07:2025**.

- `SecureTemplate` — typed placeholders, max-length enforcement, auto-escaping, role markers
- `HoneytokenStore` — AES-256-GCM encrypted storage (via `ring`), rotation, detection
- `TemplateScanner` — regex + entropy detection for secrets (AWS, GCP, OpenAI keys, JWTs, private keys)
- `RefusalPolicy` — Block / Redact / SafeResponse / Escalate modes with configurable templates
- Role isolation via delimiter markers (`[SYSTEM_START]...[SYSTEM_END]`)

**Honeytoken Detection Flow:**
1. Honeytokens injected into system prompts during template rendering
2. All LLM outputs scanned for honeytoken presence
3. Detection triggers immediate alerting via `IncidentOrchestrator`
4. Session quarantined and audit log records exfiltration attempt

**Built-in Secret Patterns for TemplateScanner:**
| Pattern ID | Description | Example Match |
|------------|-------------|---------------|
| `aws-key` | AWS access key | `AKIA...` |
| `gcp-key` | GCP API key | `AIza...` |
| `openai-key` | OpenAI API key | `sk-...` |
| `anthropic-key` | Anthropic key | `sk-ant-...` |
| `jwt` | JWT tokens | `eyJ...` |
| `private-key` | RSA/EC private keys | `-----BEGIN...KEY-----` |
| `password-url` | Passwords in URLs | `://user:pass@` |
| `high-entropy` | High entropy strings | (entropy > 4.5) |

### 6.4 `input` — Input Validation Pipeline

Addresses **LLM01:2025**, **LLM02:2025**, **LLM04:2025**.

- `ModerationStage` — backends: ONNX (LlamaGuard 3), candle, remote API, heuristic fallback
- `PIIStage` — local regex (SSN, CC, email, phone) + optional Presidio connector, actions: Block/Redact/Hash/Anonymize
- `InjectionStage` — ensemble detection: heuristic patterns + structural analysis + ML classifier + spotlighting for RAG content
- `NormalizationStage` — HTML sanitization (lol_html streaming), Unicode NFKC, control char stripping, MIME validation, truncation
- `MultimodalStage` — OCR-based text extraction from images for injection detection

**Ensemble Scoring Strategies:**
```rust
pub enum EnsembleStrategy {
    /// Any detector above threshold triggers block
    AnyAboveThreshold,
    /// Weighted average of all detector scores
    WeightedAverage { weights: HashMap<String, f32> },
    /// Majority vote among detectors
    MajorityVote,
    /// Max score from any detector
    MaxScore,
}
```

**Pre-built Pipeline Configurations:**
```rust
// Standard user prompt pipeline
InputPipeline::user_prompt_pipeline(&policy)
    // → RateLimit → Moderation → PII → Injection → Normalization

// Retrieved RAG content pipeline
InputPipeline::retrieved_content_pipeline(&policy)
    // → Normalization → Injection(spotlighting) → PII → Provenance

// Multimodal input pipeline
InputPipeline::multimodal_pipeline(&policy)
    // → RateLimit → Multimodal → Moderation
```

### 6.5 `output` — Output Validation Pipeline

Addresses **LLM02:2025**, **LLM05:2025**, **LLM09:2025**.

- `SchemaValidator` — DSL + JSON Schema import for structured output enforcement
- `Sanitizer` — HTML via `ammonia`, terminal escape stripping, Markdown safe conversion
- `CodeGuard` — code block detection, language filtering, shell command blocking
- `EgressScanner` — secret pattern detection + honeytoken detection (immediate incident trigger)
- `GroundingValidator` — claim extraction, evidence matching against retrieved chunks, citation generation

### 6.6 `tools` — Tool/MCP Security

Addresses **LLM06:2025**, MCP spec 2025-03-26.

- `ToolPolicy` — YAML/JSON allowlists, per-tool rate limits, risk scoring, approval requirements
- `ExecutionGuard` — wraps tool invocations with pre/post checks, audit logging, idempotency keys
- `MCPSecurityLayer` — OAuth 2.1 token validation, session binding, confused deputy prevention, scope minimization
- `FetchSanitizer` — URL validation (allowlist/blocklist), content sanitization, redirect limiting, size limits
- `ApprovalFlow` — request/approve/deny/timeout state machine, notification integration
- `CapabilityAccess` — per-tool capability tokens (not just session-level auth)

### 6.7 `rag` — RAG Security

Addresses **LLM01:2025** (indirect injection), **LLM08:2025**.

- `SanitizedIngestion` — wraps wg-ragsmith's `SemanticChunkingService` with domain allowlisting, hash verification, PII scanning
- `ProvenanceTagger` — attaches metadata (domain hash, timestamp, injection score) to chunks
- `GroundedRails` — verifies answer claims have supporting citations; cross-encoder scoring
- `EmbeddingSecurity` — access controls on vector stores, tenant isolation, inversion detection heuristics
- `CorpusScanner` — CLI tool for PII/secret scanning across stored corpus

### 6.8 `agents` — Agentic AI Security

Addresses OWASP Agentic AI risks.

- `DelegationTracker` — records parent→child delegation chains, enforces depth limits, capability propagation rules
- `AutonomyBoundaries` — action limits, cost/time budgets, kill switch implementation
- `AgentMemoryProtection` — update validation, entry signing (HMAC), poisoning detection
- `InterAgentSecurity` — message authentication between agents, encrypted channels, schema validation

### 6.9 `session` — Session Management

- `SecurityContext` — enhanced context with parent chain for delegation tracking
- `SessionIsolation` — prevents cross-session data leakage
- `SessionTokens` — cryptographically secure session ID generation (UUID v7 or JWT)

### 6.10 `abuse` — Abuse Prevention

Addresses **LLM10:2025**.

- `RateLimiter` — `governor` crate, multi-dimensional (per-user, per-session, per-tool, global)
- `RecursionGuard` — depth tracking, cycle detection, iteration limits with actionable errors
- `CostMonitor` — token counting (tiktoken-rs), budget enforcement, alert thresholds
- `CircuitBreaker` — Tower-compatible state machine for external service resilience

### 6.11 `telemetry` — Observability

Addresses NIST AI RMF Measure/Manage.

- `SecurityEvent` — structured event types with JSON serialization, weavegraph `EventBus` integration
- `OTLPExporter` — spans and metrics via `opentelemetry` crate stack
- `IncidentOrchestrator` — trigger logic for quarantine, token revocation, notification
- `AuditLog` — encrypted JSONL export, configurable retention, DSAR support
- `DashboardTemplates` — Grafana JSON for injection metrics, cost tracking, honeytoken hits

### 6.12 `testing` — Security Testing

- `AdversarialCorpus` — curated attack datasets (OWASP examples, Rebuff patterns, custom honeytokens)
- `AttackHarness` — pipeline testing with coverage metrics, CI integration
- `RegressionSuites` — ordering, fallback, degrade mode tests

### 6.13 `supply_chain` — Supply Chain Security

Addresses **LLM03:2025**.

- `SbomGenerator` — CycloneDX output in CI
- `AibomGenerator` — AI Bill of Materials (model IDs, checksums, provenance)
- `AuditRunner` — wrappers for `cargo audit`, `cargo deny`, license scanning

---

## 7. Weavegraph Integration Architecture

Based on analysis of weavegraph's `App`, `AppRunner`, `Scheduler`, `NodeContext`, `EventBus`, and `ReducerRegistry`:

### 7.1 Integration Points

| Hook | Location in weavegraph | Mechanism |
|------|----------------------|-----------|
| **Graph build** | `GraphBuilder::compile()` | New `.with_security_policy(policy)` method adds `SecurityHandle` to `App` |
| **Pre-node** | `Scheduler::superstep()` task creation | Wrap `node.run()` with security pre-check |
| **Post-node** | `Scheduler::superstep()` result collection | Validate `NodePartial` output before barrier |
| **Barrier** | `App::apply_barrier()` | Inject validation between aggregation and reducer application |
| **Custom reducer** | `GraphBuilder::with_reducer()` | Security reducer validates/filters state updates |
| **EventBus sink** | `EventBus::add_sink()` | Custom `SecurityAuditSink` monitors all events |
| **NodeContext** | `NodeContext.event_emitter` | Decorated emitter intercepts node communications |
| **Checkpointer** | `Checkpointer` trait | Security-aware checkpointer encrypts persisted state |

### 7.2 Integration Pattern

```rust
// Behind feature flag `security` in weavegraph:
let app = GraphBuilder::new()
    .add_node(NodeKind::Custom("llm".into()), llm_node)
    .add_edge(NodeKind::Start, NodeKind::Custom("llm".into()))
    .add_edge(NodeKind::Custom("llm".into()), NodeKind::End)
    .with_security_policy(policy)  // ← wg-bastion integration
    .compile()?;

// Security pipeline runs at:
// 1. pre_node: Input validation before node.run()
// 2. post_node: Output validation after node.run()
// 3. pre_tool: Tool security checks before tool execution
// 4. audit: SecurityAuditSink captures all events
```

### 7.3 Implementation Approach

The security integration is a **decorator/wrapper pattern** — `SecuredApp` wraps `App` and intercepts the execution lifecycle. This avoids modifying weavegraph's core code:

1. `SecuredApp::new(app, policy)` creates a security-enhanced app
2. Security pipelines constructed from the `SecurityPolicy`
3. The `Scheduler` task creation loop is wrapped to inject pre/post checks
4. A `SecurityAuditSink` is added to the `EventBus`
5. The `NodeContext` emitter is decorated for interception

### 7.4 Key weavegraph Types for Integration

```
App                    → wraps with SecuredApp
AppRunner              → wraps run_one_superstep() with pre/post checks
Scheduler::superstep() → task creation with security pre-check
Node::run()            → wrapped with security decorator
NodePartial            → validated in post-node check
NodeContext            → extended with .security_context()
EventBus               → SecurityAuditSink added
EventSink              → implement for structured security event capture
ReducerRegistry        → optional security reducer for state validation
Checkpointer           → optional encryption-aware wrapper
```

---

## 8. wg-ragsmith Integration Architecture

Based on analysis of wg-ragsmith's `SemanticChunkingService`, `EmbeddingProvider`, `SqliteChunkStore`, and ingestion pipeline:

### 8.1 Integration Points

| Hook | Location in wg-ragsmith | Mechanism |
|------|------------------------|-----------|
| **Ingestion** | `SemanticChunkingService::chunk_document()` | `SanitizedIngestion` wrapper validates before chunking |
| **Embedding** | `EmbeddingProvider::embed_batch()` | Decorator for rate limiting, cost tracking, API key protection |
| **Storage** | `SqliteChunkStore::add_chunks()` | Access control, encryption at rest, audit logging |
| **Retrieval** | `SqliteChunkStore.index().search()` | Permission-aware retrieval, tenant isolation |
| **Cache** | `DocumentCache`, `ResumeTracker` | Path traversal prevention, cache poisoning detection |

### 8.2 Integration Pattern

```rust
// Wrap wg-ragsmith service with security controls:
let secure_chunking = SanitizedIngestion::new(security_config)
    .with_domain_allowlist(vec!["docs.example.com", "wiki.internal"])
    .with_pii_scanner(pii_stage)
    .with_injection_scanner(injection_stage)
    .wrap(chunking_service);

let result = secure_chunking.chunk_document(request).await?;
// result.provenance contains domain hash, timestamp, scan results
```

### 8.3 Safety Considerations

- The `unsafe` block in `SqliteChunkStore::register_sqlite_vec()` (FFI `transmute`) needs security review
- `DocumentCache` disk writes need path traversal prevention
- `EmbeddingProvider::embed_batch()` needs cost tracking and API key rotation
- `ChunkTelemetry` may leak sensitive content in chunk previews — needs redaction

---

## 9. Workstreams & Execution Plan

> **Execution order rationale:** Prompt injection defense and system prompt hardening are the highest-priority deliverables. The execution order below front-loads _only the tasks required to unblock them_, then delivers full prompt/injection security, before proceeding to remaining workstreams.

### Phase 1: Pipeline Critical Path (Sprints 1–2) — _Unblock prompt & injection work_

> Ship the **minimum pipeline framework** needed by WS4 (Prompt Hardening) and WS5 (Injection Detection). Governance, config expansion, caching, and benchmarks are deferred to Phase 3.

#### WS2-FAST — Pipeline Framework (Critical Path Only)

**What exists:** `SecurityStage` trait (takes `&str`), `SecurityPipeline` (sequential), `SecurityContext`, `StageResult`.

**Tasks (dependency order):**

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS2-01 | `Content` enum (Text, Messages, ToolCall, ToolResult, RetrievedChunks, Multimodal) | None | All layers |
| WS2-02 | `StageOutcome` enum (Allow, Block, Transform, Escalate, Skip) | WS2-01 | All layers |
| WS2-03 | `GuardrailStage` trait with `Content` + `StageOutcome` | WS2-01, WS2-02 | All layers |
| WS2-04 | `PipelineExecutor` with parallel execution, metrics, FailMode integration | WS2-03 | LLM01–10 |
| WS2-07 | Backward-compat wrapper for existing `SecurityStage` impls | WS2-03 | Migration |

#### WS3-FAST — Build Hygiene (Parallel with WS2-FAST)

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS3-05 | Remove unused dependencies (`miette`, `jsonschema` if unused, `ring`, `zeroize` until needed) | None | Supply chain |
| WS3-06 | Add `#[cfg(feature)]` gates for all optional deps | None | Supply chain |

#### WS1-FAST — CI (Parallel with WS2-FAST)

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS1-04 | CI workflow for wg-bastion (`fmt`, `clippy`, `test`, `audit`, `deny`) | None | SSDF Produce |

**Phase 1 acceptance criteria:** `Content`, `StageOutcome`, `GuardrailStage`, `PipelineExecutor` all compile and pass unit tests. CI workflow green. Unused deps removed. Feature gates in place.

---

### Phase 2: Prompt & Injection Security (Sprints 3–5) — _Primary deliverable_

> Everything in this phase directly addresses **LLM01 (Prompt Injection)** and **LLM07 (System Prompt Leakage)**. This is the reason the execution order was reshuffled.

#### WS4 — Prompt Hardening

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS4-04 | `TemplateScanner` (regex + entropy detection for 10+ secret types) | None | LLM02, LLM07 |
| WS4-01 | `SecureTemplate` with typed placeholders, max-length, auto-escaping | WS2-01 | LLM01, LLM07 |
| WS4-02 | Role isolation markers (`[SYSTEM_START]...[SYSTEM_END]`, delimiter detection) | WS4-01 | LLM01 |
| WS4-03 | `HoneytokenStore` with AES-256-GCM encryption (via `ring`), rotation, detection | WS2-01 | LLM07, LLM02 |
| WS4-05 | `RefusalPolicy` (Block/Redact/SafeResponse/Escalate) | WS2-02 | LLM01, LLM02 |
| WS4-06 | Fuzz tests for template injection (cargo-fuzz) | WS4-01 | LLM01 |

**Acceptance criteria:** Template injection attempts blocked, honeytoken leakage detected, scanner catches 10+ secret types, fuzz tests pass without panics.

#### WS5-INJ — Injection Detection Pipeline

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS5-01 | `NormalizationStage` (HTML via `lol_html`, Unicode NFKC, MIME validation) | WS2-03 | LLM01, LLM05 |
| WS5-07 | `InjectionStage` — heuristic detector (pattern library: 50+ patterns) | WS2-03 | LLM01 |
| WS5-08 | `InjectionStage` — structural analysis (topic shift, control chars, token anomalies) | WS5-07 | LLM01 |
| WS5-09 | `InjectionStage` — ensemble scoring (AnyAboveThreshold, WeightedAverage, MajorityVote) | WS5-07, WS5-08 | LLM01 |
| WS5-10 | `InjectionStage` — spotlighting for RAG content | WS5-07 | LLM01 |
| WS5-INJ-IT | Integration tests for injection + prompt stages with adversarial samples | WS4-06, WS5-09 | Validation |

**Phase 2 acceptance criteria:** >90% injection detection on adversarial test set, <5% false positive rate, template injection blocked, honeytokens detected on egress, fuzz tests clean. **Prompt injection & system prompt hardening COMPLETE at end of Phase 2.**

---

### Phase 3: Remaining Foundation (Sprints 6–8) — _Finish deferred foundation work_

> Now that the high-priority security modules are done, complete the governance, config, pipeline, and remaining input modules.

#### WS1-REST — Governance Completion

**What exists:** Threat model, control matrix, architecture doc, LLM01 attack playbook, data flow diagram.

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS1-01 | Complete attack playbooks (LLM02–LLM10) | None | All OWASP categories |
| WS1-02 | `SECURITY.md` vulnerability disclosure policy | None | SSDF Respond |
| WS1-03 | PR template with security checklist | None | SSDF Produce |
| WS1-05 | Integration guide (initial) | None | AI RMF Govern |

**Acceptance criteria:** All 10 attack playbooks exist, PR template enforced.

#### WS2-REST — Pipeline Framework (Remaining)

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS2-05 | `StageCache` (LRU + TTL via `dashmap`) | WS2-04 | Performance |
| WS2-06 | `CircuitBreaker` state machine (closed/open/half-open) | WS2-04 | Resilience |
| WS2-08 | Pipeline benchmarks (target: <5ms orchestration overhead) | WS2-04 | Performance |

**Acceptance criteria:** Caching works, circuit breaker state transitions correct, benchmark proves <5ms overhead.

#### WS3-REST — Config Module Expansion

**What exists:** `SecurityPolicy` (3 fields), `PolicyBuilder`, `FailMode`.

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS3-01 | Per-module sub-configs (`InputPolicyConfig`, `OutputPolicyConfig`, etc.) | None | All |
| WS3-02 | Layered override hierarchy (global → graph → node → request) | WS3-01 | LLM09 |
| WS3-03 | JSON Schema export via `schemars` | WS3-01 | AI RMF Govern |
| WS3-04 | Validation rules (cannot disable all stages, production restrictions) | WS3-01 | LLM09 |

**Acceptance criteria:** Full policy schema with per-module sections, JSON Schema exports correctly, validation catches insecure configurations.

#### WS5-REST — Input Validation (Remaining)

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS5-02 | `ModerationStage` — heuristic backend (always available) | WS2-03 | LLM01, LLM04 |
| WS5-03 | `ModerationStage` — ONNX backend (LlamaGuard 3, feature-gated) | WS5-02 | LLM01, LLM04 |
| WS5-04 | `ModerationStage` — candle backend (pure Rust, feature-gated) | WS5-02 | LLM01, LLM04 |
| WS5-05 | `PIIStage` — local regex patterns (SSN, CC, email, phone, IP) | WS2-03 | LLM02 |
| WS5-06 | `PIIStage` — Presidio HTTP connector (feature-gated) | WS5-05 | LLM02 |
| WS5-11 | `MultimodalStage` — image text extraction + injection detection | WS5-07 | LLM01 |
| WS5-12 | `InputPipeline` pre-built configurations (user prompt, RAG content, multimodal) | WS5-01–WS5-10 | All input |
| WS5-13 | Integration tests with adversarial samples (full suite) | WS5-12 | Validation |

**Acceptance criteria:** Full input pipeline operational, <50ms total pipeline latency, degradation to heuristics when ML unavailable, >90% detection on full adversarial set.

---

### Phase 4: Output & Tool Security (Sprints 9–11)

#### WS6 — Output Validation

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS6-01 | `SchemaValidator` (custom DSL + `jsonschema` import) | WS2-02 | LLM05 |
| WS6-02 | `Sanitizer` (HTML via `ammonia`, terminal escapes, Markdown safe conversion) | WS2-03 | LLM05 |
| WS6-03 | `CodeGuard` (code block detection, language filtering, shell blocking) | WS2-03 | LLM05, LLM06 |
| WS6-04 | `EgressScanner` (secret patterns + honeytoken detection → incident trigger) | WS4-03, WS4-04 | LLM02, LLM07 |
| WS6-05 | `GroundingValidator` (claim extraction, evidence matching, citation generation) | WS2-03 | LLM09 |

**Acceptance criteria:** XSS/ANSI payloads blocked, code blocks detected, honeytokens in output trigger immediate incident, grounding validation matches claims to sources.

#### WS7 — Tool/MCP Security

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS7-01 | `ToolPolicy` schema (YAML/JSON, allowlists, risk scores, approval requirements) | WS3-01 | LLM06 |
| WS7-02 | `ExecutionGuard` wrapper (pre/post checks, audit logging, idempotency keys) | WS7-01 | LLM06 |
| WS7-03 | `MCPSecurityLayer` (OAuth 2.1 validation, session binding, scope minimization) | WS7-01 | LLM06, MCP spec |
| WS7-04 | `FetchSanitizer` (URL allowlist/blocklist, content sanitization, redirect limiting) | WS5-01 | LLM06 |
| WS7-05 | `ApprovalFlow` state machine (request/approve/deny/timeout) | WS7-02 | LLM06, Agentic AI |
| WS7-06 | `CapabilityAccess` (per-tool capability tokens, not just session-level) | WS7-01 | LLM06, Agentic AI |

**Acceptance criteria:** Confused deputy attacks blocked, tool calls audited, approval flow works with timeout, capabilities enforced per-tool.

#### WS8 — Weavegraph Integration

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS8-01 | Feature flag `security` in weavegraph `Cargo.toml` | None | Integration |
| WS8-02 | `SecuredApp` wrapper pattern (decorator over `App`) | WS2-04 | Integration |
| WS8-03 | Pre/post node hooks in `Scheduler::superstep()` | WS8-02 | All layers |
| WS8-04 | `SecurityAuditSink` implementing `EventSink` trait | WS8-02 | Telemetry |
| WS8-05 | `SecurityContext` on `NodeContext` | WS8-02 | Session |
| WS8-06 | Example: `examples/secured_graph.rs` | WS8-01–WS8-05 | DX |
| WS8-07 | Integration test suite | WS8-06 | Validation |

**Acceptance criteria:** Security hooks fire at correct lifecycle points, blocked requests short-circuit graph execution, audit sink captures events, example runs end-to-end.

### Phase 5: RAG & Agent Security (Sprints 12–14)

#### WS9 — RAG Hardening

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS9-01 | `SanitizedIngestion` wrapping wg-ragsmith's `SemanticChunkingService` | WS5-01, WS5-07 | LLM01, LLM04 |
| WS9-02 | `ProvenanceTagger` (domain hash, timestamp, injection score metadata) | WS9-01 | LLM08 |
| WS9-03 | `EmbeddingSecurity` (access controls, tenant isolation, inversion detection) | WS2-03 | LLM08 |
| WS9-04 | `GroundedRails` (claim-to-source matching, citation injection) | WS6-05 | LLM09 |
| WS9-05 | `CorpusScanner` CLI (PII/secret scanning across stored corpus) | WS5-05, WS4-04 | LLM02 |

**Acceptance criteria:** wg-ragsmith integration works, provenance survives retrieval, tenant isolation prevents cross-context leakage, corpus scanner produces actionable reports.

#### WS10 — Agentic Security

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS10-01 | `DelegationTracker` (chain recording, depth limits, capability propagation) | WS2-03 | Agentic AI |
| WS10-02 | `AutonomyBoundaries` (action limits, cost/time budgets, kill switch) | WS10-01 | Agentic AI |
| WS10-03 | `AgentMemoryProtection` (update validation, HMAC entry signing, poisoning detection) | WS2-03 | Agentic AI |
| WS10-04 | `InterAgentSecurity` (message authentication, schema validation) | WS10-01 | Agentic AI |

**Acceptance criteria:** Delegation chains tracked with depth limits, kill switch terminates immediately, memory updates validated and signed, inter-agent messages authenticated.

### Phase 6: Abuse, Telemetry & Testing (Sprints 15–18)

#### WS11 — Abuse Prevention

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS11-01 | `RateLimiter` (governor crate, per-user/session/tool/global) | WS2-03 | LLM10 |
| WS11-02 | `RecursionGuard` (depth tracking, cycle detection, iteration limits) | WS2-03 | LLM10 |
| WS11-03 | `CostMonitor` (token counting via tiktoken-rs, budget enforcement, alerts) | WS2-03 | LLM10 |
| WS11-04 | `CircuitBreaker` (Tower-compatible, state machine, recovery) | WS2-06 | Resilience |
| WS11-05 | Optional Redis persistence for distributed rate limiting | WS11-01 | LLM10 |

**Acceptance criteria:** Rate limiting accurate across dimensions, recursion caught, budgets enforced, circuit breaker recovers automatically.

#### WS12 — Telemetry & Incident Response

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS12-01 | `SecurityEvent` types with JSON serialization | WS2-02 | AI RMF Measure |
| WS12-02 | EventBus integration (`SecurityAuditSink` via weavegraph's `EventSink`) | WS8-04 | AI RMF Measure |
| WS12-03 | `OTLPExporter` (spans + metrics via opentelemetry, feature-gated) | WS12-01 | AI RMF Measure |
| WS12-04 | `IncidentOrchestrator` (trigger logic, quarantine, notification) | WS12-01 | AI RMF Manage |
| WS12-05 | `AuditLog` (encrypted JSONL, retention policies, DSAR export) | WS12-01 | EU AI Act, GDPR |
| WS12-06 | Grafana dashboard templates (JSON exports) | WS12-03 | Observability |

**Acceptance criteria:** Events serialize correctly, OTLP export works, incidents trigger quarantine, audit logs encrypted, dashboards import cleanly.

#### WS13 — Testing Infrastructure

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS13-01 | `AdversarialCorpus` (curated dataset, categorized, licensed) | WS5-12 | Continuous assurance |
| WS13-02 | `AttackHarness` (pipeline testing, detection/false-positive metrics) | WS13-01 | Continuous assurance |
| WS13-03 | `RegressionSuites` (ordering, fallback, degrade, edge cases) | WS2-04 | Continuous assurance |
| WS13-04 | Red team playbook (`docs/red_team_playbook.md`) | WS13-01 | AI RMF Measure |
| WS13-05 | Nightly CI job for adversarial testing | WS13-02 | Continuous assurance |
| WS13-06 | Property-based tests (proptest) for pipeline, config, stages | WS2-04 | Correctness |

**Acceptance criteria:** Corpus covers all 10 OWASP categories, harness runs in CI, >95% detection rate on corpus, regression tests block on failure.

### Phase 7: Supply Chain, DX & Release (Sprints 19–20)

#### WS14 — Supply Chain Hygiene

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS14-01 | CycloneDX SBOM generation in CI | None | LLM03 |
| WS14-02 | `AibomGenerator` (model IDs, checksums, provenance) | None | LLM03 |
| WS14-03 | `cargo audit` + `cargo deny` CI integration | None | LLM03 |
| WS14-04 | Signed releases (git tags, Cosign for containers) | None | LLM03 |

#### WS15 — Developer Experience

| Task | Deliverable | Dependency | Threat Coverage |
|------|------------|------------|-----------------|
| WS15-01 | Integration guide (`docs/integration_guide.md`) | All modules | DX |
| WS15-02 | Examples: `basic_guardrails.rs`, `rag_secured.rs`, `mcp_secured.rs`, `agentic_boundaries.rs`, `full_pipeline.rs` | All modules | DX |
| WS15-03 | CLI commands: `init-policy`, `scan-templates`, `scan-corpus`, `rotate-honeytokens`, `run-attack-suite` | All modules | DX |
| WS15-04 | FAQ & troubleshooting guide | All modules | DX |
| WS15-05 | Performance tuning guide (latency budgets, feature flag selection) | All modules | DX |

---

## 10. Sprint Roadmap

### Timeline (20 sprints, 2-week cadence, ~10 months)

> **Priority:** Prompt injection defense and system prompt hardening are fully delivered by end of Sprint 5 (Week 10). Everything else follows.

```
Phase 1: Pipeline Critical Path (unblock prompt/injection work)
  Sprint 1    (Weeks 1-2):   WS2-FAST (Content, StageOutcome, GuardrailStage) + WS1-FAST (CI) + WS3-FAST (dep cleanup)
  Sprint 2    (Weeks 3-4):   WS2-FAST cont. (PipelineExecutor, backward-compat wrapper)

Phase 2: Prompt & Injection Security ★ HIGH PRIORITY ★
  Sprint 3    (Weeks 5-6):   WS4 (SecureTemplate, role isolation, TemplateScanner, HoneytokenStore)
  Sprint 4    (Weeks 7-8):   WS4 cont. (RefusalPolicy, fuzz tests) + WS5-INJ (Normalization, Injection heuristic)
  Sprint 5    (Weeks 9-10):  WS5-INJ cont. (structural, ensemble, spotlighting, integration tests)
  ── ★ PROMPT INJECTION & SYSTEM PROMPT HARDENING COMPLETE ★ ──

Phase 3: Remaining Foundation
  Sprint 6    (Weeks 11-12): WS1-REST (playbooks, SECURITY.md) + WS2-REST (cache, circuit breaker, benchmarks)
  Sprint 7    (Weeks 13-14): WS3-REST (config sub-configs, schema export, validation)
  Sprint 8    (Weeks 15-16): WS5-REST (moderation, PII, multimodal, full InputPipeline)

Phase 4: Output & Tool Security
  Sprint 9    (Weeks 17-18): WS6 (output validation) + WS7 start (tool policy)
  Sprint 10   (Weeks 19-20): WS7 cont. (MCP, approval, capabilities) + WS8 start (weavegraph integration)
  Sprint 11   (Weeks 21-22): WS8 cont. (hooks, audit sink, example, tests)

Phase 5: RAG & Agent Security
  Sprint 12   (Weeks 23-24): WS9 (RAG hardening)
  Sprint 13   (Weeks 25-26): WS10 (agentic security — delegation, boundaries)
  Sprint 14   (Weeks 27-28): WS10 cont. (memory protection, inter-agent auth)

Phase 6: Abuse, Telemetry & Testing
  Sprint 15   (Weeks 29-30): WS11 (abuse prevention)
  Sprint 16   (Weeks 31-32): WS12 (telemetry, incident response)
  Sprint 17   (Weeks 33-34): WS13 (testing infrastructure, adversarial corpus)
  Sprint 18   (Weeks 35-36): WS13 cont. (red team playbook, nightly CI, property tests)

Phase 7: Supply Chain, DX & Release
  Sprint 19   (Weeks 37-38): WS14 (supply chain) + WS15 (DX: guide, examples, CLI)
  Sprint 20   (Weeks 39-40): Hardening, performance tuning, release prep → v0.1.0
```

### Sprint Deliverables

| Sprint | Focus | Key Deliverables | Gate |
|--------|-------|------------------|------|
| 1 | **Pipeline foundation** | `Content` enum, `StageOutcome`, `GuardrailStage` trait, CI workflow, dep cleanup, feature gates | CI green, types compile |
| 2 | **Pipeline executor** | `PipelineExecutor`, backward-compat wrapper | Executor passes unit tests |
| 3 | **★ Prompt hardening** | `SecureTemplate`, role isolation, `TemplateScanner`, `HoneytokenStore` | Template injection blocked |
| 4 | **★ Prompt + injection** | `RefusalPolicy`, fuzz tests, `NormalizationStage`, `InjectionStage` (heuristic) | Leakage detected, 50+ patterns |
| 5 | **★ Injection complete** | Injection structural + ensemble + spotlighting, integration tests | >90% detection, <5% FP |
| 6 | Governance + pipeline | Attack playbooks, `SECURITY.md`, `StageCache`, `CircuitBreaker`, benchmarks | Playbooks done, <5ms overhead |
| 7 | Config expansion | Per-module configs, JSON Schema, validation rules | Schema exports, validates |
| 8 | Input: remaining | Moderation (heuristic/ONNX/candle), PII, multimodal, `InputPipeline` | Full input pipeline works |
| 9 | Output | Schema validator, sanitizer, code guard, egress scanner, grounding | XSS blocked |
| 10 | Tools + integration | Tool policy, MCP layer, `SecuredApp` start | Tools secured |
| 11 | Weavegraph integration | Hooks, audit sink, example, integration tests | Hooks verified |
| 12 | RAG | Sanitized ingestion, provenance, embedding security | RAG protected |
| 13 | Agents | Delegation, boundaries, memory protection | Kill switch works |
| 14 | Agents continued | Inter-agent security, integration tests | Auth works |
| 15 | Abuse | Rate limiter, recursion guard, cost monitor | Limits enforced |
| 16 | Telemetry | Events, OTLP, incident orchestrator, audit logs | Metrics export |
| 17 | Testing | Adversarial corpus, attack harness, regression suites | >95% detection |
| 18 | Testing continued | Red team playbook, property tests, nightly CI | CI nightly green |
| 19 | Supply chain + DX | SBOM, AIBOM, integration guide, examples, CLI | SBOM valid, examples run |
| 20 | Release | Final docs, CHANGELOG, version bump, release | v0.1.0 published |

---

## 11. External Dependencies

### Runtime Dependencies

| Crate | Version | Purpose | Feature Gate | Security Notes |
|-------|---------|---------|-------------|----------------|
| `thiserror` | 2.x | Error types | always | Widely used |
| `tracing` | 0.1.x | Structured logging | always | Async-safe |
| `tokio` | 1.x | Async runtime | always | Match weavegraph |
| `async-trait` | 0.1.x | Async trait support | always | Until Rust stable async traits |
| `serde` / `serde_json` | 1.x | Serialization | always | |
| `serde_yaml` | 0.9.x | YAML config loading | always | |
| `toml` | 0.8.x | TOML config loading | always | |
| `dotenvy` | 0.15.x | .env loading | always | |
| `validator` | 0.18.x | Struct validation | always | |
| `schemars` | 0.8.x | JSON Schema generation | always | |
| `dashmap` | 6.x | Concurrent cache | always | Lock-free reads |
| `bytes` | 1.x | Multimodal content | always | |
| `ring` | 0.17.x | AES-GCM, HMAC, PBKDF2 | always (for honeytokens) | Audited crypto |
| `zeroize` | 1.8.x | Secret memory clearing | always | Memory safety |
| `governor` | 0.8.x | Rate limiting | always | Token bucket / GCRA |
| `ammonia` | 4.x | HTML sanitization | always | Servo-backed |
| `regex` | 1.x | Pattern matching | always | For PII/injection heuristics |
| `ort` | 2.0.0-rc.11 | ONNX inference | `moderation-onnx` | LlamaGuard 3, PromptGuard |
| `candle-core` | 0.9.x | Pure Rust ML | `moderation-candle` | No C deps |
| `reqwest` | 0.12.x | HTTP client (rustls) | `moderation-remote`, `pii-presidio` | TLS by default |
| `redis` | 0.27.x | Distributed state | `storage-redis` | |
| `sqlx` | 0.8.x | SQLite/Postgres | `storage-sqlite` | |
| `opentelemetry` | 0.27.x | Telemetry | `telemetry-otlp` | Standard protocol |
| `opentelemetry_sdk` | 0.27.x | Telemetry SDK | `telemetry-otlp` | |
| `opentelemetry-otlp` | 0.27.x | OTLP exporter | `telemetry-otlp` | |
| `lol_html` | 2.x | Streaming HTML | always (normalization) | CloudFlare maintained |
| `jsonschema` | 0.18.x | Schema validation | always (output validator) | Draft 2020-12 |
| `tiktoken-rs` | 0.7.x | Token counting | always (cost monitor) | |
| `blake3` | 1.x | Fast hashing | always (provenance) | 4x faster than SHA-256 |

### Dev Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` (full, test-util) | Async test runtime |
| `tempfile` | Temp dirs for tests |
| `proptest` | Property-based testing |
| `criterion` | Benchmarks |
| `cargo-fuzz` | Fuzz testing (template, injection) |
| `httpmock` | Mock HTTP servers (Presidio, remote moderation) |
| `tracing-subscriber` | Test logging |

### External Services (Optional)

| Service | Purpose | Deployment |
|---------|---------|-----------|
| Microsoft Presidio | PII detection | Docker Compose (self-hosted) |
| Grafana + OTLP Collector | Observability | Docker Compose |
| Redis | Distributed rate limiting | Docker Compose |

---

## 12. Risk Register

| ID | Risk | Likelihood | Impact | Mitigation | Status |
|----|------|-----------|--------|-----------|--------|
| R01 | Prompt injection bypass | High | Critical | Multi-stage ensemble (heuristic + structural + ML), continuous adversarial testing | In Design |
| R02 | System prompt extraction | High | High | Role isolation, honeytokens, egress scanning, refusal policy | In Design |
| R03 | PII leakage in outputs | Medium | Critical | Input PII masking + output egress scanning, dual-layer protection | In Design |
| R04 | RAG corpus poisoning | Medium | High | Domain allowlisting, document signing, ingestion scanning, provenance tracking | In Design |
| R05 | MCP confused deputy | Medium | High | Per-tool capability tokens, OAuth 2.1 validation, session binding | In Design |
| R06 | Agent autonomy abuse | Medium | Critical | Delegation tracking, budget enforcement, kill switch, human-in-the-loop | In Design |
| R07 | Wallet-draining DoS | Medium | Medium | Multi-dimensional rate limiting, cost budgets, circuit breakers | In Design |
| R08 | Embedding inversion | Low | Medium | Access controls, tenant isolation, query rate limiting | In Design |
| R09 | Supply chain compromise | Low | Critical | SBOM, AIBOM, signed releases, `cargo deny`, `cargo audit` | In Design |
| R10 | Latency inflation | Medium | High | <50ms budget, caching, circuit breakers, benchmarks in CI | In Design |
| R11 | False positives blocking users | Medium | Medium | Tunable thresholds, shadow mode (LogOnly), attack harness FP tracking | In Design |
| R12 | External service downtime | Medium | Medium | Graceful degradation, heuristic fallbacks, circuit breakers | In Design |
| R13 | Model drift / staleness | Low | Medium | AIBOM checksums, version tracking, quarterly review cadence | In Design |
| R14 | Secret leakage in logs | Low | High | Mandatory redaction in logging macros, log scanning, retention policies | In Design |
| R15 | Team bandwidth constraints | High | Medium | Prioritized sprint roadmap, MVP first, defer non-critical modules | Active |

**Review cadence:** Bi-weekly during active development, monthly post-launch.

---

## 13. Rollout Strategy & Release Gates

### Phase Gates

| Phase | Gate Criteria | Blocking? |
|-------|-------------|-----------|
| **Phase 1** | CI green, pipeline benchmarks <5ms, config validates | Yes |
| **Phase 2** | >90% injection detection, <5% FP rate, P95 <50ms | Yes |
| **Phase 3** | XSS blocked, tool calls audited, weavegraph hook tests pass | Yes |
| **Phase 4** | RAG provenance works, agent kill switch tested, tenant isolation verified | Yes |
| **Phase 5** | >95% attack suite detection, OTLP export operational, audit logs encrypted | Yes |
| **Phase 6** | SBOM valid, examples run clean, integration guide reviewed | Yes |

### Release Milestones

| Version | Phase | Contents |
|---------|-------|----------|
| **v0.1.0-alpha** | After Phase 2 | Config, pipeline, prompt protection, basic input validation |
| **v0.1.0-beta** | After Phase 3 | + Output validation, tool security, weavegraph integration |
| **v0.1.0-rc** | After Phase 5 | + RAG, agents, abuse, telemetry, testing |
| **v0.1.0** | After Phase 6 | GA with full documentation, supply chain, signed release |

---

## 14. MVP Definition (v0.1.0-alpha.1) — Prompt Injection & System Prompt Hardening

The **minimum security-valuable release**, achievable by end of **Phase 2, Sprint 5 (Week 10)**. Prioritizes the two highest-risk LLM attack vectors.

### Included

- `pipeline` — `Content` enum, `StageOutcome`, `GuardrailStage` trait, `PipelineExecutor` (parallel, FailMode-aware), backward-compat wrapper
- `config` — Existing `SecurityPolicy`/`PolicyBuilder`/`FailMode` (Sprint 1 scaffold, pre-expansion)
- `prompt` — **Full module:** `SecureTemplate`, `HoneytokenStore` (AES-256-GCM), `TemplateScanner` (10+ secret patterns), `RefusalPolicy`, role isolation markers
- `input` — `NormalizationStage`, `InjectionStage` (heuristic 50+ patterns, structural analysis, ensemble scoring, RAG spotlighting)
- Integration tests with adversarial injection samples
- Fuzz tests for template injection
- CI workflow (`fmt`, `clippy`, `test`, `audit`, `deny`)

### Deferred to v0.1.0-alpha.2 (Phase 3, Sprint 8)

- Config expansion (sub-configs, schema export, validation rules)
- `StageCache`, `CircuitBreaker`, pipeline benchmarks
- Moderation stages (heuristic, ONNX, candle)
- PII detection
- Multimodal input validation
- Attack playbooks LLM02–10, `SECURITY.md`, PR template

### Deferred to v0.1.0-beta (Phase 4, Sprint 11)

- Output validation suite
- Tool/MCP security
- Weavegraph integration hooks

### Deferred to v0.1.0-rc (Phase 6, Sprint 18)

- RAG hardening
- Agent security
- Abuse prevention (rate limiter, cost monitor)
- Full telemetry (OTLP, dashboards)
- Testing infrastructure (adversarial corpus, attack harness)

### Deferred to v0.2.0+

- Python integration
- Advanced MCP security
- AIBOM generation
- Automated red teaming
- Inter-agent encryption

---

## 15. Release Checklist

Before each release:

- [ ] All tests pass (unit, integration, property)
- [ ] `cargo fmt --check` clean
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo doc --no-deps` builds without warnings
- [ ] `cargo audit` clean (no critical/high advisories)
- [ ] `cargo deny check` passes (licenses, duplicates, yanked)
- [ ] No unused dependencies (`cargo machete`)
- [ ] Documentation updated
- [ ] CHANGELOG updated
- [ ] Version bumped in Cargo.toml
- [ ] Attack suite run (>95% detection for RC/GA)
- [ ] Performance benchmarks run (P95 <100ms for full pipeline)
- [ ] SBOM generated (for RC/GA)
- [ ] Git tag signed
- [ ] Container images signed if applicable

---

## 16. Grounding Resources

### Security Frameworks & Standards

| Resource | URL | Used For |
|----------|-----|----------|
| OWASP LLM Top 10 (2025) | [genai.owasp.org/llm-top-10](https://genai.owasp.org/llm-top-10/) | Primary risk taxonomy |
| OWASP Agentic AI Security | [genai.owasp.org/initiatives/agentic-security-initiative](https://genai.owasp.org/initiatives/agentic-security-initiative/) | Agent threat modeling |
| NIST AI RMF 1.0 | [nist.gov/ai-rmf](https://www.nist.gov/artificial-intelligence/risk-management-framework) | Governance framework |
| NIST AI 600-1 GenAI Profile | [airc.nist.gov/Docs/1](https://airc.nist.gov/Docs/1) | GenAI-specific risks |
| NIST AI 100-2e2023 | [csrc.nist.gov/pubs/ai/100/2/e2023](https://csrc.nist.gov/pubs/ai/100/2/e2023/final) | Adversarial ML taxonomy |
| NIST SSDF SP 800-218A | [csrc.nist.gov/projects/ssdf](https://csrc.nist.gov/Projects/ssdf) | Secure development |
| EU AI Act | [eur-lex.europa.eu](https://eur-lex.europa.eu/eli/reg/2024/1689/oj) | Compliance requirements |
| MCP Specification | [modelcontextprotocol.io/specification/2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26/) | Tool-calling protocol security |
| MITRE ATLAS | [atlas.mitre.org](https://atlas.mitre.org/) | Threat modeling taxonomy |

### Architectural Patterns & References

| Pattern | Source | Applied In |
|---------|--------|-----------|
| Defense-in-depth for LLM applications | NVIDIA NeMo Guardrails (5 rail types) | Pipeline architecture |
| Colang-style programmable rails | [github.com/NVIDIA/NeMo-Guardrails](https://github.com/NVIDIA/NeMo-Guardrails) | Stage DSL design |
| Tower middleware composition | [docs.rs/tower](https://docs.rs/tower/latest/tower/) | `PipelineExecutor`, `CircuitBreaker` |
| Zero-trust LLM architecture | Google SAIF Framework | Input/output/tool validation |
| Capability-based access control | Principle of least privilege (NIST SP 800-53) | `tools.capability` |
| Circuit breaker pattern | Michael Nygard, "Release It!" | `pipeline.circuit`, `abuse.circuit` |
| Token bucket rate limiting | GCRA algorithm | `abuse.rate_limit` (governor) |
| Spotlighting for RAG | Microsoft Prompt Shields research | `input.injection` spotlighting |
| Honeytoken detection | Canary trap / honeypot tradition | `prompt.honeytoken` |
| Content-addressable provenance | Supply chain integrity (SLSA framework) | `rag.provenance` |
| Ensemble classification | ML ensemble methods (bagging, boosting, stacking) | `input.injection` ensemble |

### Security Testing References

| Resource | URL | Used For |
|----------|-----|----------|
| Garak (NVIDIA) | [github.com/NVIDIA/garak](https://github.com/NVIDIA/garak) | LLM vulnerability scanner patterns |
| PyRIT (Microsoft) | [github.com/Azure/PyRIT](https://github.com/Azure/PyRIT) | Automated red teaming methodology |
| promptfoo | [github.com/promptfoo/promptfoo](https://github.com/promptfoo/promptfoo) | LLM evaluation/red-teaming |
| Rebuff | [github.com/protectai/rebuff](https://github.com/protectai/rebuff) | Prompt injection framework |
| OWASP FinBot CTF | OWASP Agentic Security Initiative | Agent attack scenarios |

### Open-Source Safety Models

| Model | Provider | Use Case | Integration |
|-------|----------|----------|-------------|
| LlamaGuard 3 (8B) | Meta | Content safety + jailbreak detection | ONNX export → `ort` crate |
| PromptGuard | Meta | Prompt injection classification | ONNX export → `ort` crate |
| ShieldGemma | Google DeepMind | Content safety classification | `candle` inference |
| Aegis Guard | NVIDIA | Content safety + jailbreak | ONNX or candle |
| WildGuard | Allen AI | Adversarial prompt detection | ONNX or candle |

### Rust Library References

| Library | Docs | Used For |
|---------|------|----------|
| `ort` (ONNX Runtime) | [docs.rs/ort](https://docs.rs/ort/latest/ort/) | ML inference |
| `candle` (HuggingFace) | [github.com/huggingface/candle](https://github.com/huggingface/candle) | Pure Rust ML |
| `ring` | [docs.rs/ring](https://docs.rs/ring/latest/ring/) | Cryptography |
| `governor` | [docs.rs/governor](https://docs.rs/governor/latest/governor/) | Rate limiting |
| `ammonia` | [docs.rs/ammonia](https://docs.rs/ammonia/latest/ammonia/) | HTML sanitization |
| `lol_html` | [docs.rs/lol_html](https://docs.rs/lol_html/latest/lol_html/) | Streaming HTML parsing |
| `dashmap` | [docs.rs/dashmap](https://docs.rs/dashmap/latest/dashmap/) | Concurrent cache |
| `blake3` | [docs.rs/blake3](https://docs.rs/blake3/latest/blake3/) | Fast hashing |
| `tiktoken-rs` | [docs.rs/tiktoken-rs](https://docs.rs/tiktoken_rs/latest/tiktoken_rs/) | Token counting |
| `jsonschema` | [docs.rs/jsonschema](https://docs.rs/jsonschema/latest/jsonschema/) | Schema validation |
| `opentelemetry` | [docs.rs/opentelemetry](https://docs.rs/opentelemetry/latest/opentelemetry/) | Telemetry |

---

## 17. Appendices

### A. OWASP LLM:2025 → Module Mapping

| ID | Vulnerability | Primary Module(s) | Secondary Module(s) |
|----|--------------|-------------------|---------------------|
| LLM01 | Prompt Injection | `input.injection` | `prompt.isolation`, `input.normalization`, `rag.ingestion` |
| LLM02 | Sensitive Info Disclosure | `input.pii`, `output.egress` | `prompt.honeytoken`, `telemetry.audit` |
| LLM03 | Supply Chain | `supply_chain.sbom`, `supply_chain.aibom` | CI/CD hygiene |
| LLM04 | Data/Model Poisoning | `rag.ingestion`, `rag.provenance` | `input.moderation`, `supply_chain.aibom` |
| LLM05 | Improper Output | `output.schema`, `output.sanitizer` | `output.code_guard` |
| LLM06 | Excessive Agency | `tools.policy`, `tools.guard` | `tools.approval`, `agents.boundaries` |
| LLM07 | System Prompt Leakage | `prompt.honeytoken`, `prompt.template` | `prompt.scanner`, `output.egress` |
| LLM08 | Vector/Embedding | `rag.embedding`, `rag.access_control` | `rag.provenance` |
| LLM09 | Misinformation | `output.grounding` | `rag.grounding` |
| LLM10 | Unbounded Consumption | `abuse.rate_limit`, `abuse.cost` | `abuse.recursion`, `abuse.circuit` |

### B. NIST AI RMF Function → Module Mapping

| Function | Outcome | wg-bastion Control |
|----------|---------|-------------------|
| GOVERN-1 | Legal and regulatory requirements identified | `config.policy`, compliance documentation |
| GOVERN-2 | Accountability structures in place | PR template, security review checklist |
| MAP-1 | AI system risks contextualized | Threat model, risk register |
| MAP-2 | AI system categorized | Control matrix categorization |
| MEASURE-1 | Risks assessed with metrics | Attack harness, telemetry metrics |
| MEASURE-2 | AI system monitored | OTLP export, dashboards, anomaly detection |
| MANAGE-1 | Risks prioritized and treated | Incident orchestrator, policy enforcement |
| MANAGE-2 | Residual risks documented | Risk register, CHANGELOG |

### C. EU AI Act Article → Feature Mapping

| Article | Requirement | wg-bastion Feature |
|---------|-------------|-------------------|
| Article 9 | Risk management system | `config` module, risk register |
| Article 12 | Record-keeping | `telemetry.audit`, encrypted JSONL |
| Article 14 | Human oversight | `tools.approval`, `agents.boundaries` |
| Article 52 | Transparency obligations | `supply_chain.aibom`, content provenance |

### D. Performance Budget

| Component | P50 Target | P95 Target | Technique |
|-----------|-----------|-----------|-----------|
| Pipeline orchestration | 2ms | 5ms | Parallel stages, zero-copy |
| Heuristic detection | 1ms | 3ms | Pre-compiled regex, Aho-Corasick |
| ML classifier | 15ms | 30ms | ONNX int8 quantization, model pre-warm |
| Token counting | 0.5ms | 1ms | tiktoken-rs |
| Rate limit check | 0.1ms | 0.5ms | governor (lock-free) |
| Cache lookup | 0.1ms | 0.5ms | dashmap |
| Audit log write | 2ms | 5ms | Buffered async channel |
| **Full pipeline** | **20ms** | **45ms** | Short-circuit, parallel, cached |

### E. Feature Flag Decision Matrix

| Deployment | Recommended Flags | Notes |
|-----------|-------------------|-------|
| **Development** | `default` (heuristics only) | Fast builds, no ML deps |
| **Staging** | `default` + `moderation-onnx` | ML testing with real models |
| **Production (lightweight)** | `default` + `telemetry-otlp` | Heuristics + observability |
| **Production (full)** | `full` | All backends enabled |
| **CI testing** | `testing` + `adversarial-corpus` | Attack suite |

### F. Document History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 (wg-security_plan.md) | 2023 | Original plan — 12 workstreams, 8 modules |
| 2.0 (wg-bastion_plan_v2.md) | January 2026 | Full rewrite: OWASP 2025, NIST AI RMF, EU AI Act, MCP, agentic security, 13 workstreams |
| **3.0 (this document)** | **February 2026** | Reconciliation against actual codebase, MCP spec 2025-03-26, candle/burn ML backends, refined sprint roadmap based on implemented Sprint 1, added grounding resources, performance budgets, feature flag decision matrix, weavegraph/ragsmith integration architecture |

**Key differences from v2.0:**
- Documents actual codebase state (Sprint 1 complete) and builds on it
- Updates MCP spec reference from 2025-11-25 to 2025-03-26
- Adds `candle` as pure-Rust ML alternative alongside ONNX
- Adds `lol_html` for streaming HTML (more appropriate than html5ever for security use)
- Adds `blake3` for fast provenance hashing
- Adds `governor` for rate limiting (proven GCRA algorithm)
- Restructures sprint roadmap: 20 sprints (was 13) with realistic phasing
- Identifies and plans fixes for existing code issues (unused deps, missing feature gates)
- Adds weavegraph/ragsmith integration architecture based on actual codebase analysis
- Adds performance budget appendix with per-component targets
- Adds feature flag decision matrix for different deployment scenarios
- Adds comprehensive grounding resources section with URLs

---

*End of wg-bastion Master Plan v3.0*
