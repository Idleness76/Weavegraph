# `wg-bastion` Standalone Crate Master Plan v2.0

**Last Updated:** January 2026  
**Status:** Strategic Planning Document  
**Target Rust Version:** 1.89+ (Edition 2024)

`wg-bastion` is a sibling crate to `weavegraph` and `wg-ragsmith`, delivering a comprehensive, opt-in security suite for graph-driven LLM applications. This plan incorporates the latest security frameworks, threat intelligence, and industry best practices as of 2025-2026.

---

## Executive Summary

The AI security landscape has evolved dramatically since the original plan. This v2.0 update addresses:

- **OWASP LLM Top 10 2025** with new categories (System Prompt Leakage, Vector/Embedding Weaknesses)
- **NIST AI RMF 1.0 + AI 600-1** Generative AI Profile (July 2024)
- **EU AI Act** compliance requirements (effective 2025-2027)
- **Agentic AI security** patterns for autonomous multi-agent systems
- **MCP Protocol security** based on the 2025-11-25 specification
- **Modern guardrails architectures** (NeMo Guardrails, defense-in-depth patterns)

The crate provides defense-in-depth security for LLM applications with minimal performance overhead (<50ms per request) and graceful degradation when external services are unavailable.

---

## 0. Security Framework Anchors

### OWASP LLM Top 10 (2025 Edition)

The OWASP GenAI Security Project has matured significantly with 600+ contributing experts. The 2025 edition (released November 2024) introduces critical new categories:

| Risk ID | Vulnerability | Changes from 2023 |
|---------|--------------|-------------------|
| **LLM01:2025** | Prompt Injection | Expanded: multimodal injection, adversarial suffixes, multilingual/obfuscated attacks |
| **LLM02:2025** | Sensitive Information Disclosure | Broadened scope (previously "Insecure Output Handling") |
| **LLM03:2025** | Supply Chain | Enhanced focus on model repositories, malicious pickling, AIBOM |
| **LLM04:2025** | Data and Model Poisoning | Added backdoor/sleeper agent considerations |
| **LLM05:2025** | Improper Output Handling | Refined validation requirements |
| **LLM06:2025** | Excessive Agency | Critical for agentic systems, tool abuse patterns |
| **LLM07:2025** | System Prompt Leakage | **NEW** – dedicated category for template exposure |
| **LLM08:2025** | Vector and Embedding Weaknesses | **NEW** – RAG-specific security risks |
| **LLM09:2025** | Misinformation | Hallucination, factual accuracy, grounding requirements |
| **LLM10:2025** | Unbounded Consumption | Resource exhaustion, cost explosion, DoS patterns |

We map every module and task to specific OWASP LLM-XX:2025 identifiers for traceability.

### NIST AI Risk Management Framework

#### AI RMF 1.0 (January 2023)
Core framework with four governance functions:
- **GOVERN**: Establish AI risk management culture, policies, and accountability
- **MAP**: Identify and contextualize AI system risks within organizational scope
- **MEASURE**: Analyze, assess, and track AI risks with quantitative metrics
- **MANAGE**: Prioritize, respond to, and monitor AI risks continuously

#### AI 600-1: Generative AI Profile (July 2024)
Cross-sectoral profile pursuant to Executive Order 14110, addressing unique GenAI risks:
- Confabulation/hallucination mitigation
- Dangerous content generation controls
- Data privacy and integrity protections
- Homogenization and diversity concerns
- Environmental impact considerations
- Harmful bias detection and mitigation
- Information security hardening
- Intellectual property safeguards

#### AI 100-2e2023: Adversarial ML Taxonomy
Comprehensive attack/defense taxonomy:
- Data poisoning attacks and defenses
- Model evasion and robustness
- Model extraction prevention
- Inference attack mitigation

### NIST Secure Software Development Framework (SSDF) SP 800-218A
Updated for AI contexts with four practice areas:
- **Prepare**: Organization-level security preparation
- **Protect**: Software protection throughout development
- **Produce**: Secure software production practices
- **Respond**: Vulnerability response and disclosure

### EU AI Act (Regulation 2024/1689)

Risk-based classification with compliance timelines:

| Risk Level | Requirements | Effective Date |
|------------|-------------|----------------|
| **Unacceptable** | 8 prohibited practices (social scoring, harmful manipulation) | February 2025 |
| **High-Risk** | Conformity assessment, human oversight, documentation | August 2026-2027 |
| **Transparency** | Disclosure for chatbots, deepfakes, AI-generated content | August 2026 |
| **GPAI Models** | Transparency, copyright compliance, systemic risk assessment | August 2025 |

`wg-bastion` provides hooks for EU AI Act compliance but does not guarantee certification without customization.

### OWASP Agentic AI Top 10 (2026 Preview)

Released December 2025, addressing autonomous AI systems:
- Tool/function calling abuse patterns
- Multi-agent orchestration vulnerabilities
- Autonomous decision-making oversight
- Agent-to-agent communication security
- Memory and state persistence attacks
- Goal specification and constraint bypass

### Mapping Approach

For each workstream/task we capture:
- **OWASP LLM-XX:2025** risk identifiers
- **AI RMF** function outcomes (Govern/Map/Measure/Manage)
- **SSDF** practice references
- **EU AI Act** article mappings where applicable
- **MITRE ATLAS** technique identifiers for threat intelligence

A living control matrix (WS1-02 deliverable) maintains traceability as implementation evolves.

---

## 1. Threat Landscape & Actors (2025-2026)

The threat landscape for LLM applications has expanded significantly with the rise of agentic AI, MCP protocol adoption, and sophisticated multi-vector attacks.

### 1.1 Threat Actor Profiles

| Threat Actor | Profile | Motivations | Primary Attack Surfaces | Mitigations (modules) |
|--------------|---------|-------------|--------------------------|------------------------|
| **Malicious End-Users** | External users interacting via UI/API | Data exfiltration, guardrail bypass, prompt abuse, tool manipulation | User prompts, file uploads, tool requests, response rendering | `prompt`, `input`, `output`, `abuse`, `telemetry` |
| **Adversarial Retrievers** | Poisoned documents/web pages in RAG corpora | Indirect prompt injection, misinformation, credential harvesting | Ingestion pipeline, vector stores, context assembly | `input.normalization`, `rag.ingestion`, `rag.provenance`, `rag.grounding` |
| **MCP Confused Deputy** | Third-party MCP servers or compromised agents | Session hijacking, credential reuse, lateral movement | MCP tool calls, session tokens, authorization flows | `tools.mcp`, `tools.policy`, `session`, `telemetry.incident` |
| **Rogue Insiders** | Developers/operators with configuration access | Disable controls, leak system prompts, misuse honeytokens | Templates, config overrides, logs, release process | `config`, `prompt.scanners`, `telemetry.log_retention`, `supply_chain` |
| **Automated Adversaries** | Bots/scripts for high-volume attacks | Resource exhaustion, cost explosion, model extraction | Prompt API, tool execution, embedding endpoints | `abuse.rate_limiting`, `abuse.cost`, `abuse.circuit_breakers` |
| **Supply Chain Attackers** | Actors targeting dependencies/releases | Pipeline compromise, backdoor insertion, model poisoning | Crate dependencies, model weights, Docker images | `supply_chain`, `aibom`, WS11 tasks |
| **Multi-Agent Exploiters** | Attackers leveraging agent-to-agent trust | Privilege escalation via agent chains, trust boundary violations | Inter-agent communication, shared context, delegated actions | `agents`, `tools.delegation`, `session.isolation` |
| **Embedding Inverters** | Sophisticated attackers with ML expertise | Recover training data from embeddings, membership inference | Vector stores, embedding APIs, similarity search | `rag.embedding_security`, `rag.access_control` |

### 1.2 Attack Vector Evolution (2024-2025)

#### Prompt Injection Advances
- **Multimodal Injection**: Attacks embedded in images, audio, or structured data
- **Adversarial Suffixes**: Gradient-based token sequences that bypass filters
- **Multilingual Obfuscation**: Exploiting translation inconsistencies
- **Instruction Hierarchy Attacks**: Manipulating system/user/assistant role boundaries

#### MCP-Specific Threats (per MCP Spec 2025-11-25)
- **Confused Deputy Problem**: Static client IDs enabling consent cookie exploitation
- **Token Passthrough Anti-Pattern**: Accepting tokens not explicitly issued for the server
- **Session Hijacking**: Guessed session IDs or impersonation attacks
- **Local Server Compromise**: Malicious startup commands, DNS rebinding on localhost
- **Scope Creep**: Over-broad token scopes increasing blast radius

#### RAG/Vector Store Threats (NEW - LLM08:2025)
- **Unauthorized Embedding Access**: Inadequate access controls on vector stores
- **Cross-Context Leakage**: Multi-tenant data bleeding across retrieval boundaries
- **Embedding Inversion**: Recovering source documents from embedding vectors
- **Retrieval Poisoning**: Corrupted sources influencing model behavior
- **Federation Conflicts**: Contradictory data from multiple knowledge sources

#### Agentic AI Threats
- **Tool Chain Exploitation**: Chaining tool calls to achieve unauthorized outcomes
- **Autonomy Abuse**: Agents exceeding intended operational boundaries
- **Memory Poisoning**: Corrupting persistent agent state/memory
- **Goal Hijacking**: Manipulating agent objectives through context injection
- **Delegation Attacks**: Exploiting trust between cooperating agents

### 1.3 MITRE ATLAS Technique Mapping

We map threats to MITRE ATLAS (Adversarial Threat Landscape for AI Systems) techniques:

| Technique ID | Name | Relevant Modules |
|--------------|------|------------------|
| AML.T0051 | LLM Prompt Injection | `input.injection`, `prompt` |
| AML.T0054 | LLM Jailbreak | `input.moderation`, `output` |
| AML.T0057 | LLM Data Leakage | `output.egress`, `prompt.canaries` |
| AML.T0043 | Craft Adversarial Data | `rag.ingestion`, `input.normalization` |
| AML.T0048 | Embed Malware | `tools.fetch`, `rag.ingestion` |
| AML.T0040 | ML Model Inference API Access | `abuse.rate_limiting`, `session` |

Threat modeling (WS1-01) elaborates full attack trees with detection and response playbooks.

---

## 2. Vision, Scope, and Constraints

### 2.1 Vision Statement

Deliver an extensible, production-grade Rust security crate that provides defense-in-depth guardrails for weavegraph LLM applications. The crate protects against the OWASP LLM Top 10 2025 threats while maintaining developer ergonomics, minimal latency overhead, and alignment with NIST AI RMF governance expectations.

### 2.2 Scope

#### Core Security Capabilities

**Input Security Layer**
- Prompt injection detection (heuristic + ML-based)
- Content moderation and safety classification
- PII/secret detection and masking
- Input normalization and sanitization
- Encoding and format validation

**Context Management Layer**
- System prompt protection and isolation
- Role boundary enforcement (system/user/assistant)
- External content segregation markers
- Context window management and truncation
- Honeytoken insertion and detection

**Output Security Layer**
- Structured output schema validation
- Content moderation and filtering
- Sensitive data egress scanning
- HTML/terminal/code sanitization
- Fact-checking and grounding hooks

**Tool/MCP Security Layer**
- Capability-based access control
- Tool allowlisting and risk scoring
- Execution sandboxing primitives
- Human-in-the-loop approval workflows
- Session binding and token management
- MCP-specific security controls (per 2025-11-25 spec)

**RAG Security Layer**
- Permission-aware retrieval
- Data source validation and allowlisting
- Embedding access controls
- Provenance tracking and tagging
- Grounded answering validation
- Corpus scanning for PII/secrets

**Agentic Security Layer** (NEW)
- Multi-agent communication security
- Delegation chain tracking
- Autonomy boundaries and kill switches
- Agent memory/state protection
- Goal constraint enforcement

**Abuse Prevention Layer**
- Multi-dimensional rate limiting
- Token/cost budget enforcement
- Recursion and loop guards
- Circuit breakers for external services
- Anomaly detection hooks

**Observability Layer**
- Structured security event logging
- OTLP/metrics export
- Incident detection and response automation
- Audit trail with retention policies
- AIBOM (AI Bill of Materials) generation

**Testing & Validation Layer**
- Adversarial corpus management
- Red team attack harness
- Regression test suites
- Continuous validation pipelines

### 2.3 Non-Goals

- **Authentication/Authorization**: End-user identity management and access control (use dedicated auth libraries)
- **Billing/Metering**: Usage-based billing or subscription enforcement
- **Proprietary Integrations**: Vendor-specific integrations requiring paid licenses
- **Industry Certification**: Guaranteeing compliance for regulated industries (healthcare, finance) without customization
- **Model Training Security**: Protecting the model training pipeline itself (focus is on inference-time security)

### 2.4 Technical Constraints

| Constraint | Requirement | Rationale |
|------------|-------------|-----------|
| **MSRV** | Rust 1.89+ (Edition 2024) | Align with weavegraph workspace |
| **Async Runtime** | Tokio-compatible | Match weavegraph's async model |
| **GPU Requirement** | Optional (CPU-only must work) | Accessibility for all deployments |
| **Dependency Isolation** | Feature-gated heavy deps | Keep base weavegraph lightweight |
| **Performance** | <50ms added latency (default config) | Production viability |
| **Degradation** | Graceful fallback when services unavailable | Operational resilience |
| **Security Posture** | Code review, fuzzing, signed releases | SSDF compliance |

### 2.5 Integration Points with Weavegraph

