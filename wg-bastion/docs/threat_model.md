# wg-bastion Threat Model

**Version:** 1.0  
**Last Updated:** January 2026  
**Status:** Active - Sprint 1 (WS1-01)

This document provides a comprehensive threat analysis for wg-bastion and applications built with the security framework. It incorporates attack trees, data flow diagrams, and per-threat response playbooks aligned with OWASP LLM Top 10 2025, NIST AI RMF, and MITRE ATLAS.

---

## Table of Contents

1. [Asset Inventory](#asset-inventory)
2. [Trust Boundaries](#trust-boundaries)
3. [Data Flow Diagrams](#data-flow-diagrams)
4. [Threat Actor Profiles](#threat-actor-profiles)
5. [Attack Trees by OWASP Category](#attack-trees-by-owasp-category)
6. [MITRE ATLAS Mapping](#mitre-atlas-mapping)
7. [Response Playbooks](#response-playbooks)

---

## Asset Inventory

Critical assets requiring protection:

| Asset | Classification | Threats | Controls |
|-------|---------------|---------|----------|
| **System Prompts / Templates** | Confidential | LLM07:2025 (Leakage), LLM01:2025 (Injection) | Fragmentation, honeytokens, egress scanning |
| **User PII** | Regulated | LLM02:2025 (Disclosure), LLM04:2025 (Poisoning) | PII detection, redaction, audit logging |
| **API Keys / Credentials** | Critical | LLM02:2025 (Disclosure), LLM06:2025 (Misuse) | Secret scanning, zeroization, env isolation |
| **Vector Embeddings / RAG Corpus** | Sensitive | LLM08:2025 (Embedding Weaknesses), LLM04:2025 (Poisoning) | Access control, provenance, ingestion validation |
| **Tool Execution Context** | Trusted | LLM06:2025 (Excessive Agency), MCP security | Policy enforcement, approval flows, session binding |
| **Agent State / Memory** | Persistent | Agentic AI threats, Memory poisoning | State validation, signatures, boundaries |
| **Cost/Budget Allocations** | Business | LLM10:2025 (Unbounded Consumption) | Rate limiting, budget monitors, alerts |
| **Audit Logs** | Compliance | Tampering, deletion | Encrypted storage, retention policies, WORM |
| **Security Configuration** | Operational | Misconfiguration, insider tampering | Validation, version control, change auditing |

---

## Trust Boundaries

```text
┌─────────────────────────────────────────────────────────────────┐
│ UNTRUSTED ZONE                                                  │
│                                                                 │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐       │
│  │ End Users    │   │ RAG Sources  │   │ MCP Servers  │       │
│  │ (Prompts)    │   │ (Documents)  │   │ (Tools)      │       │
│  └──────┬───────┘   └──────┬───────┘   └──────┬───────┘       │
│         │                  │                   │                │
└─────────┼──────────────────┼───────────────────┼────────────────┘
          │                  │                   │
          ▼                  ▼                   ▼
┌─────────────────────────────────────────────────────────────────┐
│ SECURITY BOUNDARY (wg-bastion)                                  │
│                                                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │ Input       │  │ RAG         │  │ Tool        │            │
│  │ Pipeline    │  │ Security    │  │ Guard       │            │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘            │
│         │                │                 │                    │
│         └────────────────┴─────────────────┘                    │
│                          │                                      │
└──────────────────────────┼──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│ SEMI-TRUSTED ZONE (weavegraph application)                      │
│                                                                 │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐       │
│  │ App Nodes    │   │ State Store  │   │ EventBus     │       │
│  │ (LLM Logic)  │   │ (Versioned)  │   │ (Telemetry)  │       │
│  └──────┬───────┘   └──────┬───────┘   └──────┬───────┘       │
│         │                  │                   │                │
└─────────┼──────────────────┼───────────────────┼────────────────┘
          │                  │                   │
          ▼                  ▼                   ▼
┌─────────────────────────────────────────────────────────────────┐
│ TRUSTED ZONE (Infrastructure)                                   │
│                                                                 │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐       │
│  │ LLM Provider │   │ Vector DB    │   │ Audit Store  │       │
│  │ (OpenAI/etc) │   │ (SQLite)     │   │ (OTLP)       │       │
│  └──────────────┘   └──────────────┘   └──────────────┘       │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**Key Observations**:
- All external inputs cross the security boundary and must be validated
- RAG sources are untrusted by default (indirect injection risk)
- MCP servers require session binding and scope validation
- weavegraph application is semi-trusted (may have logic bugs)
- Infrastructure is trusted (but should be monitored)

---

## Data Flow Diagrams

See [diagrams/data_flow.mmd](diagrams/data_flow.mmd) for detailed Mermaid flowcharts.

### High-Level Data Flow

```text
User Prompt ──► Input Pipeline ──► Prompt Guard ──► App Node (LLM)
                     │                  │                  │
                     ├─ Injection?      ├─ Honeytoken?    │
                     ├─ PII?            └─ Fragment        │
                     └─ Moderation?                        │
                                                           │
RAG Query ──────────► RAG Security ──────────────────────►│
                          │                                │
                          ├─ Sanitize Ingestion           │
                          ├─ Provenance Tag               │
                          └─ Access Control               │
                                                           │
Tool Request ────────► Tool Guard ─────────────────────► Tool Execution
                          │                                │
                          ├─ Policy Check                  │
                          ├─ MCP Validation                │
                          └─ Approval (if needed)          │
                                                           │
LLM Response ──────────────────────────────────────────► Output Pipeline
                                                               │
                                                               ├─ Schema Valid?
                                                               ├─ Egress Scan
                                                               ├─ Sanitize
                                                               └─ Grounding Check
                                                               │
                                                               ▼
                                                         User Response

All Stages ──────────────────────────────────────────► Telemetry Sink
                                                           │
                                                           ├─ Security Events
                                                           ├─ Audit Log
                                                           └─ Incident Detection
```

---

## Threat Actor Profiles

| Threat Actor | Profile | Motivations | Primary Attacks | Skill Level |
|--------------|---------|-------------|-----------------|-------------|
| **Malicious End-Users** | External users via UI/API | Data exfiltration, guardrail bypass, abuse | Prompt injection, jailbreak, PII harvesting, tool manipulation | Low to Medium |
| **Adversarial Retrievers** | Poisoned documents/web pages | Indirect injection, misinformation, credential theft | RAG corpus poisoning, context injection | Medium |
| **MCP Confused Deputy** | Compromised/malicious MCP servers | Session hijacking, lateral movement | Token reuse, scope abuse, DNS rebinding | Medium to High |
| **Rogue Insiders** | Developers/operators | System prompt theft, control bypass, data exfiltration | Config tampering, honeytoken detection, log manipulation | High |
| **Automated Adversaries** | Bots/scripts | Resource exhaustion, cost explosion | High-volume attacks, model extraction | Low (volume-based) |
| **Supply Chain Attackers** | Dependency/build compromise | Backdoor insertion, data theft | Malicious deps, model poisoning | High |
| **Multi-Agent Exploiters** | Sophisticated attackers | Privilege escalation, boundary violations | Agent chain manipulation, trust exploitation | High |
| **Embedding Inverters** | ML researchers/adversaries | Training data extraction | Embedding inversion, membership inference | Very High |

---

## Attack Trees by OWASP Category

### LLM01:2025 - Prompt Injection

**Root Goal**: Bypass security controls via malicious prompts

```
[Prompt Injection Success]
├─► [Direct Injection]
│   ├─► Role manipulation ("Ignore previous instructions...")
│   ├─► Delimiter confusion ("### SYSTEM ###")
│   ├─► Adversarial suffix (gradient-optimized tokens)
│   └─► Multilingual obfuscation (Unicode tricks)
│
├─► [Indirect Injection]
│   ├─► RAG corpus poisoning
│   ├─► Web page injection (fetched content)
│   ├─► File upload injection (document content)
│   └─► Agent memory poisoning
│
└─► [Context Window Attacks]
    ├─► Prompt stuffing (exhaust system prompt)
    ├─► Instruction hierarchy confusion
    └─► Multimodal injection (image/audio)
```

**Likelihood**: High (actively exploited in the wild)  
**Impact**: Critical (full control bypass)  
**Detection Strategy**: Multi-stage pipeline (heuristic + ML), honeytokens, metadata  
**Playbook**: [attack_playbooks/llm01_prompt_injection.md](attack_playbooks/llm01_prompt_injection.md)

---

### LLM02:2025 - Sensitive Information Disclosure

**Root Goal**: Extract PII, credentials, or confidential data

```
[Data Leakage Success]
├─► [Output Channel]
│   ├─► Direct response leakage (model training data)
│   ├─► System prompt extraction (via injection)
│   ├─► Honeytoken detection (reverse engineering)
│   └─► Error message leakage
│
├─► [Side Channels]
│   ├─► Timing attacks (response latency)
│   ├─► Log exfiltration (telemetry data)
│   └─► Embedding inversion (vector stores)
│
└─► [Storage/Transit]
    ├─► Insecure checkpointing (plaintext state)
    ├─► Unencrypted audit logs
    └─► TLS downgrade attacks
```

**Likelihood**: Medium (requires specific conditions)  
**Impact**: Critical (regulatory compliance violations)  
**Detection Strategy**: PII scanning, egress monitoring, honeytoken alerts  
**Playbook**: [attack_playbooks/llm02_data_disclosure.md](attack_playbooks/llm02_data_disclosure.md)

---

### LLM03:2025 - Supply Chain Vulnerabilities

**Root Goal**: Compromise wg-bastion or its dependencies

```
[Supply Chain Attack]
├─► [Dependency Compromise]
│   ├─► Malicious crate upload (crates.io typosquatting)
│   ├─► Compromised maintainer account
│   ├─► Vulnerable transitive dependency
│   └─► License poisoning
│
├─► [Model/Weight Tampering]
│   ├─► Backdoored ML classifier
│   ├─► Poisoned embeddings model
│   └─► Trojan in ONNX runtime
│
└─► [Build/Release Tampering]
    ├─► CI/CD pipeline compromise
    ├─► Unsigned release artifacts
    └─► Container image manipulation
```

**Likelihood**: Low (requires sophisticated attacker)  
**Impact**: Critical (full compromise)  
**Detection Strategy**: SBOM/AIBOM tracking, cargo-deny, signed releases  
**Playbook**: [attack_playbooks/llm03_supply_chain.md](attack_playbooks/llm03_supply_chain.md)

---

### LLM04:2025 - Data and Model Poisoning

**Root Goal**: Corrupt training data or model behavior

```
[Poisoning Attack]
├─► [Training Data Poisoning]
│   ├─► RAG corpus injection (pre-ingestion)
│   ├─► Feedback loop poisoning (RLHF)
│   └─► Embedding backdoors
│
├─► [Model Backdoors]
│   ├─► Trigger word activation
│   ├─► Sleeper agent behavior
│   └─► Adversarial weight perturbation
│
└─► [Runtime State Poisoning]
    ├─► Agent memory corruption
    ├─► Versioned state tampering
    └─► Cache poisoning
```

**Likelihood**: Medium (RAG/agent systems vulnerable)  
**Impact**: High (subtle behavior changes)  
**Detection Strategy**: Provenance tagging, signature verification, anomaly detection  
**Playbook**: [attack_playbooks/llm04_poisoning.md](attack_playbooks/llm04_poisoning.md)

---

### LLM05:2025 - Improper Output Handling

**Root Goal**: Exploit output rendering or downstream processing

```
[Output Exploit]
├─► [Injection in Outputs]
│   ├─► XSS in web rendering
│   ├─► Terminal escape injection
│   ├─► SQL injection (downstream DB)
│   └─► Code injection (eval'd output)
│
├─► [Schema Violations]
│   ├─► Type confusion attacks
│   ├─► Overflow attacks (size limits)
│   └─► Encoding attacks (charset)
│
└─► [Unsafe Code Output]
    ├─► Shell command generation
    ├─► Executable code blocks
    └─► Unsafe deserialization payloads
```

**Likelihood**: Medium (depends on integration)  
**Impact**: High (downstream system compromise)  
**Detection Strategy**: Schema validation, sanitization, code guard  
**Playbook**: [attack_playbooks/llm05_output_handling.md](attack_playbooks/llm05_output_handling.md)

---

### LLM06:2025 - Excessive Agency

**Root Goal**: Abuse tool execution or autonomous behavior

```
[Excessive Agency]
├─► [Tool Abuse]
│   ├─► Unauthorized tool invocation
│   ├─► Tool chain exploitation (A→B→C escalation)
│   ├─► MCP confused deputy
│   └─► Parameter injection in tool args
│
├─► [Autonomy Violations]
│   ├─► Budget exhaustion (cost explosion)
│   ├─► Recursion abuse (infinite loops)
│   ├─► Goal hijacking (context injection)
│   └─► Kill switch bypass
│
└─► [Delegation Attacks]
    ├─► Capability escalation (agent chains)
    ├─► Trust boundary violations
    └─► Inter-agent impersonation
```

**Likelihood**: High (agentic systems growing)  
**Impact**: Critical (unauthorized actions)  
**Detection Strategy**: Tool policies, approval flows, budget monitors, delegation tracking  
**Playbook**: [attack_playbooks/llm06_excessive_agency.md](attack_playbooks/llm06_excessive_agency.md)

---

### LLM07:2025 - System Prompt Leakage (NEW)

**Root Goal**: Extract system prompts or internal instructions

```
[Prompt Leakage]
├─► [Direct Extraction]
│   ├─► "Repeat your instructions" attacks
│   ├─► Completion manipulation
│   └─► Delimiter confusion
│
├─► [Indirect Extraction]
│   ├─► Behavioral inference (black-box)
│   ├─► Error message clues
│   └─► Token-by-token extraction
│
└─► [Honeytoken Detection]
    ├─► Pattern recognition (UUID/markers)
    ├─► Frequency analysis
    └─► Statistical inference
```

**Likelihood**: High (common attack)  
**Impact**: High (reveals security design)  
**Detection Strategy**: Fragmentation, honeytokens with unique IDs, egress scanning  
**Playbook**: [attack_playbooks/llm07_prompt_leakage.md](attack_playbooks/llm07_prompt_leakage.md)

---

### LLM08:2025 - Vector and Embedding Weaknesses (NEW)

**Root Goal**: Exploit RAG/vector store vulnerabilities

```
[Embedding Attack]
├─► [Access Control Bypass]
│   ├─► Unauthenticated vector search
│   ├─► Cross-tenant data access
│   └─► Privilege escalation via similarity
│
├─► [Embedding Inversion]
│   ├─► Training data reconstruction
│   ├─► Membership inference
│   └─► Attribute inference
│
└─► [Retrieval Manipulation]
    ├─► Adversarial query crafting
    ├─► Index poisoning
    └─► Federation conflicts (contradictory sources)
```

**Likelihood**: Medium (emerging threat)  
**Impact**: High (data leakage, misinformation)  
**Detection Strategy**: Access control, tenant isolation, inversion detection, provenance  
**Playbook**: [attack_playbooks/llm08_embedding_weaknesses.md](attack_playbooks/llm08_embedding_weaknesses.md)

---

### LLM09:2025 - Misinformation

**Root Goal**: Generate false, misleading, or harmful content

```
[Misinformation]
├─► [Hallucination]
│   ├─► Fabricated facts
│   ├─► Ungrounded reasoning
│   └─► Source misattribution
│
├─► [Manipulation]
│   ├─► Biased outputs
│   ├─► Harmful stereotypes
│   └─► Adversarial framing
│
└─► [Grounding Failures]
    ├─► Ignoring retrieved context
    ├─► Citation errors
    └─► Contradictory sources
```

**Likelihood**: High (inherent LLM behavior)  
**Impact**: Medium to High (depends on domain)  
**Detection Strategy**: Grounding validation, citation checks, fact-checking APIs  
**Playbook**: [attack_playbooks/llm09_misinformation.md](attack_playbooks/llm09_misinformation.md)

---

### LLM10:2025 - Unbounded Consumption

**Root Goal**: Exhaust resources or explode costs

```
[Resource Abuse]
├─► [Cost Explosion]
│   ├─► High-volume prompt spam
│   ├─► Expensive tool invocations
│   └─► Excessive context window usage
│
├─► [Denial of Service]
│   ├─► Infinite recursion
│   ├─► Slow queries (complexity attacks)
│   └─► Memory exhaustion
│
└─► [Model Extraction]
    ├─► High-frequency API queries
    ├─► Systematic prompt probing
    └─► Weight reconstruction attacks
```

**Likelihood**: Medium (opportunistic attackers)  
**Impact**: High (financial/availability)  
**Detection Strategy**: Rate limiting, cost monitors, recursion guards, circuit breakers  
**Playbook**: [attack_playbooks/llm10_unbounded_consumption.md](attack_playbooks/llm10_unbounded_consumption.md)

---

## MITRE ATLAS Mapping

| ATLAS Technique | OWASP LLM | wg-bastion Module | Priority |
|-----------------|-----------|-------------------|----------|
| AML.T0051 (LLM Prompt Injection) | LLM01 | `input.injection`, `prompt` | P0 |
| AML.T0054 (LLM Jailbreak) | LLM01, LLM05 | `input.moderation`, `output` | P0 |
| AML.T0057 (LLM Data Leakage) | LLM02, LLM07 | `output.egress`, `prompt.honeytokens` | P0 |
| AML.T0043 (Craft Adversarial Data) | LLM04 | `rag.ingestion`, `input.normalization` | P1 |
| AML.T0048 (Embed Malware) | LLM03 | `tools.fetch`, `rag.ingestion` | P1 |
| AML.T0040 (ML Model Inference API) | LLM10 | `abuse.rate_limiting` | P1 |
| AML.T0020 (Poison Training Data) | LLM04 | `rag.provenance`, `agents.memory` | P2 |
| AML.T0024 (Backdoor ML Model) | LLM03 | `supply_chain`, `aibom` | P2 |

---

## Response Playbooks

For each OWASP LLM category, we maintain detailed response playbooks in [attack_playbooks/](attack_playbooks/):

- **Detection**: How to recognize the attack (indicators, patterns, metrics)
- **Containment**: Immediate actions to stop the attack (block, degrade, isolate)
- **Eradication**: How to remove the threat (clean state, rotate credentials)
- **Recovery**: How to restore service (rollback, re-enable controls)
- **Lessons Learned**: Post-incident analysis and control improvements

**Playbook Index**:
1. [LLM01: Prompt Injection](attack_playbooks/llm01_prompt_injection.md)
2. [LLM02: Data Disclosure](attack_playbooks/llm02_data_disclosure.md)
3. [LLM03: Supply Chain](attack_playbooks/llm03_supply_chain.md)
4. [LLM04: Poisoning](attack_playbooks/llm04_poisoning.md)
5. [LLM05: Output Handling](attack_playbooks/llm05_output_handling.md)
6. [LLM06: Excessive Agency](attack_playbooks/llm06_excessive_agency.md)
7. [LLM07: Prompt Leakage](attack_playbooks/llm07_prompt_leakage.md)
8. [LLM08: Embedding Weaknesses](attack_playbooks/llm08_embedding_weaknesses.md)
9. [LLM09: Misinformation](attack_playbooks/llm09_misinformation.md)
10. [LLM10: Unbounded Consumption](attack_playbooks/llm10_unbounded_consumption.md)

---

## Next Steps

- **WS1-02**: Create control matrix mapping these threats to specific wg-bastion modules
- **WS11**: Build adversarial corpus with examples from each attack tree branch
- **Continuous**: Update threat model quarterly as new attack techniques emerge

**Review Cadence**: Quarterly threat model review, monthly playbook updates based on incidents

---

*Last updated: January 2026 - Sprint 1 (WS1-01)*