```
┌─────────────────────────────────────────────────────────────────┐
│                        weavegraph Runtime                        │
├─────────────────────────────────────────────────────────────────┤
│  GraphBuilder                                                    │
│    └─► .with_security_policy(policy)  ◄── wg-bastion hook      │
│                                                                  │
│  App                                                             │
│    └─► invoke() / invoke_streaming()                             │
│          │                                                       │
│          ├─► pre_node_hook()  ◄── SecurityPipeline.pre_execute() │
│          ├─► Node::run()                                         │
│          └─► post_node_hook() ◄── SecurityPipeline.post_execute()│
│                                                                  │
│  NodeContext                                                     │
│    └─► .security_context()  ◄── Session-scoped security state   │
│                                                                  │
│  EventBus                                                        │
│    └─► SecurityEventSink  ◄── Audit logging and alerting        │
└─────────────────────────────────────────────────────────────────┘
```

### 2.6 Integration Points with wg-ragsmith

```
┌─────────────────────────────────────────────────────────────────┐
│                        wg-ragsmith Pipeline                      │
├─────────────────────────────────────────────────────────────────┤
│  Ingestion                                                       │
│    └─► SanitizedIngestion  ◄── wg-bastion validation layer     │
│                                                                  │
│  SemanticChunkingService                                         │
│    └─► .with_security_scanner()  ◄── PII/injection scanning     │
│                                                                  │
│  SqliteChunkStore                                                │
│    └─► .with_access_control()  ◄── Permission-aware retrieval   │
│                                                                  │
│  Retrieved Chunks                                                │
│    └─► ProvenanceTagger  ◄── Metadata for downstream gating     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. Architecture & Project Layout

### 3.1 Workspace Structure

```
Weavegraph/
├── Cargo.toml                    (workspace root)
├── weavegraph/                   (orchestration runtime)
├── wg-ragsmith/                  (RAG utilities)
├── wg-bastion/                  (this crate)
│   ├── Cargo.toml
│   ├── README.md
│   ├── src/
│   │   ├── lib.rs                (crate entry; feature gates, re-exports)
│   │   │
│   │   ├── config/               (policy schema + configuration)
│   │   │   ├── mod.rs
│   │   │   ├── policy.rs         (SecurityPolicy struct)
│   │   │   ├── builder.rs        (PolicyBuilder with layered overrides)
│   │   │   ├── validator.rs      (policy validation rules)
│   │   │   └── schema.rs         (JSON schema export)
│   │   │
│   │   ├── pipeline/             (guardrail execution engine)
│   │   │   ├── mod.rs
│   │   │   ├── stage.rs          (GuardrailStage trait)
│   │   │   ├── executor.rs       (PipelineExecutor orchestration)
│   │   │   ├── outcome.rs        (StageOutcome types)
│   │   │   ├── cache.rs          (result caching)
│   │   │   └── circuit.rs        (circuit breakers)
│   │   │
│   │   ├── prompt/               (system prompt security)
│   │   │   ├── mod.rs
│   │   │   ├── template.rs       (SecureTemplate engine)
│   │   │   ├── honeytoken.rs     (HoneytokenStore)
│   │   │   ├── scanner.rs        (TemplateScanner for secrets)
│   │   │   ├── isolation.rs      (role boundary enforcement)
│   │   │   └── refusal.rs        (RefusalPolicy)
│   │   │
│   │   ├── input/                (input validation pipeline)
│   │   │   ├── mod.rs
│   │   │   ├── moderation.rs     (ModerationStage)
│   │   │   ├── pii.rs            (PIIStage)
│   │   │   ├── injection.rs      (InjectionStage)
│   │   │   ├── normalization.rs  (NormalizationStage)
│   │   │   └── multimodal.rs     (image/audio validation)
│   │   │
│   │   ├── output/               (output validation pipeline)
│   │   │   ├── mod.rs
│   │   │   ├── schema.rs         (SchemaValidator)
│   │   │   ├── sanitizer.rs      (HTML/terminal sanitization)
│   │   │   ├── code_guard.rs     (code output controls)
│   │   │   ├── egress.rs         (EgressScanner)
│   │   │   └── grounding.rs      (fact-checking hooks)
│   │   │
│   │   ├── tools/                (tool/MCP security)
│   │   │   ├── mod.rs
│   │   │   ├── policy.rs         (ToolPolicy schema)
│   │   │   ├── guard.rs          (ExecutionGuard wrapper)
│   │   │   ├── mcp.rs            (MCP-specific controls)
│   │   │   ├── fetch.rs          (FetchSanitizer)
│   │   │   ├── approval.rs       (ApprovalFlow state machine)
│   │   │   └── capability.rs     (capability-based access)
│   │   │
│   │   ├── rag/                  (RAG security)
│   │   │   ├── mod.rs
│   │   │   ├── ingestion.rs      (SanitizedIngestion)
│   │   │   ├── provenance.rs     (ProvenanceTagger)
│   │   │   ├── grounding.rs      (GroundedRails)
│   │   │   ├── embedding.rs      (embedding security)
│   │   │   └── scanner.rs        (CorpusScanner)
│   │   │
│   │   ├── agents/               (NEW: agentic AI security)
│   │   │   ├── mod.rs
│   │   │   ├── delegation.rs     (delegation chain tracking)
│   │   │   ├── boundaries.rs     (autonomy boundaries)
│   │   │   ├── memory.rs         (agent memory protection)
│   │   │   └── communication.rs  (inter-agent security)
│   │   │
│   │   ├── session/              (NEW: session management)
│   │   │   ├── mod.rs
│   │   │   ├── context.rs        (SecurityContext)
│   │   │   ├── isolation.rs      (session isolation)
│   │   │   └── tokens.rs         (session token management)
│   │   │
│   │   ├── abuse/                (abuse prevention)
│   │   │   ├── mod.rs
│   │   │   ├── rate_limit.rs     (RateLimiter)
│   │   │   ├── recursion.rs      (RecursionGuard)
│   │   │   ├── cost.rs           (CostMonitor)
│   │   │   └── circuit.rs        (CircuitBreaker)
│   │   │
│   │   ├── telemetry/            (observability)
│   │   │   ├── mod.rs
│   │   │   ├── events.rs         (SecurityEvent types)
│   │   │   ├── exporter.rs       (OTLP exporter)
│   │   │   ├── incident.rs       (IncidentOrchestrator)
│   │   │   ├── audit.rs          (audit log management)
│   │   │   └── dashboards.rs     (Grafana templates)
│   │   │
│   │   ├── testing/              (security testing)
│   │   │   ├── mod.rs
│   │   │   ├── corpus.rs         (AdversarialCorpus)
│   │   │   ├── harness.rs        (AttackHarness)
│   │   │   └── regression.rs     (RegressionSuites)
│   │   │
│   │   ├── supply_chain/         (supply chain security)
│   │   │   ├── mod.rs
│   │   │   ├── sbom.rs           (SBOM generation)
│   │   │   ├── aibom.rs          (NEW: AI Bill of Materials)
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
│   └── docs/
│       ├── architecture.md
│       ├── integration_guide.md
│       ├── red_team_playbook.md
│       └── threat_model.md
│
└── docs/
    └── wg-bastion_plan_v2.md    (this document)
```

### 3.2 Feature Flags

```toml
[features]
default = ["heuristics"]

# Core functionality
heuristics = []                    # Pattern-based detection (no ML)
full = ["moderation-onnx", "pii-presidio", "telemetry-otlp"]

# Moderation backends
moderation-onnx = ["ort"]          # Llama Guard via ONNX Runtime
moderation-remote = ["reqwest"]    # Remote moderation API

# PII detection backends  
pii-presidio = ["reqwest"]         # Microsoft Presidio connector
pii-local = []                     # Local regex/dictionary only

# Injection detection
injection-classifier = ["ort"]     # ML-based injection detection
injection-heuristics = []          # Pattern-based only (default)

# Secret scanning
secrets-trufflehog = []            # TruffleHog subprocess
secrets-builtin = []               # Built-in entropy + regex

# Telemetry backends
telemetry-otlp = ["opentelemetry", "opentelemetry-otlp"]
telemetry-json = []                # JSON Lines logging

# Storage backends (for rate limiting, sessions)
storage-redis = ["redis"]          # Redis for distributed state
storage-sqlite = ["sqlx"]          # SQLite for local state

# Testing utilities
testing = []                       # Expose test harness APIs
adversarial-corpus = ["testing"]   # Include adversarial datasets
```

### 3.3 Defense-in-Depth Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           User Request                                   │
├─────────────────────────────────────────────────────────────────────────┤
│  Layer 1: INPUT VALIDATION                                               │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐        │
│  │ Rate Limit  │→│ Moderation  │→│ PII Scan    │→│ Injection   │        │
│  │ & Session   │ │ Classifier  │ │ & Mask      │ │ Detection   │        │
│  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘        │
├─────────────────────────────────────────────────────────────────────────┤
│  Layer 2: CONTEXT MANAGEMENT                                             │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐        │
│  │ System      │→│ Role        │→│ External    │→│ Honeytoken  │        │
│  │ Prompt Iso  │ │ Boundaries  │ │ Segregation │ │ Injection   │        │
│  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘        │
├─────────────────────────────────────────────────────────────────────────┤
│  Layer 3: LLM INTERACTION                                                │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐                        │
│  │ Constrained │→│ Token       │→│ Cost        │                        │
│  │ Behavior    │ │ Budget      │ │ Monitoring  │                        │
│  └─────────────┘ └─────────────┘ └─────────────┘                        │
├─────────────────────────────────────────────────────────────────────────┤
│  Layer 4: TOOL / MCP EXECUTION                                           │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐        │
│  │ Capability  │→│ Sandboxed   │→│ Human-in-   │→│ Recursion   │        │
│  │ Access Ctrl │ │ Execution   │ │ the-Loop    │ │ Guard       │        │
│  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘        │
├─────────────────────────────────────────────────────────────────────────┤
│  Layer 5: OUTPUT VALIDATION                                              │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐        │
│  │ Schema      │→│ Content     │→│ PII/Secret  │→│ Grounding   │        │
│  │ Validation  │ │ Moderation  │ │ Egress Scan │ │ Check       │        │
│  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘        │
├─────────────────────────────────────────────────────────────────────────┤
│  Layer 6: AUDIT & OBSERVABILITY                                          │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐        │
│  │ Event       │→│ Anomaly     │→│ Provenance  │→│ Incident    │        │
│  │ Logging     │ │ Detection   │ │ Tracking    │ │ Response    │        │
│  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘        │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.4 Core Traits and Types

```rust
/// Outcome from a guardrail stage evaluation
#[derive(Debug, Clone)]
pub enum StageOutcome {
    /// Allow the content to proceed unchanged
    Allow,
    /// Allow with modifications applied
    Transform { 
        content: String, 
        modifications: Vec<Modification> 
    },
    /// Block the content with a reason
    Block { 
        reason: String, 
        severity: Severity,
        evidence: Option<Evidence>,
    },
    /// Escalate to human review or secondary check
    Escalate { 
        reason: String, 
        timeout: Duration,
        fallback: Box<StageOutcome>,
    },
    /// Degrade to fallback behavior (external service unavailable)
    Degrade { 
        reason: String, 
        fallback_result: Box<StageOutcome>,
    },
}

/// Trait for implementing guardrail stages
#[async_trait]
pub trait GuardrailStage: Send + Sync {
    /// Unique identifier for this stage
    fn id(&self) -> &'static str;
    
    /// Execute the stage with the given context
    async fn evaluate(
        &self,
        content: &Content,
        ctx: &SecurityContext,
    ) -> Result<StageOutcome, StageError>;
    
    /// Whether this stage can be skipped under degraded mode
    fn degradable(&self) -> bool { true }
    
    /// Priority for execution ordering (lower = earlier)
    fn priority(&self) -> u32 { 100 }
    
    /// Metrics labels for observability
    fn metrics_labels(&self) -> Vec<(&'static str, String)> { vec![] }
}

/// Security context passed through the pipeline
#[derive(Debug, Clone)]
pub struct SecurityContext {
    pub session_id: String,
    pub request_id: String,
    pub user_id: Option<String>,
    pub policy: Arc<SecurityPolicy>,
    pub metadata: HashMap<String, Value>,
    pub parent_context: Option<Arc<SecurityContext>>,  // For agent delegation
}

/// Content being evaluated
#[derive(Debug, Clone)]
pub enum Content {
    UserPrompt(String),
    SystemPrompt(String),
    AssistantResponse(String),
    ToolRequest { name: String, arguments: Value },
    ToolResponse { name: String, result: Value },
    RetrievedChunk { source: String, content: String, metadata: Value },
    Multimodal { modality: Modality, data: Bytes },
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

---

## 4. Module Deep Dive

### 4.1 `config` – Policy Schema & Configuration

**Responsibilities**: Define the `SecurityPolicy` schema, support layered configuration overrides, environment-based loading, and JSON schema export for validation tooling.

**Key Components**:

```rust
/// Top-level security policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    /// Policy version for migration support
    pub version: String,
    
    /// Global enable/disable (requires audit flag to disable)
    pub enabled: bool,
    
    /// Fail-open vs fail-closed behavior
    pub fail_mode: FailMode,
    
    /// Per-module configurations
    pub input: InputPolicyConfig,
    pub output: OutputPolicyConfig,
    pub prompt: PromptPolicyConfig,
    pub tools: ToolPolicyConfig,
    pub rag: RagPolicyConfig,
    pub agents: AgentPolicyConfig,
    pub abuse: AbusePolicyConfig,
    pub telemetry: TelemetryPolicyConfig,
}

/// Policy builder supporting layered overrides
pub struct PolicyBuilder {
    base: SecurityPolicy,
    overrides: Vec<PolicyOverride>,
}

impl PolicyBuilder {
    /// Load from default configuration
    pub fn new() -> Self;
    
    /// Apply file-based configuration
    pub fn with_file(self, path: impl AsRef<Path>) -> Result<Self, ConfigError>;
    
    /// Apply environment-based overrides (WG_SECURITY_*)
    pub fn with_env(self) -> Self;
    
    /// Apply runtime overrides
    pub fn with_override(self, key: &str, value: Value) -> Self;
    
    /// Apply graph-level overrides
    pub fn for_graph(self, graph_id: &str) -> Self;
    
    /// Apply node-level overrides
    pub fn for_node(self, node_id: &str) -> Self;
    
    /// Build and validate the final policy
    pub fn build(self) -> Result<SecurityPolicy, ValidationError>;
}
```

**Override Hierarchy** (later overrides win):
1. Compiled defaults (secure by default)
2. Global config file (`wg-bastion.toml`)
3. Environment variables (`WG_SECURITY_INPUT_MODERATION_ENABLED=false`)
4. Graph-level overrides (via `GraphBuilder`)
5. Node-level overrides (via node configuration)
6. Request-level overrides (with audit logging)

**Threat Coverage**:
- **LLM09:2025** (Misinformation via misconfiguration) – Prevents insecure defaults
- **AI RMF Govern** – Documents controls and enables policy auditing

**Validation Rules**:
- Cannot disable all input stages without `SECURITY_AUDIT_OVERRIDE=true`
- Cannot set fail_mode to `FailOpen` for production environments
- Must specify at least one moderation backend
- Rate limits cannot be set to unlimited without audit flag

---

### 4.2 `pipeline` – Guardrail Execution Engine

**Responsibilities**: Orchestrate guardrail stages in deterministic order, manage caching, implement circuit breakers, and expose telemetry hooks for observability.

**Key Components**:

```rust
/// Pipeline executor that orchestrates guardrail stages
pub struct PipelineExecutor {
    stages: Vec<Arc<dyn GuardrailStage>>,
    cache: StageCache,
    circuit_breakers: CircuitBreakerRegistry,
    metrics: PipelineMetrics,
}

impl PipelineExecutor {
    /// Execute all stages for the given content
    pub async fn execute(
        &self,
        content: Content,
        ctx: &SecurityContext,
    ) -> Result<PipelineResult, PipelineError>;
    
    /// Execute with custom stage selection
    pub async fn execute_stages(
        &self,
        content: Content,
        ctx: &SecurityContext,
        stage_ids: &[&str],
    ) -> Result<PipelineResult, PipelineError>;
}

/// Result from pipeline execution
#[derive(Debug)]
pub struct PipelineResult {
    pub final_outcome: StageOutcome,
    pub stage_results: Vec<StageResult>,
    pub metrics: ExecutionMetrics,
    pub cache_hits: usize,
    pub degraded_stages: Vec<String>,
}

/// Cache for stage results (LRU with TTL)
pub struct StageCache {
    inner: Arc<DashMap<CacheKey, CachedResult>>,
    config: CacheConfig,
}

/// Circuit breaker for external service calls
pub struct CircuitBreaker {
    state: AtomicU8,  // Closed, Open, HalfOpen
    failure_count: AtomicU32,
    last_failure: AtomicU64,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Check if the circuit allows a call
    pub fn allow(&self) -> bool;
    
    /// Record a successful call
    pub fn record_success(&self);
    
    /// Record a failed call
    pub fn record_failure(&self);
    
    /// Get current state for monitoring
    pub fn state(&self) -> CircuitState;
}
```

**Execution Flow**:

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
Pipeline Result
```

**Parallel Execution**: Independent stages (no data dependencies) execute concurrently using `tokio::join!` while maintaining deterministic result ordering.

**Threat Coverage**:
- **LLM01:2025** (Prompt Injection) – Ensures injection checks always run
- **LLM02:2025** (Sensitive Information) – Enforces egress scanning
- **LLM08:2025** (Excessive Agency) – Gates tool execution

**Circuit Breaker Configuration**:
```toml
[pipeline.circuit_breaker]
failure_threshold = 5        # Open after 5 consecutive failures
reset_timeout_secs = 30      # Try half-open after 30s
half_open_max_calls = 3      # Allow 3 test calls in half-open
```

---

### 4.3 `prompt` – System Prompt Security

**Responsibilities**: Protect system prompts from leakage and manipulation through secure templating, honeytoken injection, role isolation, and refusal policies.

**Key Components**:

#### SecureTemplate Engine

```rust
/// Secure template with typed placeholders and automatic escaping
pub struct SecureTemplate {
    template: String,
    placeholders: HashMap<String, PlaceholderConfig>,
    role: PromptRole,
}

#[derive(Debug, Clone)]
pub struct PlaceholderConfig {
    pub name: String,
    pub required: bool,
    pub validator: Option<Box<dyn Fn(&str) -> bool>>,
    pub sanitizer: Option<Box<dyn Fn(&str) -> String>>,
    pub max_length: Option<usize>,
}

impl SecureTemplate {
    /// Create template with role designation
    pub fn system(template: &str) -> Self;
    pub fn user(template: &str) -> Self;
    pub fn assistant(template: &str) -> Self;
    
    /// Add typed placeholder with validation
    pub fn with_placeholder(self, config: PlaceholderConfig) -> Self;
    
    /// Render template with values (validates and sanitizes)
    pub fn render(&self, values: &HashMap<String, String>) -> Result<String, TemplateError>;
    
    /// Inject honeytokens into rendered output
    pub fn with_honeytokens(self, store: &HoneytokenStore) -> Self;
}
```

**Role Isolation**:
- System prompts are tagged with `[SYSTEM]` markers that the LLM recognizes
- User content is wrapped with clear delimiters: `[USER_INPUT_START]...[USER_INPUT_END]`
- Assistant content is similarly delimited
- Cross-role content injection attempts are detected and blocked

#### Honeytoken Management

```rust
/// Encrypted store for honeytokens with rotation support
pub struct HoneytokenStore {
    tokens: Arc<RwLock<Vec<Honeytoken>>>,
    crypto: CryptoProvider,
    persistence: Option<Box<dyn TokenPersistence>>,
}

#[derive(Debug, Clone)]
pub struct Honeytoken {
    pub id: String,
    pub value: String,
    pub token_type: HoneytokenType,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub enum HoneytokenType {
    ApiKey,           // Fake API key format
    Credential,       // Fake username/password
    Url,              // Canary URL
    CustomMarker,     // Custom detection string
}

impl HoneytokenStore {
    /// Generate and store a new honeytoken
    pub fn generate(&self, token_type: HoneytokenType) -> Honeytoken;
    
    /// Rotate all tokens (generates new, marks old as expired)
    pub fn rotate_all(&self) -> Vec<Honeytoken>;
    
    /// Check if a string contains any honeytoken
    pub fn detect(&self, content: &str) -> Option<HoneytokenMatch>;
    
    /// Export tokens for external monitoring systems
    pub fn export_for_monitoring(&self) -> Vec<MonitoringToken>;
}
```

**Honeytoken Detection Flow**:
1. Honeytokens are injected into system prompts during template rendering
2. All LLM outputs are scanned for honeytoken presence
3. Detection triggers immediate alerting via `IncidentOrchestrator`
4. Session is quarantined and audit log records the exfiltration attempt

#### Template Scanner

```rust
/// Static analyzer for detecting secrets in templates
pub struct TemplateScanner {
    patterns: Vec<SecretPattern>,
    entropy_threshold: f64,
}

impl TemplateScanner {
    /// Scan template for potential secrets
    pub fn scan(&self, template: &str) -> Vec<SecretFinding>;
    
    /// Scan with context (file path, line numbers)
    pub fn scan_file(&self, path: &Path) -> Vec<SecretFinding>;
    
    /// Scan entire directory of templates
    pub fn scan_directory(&self, dir: &Path) -> Vec<SecretFinding>;
}

#[derive(Debug)]
pub struct SecretFinding {
    pub pattern_id: String,
    pub matched_text: String,
    pub location: Location,
    pub severity: Severity,
    pub recommendation: String,
}
```

**Built-in Patterns**:
- API keys (AWS, GCP, Azure, OpenAI, Anthropic, etc.)
- Passwords in connection strings
- Private keys (RSA, EC, Ed25519)
- JWT tokens
- High-entropy strings (potential secrets)
- URLs with embedded credentials

#### Refusal Policy

```rust
/// Policy for handling detected prompt leakage or injection
pub struct RefusalPolicy {
    pub mode: RefusalMode,
    pub fallback_response: String,
    pub audit_log: bool,
    pub notify_incident: bool,
}

#[derive(Debug, Clone)]
pub enum RefusalMode {
    /// Block entirely with generic error
    Block,
    /// Replace sensitive content with redacted markers
    Redact,
    /// Respond with predefined safe message
    SafeResponse,
    /// Escalate to human review
    Escalate { timeout: Duration },
}

impl RefusalPolicy {
    /// Apply the refusal policy to content
    pub fn apply(&self, content: &str, reason: &str) -> RefusalResult;
}
```

**Threat Coverage**:
- **LLM01:2025** (Prompt Injection) – Role isolation prevents injection escalation
- **LLM07:2025** (System Prompt Leakage) – Honeytokens detect exfiltration
- **LLM02:2025** (Sensitive Information) – Template scanning prevents secret embedding
- **AI RMF Manage** – Refusal policies enforce mitigation

---

### 4.4 `input` – Input Validation Pipeline

**Responsibilities**: Validate and sanitize all incoming content including user prompts, retrieved chunks, tool suggestions, and multimodal inputs.

#### ModerationStage

```rust
/// Content moderation using ML classifiers or heuristics
pub struct ModerationStage {
    backend: ModerationBackend,
    config: ModerationConfig,
    fallback: Option<Box<dyn FallbackModerator>>,
}

pub enum ModerationBackend {
    /// ONNX Runtime with Llama Guard 3 (recommended)
    OnnxLlamaGuard {
        model_path: PathBuf,
        quantization: Quantization,
    },
    /// Remote API (OpenAI Moderation, Perspective, etc.)
    RemoteApi {
        endpoint: Url,
        api_key: SecretString,
        timeout: Duration,
    },
    /// Heuristic-only (no ML)
    Heuristics {
        patterns: Vec<ModerationPattern>,
    },
}

#[derive(Debug, Clone)]
pub struct ModerationConfig {
    /// Categories to check
    pub categories: Vec<ModerationCategory>,
    /// Threshold scores for blocking (per category)
    pub thresholds: HashMap<ModerationCategory, f32>,
    /// Maximum input length before truncation
    pub max_input_length: usize,
    /// Batch size for multiple inputs
    pub batch_size: usize,
    /// Timeout for moderation check
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModerationCategory {
    Violence,
    Hate,
    Sexual,
    SelfHarm,
    Harassment,
    IllegalActivity,
    Malware,
    JailbreakAttempt,  // NEW in 2025
    PromptInjection,    // NEW in 2025
}
```

**Model Recommendations (2025)**:
- **Llama Guard 3** (8B): Best open-source moderation model, ONNX export available
- **ShieldGemma**: Google's safety classifier
- **Aegis**: NVIDIA's defense model family
- Fallback to heuristics when GPU/large models unavailable

#### PIIStage

```rust
/// PII detection and handling
pub struct PIIStage {
    backend: PIIBackend,
    config: PIIConfig,
}

pub enum PIIBackend {
    /// Microsoft Presidio (self-hosted or cloud)
    Presidio {
        endpoint: Url,
        recognizers: Vec<String>,
    },
    /// Built-in regex/dictionary patterns
    Local {
        patterns: PIIPatternSet,
    },
    /// Hybrid: local fast-path + Presidio for complex cases
    Hybrid {
        local: PIIPatternSet,
        presidio: Option<PresidioConfig>,
    },
}

#[derive(Debug, Clone)]
pub struct PIIConfig {
    /// Entity types to detect
    pub entity_types: Vec<PIIEntityType>,
    /// Action to take when PII is found
    pub action: PIIAction,
    /// Minimum confidence threshold
    pub min_confidence: f32,
    /// Allow overrides for specific fields
    pub field_exceptions: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum PIIAction {
    /// Block the entire request
    Block,
    /// Replace with placeholder: [EMAIL_REDACTED]
    Redact,
    /// Replace with consistent hash (for analytics)
    Hash { salt: String },
    /// Replace with fake but valid-looking data
    Anonymize,
    /// Log only, allow through
    LogOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PIIEntityType {
    Email,
    Phone,
    SSN,
    CreditCard,
    BankAccount,
    DriversLicense,
    Passport,
    IPAddress,
    Location,
    PersonName,
    DateOfBirth,
    MedicalRecord,
    BiometricData,
    Custom(u32),  // User-defined patterns
}
```

**Hybrid Detection Strategy**:
1. Fast local regex/dictionary check for common patterns (SSN, CC, email)
2. Route complex cases (names, addresses) to Presidio when available
3. Degrade to local-only when Presidio unavailable
4. Cache detection results for identical content

#### InjectionStage

```rust
/// Prompt injection detection
pub struct InjectionStage {
    detectors: Vec<Box<dyn InjectionDetector>>,
    config: InjectionConfig,
}

#[async_trait]
pub trait InjectionDetector: Send + Sync {
    fn name(&self) -> &'static str;
    async fn detect(&self, content: &str, ctx: &SecurityContext) -> InjectionResult;
}

/// Heuristic-based detection
pub struct HeuristicDetector {
    patterns: Vec<InjectionPattern>,
    structural_checks: Vec<StructuralCheck>,
}

/// ML-based detection (Prompt Shields, DeBERTa classifiers)
pub struct ClassifierDetector {
    model: Box<dyn InjectionClassifier>,
    threshold: f32,
}

/// Spotlighting detector for RAG content
pub struct SpotlightingDetector {
    delimiter_chars: Vec<char>,
    encoding: SpotlightEncoding,
}

#[derive(Debug, Clone)]
pub struct InjectionConfig {
    /// Minimum confidence to flag as injection
    pub threshold: f32,
    /// Weight combination strategy for multiple detectors
    pub ensemble_strategy: EnsembleStrategy,
    /// Categories of injection to detect
    pub categories: Vec<InjectionCategory>,
    /// Enable spotlighting for retrieved content
    pub spotlight_retrieved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionCategory {
    /// Direct: "ignore previous instructions"
    DirectInstruction,
    /// Indirect: hidden instructions in retrieved docs
    IndirectContext,
    /// Jailbreak: attempts to remove safety constraints
    Jailbreak,
    /// Role manipulation: "you are now DAN"
    RoleManipulation,
    /// Encoding attacks: base64, unicode tricks
    EncodingAttack,
    /// Multimodal: instructions in images/audio
    Multimodal,
    /// Adversarial suffixes (gradient-based)
    AdversarialSuffix,
}
```

**Detection Techniques (2025 Best Practices)**:

1. **Heuristic Patterns**:
   - Instruction override phrases ("ignore", "disregard", "forget")
   - Role reassignment ("you are", "act as", "pretend to be")
   - System prompt extraction ("repeat", "show me your instructions")
   - Delimiter confusion ("```system", "[INST]", etc.)
   - Unicode homoglyphs and confusables

2. **Structural Analysis**:
   - Sudden topic/language shifts
   - Unusual character distributions
   - Control character presence
   - Abnormal token sequences

3. **ML Classification**:
   - Fine-tuned DeBERTa/RoBERTa classifiers
   - Microsoft Prompt Shields API
   - NVIDIA Aegis models

4. **Spotlighting** (for RAG):
   - Encode retrieved content with special delimiters
   - Use character-level transformations
   - Prevents instruction hiding in documents

**Ensemble Scoring**:
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

#### NormalizationStage

```rust
/// Input normalization and sanitization
pub struct NormalizationStage {
    config: NormalizationConfig,
    parsers: Vec<Box<dyn ContentParser>>,
}

#[derive(Debug, Clone)]
pub struct NormalizationConfig {
    /// Maximum content length (truncate if exceeded)
    pub max_length: usize,
    /// Allowed MIME types for multimodal
    pub allowed_mime_types: Vec<String>,
    /// Strip HTML tags
    pub strip_html: bool,
    /// Strip script/style tags specifically
    pub strip_scripts: bool,
    /// Normalize unicode (NFKC)
    pub normalize_unicode: bool,
    /// Remove control characters
    pub strip_control_chars: bool,
    /// Collapse excessive whitespace
    pub normalize_whitespace: bool,
    /// Encoding normalization
    pub target_encoding: String,
}

/// HTML content parser (streaming for large documents)
pub struct HtmlParser {
    inner: lol_html::HtmlRewriter,
    config: HtmlParserConfig,
}

impl HtmlParser {
    /// Parse and sanitize HTML, extracting text content
    pub fn parse(&self, html: &[u8]) -> Result<String, ParseError>;
    
    /// Stream parse for large documents
    pub fn parse_streaming<R: Read>(&self, reader: R) -> impl Iterator<Item = Result<String, ParseError>>;
}
```

**Normalization Pipeline**:
1. Detect encoding, transcode to UTF-8
2. Apply Unicode NFKC normalization
3. Strip control characters (except newlines)
4. Parse HTML if detected, extract text
5. Remove script/style blocks
6. Collapse excessive whitespace
7. Truncate to max length with clean boundary
8. Validate MIME type for multimodal

#### MultimodalStage (NEW)

```rust
/// Validation for image, audio, and other multimodal inputs
pub struct MultimodalStage {
    validators: HashMap<Modality, Box<dyn ModalityValidator>>,
    config: MultimodalConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modality {
    Image,
    Audio,
    Video,
    Document,
}

#[async_trait]
pub trait ModalityValidator: Send + Sync {
    async fn validate(&self, data: &Bytes, ctx: &SecurityContext) -> ValidationResult;
}

/// Image validator (detects text-based injection in images)
pub struct ImageValidator {
    ocr_engine: Option<Box<dyn OcrEngine>>,
    text_injection_detector: InjectionStage,
}

impl ImageValidator {
    /// Extract text via OCR and check for prompt injection
    pub async fn validate_image(&self, image: &Bytes) -> ValidationResult;
}
```

**Multimodal Threat Coverage**:
- Text embedded in images (OCR extraction + injection detection)
- Steganographic payloads (detection via statistical analysis)
- Audio transcription attacks
- Document macro detection

#### Pipeline Composition

```rust
/// Compose input validation stages with metadata propagation
pub struct InputPipeline {
    executor: PipelineExecutor,
    stages: Vec<Arc<dyn GuardrailStage>>,
}

impl InputPipeline {
    /// Standard input pipeline for user prompts
    pub fn user_prompt_pipeline(config: &SecurityPolicy) -> Self {
        Self::new()
            .add_stage(RateLimitStage::new(&config.abuse))
            .add_stage(ModerationStage::new(&config.input.moderation))
            .add_stage(PIIStage::new(&config.input.pii))
            .add_stage(InjectionStage::new(&config.input.injection))
            .add_stage(NormalizationStage::new(&config.input.normalization))
    }
    
    /// Pipeline for retrieved RAG chunks
    pub fn retrieved_content_pipeline(config: &SecurityPolicy) -> Self {
        Self::new()
            .add_stage(NormalizationStage::new(&config.input.normalization))
            .add_stage(InjectionStage::with_spotlighting(&config.input.injection))
            .add_stage(PIIStage::new(&config.input.pii))
            .add_stage(ProvenanceStage::new(&config.rag.provenance))
    }
    
    /// Pipeline for multimodal inputs
    pub fn multimodal_pipeline(config: &SecurityPolicy) -> Self {
        Self::new()
            .add_stage(RateLimitStage::new(&config.abuse))
            .add_stage(MultimodalStage::new(&config.input.multimodal))
            .add_stage(ModerationStage::new(&config.input.moderation))
    }
}
```

**Threat Coverage Summary for Input Module**:

| Stage | OWASP LLM:2025 | NIST AI RMF | Description |
|-------|----------------|-------------|-------------|
| ModerationStage | LLM01, LLM04 | Manage | Blocks harmful content |
| PIIStage | LLM02 | Measure, Manage | Detects/masks PII |
| InjectionStage | LLM01 | Measure, Manage | Detects prompt injection |
| NormalizationStage | LLM01, LLM05 | Prepare | Sanitizes inputs |
| MultimodalStage | LLM01 | Measure | Validates non-text inputs |

---

## 5. Workstreams – Part 1: Foundation & Input Security

Each workstream (WS) groups cohesive milestones with detailed task breakdowns. For every task we specify purpose, deliverables, dependencies, acceptance criteria, and threat coverage mapping.

### WS1 – Foundations & Governance

**Context**: Establish governance foundations per AI RMF "Govern" function and SSDF "Prepare" practices. Create the compliance baseline that enables auditable, traceable security implementation.

**Timeline**: Sprint 1 (2 weeks)

#### WS1-01 – Threat Model & Attack Tree

**Purpose**: Create comprehensive threat documentation that informs all subsequent security decisions.

**Deliverables**:
- `docs/threat_model.md` – Full threat model document
- `docs/diagrams/data_flow.mmd` – Mermaid data flow diagrams
- `docs/diagrams/attack_tree.mmd` – Attack tree visualizations
- `docs/attack_playbooks/` – Per-threat response playbooks

**Content Requirements**:
- Data flow diagrams showing trust boundaries
- Asset inventory (prompts, state, credentials, embeddings)
- Threat actor profiles with capability levels
- Attack trees for each OWASP LLM Top 10 2025 category
- Mapping to MITRE ATLAS techniques
- Detection strategies per attack
- Response playbooks per threat

**Threat Coverage**: Baseline for all subsequent work

**Acceptance Criteria**:
- [ ] All OWASP LLM:2025 categories addressed
- [ ] Attack trees include likelihood and impact scores
- [ ] At least one detection strategy per threat
- [ ] Response playbooks reviewed by team

#### WS1-02 – Control Matrix

**Purpose**: Maintain traceability between threats, controls, and implementation.

**Deliverables**:
- `docs/control_matrix.csv` – Machine-readable control mapping
- `docs/control_matrix.md` – Narrative documentation
- GitHub issue templates linked to control IDs

**Content Requirements**:
- Map each OWASP LLM-XX:2025 to specific modules/stages/tests
- Map NIST AI RMF outcomes to implementation tasks
- Map EU AI Act requirements (where applicable)
- Include test identifiers for verification
- Track implementation status (planned/in-progress/done)

**Dependencies**: WS1-01

**Acceptance Criteria**:
- [ ] 100% coverage of OWASP LLM:2025
- [ ] Traceable to specific code modules
- [ ] Linked to test identifiers
- [ ] Reviewed for completeness

#### WS1-03 – Governance Documentation

**Purpose**: Establish development standards that ensure the security crate itself is developed securely.

**Deliverables**:
- `wg-bastion/README.md` – Crate overview and quick start
- `docs/architecture.md` – Technical architecture document
- `CONTRIBUTING.md` updates – Security review requirements
- `.github/PULL_REQUEST_TEMPLATE.md` – Security checklist
- `SECURITY.md` – Vulnerability disclosure policy

**Content Requirements**:
- Coding standards (no `unsafe` without review, mandatory doc comments)
- Linting requirements (clippy::pedantic subset)
- Required CI checks before merge
- Security review checklist for PRs
- Vulnerability disclosure process
- Incident response contacts

**Threat Coverage**: 
- **LLM09:2025** – Prevents misconfiguration via clear standards
- **AI RMF Govern** – Establishes accountability

**Acceptance Criteria**:
- [ ] README enables quick start in <10 minutes
- [ ] PR template includes security checklist
- [ ] CI enforces all required checks
- [ ] Vulnerability disclosure policy published

---

### WS2 – Crate Scaffolding & Policy Runtime

**Context**: Create the foundational crate structure and policy execution engine that all security modules build upon.

**Timeline**: Sprint 1-2 (2-4 weeks)

#### WS2-01 – Project Scaffold

**Purpose**: Establish the crate with proper structure, dependencies, and CI pipeline.

**Deliverables**:
- `wg-bastion/Cargo.toml` – Crate manifest with feature flags
- `wg-bastion/src/lib.rs` – Entry point with re-exports
- Module stubs for all planned modules
- CI workflow (`.github/workflows/wg-bastion.yml`)
- `clippy.toml` configuration
- `deny.toml` for dependency auditing

**Technical Requirements**:
- MSRV: 1.89
- Edition: 2024
- Default features: minimal (heuristics-only)
- Workspace member configuration
- Doc comment skeleton for all public types
- Baseline benchmarks structure

**CI Pipeline**:
```yaml
jobs:
  check:
    - cargo fmt --check
    - cargo clippy -- -D warnings
    - cargo test --all-features
    - cargo doc --no-deps
  
  security:
    - cargo audit
    - cargo deny check
    - cargo machete (unused deps)
  
  coverage:
    - cargo tarpaulin --out Xml
```

**Acceptance Criteria**:
- [ ] `cargo build` succeeds with default features
- [ ] `cargo build --all-features` succeeds
- [ ] CI passes on all checks
- [ ] Documentation builds without warnings

#### WS2-02 – Policy Schema

**Purpose**: Implement the `SecurityPolicy` configuration system with validation.

**Deliverables**:
- `src/config/policy.rs` – SecurityPolicy struct
- `src/config/builder.rs` – PolicyBuilder
- `src/config/validator.rs` – Validation rules
- `src/config/schema.rs` – JSON schema export
- `examples/policy.toml` – Example configuration
- Unit tests for all validation rules

**Technical Requirements**:
```rust
// Must support:
let policy = PolicyBuilder::new()
    .with_file("security.toml")?
    .with_env()
    .for_graph("my_graph")
    .build()?;

// Must export JSON Schema for IDE support:
let schema = SecurityPolicy::json_schema();
```

**Validation Rules**:
- At least one moderation backend enabled
- Rate limits within sane ranges
- Fail mode restrictions for production
- Audit flag required for dangerous overrides

**Threat Coverage**:
- **LLM09:2025** – Secure defaults prevent misconfiguration
- **AI RMF Govern** – Policy documentation

**Acceptance Criteria**:
- [ ] TOML/JSON configuration loading works
- [ ] Environment variable overrides work
- [ ] Validation catches invalid configurations
- [ ] JSON schema exports correctly
- [ ] 90%+ test coverage

#### WS2-03 – GuardrailStage Trait & Pipeline Executor

**Purpose**: Build the core execution engine for guardrail stages.

**Deliverables**:
- `src/pipeline/stage.rs` – GuardrailStage trait
- `src/pipeline/executor.rs` – PipelineExecutor
- `src/pipeline/outcome.rs` – StageOutcome types
- Integration tests with mock stages
- Benchmark for pipeline overhead

**Technical Requirements**:
```rust
// Trait must support:
#[async_trait]
pub trait GuardrailStage: Send + Sync {
    fn id(&self) -> &'static str;
    async fn evaluate(&self, content: &Content, ctx: &SecurityContext) 
        -> Result<StageOutcome, StageError>;
    fn degradable(&self) -> bool { true }
    fn priority(&self) -> u32 { 100 }
}

// Executor must support:
let result = executor.execute(content, &ctx).await?;
assert!(result.metrics.total_latency < Duration::from_millis(50));
```

**Performance Target**: <5ms overhead for pipeline orchestration (excluding stage execution)

**Acceptance Criteria**:
- [ ] Async execution works correctly
- [ ] Stage ordering by priority works
- [ ] Short-circuit on Block outcome works
- [ ] Metrics collection works
- [ ] Benchmark shows <5ms overhead

#### WS2-04 – Cache & Circuit Breakers

**Purpose**: Implement performance optimizations and resilience patterns.

**Deliverables**:
- `src/pipeline/cache.rs` – StageCache (LRU with TTL)
- `src/pipeline/circuit.rs` – CircuitBreaker
- Configuration for cache sizing and TTLs
- Integration with Tower middleware (optional)

**Technical Requirements**:
```rust
// Cache must support:
let cache = StageCache::new(CacheConfig {
    max_entries: 10_000,
    ttl: Duration::from_secs(300),
    entry_max_size: 1024 * 10,  // 10KB
});

// Circuit breaker must support:
let breaker = CircuitBreaker::new(CircuitConfig {
    failure_threshold: 5,
    reset_timeout: Duration::from_secs(30),
    half_open_max_calls: 3,
});
```

**Degrade Behavior**:
- When circuit opens: log warning, apply fallback
- Fallback strategies: allow-through, block-all, heuristics-only
- Configurable per-stage

**Acceptance Criteria**:
- [ ] Cache reduces repeated evaluations
- [ ] Circuit breaker opens on failures
- [ ] Circuit breaker recovers after timeout
- [ ] Degrade modes work correctly
- [ ] Telemetry reports circuit state

#### WS2-05 – Weavegraph Integration

**Purpose**: Integrate wg-bastion with weavegraph's execution model.

**Deliverables**:
- Feature flag `security` in weavegraph
- `SecurityHandle` integration point
- Pre/post node hooks
- Example: `examples/secured_graph.rs`
- Integration test suite

**Integration Pattern**:
```rust
// In weavegraph/src/app.rs (behind feature flag):
impl App {
    pub fn with_security(self, policy: SecurityPolicy) -> SecuredApp {
        SecuredApp::new(self, policy)
    }
}

// Usage:
let app = GraphBuilder::new()
    .add_node(...)
    .compile()?
    .with_security(policy);

let result = app.invoke_secured(state).await?;
```

**Hook Points**:
- `pre_prompt`: Before any prompt processing
- `pre_node`: Before each node execution
- `post_node`: After each node execution
- `pre_tool`: Before tool/MCP invocation
- `post_tool`: After tool/MCP result
- `pre_output`: Before final response delivery

**Acceptance Criteria**:
- [ ] Security hooks called at correct points
- [ ] Blocked requests short-circuit execution
- [ ] Transformed content propagates correctly
- [ ] Example runs end-to-end
- [ ] Feature flag isolation works

---

### WS3 – Prompt Hardening Module

**Context**: Address OWASP LLM-01 (Prompt Injection) and LLM-07 (System Prompt Leakage) early as they are foundational threats.

**Timeline**: Sprint 2-3 (2-4 weeks)

#### WS3-01 – Secure Template Engine

**Purpose**: Provide secure system prompt templating with role isolation.

**Deliverables**:
- `src/prompt/template.rs` – SecureTemplate implementation
- `src/prompt/isolation.rs` – Role boundary enforcement
- Template validation at compile time (proc macro optional)
- Fuzz tests for injection attempts

**Technical Requirements**:
```rust
// Must support typed placeholders:
let template = SecureTemplate::system(r#"
You are a helpful assistant. User's name is {{name}}.
{{#if context}}
Context: {{context}}
{{/if}}
"#)
.with_placeholder(PlaceholderConfig {
    name: "name".into(),
    required: true,
    max_length: Some(100),
    sanitizer: Some(Box::new(|s| s.replace('\n', " "))),
    ..Default::default()
})
.with_placeholder(PlaceholderConfig {
    name: "context".into(),
    required: false,
    max_length: Some(4000),
    ..Default::default()
});

let rendered = template.render(&values)?;
```

**Role Isolation Strategy**:
- System prompts wrapped with `[SYSTEM_START]...[SYSTEM_END]`
- User content wrapped with `[USER_START]...[USER_END]`
- Delimiter detection in inputs triggers block
- Alternative: XML-style tags `<|system|>...</|system|>`

**Fuzz Testing**:
- Attempt role escape: `}}[SYSTEM_START]new instructions{{`
- Attempt delimiter injection
- Unicode confusable attacks
- Maximum length bypass attempts

**Threat Coverage**:
- **LLM01:2025** – Prevents role manipulation
- **LLM07:2025** – Protects system prompt structure

**Acceptance Criteria**:
- [ ] Typed placeholders validate correctly
- [ ] Role isolation markers applied
- [ ] Injection attempts in placeholders blocked
- [ ] Fuzz tests pass without panics
- [ ] 100% test coverage on template module

#### WS3-02 – Honeytoken Manager

**Purpose**: Detect system prompt exfiltration via canary tokens.

**Deliverables**:
- `src/prompt/honeytoken.rs` – HoneytokenStore
- Encrypted persistence (optional file/sqlite backend)
- Rotation CLI command
- Detection integration with output scanning

**Technical Requirements**:
```rust
// Must support:
let store = HoneytokenStore::new(CryptoConfig::default())?
    .with_persistence(SqlitePersistence::new("honeytokens.db")?);

// Generate tokens
let api_token = store.generate(HoneytokenType::ApiKey);
// Output: "sk_live_CANARY_a1b2c3d4e5f6"

// Inject into template
let template = template.with_honeytokens(&store);

// Detect in output
if let Some(match_) = store.detect(&llm_output) {
    incident_orchestrator.trigger(IncidentType::HoneytokenLeak {
        token_id: match_.token_id,
        context: match_.context,
    });
}
```

**Token Types**:
- `ApiKey`: Fake API keys matching common formats
- `Credential`: username:password pairs
- `Url`: Canary URLs that trigger on access
- `CustomMarker`: Application-specific markers

**Encryption**: AES-256-GCM using `ring` or `orion` crate

**Rotation Strategy**:
- CLI command: `cargo xtask security rotate-honeytokens`
- Configurable rotation period (default: 7 days)
- Old tokens remain detectable for overlap period
- Audit log of rotations

**Threat Coverage**:
- **LLM07:2025** – Detects system prompt leakage
- **LLM02:2025** – Detects data exfiltration attempts

**Acceptance Criteria**:
- [ ] Token generation produces valid-looking formats
- [ ] Detection works on partial matches
- [ ] Encryption protects stored tokens
- [ ] Rotation preserves detection of old tokens
- [ ] Integration with incident response works

#### WS3-03 – Template Secret Scanner

**Purpose**: Prevent accidental secret embedding in templates at development time.

**Deliverables**:
- `src/prompt/scanner.rs` – TemplateScanner
- CLI command: `cargo xtask security scan-templates`
- Pre-commit hook integration
- CI integration example

**Technical Requirements**:
```rust
// Must detect:
let findings = scanner.scan_file("prompts/system.txt")?;
for finding in findings {
    eprintln!(
        "[{}] {} at {}:{} - {}",
        finding.severity,
        finding.pattern_id,
        finding.location.file,
        finding.location.line,
        finding.recommendation
    );
}
```

**Built-in Patterns** (with examples):
| Pattern ID | Description | Example Match |
|------------|-------------|---------------|
| `aws-key` | AWS access key | AKIA... |
| `gcp-key` | GCP API key | AIza... |
| `openai-key` | OpenAI API key | sk-... |
| `anthropic-key` | Anthropic key | sk-ant-... |
| `jwt` | JWT tokens | eyJ... |
| `private-key` | RSA/EC private keys | -----BEGIN...KEY----- |
| `password-url` | Passwords in URLs | ://user:pass@ |
| `high-entropy` | High entropy strings | (entropy > 4.5) |

**CLI Output**:
```
$ cargo xtask security scan-templates ./prompts/

Scanning 12 template files...

[HIGH] openai-key found in prompts/assistant.txt:15
       Matched: sk-proj-abcd1234...
       Recommendation: Remove API key, use environment variable

[MEDIUM] high-entropy string in prompts/rag_context.txt:42
       Matched: a1b2c3d4e5f6g7h8i9j0...
       Recommendation: Verify this is not a secret

Scan complete: 2 findings (1 high, 1 medium)
```

**Pre-commit Integration**:
```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: wg-bastion-scan
        name: Scan templates for secrets
        entry: cargo xtask security scan-templates
        language: system
        files: \.(txt|md|toml|yaml|json)$
```

**Threat Coverage**:
- **LLM02:2025** – Prevents secret embedding
- **LLM07:2025** – Protects system prompt secrets
- **SSDF Produce** – Secure development practices

**Acceptance Criteria**:
- [ ] All built-in patterns have tests
- [ ] Entropy calculation works correctly
- [ ] CLI returns non-zero on findings
- [ ] Pre-commit hook integration documented
- [ ] CI integration example provided

#### WS3-04 – Refusal Policy

**Purpose**: Define response behavior when security violations are detected.

**Deliverables**:
- `src/prompt/refusal.rs` – RefusalPolicy implementation
- Configurable response templates
- Audit logging for all refusals
- Integration with output pipeline

**Technical Requirements**:
```rust
// Must support multiple modes:
let policy = RefusalPolicy {
    mode: RefusalMode::SafeResponse,
    fallback_response: "I'm unable to process that request.".into(),
    audit_log: true,
    notify_incident: true,
};

// Apply to violation
let result = policy.apply(
    &blocked_content,
    "Prompt injection detected",
);

match result {
    RefusalResult::Blocked { response, audit_id } => {
        // Return safe response to user
    }
    RefusalResult::Escalated { ticket_id, timeout } => {
        // Wait for human review or timeout
    }
}
```

**Refusal Modes**:
- `Block`: Return generic error, log details
- `Redact`: Remove sensitive portions, allow remainder
- `SafeResponse`: Return predefined safe message
- `Escalate`: Queue for human review with timeout

**Response Templates** (configurable):
```toml
[refusal.templates]
default = "I'm unable to process that request. Please try rephrasing."
injection = "Your request contained content I cannot process safely."
pii = "Your request contained personal information that I cannot handle."
moderation = "I cannot assist with that type of request."
```

**Audit Requirements**:
- Log: timestamp, session_id, user_id, violation_type, severity
- Log: original content (encrypted or hashed based on policy)
- Log: action taken, response delivered
- Correlation ID for incident tracking

**Threat Coverage**:
- **LLM01:2025** – Enforces injection blocks
- **LLM02:2025** – Enforces PII blocks
- **AI RMF Manage** – Documents response actions

**Acceptance Criteria**:
- [ ] All refusal modes work correctly
- [ ] Audit logs contain required fields
- [ ] Response templates are customizable
- [ ] Escalation workflow with timeout works
- [ ] Integration with telemetry/incident response

---

### WS4 – Input Guard Pipelines

**Context**: Build the input validation pipeline addressing direct and indirect prompt injection, harmful content, and data protection.

**Timeline**: Sprint 3-5 (4-6 weeks)

#### WS4-01 – Moderation Stage

**Purpose**: Classify and filter harmful or policy-violating content.

**Deliverables**:
- `src/input/moderation.rs` – ModerationStage
- ONNX Runtime integration (feature-gated)
- Remote API connector (feature-gated)
- Heuristic fallback (always available)
- Benchmarks for latency/throughput

**Technical Requirements**:
```rust
// Must support multiple backends:
let stage = ModerationStage::new(ModerationConfig {
    backend: ModerationBackend::OnnxLlamaGuard {
        model_path: "models/llama-guard-3.onnx".into(),
        quantization: Quantization::Int8,
    },
    categories: vec![
        ModerationCategory::Violence,
        ModerationCategory::Hate,
        ModerationCategory::JailbreakAttempt,
    ],
    thresholds: hashmap! {
        ModerationCategory::Violence => 0.8,
        ModerationCategory::Hate => 0.7,
        ModerationCategory::JailbreakAttempt => 0.6,
    },
    timeout: Duration::from_millis(100),
    ..Default::default()
});
```

**Model Support (2025)**:
- **Llama Guard 3** (primary): Best open-source, ONNX export available
- **ShieldGemma**: Alternative from Google
- **OpenAI Moderation API**: For teams already using OpenAI
- **Heuristic patterns**: No-ML fallback

**Performance Targets**:
- ONNX inference: <50ms per request (CPU)
- Batching: Support up to 8 concurrent requests
- Memory: <500MB for loaded model

**Degradation**:
- If ONNX fails to load: fall back to heuristics
- If timeout exceeded: log, apply fallback policy
- Circuit breaker for remote APIs

**Threat Coverage**:
- **LLM01:2025** – Jailbreak detection
- **LLM04:2025** – Harmful content blocking
- **AI RMF Measure** – Classification metrics

**Acceptance Criteria**:
- [ ] ONNX backend works on CPU
- [ ] Remote API backend works
- [ ] Heuristic fallback works standalone
- [ ] Batching improves throughput
- [ ] Degradation path tested
- [ ] Benchmark results documented

#### WS4-02 – PII Detection

**Purpose**: Detect and handle personally identifiable information in inputs.

**Deliverables**:
- `src/input/pii.rs` – PIIStage
- Local pattern-based detection
- Presidio connector (feature-gated)
- Action handlers (mask, hash, redact, block)
- Configuration for regional patterns (SSN formats, etc.)

**Technical Requirements**:
```rust
// Must support:
let stage = PIIStage::new(PIIConfig {
    backend: PIIBackend::Hybrid {
        local: PIIPatternSet::us_patterns(),
        presidio: Some(PresidioConfig {
            endpoint: "http://presidio:8080".parse()?,
            recognizers: vec!["email", "phone", "ssn", "credit_card"],
        }),
    },
    action: PIIAction::Redact,
    min_confidence: 0.85,
    entity_types: vec![
        PIIEntityType::Email,
        PIIEntityType::Phone,
        PIIEntityType::SSN,
        PIIEntityType::CreditCard,
    ],
    ..Default::default()
});
```

**Local Pattern Coverage**:
| Entity Type | Pattern Examples |
|-------------|-----------------|
| Email | RFC 5322 compliant |
| Phone | US, UK, EU formats |
| SSN | US format (XXX-XX-XXXX) |
| Credit Card | Luhn-validated, major issuers |
| IP Address | IPv4 and IPv6 |

**Action Implementations**:
```rust
match action {
    PIIAction::Block => StageOutcome::Block { ... },
    PIIAction::Redact => {
        // "My SSN is 123-45-6789" → "My SSN is [SSN_REDACTED]"
        StageOutcome::Transform { content: redacted, ... }
    },
    PIIAction::Hash => {
        // Consistent hashing for analytics
        // "john@example.com" → "[EMAIL:a1b2c3d4]"
        StageOutcome::Transform { content: hashed, ... }
    },
    PIIAction::Anonymize => {
        // Replace with synthetic data
        // "John Smith" → "Jane Doe"
        StageOutcome::Transform { content: anonymized, ... }
    },
}
```

**Presidio Integration**:
- HTTP client with retry/timeout
- Recognizer configuration
- Entity mapping to internal types
- Circuit breaker for availability

**Threat Coverage**:
- **LLM02:2025** – PII protection
- **EU AI Act** – Data protection requirements
- **AI RMF Manage** – Privacy controls

**Acceptance Criteria**:
- [ ] Local patterns detect 95%+ of test cases
- [ ] Presidio integration works
- [ ] All actions implemented correctly
- [ ] Degradation to local-only works
- [ ] Regional pattern sets available

#### WS4-03 – Prompt Injection Detection

**Purpose**: Detect and block prompt injection attempts using multiple techniques.

**Deliverables**:
- `src/input/injection.rs` – InjectionStage
- Heuristic detector with pattern library
- ML classifier integration (optional)
- Spotlighting for RAG content
- Scoring and ensemble logic

**Technical Requirements**:
```rust
// Must support ensemble detection:
let stage = InjectionStage::new(InjectionConfig {
    detectors: vec![
        Box::new(HeuristicDetector::new(InjectionPatterns::default())),
        Box::new(StructuralDetector::new()),
        Box::new(ClassifierDetector::new(classifier_model)?),
    ],
    ensemble_strategy: EnsembleStrategy::WeightedAverage {
        weights: hashmap! {
            "heuristic" => 0.3,
            "structural" => 0.3,
            "classifier" => 0.4,
        },
    },
    threshold: 0.7,
    categories: vec![
        InjectionCategory::DirectInstruction,
        InjectionCategory::IndirectContext,
        InjectionCategory::Jailbreak,
        InjectionCategory::RoleManipulation,
    ],
    spotlight_retrieved: true,
    ..Default::default()
});
```

**Heuristic Patterns** (comprehensive library):
```rust
pub struct InjectionPatterns {
    // Instruction override
    instruction_override: Vec<Regex>,  // "ignore", "disregard", "forget"
    
    // Role manipulation
    role_change: Vec<Regex>,  // "you are now", "act as", "pretend"
    
    // System prompt extraction
    extraction: Vec<Regex>,  // "repeat your instructions", "show system prompt"
    
    // Delimiter confusion
    delimiter: Vec<Regex>,  // "```system", "[INST]", "<|im_start|>"
    
    // Encoding attacks
    encoding: Vec<Regex>,  // Base64, unicode escapes
}
```

**Structural Analysis**:
- Sudden topic shifts (embedding distance)
- Language distribution anomalies
- Control character presence
- Unusual token patterns
- Bracket/delimiter imbalance

**Spotlighting Implementation** (for RAG):
```rust
/// Apply spotlighting transformation to retrieved content
pub fn spotlight(content: &str, config: &SpotlightConfig) -> String {
    // Strategy 1: Delimiter encoding
    // "Hello world" → "⟦H⟧⟦e⟧⟦l⟧⟦l⟧⟦o⟧..."
    
    // Strategy 2: Marker prefixing
    // "Hello world" → "[RETRIEVED] Hello world [/RETRIEVED]"
    
    // Strategy 3: Character transformation
    // "Hello" → "Ⓗⓔⓛⓛⓞ" (circled letters)
}
```

**Ensemble Scoring**:
```rust
impl EnsembleStrategy {
    pub fn combine(&self, scores: &HashMap<String, f32>) -> f32 {
        match self {
            Self::AnyAboveThreshold => scores.values().max().copied().unwrap_or(0.0),
            Self::WeightedAverage { weights } => {
                let total_weight: f32 = weights.values().sum();
                scores.iter()
                    .map(|(k, v)| weights.get(k.as_str()).unwrap_or(&0.0) * v)
                    .sum::<f32>() / total_weight
            },
            Self::MajorityVote => {
                let above = scores.values().filter(|&&v| v > 0.5).count();
                if above > scores.len() / 2 { 1.0 } else { 0.0 }
            },
            Self::MaxScore => scores.values().max().copied().unwrap_or(0.0),
        }
    }
}
```

**Threat Coverage**:
- **LLM01:2025** – Primary injection defense
- **AI RMF Measure** – Detection metrics

**Acceptance Criteria**:
- [ ] Heuristic detector catches common patterns
- [ ] Structural analysis detects anomalies
- [ ] Ensemble scoring works correctly
- [ ] Spotlighting transforms content properly
- [ ] False positive rate <5% on benign test set
- [ ] Detection rate >90% on adversarial test set

#### WS4-04 – Retrieval Normalization

**Purpose**: Sanitize retrieved content from RAG pipelines before LLM processing.

**Deliverables**:
- `src/input/normalization.rs` – NormalizationStage
- HTML parser (streaming for large documents)
- MIME type validation
- Unicode normalization
- Size limits and truncation

**Technical Requirements**:
```rust
// Must handle:
let stage = NormalizationStage::new(NormalizationConfig {
    max_length: 16_000,  // characters
    allowed_mime_types: vec![
        "text/plain",
        "text/html",
        "text/markdown",
        "application/json",
    ],
    strip_html: true,
    strip_scripts: true,  // Remove <script>, <style>
    normalize_unicode: true,  // NFKC normalization
    strip_control_chars: true,
    normalize_whitespace: true,
    target_encoding: "utf-8".into(),
});
```

**HTML Processing** (using `lol_html` for streaming):
```rust
pub struct HtmlSanitizer {
    rewriter: HtmlRewriter<'static, Vec<u8>>,
}

impl HtmlSanitizer {
    pub fn sanitize(&self, html: &[u8]) -> Result<String, SanitizeError> {
        // 1. Remove <script>, <style>, <object>, <embed>
        // 2. Strip all attributes except safe list (href, src with validation)
        // 3. Convert block elements to newlines
        // 4. Extract text content
        // 5. Normalize whitespace
    }
}
```

**MIME Validation**:
```rust
pub fn validate_mime(content: &[u8], claimed_type: Option<&str>) -> MimeResult {
    // 1. Detect actual MIME from magic bytes
    let detected = tree_magic_mini::from_u8(content);
    
    // 2. Compare with claimed type
    // 3. Reject mismatches or dangerous types
    // 4. Log discrepancies
}
```

**Truncation Strategy**:
```rust
pub fn truncate_safely(content: &str, max_len: usize) -> String {
    if content.len() <= max_len {
        return content.to_string();
    }
    
    // Find safe truncation point (sentence/paragraph boundary)
    let truncation_point = find_safe_boundary(content, max_len);
    
    // Add truncation marker
    format!("{}... [TRUNCATED]", &content[..truncation_point])
}
```

**Threat Coverage**:
- **LLM01:2025** – Prevents script injection
- **LLM05:2025** – Validates output handling
- **AI RMF Prepare** – Input validation

**Acceptance Criteria**:
- [ ] HTML sanitization removes scripts
- [ ] Streaming works for large documents (>1MB)
- [ ] MIME detection is accurate
- [ ] Unicode normalization works
- [ ] Truncation preserves readability
- [ ] Performance: <10ms for typical documents

#### WS4-05 – Pipeline Composition

**Purpose**: Compose input stages into configurable pipelines with proper metadata propagation.

**Deliverables**:
- `src/input/pipeline.rs` – InputPipeline builder
- Pre-built pipeline configurations
- Metadata propagation through stages
- Channel-specific pipelines (user vs. RAG)

**Technical Requirements**:
```rust
// Pre-built pipelines:
let user_pipeline = InputPipeline::user_prompt_pipeline(&policy);
let rag_pipeline = InputPipeline::retrieved_content_pipeline(&policy);
let multimodal_pipeline = InputPipeline::multimodal_pipeline(&policy);

// Custom composition:
let custom = InputPipeline::builder()
    .add_stage(RateLimitStage::new(&config))
    .add_stage(CustomFilterStage::new())
    .add_stage(ModerationStage::new(&config))
    .add_stage_conditional(
        PIIStage::new(&config),
        |ctx| ctx.metadata.get("check_pii") == Some(&Value::Bool(true)),
    )
    .build();
```

**Metadata Propagation**:
```rust
/// Metadata accumulated through pipeline stages
#[derive(Debug, Clone, Default)]
pub struct PipelineMetadata {
    /// Verdicts from each stage
    pub stage_verdicts: HashMap<String, StageVerdict>,
    
    /// Detected entities (PII, injection patterns, etc.)
    pub detected_entities: Vec<DetectedEntity>,
    
    /// Applied transformations
    pub transformations: Vec<Transformation>,
    
    /// Risk score (aggregate)
    pub risk_score: f32,
    
    /// Provenance information (for RAG)
    pub provenance: Option<ProvenanceInfo>,
}
```

**Channel-Specific Configuration**:
```toml
[input.channels.user_prompt]
moderation = true
pii = true
injection = true
normalization = false  # User prompts don't need HTML stripping

[input.channels.retrieved_content]
moderation = false  # Already vetted during ingestion
pii = true
injection = true  # Spotlighting enabled
normalization = true
```

**Threat Coverage**: Comprehensive input layer defense

**Acceptance Criteria**:
- [ ] Pre-built pipelines work out-of-box
- [ ] Custom composition is flexible
- [ ] Metadata propagates correctly
- [ ] Conditional stages work
- [ ] Channel configuration works
- [ ] Performance: <50ms total for standard pipeline

---

## 5. Workstreams – Part 2: Output, Tools, RAG & Beyond

### WS5 – Output Validation & Egress Safety

**Context**: Guard outputs against injection, XSS, data leakage, and hallucination (OWASP LLM-02/05/09).

**Timeline**: Sprint 5-6 (3-4 weeks)

#### WS5-01 – Schema Validator

**Purpose**: Enforce structured output schemas for LLM responses.

**Deliverables**:
- `src/output/schema.rs` – SchemaValidator implementation
- DSL parser for schema definitions
- JSON Schema import support
- Integration with serde_json

**Technical Requirements**:
```rust
// Support both DSL and JSON Schema:
let validator = SchemaValidator::new()
    .register_dsl(r#"
        schema Response {
            answer: string(max=2000) @no_pii @required
            confidence: number(min=0, max=1) @required
            sources: array<string>(max_items=5)
        }
    "#)?
    .register_json_schema(include_str!("schemas/response.json"))?;

let result = validator.validate("Response", &llm_output)?;
```

**Threat Coverage**:
- **LLM05:2025** – Enforces output structure
- **LLM02:2025** – Schema-level PII checks

**Acceptance Criteria**:
- [ ] DSL parsing works correctly
- [ ] JSON Schema import works
- [ ] All field types validated
- [ ] Custom validators work
- [ ] Error messages are actionable

#### WS5-02 – Sanitization Pipeline

**Purpose**: Remove dangerous content from LLM outputs.

**Deliverables**:
- `src/output/sanitizer.rs` – Sanitizer implementation
- HTML sanitization (ammonia wrapper)
- Terminal escape sanitization
- Markdown safe conversion

**Technical Requirements**:
- HTML: allowlist-based tag filtering
- Terminal: ANSI escape removal
- Markdown: convert to HTML, then sanitize
- Streaming support for large outputs

**Acceptance Criteria**:
- [ ] XSS payloads blocked
- [ ] ANSI injection blocked
- [ ] Streaming works for >1MB
- [ ] Configurable allowlists

#### WS5-03 – Code Guard

**Purpose**: Control code output from LLMs.

**Deliverables**:
- `src/output/code_guard.rs` – CodeGuard implementation
- Code block detection
- Language filtering
- Shell command blocking

**Threat Coverage**:
- **LLM05:2025** – Code output controls
- **LLM06:2025** – Prevents code execution abuse

**Acceptance Criteria**:
- [ ] Code blocks detected accurately
- [ ] Language filtering works
- [ ] Shell commands identified
- [ ] Approval flow integration

#### WS5-04 – Egress Scanner

**Purpose**: Scan outputs for secrets and PII before delivery.

**Deliverables**:
- `src/output/egress.rs` – EgressScanner
- Secret pattern detection
- Honeytoken detection (priority)
- TruffleHog integration (optional)

**Technical Requirements**:
```rust
let scanner = EgressScanner::new()
    .with_secret_patterns(SecretPatterns::default())
    .with_pii_stage(pii_stage.clone())
    .with_honeytoken_store(store.clone());

let result = scanner.scan(&llm_output, &ctx).await?;
if result.honeytoken_leaked.is_some() {
    incident_orchestrator.trigger_immediate(...);
}
```

**Threat Coverage**:
- **LLM02:2025** – Data leakage prevention
- **LLM07:2025** – System prompt leakage detection

**Acceptance Criteria**:
- [ ] Secret patterns detected
- [ ] Honeytoken detection immediate
- [ ] TruffleHog integration works
- [ ] Redaction accurate

#### WS5-05 – Grounding Validator

**Purpose**: Validate outputs are grounded in retrieved context.

**Deliverables**:
- `src/output/grounding.rs` – GroundingValidator
- Claim extraction
- Evidence matching
- Citation generation

**Threat Coverage**:
- **LLM09:2025** – Misinformation prevention

**Acceptance Criteria**:
- [ ] Claims extracted from text
- [ ] Matching to sources works
- [ ] Citations generated correctly
- [ ] Grounding score accurate

---

### WS6 – Tool & MCP Security

**Context**: Prevent excessive agency and confused deputy attacks (OWASP LLM-06/08).

**Timeline**: Sprint 6-7 (3-4 weeks)

#### WS6-01 – Tool Policy Schema

**Purpose**: Define YAML/JSON structure for tool access control.

**Deliverables**:
- `src/tools/policy.rs` – ToolPolicy struct
- YAML/JSON schema definition
- Policy validation
- Default secure policy

**Acceptance Criteria**:
- [ ] Schema documented
- [ ] YAML parsing works
- [ ] Validation catches errors
- [ ] Defaults are secure

#### WS6-02 – Execution Guard

**Purpose**: Wrap tool execution with security checks.

**Deliverables**:
- `src/tools/guard.rs` – ExecutionGuard
- Pre-execution validation
- Post-execution sanitization
- Audit logging

**Technical Requirements**:
```rust
let guard = ExecutionGuard::new(policy, rate_limiter, approval_flow);

// All tool calls go through guard
let result = guard.execute(&tool_request, &ctx, async {
    actual_tool.call(args).await
}).await?;
```

**Acceptance Criteria**:
- [ ] Policy enforcement works
- [ ] Rate limiting works
- [ ] Approval flow integration
- [ ] All calls audited

#### WS6-03 – MCP Security Layer

**Purpose**: MCP-specific security controls per 2025-11-25 spec.

**Deliverables**:
- `src/tools/mcp.rs` – MCPSecurityLayer
- Confused deputy prevention
- Token binding validation
- Secure session ID generation

**Threat Coverage**:
- **LLM06:2025** – MCP-specific controls

**Acceptance Criteria**:
- [ ] Session ID entropy sufficient
- [ ] Token binding validated
- [ ] Confused deputy detected
- [ ] Scope minimization enforced

#### WS6-04 – Fetch Sanitizer

**Purpose**: Sanitize HTTP responses from tools.

**Deliverables**:
- `src/tools/fetch.rs` – FetchSanitizer
- URL validation
- Content sanitization
- Redirect limiting

**Acceptance Criteria**:
- [ ] URL allowlist/blocklist works
- [ ] Content sanitized
- [ ] Redirects limited
- [ ] Size limits enforced

#### WS6-05 – Approval Workflow

**Purpose**: Human-in-the-loop for high-risk actions.

**Deliverables**:
- `src/tools/approval.rs` – ApprovalFlow
- Pending request management
- Notification integration
- Timeout handling

**Acceptance Criteria**:
- [ ] Requests queue correctly
- [ ] Notifications sent
- [ ] Approvals/denials work
- [ ] Timeouts handled

---

### WS7 – RAG Hardening

**Context**: Address indirect prompt injection and embedding security (OWASP LLM-01/08).

**Timeline**: Sprint 7-8 (3-4 weeks)

#### WS7-01 – Sanitized Ingestion

**Purpose**: Secure the document ingestion pipeline.

**Deliverables**:
- `src/rag/ingestion.rs` – SanitizedIngestion
- Domain allowlisting
- Document hashing/signing
- PII/injection scanning

**Integration with wg-ragsmith**:
```rust
// Wrap wg-ragsmith ingestion
let secure_ingestion = SanitizedIngestion::new(config)
    .wrap(ragsmith_service);

let prepared = secure_ingestion.prepare(&raw_doc).await?;
```

**Acceptance Criteria**:
- [ ] Domain filtering works
- [ ] Hashing implemented
- [ ] Scanning detects issues
- [ ] wg-ragsmith integration smooth

#### WS7-02 – Provenance Tagging

**Purpose**: Track document origins through retrieval.

**Deliverables**:
- `src/rag/provenance.rs` – ProvenanceTagger
- Metadata attachment
- Trust level computation
- Audit trail

**Acceptance Criteria**:
- [ ] Metadata attached to chunks
- [ ] Trust levels computed
- [ ] Provenance survives retrieval

#### WS7-03 – Embedding Security

**Purpose**: Protect vector stores from exploitation.

**Deliverables**:
- `src/rag/embedding.rs` – EmbeddingSecurity
- Access control
- Tenant isolation
- Inversion detection

**Threat Coverage**:
- **LLM08:2025** – Vector store security

**Acceptance Criteria**:
- [ ] Access control enforced
- [ ] Tenant isolation works
- [ ] Inversion attempts detected

#### WS7-04 – Grounded Rails

**Purpose**: Enforce grounded answering.

**Deliverables**:
- `src/rag/grounding.rs` – GroundedRails
- Claim-to-source matching
- Citation injection
- Ungrounded filtering

**Acceptance Criteria**:
- [ ] Grounding validation works
- [ ] Citations accurate
- [ ] Filtering preserves coherence

#### WS7-05 – Corpus Scanner CLI

**Purpose**: Scan stored corpus for issues.

**Deliverables**:
- CLI command: `cargo xtask security scan-corpus`
- PII detection across corpus
- Secret detection
- Severity reporting

**Acceptance Criteria**:
- [ ] Full corpus scanned
- [ ] Findings categorized
- [ ] Actionable reports generated

---

### WS8 – Agentic AI Security (NEW)

**Context**: Address autonomous agent threats per OWASP Agentic AI Top 10.

**Timeline**: Sprint 8-9 (3-4 weeks)

#### WS8-01 – Delegation Tracking

**Purpose**: Track and validate agent delegation chains.

**Deliverables**:
- `src/agents/delegation.rs` – DelegationTracker
- Chain recording
- Validation logic
- Capability propagation

**Acceptance Criteria**:
- [ ] Chains recorded accurately
- [ ] Depth limits enforced
- [ ] Capabilities propagate correctly

#### WS8-02 – Autonomy Boundaries

**Purpose**: Enforce limits on autonomous behavior.

**Deliverables**:
- `src/agents/boundaries.rs` – AutonomyBoundaries
- Action limits
- Cost/time budgets
- Kill switch implementation

**Acceptance Criteria**:
- [ ] Action limits enforced
- [ ] Budgets tracked
- [ ] Kill switch works immediately

#### WS8-03 – Agent Memory Protection

**Purpose**: Protect persistent agent state.

**Deliverables**:
- `src/agents/memory.rs` – AgentMemoryProtection
- Update validation
- Entry signing
- Poisoning detection

**Acceptance Criteria**:
- [ ] Updates validated
- [ ] Signatures verified
- [ ] Poisoning attempts detected

#### WS8-04 – Inter-Agent Security

**Purpose**: Secure agent-to-agent communication.

**Deliverables**:
- `src/agents/communication.rs` – InterAgentSecurity
- Message authentication
- Encryption
- Schema validation

**Acceptance Criteria**:
- [ ] Authentication works
- [ ] Encryption/decryption works
- [ ] Schemas validated

---

### WS9 – Abuse & Availability Controls

**Context**: Prevent DoS and cost explosion (OWASP LLM-10).

**Timeline**: Sprint 9-10 (2-3 weeks)

#### WS9-01 – Rate Limiter & Quotas

**Purpose**: Multi-dimensional rate limiting.

**Deliverables**:
- `src/abuse/rate_limit.rs` – RateLimiter
- Token bucket implementation
- Multiple dimensions (user/session/tool/global)
- Optional Redis persistence

**Acceptance Criteria**:
- [ ] Rate limiting accurate
- [ ] Multiple dimensions work
- [ ] Redis sync works (if enabled)

#### WS9-02 – Recursion Guard

**Purpose**: Prevent infinite loops.

**Deliverables**:
- `src/abuse/recursion.rs` – RecursionGuard
- Depth tracking
- Cycle detection
- Iteration limits

**Acceptance Criteria**:
- [ ] Depth limits enforced
- [ ] Cycles detected
- [ ] Clear error messages

#### WS9-03 – Cost Monitor

**Purpose**: Track and limit costs.

**Deliverables**:
- `src/abuse/cost.rs` – CostMonitor
- Token counting
- Budget management
- Alert thresholds

**Acceptance Criteria**:
- [ ] Costs tracked accurately
- [ ] Budgets enforced
- [ ] Alerts sent at thresholds

#### WS9-04 – Circuit Breakers

**Purpose**: Resilience for external services.

**Deliverables**:
- `src/abuse/circuit.rs` – CircuitBreaker
- State machine (closed/open/half-open)
- Failure counting
- Recovery logic

**Acceptance Criteria**:
- [ ] Circuit opens on failures
- [ ] Half-open testing works
- [ ] Recovery automatic

---

### WS10 – Telemetry & Incident Response

**Context**: Satisfy AI RMF Measure/Manage; enable rapid incident response.

**Timeline**: Sprint 10-11 (3-4 weeks)

#### WS10-01 – Security Event Schema

**Purpose**: Structured security event format.

**Deliverables**:
- `src/telemetry/events.rs` – SecurityEvent types
- JSON schema documentation
- Integration with weavegraph EventBus

**Acceptance Criteria**:
- [ ] Events serialize correctly
- [ ] Schema documented
- [ ] EventBus integration works

#### WS10-02 – OTLP Exporter

**Purpose**: OpenTelemetry export for observability.

**Deliverables**:
- `src/telemetry/exporter.rs` – OTLPExporter
- Span export
- Metrics export
- Configuration

**Acceptance Criteria**:
- [ ] OTLP export works
- [ ] Metrics accurate
- [ ] Configurable endpoint

#### WS10-03 – Incident Orchestrator

**Purpose**: Automated incident response.

**Deliverables**:
- `src/telemetry/incident.rs` – IncidentOrchestrator
- Trigger logic
- Notifier integrations
- Action execution

**Acceptance Criteria**:
- [ ] Incidents triggered correctly
- [ ] Notifications sent
- [ ] Actions executed

#### WS10-04 – Grafana Dashboards

**Purpose**: Pre-built monitoring dashboards.

**Deliverables**:
- `dashboards/security_overview.json`
- `dashboards/injection_metrics.json`
- `dashboards/cost_tracking.json`
- Documentation for import

**Acceptance Criteria**:
- [ ] Dashboards import correctly
- [ ] Metrics visualized accurately
- [ ] Documentation clear

#### WS10-05 – Audit Log Retention

**Purpose**: Compliant audit logging.

**Deliverables**:
- `src/telemetry/audit.rs` – AuditLog
- Encrypted JSONL export
- Retention policies
- DSAR support

**Acceptance Criteria**:
- [ ] Logs encrypted at rest
- [ ] Retention enforced
- [ ] Export for DSAR works

---

### WS11 – Testing & Red Team Tooling

**Context**: Ensure controls remain effective as threats evolve.

**Timeline**: Sprint 11-12 (2-3 weeks)

#### WS11-01 – Adversarial Corpus

**Purpose**: Curated attack dataset.

**Deliverables**:
- `testing/corpus/` – Adversarial prompts
- Category labels
- Licensing documentation
- Update process

**Corpus Sources**:
- OWASP prompt injection examples
- Rebuff attack patterns
- Custom honeytokens
- Jailbreak datasets (with licensing)

**Acceptance Criteria**:
- [ ] Corpus covers all categories
- [ ] Labels accurate
- [ ] Licensing clear

#### WS11-02 – Attack Harness

**Purpose**: Automated security testing.

**Deliverables**:
- `src/testing/harness.rs` – AttackHarness
- Pipeline testing
- Coverage metrics
- CI integration

**Technical Requirements**:
```rust
let harness = AttackHarness::new()
    .with_corpus(AdversarialCorpus::load("corpus/")?)
    .with_pipeline(input_pipeline);

let results = harness.run().await;
println!("Detection rate: {}%", results.detection_rate * 100.0);
println!("False positive rate: {}%", results.false_positive_rate * 100.0);
```

**Acceptance Criteria**:
- [ ] All corpus tested
- [ ] Metrics accurate
- [ ] CI job runs nightly

#### WS11-03 – Regression Suites

**Purpose**: Prevent security regressions.

**Deliverables**:
- `tests/regression/` – Test suites
- Pipeline ordering tests
- Degrade mode tests
- Error handling tests

**Acceptance Criteria**:
- [ ] All edge cases covered
- [ ] CI blocks on failures
- [ ] Easy to add new cases

#### WS11-04 – Red Team Playbook

**Purpose**: Guide manual security testing.

**Deliverables**:
- `docs/red_team_playbook.md`
- Scenario descriptions
- Success metrics
- Frequency recommendations

**Acceptance Criteria**:
- [ ] All threat actors covered
- [ ] Clear procedures
- [ ] Metrics defined

---

### WS12 – Supply Chain & Release Hygiene

**Context**: Prevent tampering and ensure traceability (OWASP LLM-03/10, SSDF).

**Timeline**: Sprint 12 (2 weeks)

#### WS12-01 – SBOM Generation

**Purpose**: Software Bill of Materials.

**Deliverables**:
- CI job for CycloneDX generation
- SBOM stored with releases
- Verification tooling

**Acceptance Criteria**:
- [ ] SBOM generated on release
- [ ] Format valid
- [ ] Stored accessibly

#### WS12-02 – AI Bill of Materials (NEW)

**Purpose**: Track AI/ML components.

**Deliverables**:
- `src/supply_chain/aibom.rs` – AIBOM generator
- Model provenance tracking
- Training data documentation

**Content**:
- Model identifiers and versions
- Model checksums
- Training data sources (when known)
- Fine-tuning documentation

**Acceptance Criteria**:
- [ ] AIBOM generated
- [ ] Models tracked
- [ ] Format standard-compliant

#### WS12-03 – Dependency Audits

**Purpose**: Continuous dependency security.

**Deliverables**:
- CI integration for `cargo audit`
- `deny.toml` configuration
- License compliance checks

**Acceptance Criteria**:
- [ ] Audits run on PR
- [ ] Blocking on critical
- [ ] License policy enforced

#### WS12-04 – Signed Releases

**Purpose**: Release integrity.

**Deliverables**:
- GitHub Actions for signing
- Cosign for container images
- Verification documentation

**Acceptance Criteria**:
- [ ] Git tags signed
- [ ] Container images signed
- [ ] Verification works

---

### WS13 – Developer Experience & Adoption

**Context**: Enable adoption without specialist knowledge.

**Timeline**: Sprint 12-13 (2-3 weeks)

#### WS13-01 – Integration Guide

**Purpose**: Step-by-step adoption documentation.

**Deliverables**:
- `docs/integration_guide.md`
- Config examples
- Migration guide (from no security)
- Troubleshooting

**Acceptance Criteria**:
- [ ] Complete walkthrough
- [ ] Examples tested
- [ ] Common issues addressed

#### WS13-02 – Examples

**Purpose**: Working code examples.

**Deliverables**:
- `examples/basic_guardrails.rs`
- `examples/rag_secured.rs`
- `examples/mcp_secured.rs`
- `examples/agentic_boundaries.rs`
- `examples/full_pipeline.rs`

**Acceptance Criteria**:
- [ ] All examples run
- [ ] Well-documented
- [ ] Progressive complexity

#### WS13-03 – CLI Enhancements

**Purpose**: Developer tooling.

**Deliverables**:
- `cargo xtask security init-policy`
- `cargo xtask security scan-templates`
- `cargo xtask security scan-corpus`
- `cargo xtask security rotate-honeytokens`
- `cargo xtask security run-attack-suite`

**Acceptance Criteria**:
- [ ] All commands documented
- [ ] Help text clear
- [ ] Exit codes meaningful

#### WS13-04 – FAQ & Troubleshooting

**Purpose**: Self-service support.

**Deliverables**:
- `docs/faq.md`
- Latency tuning guide
- False positive handling
- CI/CD integration

**Acceptance Criteria**:
- [ ] Common questions covered
- [ ] Solutions tested
- [ ] Links to detailed docs

---

## 6. External Dependencies

| Dependency | Usage | Version | Security Notes |
|------------|-------|---------|----------------|
| `ring` | PBKDF2, honeytokens | ≥0.17 | Audited crypto |
| `zeroize` | Secret clearing | ≥1.7 | Memory safety |
| `ammonia` | HTML sanitization | ≥3.3 | Actively maintained |
| `validator` | Input validation | ≥0.16 | Type-safe |
| `pyo3` (optional) | Python ML integration | ≥0.20 | Sandboxing recommended |
| `onnxruntime` (optional) | Local classifiers | ≥0.17 | ONNX format risks |
| `tiktoken-rs` | Token counting | ≥0.5 | Byte-level encoding |
| `governor` | Rate limiting | ≥0.6 | Proven algorithms |
| `tracing` | Structured logging | ≥0.1.40 | Async-safe |
| `opentelemetry` | OTLP export | ≥0.21 | Standard protocol |
| `reqwest` | HTTP client | ≥0.11 | TLS by default |
| `serde` | Serialization | ≥1.0 | YAML/JSON support |
| `jsonschema` | Schema validation | ≥0.17 | Draft 2020-12 |

**Dependency Hygiene**:
- `cargo deny` runs in CI (licenses, duplicates, yanked)
- `cargo audit` runs nightly
- Renovate/Dependabot for automated updates
- SBOM generated with each release

---

## 7. Risk Register

| ID | Threat | Likelihood | Impact | Mitigation(s) | Owner | Status |
|----|--------|------------|--------|---------------|-------|--------|
| R1 | Prompt injection bypass | High | Critical | Multi-stage pipeline, ML classifier, continuous testing | Security Lead | In Design |
| R2 | System prompt extraction | High | High | Fragmentation, honeytokens, egress scan | Security Lead | In Design |
| R3 | PII in training/outputs | Medium | Critical | Presidio scan, differential privacy, data governance | Data Protection | In Design |
| R4 | RAG poisoning | Medium | High | Domain allowlist, signature, quarantine | Security Lead | In Design |
| R5 | MCP confused deputy | Medium | High | Token binding, scope validation | Security Lead | In Design |
| R6 | Agent autonomy abuse | Medium | Critical | Boundaries, budgets, kill switch | Security Lead | In Design |
| R7 | Cost explosion (DoS) | Medium | Medium | Rate limiting, budgets, circuit breakers | Ops | In Design |
| R8 | Embedding inversion | Low | Medium | Access control, tenant isolation | Security Lead | In Design |
| R9 | Supply chain compromise | Low | Critical | SBOM, signing, audit | Security Lead | In Design |
| R10 | Model substitution | Low | High | AIBOM, checksums, provenance | Security Lead | In Design |

**Risk Review Cadence**: Bi-weekly during development, monthly post-launch

---

## 8. Rollout Strategy

### Phase 1: Foundation (Sprints 1-4)
- Config management
- Core pipeline framework
- Prompt protection (honeytokens, fragmentation)
- Basic input validation

**Gate**: Prompt injection POC working, 0 critical deps, docs complete

### Phase 2: Defense in Depth (Sprints 5-8)
- Full input pipeline
- Output validation
- Tool/MCP security
- RAG hardening

**Gate**: All OWASP LLM Top 10 addressed, attack suite passing

### Phase 3: Agentic & Operations (Sprints 9-11)
- Agent security controls
- Abuse prevention
- Telemetry & incidents
- Testing infrastructure

**Gate**: Full pipeline <100ms P95, monitoring operational

### Phase 4: Polish & Release (Sprints 12-13)
- Supply chain
- Developer experience
- Performance tuning
- Documentation

**Gate**: External audit passed, examples run clean, SBOM generated

---

## 9. Sprint Roadmap (13-week plan)

```
Sprint 1-2 (Weeks 1-4):    WS1 (Config), WS2 (Pipeline Framework)
Sprint 3-4 (Weeks 5-8):    WS3 (Prompt Protection), WS4 (Input Security)
Sprint 5-6 (Weeks 9-12):   WS5 (Output Validation)
Sprint 6-7 (Weeks 11-14):  WS6 (Tool/MCP Security)
Sprint 7-8 (Weeks 13-16):  WS7 (RAG Hardening)
Sprint 8-9 (Weeks 15-18):  WS8 (Agentic Security)
Sprint 9-10 (Weeks 17-20): WS9 (Abuse Controls)
Sprint 10-11 (Weeks 19-22):WS10 (Telemetry/Incidents)
Sprint 11-12 (Weeks 21-24):WS11 (Testing), WS12 (Supply Chain)
Sprint 12-13 (Weeks 23-26):WS13 (DX), Hardening, Release Prep
```

### Sprint-by-Sprint Breakdown

| Sprint | Focus | Key Deliverables | Success Metrics |
|--------|-------|------------------|-----------------|
| 1 | Config | PolicyConfig, YAML loading, env overrides | Config tests pass |
| 2 | Pipeline | SecurityPipeline trait, builder, async | Pipeline compiles |
| 3 | Prompt | PromptGuard, fragmentation, honeytokens | Leakage detected in tests |
| 4 | Input | InputValidator, injection scanner, PII | OWASP prompts blocked |
| 5 | Output | SchemaValidator, Sanitizer, CodeGuard | XSS blocked |
| 6 | Output+Tools | EgressScanner, ToolPolicy, ExecutionGuard | Secrets not leaked |
| 7 | Tools+RAG | MCP layer, SanitizedIngestion | Confused deputy blocked |
| 8 | RAG+Agents | GroundedRails, DelegationTracker | Grounding works |
| 9 | Agents+Abuse | AutonomyBoundaries, RateLimiter | Kill switch tested |
| 10 | Abuse+Telemetry | CostMonitor, SecurityEvent, OTLP | Metrics export works |
| 11 | Telemetry+Testing | IncidentOrchestrator, AttackHarness | Attack suite >95% |
| 12 | Supply+DX | SBOM, AIBOM, integration guide | SBOM valid |
| 13 | Polish | Examples, FAQ, performance tuning | P95 <100ms |

---

## 10. MVP Cut (v0.1.0 Target)

For initial release, focus on highest-impact controls:

### Included in MVP
- [x] `config` module – Full implementation
- [x] `pipeline` module – Full implementation
- [x] `prompt` module – PromptGuard, honeytokens
- [x] `input` module – InputValidator, injection scanner, PII stage
- [x] `output` module – SchemaValidator, Sanitizer
- [x] `telemetry` module – Basic events, OTLP export
- [x] `abuse` module – RateLimiter, RecursionGuard
- [x] Examples – `basic_guardrails.rs`, `full_pipeline.rs`
- [x] Integration guide

### Deferred to v0.2.0
- [ ] Local ML classifier (optional dependency)
- [ ] Advanced grounding validation
- [ ] Full RAG security suite
- [ ] Agent security (delegation, memory protection)
- [ ] Corpus scanner CLI
- [ ] Red team playbook
- [ ] Grafana dashboards

### Deferred to v0.3.0
- [ ] Python integration
- [ ] Advanced MCP security
- [ ] Inter-agent security
- [ ] AIBOM generation
- [ ] Automated red teaming

---

## 11. Release Checklist

Before each release:

- [ ] All tests pass (unit, integration, property)
- [ ] `cargo clippy` clean
- [ ] `cargo fmt` applied
- [ ] `cargo audit` clean (no critical)
- [ ] `cargo deny check` passes
- [ ] Documentation updated
- [ ] CHANGELOG updated
- [ ] Version bumped
- [ ] Attack suite run (>95% detection)
- [ ] Performance benchmarks run (P95 <100ms)
- [ ] SBOM generated
- [ ] Git tag signed
- [ ] Container image signed (if applicable)

---

## 12. Appendices

### A. OWASP LLM Top 10 (2025) Quick Reference

| ID | Name | wg-bastion Coverage |
|----|------|---------------------|
| LLM01 | Prompt Injection | input scanner, prompt fragmentation |
| LLM02 | Sensitive Information Disclosure | PII stage, egress scanner, honeytokens |
| LLM03 | Supply Chain Vulnerabilities | SBOM, AIBOM, signed releases |
| LLM04 | Data & Model Poisoning | RAG sanitization, provenance |
| LLM05 | Improper Output Handling | schema validator, sanitizer, code guard |
| LLM06 | Excessive Agency | tool policies, execution guard, approval |
| LLM07 | System Prompt Leakage (NEW) | fragmentation, honeytokens, egress scan |
| LLM08 | Vector/Embedding Weaknesses (NEW) | access control, tenant isolation |
| LLM09 | Misinformation | grounding validator, citation |
| LLM10 | Unbounded Consumption | rate limiter, cost monitor, budgets |

### B. NIST AI RMF Mapping

| Function | wg-bastion Controls |
|----------|---------------------|
| GOVERN | PolicyConfig, documentation |
| MAP | Threat modeling, risk register |
| MEASURE | Attack harness, regression suites |
| MANAGE | Incident orchestrator, telemetry |

### C. EU AI Act Considerations

- **Risk Classification**: Most LLM apps are limited risk; some may be high-risk
- **Transparency**: Prompt protection helps demonstrate AI disclosure
- **Documentation**: SBOM/AIBOM support documentation requirements
- **Human Oversight**: Approval workflow supports human-in-the-loop requirements

### D. Feature Flag Reference

| Flag | Default | Description |
|------|---------|-------------|
| `ml-classifiers` | off | ONNX-based injection detection |
| `python-integration` | off | PyO3 for Python ML models |
| `redis-backend` | off | Distributed rate limiting |
| `postgres-backend` | off | Enterprise audit logging |
| `full-telemetry` | off | All OTLP features |
| `strict-mode` | off | Aggressive blocking |
| `rag-security` | on | RAG hardening features |
| `agent-security` | on | Agent boundary features |
| `mcp-security` | on | MCP protocol security |

---

## 13. Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2023-xx-xx | Original | Initial plan |
| 2.0 | 2025-01-xx | Updated | Comprehensive rewrite for 2025 standards |

### Version 2.0 Changes Summary
- Updated to OWASP LLM Top 10 2025 (added LLM07, LLM08)
- Added OWASP Agentic AI Top 10 coverage
- Added NIST AI RMF 1.0 / AI 600-1 alignment
- Added EU AI Act considerations
- Added MCP security per 2025-11-25 specification
- Expanded to 13 workstreams from original 8
- Added detailed acceptance criteria throughout
- Added feature flags for optional components
- Added comprehensive testing infrastructure
- Added supply chain security (SBOM, AIBOM)
- Added sprint roadmap with success metrics
- Added risk register
- Added rollout strategy with gates

---

*End of wg-bastion Plan v2.0*

