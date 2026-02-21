# wg-bastion Control Matrix

**Version:** 1.0  
**Last Updated:** January 2026  
**Purpose:** Traceability between security requirements and implementation

This document provides a human-readable narrative of the control matrix. The machine-readable version is maintained in [control_matrix.csv](control_matrix.csv).

---

## Overview

The control matrix maps security requirements from multiple frameworks to specific wg-bastion modules, implementations, and test identifiers. This enables:

- **Traceability**: Link threats → controls → code → tests
- **Compliance**: Demonstrate OWASP/NIST/EU AI Act coverage
- **Auditability**: Verify all requirements are implemented
- **Testability**: Ensure every control has verification tests

---

## Framework Coverage Summary

| Framework | Total Controls | Implemented | Planned | Coverage |
|-----------|----------------|-------------|---------|----------|
| OWASP LLM Top 10 2025 | 42 | 6 | 36 | 100% (scope) |
| NIST AI RMF Functions | 70 | 6 | 64 | 100% (scope) |
| EU AI Act Articles | 12 | 0 | 12 | 100% (applicable) |
| MITRE ATLAS Techniques | 6 | 0 | 6 | 100% (mapped) |

**Status Key**:
- **Implemented**: Code complete, tests passing, documented
- **In Progress**: Partial implementation, active development
- **Planned**: Scope defined, not yet started

---

## Control Breakdown by OWASP Category

### LLM01:2025 - Prompt Injection

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C001 | Injection Pattern Scanner | `input::injection` | Planned |
| C002 | ML-Based Injection Classifier | `input::moderation` | Planned |
| C003 | Injection Detection Telemetry | `telemetry::events` | Planned |

**Implementation Notes**:
- C001 uses regex/keyword patterns for fast heuristic detection
- C002 is optional (requires `moderation-onnx` feature) for advanced detection
- C003 emits structured events for SIEM integration

### LLM02:2025 - Sensitive Information Disclosure

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C004 | PII Detection Scanner | `input::pii` | Planned |
| C005 | PII Redaction/Masking | `input::pii` | Planned |
| C006 | PII Detection Audit Log | `telemetry::audit` | Planned |
| C007 | Egress Secret Scanner | `output::egress` | Planned |

**Implementation Notes**:
- C004 integrates with Microsoft Presidio (optional `pii-presidio` feature)
- C005 supports redaction (replace with `[REDACTED]`) or masking (partial reveal)
- C007 scans for API keys, passwords, honeytokens in LLM outputs

### LLM03:2025 - Supply Chain Vulnerabilities

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C008 | SBOM Generation | `supply_chain::sbom` | Planned |
| C009 | AIBOM Generation | `supply_chain::aibom` | Planned |
| C010 | Dependency Audit | `supply_chain::audit` | Planned |
| C011 | License Compliance | `supply_chain::audit` | Planned |
| C012 | Signed Releases | `supply_chain::signing` | Planned |

**Implementation Notes**:
- C008/C009 run in CI to generate CycloneDX SBOM and AI Bill of Materials
- C010/C011 use `cargo-audit` and `cargo-deny` for continuous monitoring
- C012 signs Git tags and container images (Cosign)

### LLM04:2025 - Data and Model Poisoning

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C013 | RAG Ingestion Sanitization | `rag::ingestion` | Planned |
| C014 | Document Provenance Tagging | `rag::provenance` | Planned |
| C015 | Document Hash/Signature | `rag::ingestion` | Planned |
| C016 | Agent Memory Validation | `agents::memory` | Planned |

**Implementation Notes**:
- C013 sanitizes HTML, removes scripts, validates URLs before embedding
- C014 tags every chunk with source URL, timestamp, trust level
- C016 validates agent state updates, detects poisoning attempts

### LLM05:2025 - Improper Output Handling

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C017 | Schema Validation | `output::schema` | Planned |
| C018 | HTML Sanitization | `output::sanitizer` | Planned |
| C019 | Terminal Escape Removal | `output::sanitizer` | Planned |
| C020 | Code Output Guard | `output::code_guard` | Planned |

**Implementation Notes**:
- C017 supports JSON Schema and custom DSL for structured output validation
- C018 uses `ammonia` crate for HTML sanitization (allowlist-based)
- C019 strips ANSI escape codes to prevent terminal injection

### LLM06:2025 - Excessive Agency

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C021 | Tool Policy Schema | `tools::policy` | Planned |
| C022 | Tool Execution Guard | `tools::guard` | Planned |
| C023 | Tool Execution Audit | `telemetry::audit` | Planned |
| C024 | Human Approval Workflow | `tools::approval` | Planned |
| C025 | MCP Session Binding | `tools::mcp` | Planned |
| C026 | MCP Token Binding | `tools::mcp` | Planned |
| C027 | MCP Confused Deputy Prevention | `tools::mcp` | Planned |
| C028 | Agent Delegation Tracking | `agents::delegation` | Planned |
| C029 | Agent Autonomy Boundaries | `agents::boundaries` | Planned |

**Implementation Notes**:
- C021 defines YAML/JSON policies for tool allowlists, risk levels, quotas
- C024 queues high-risk tool calls for human approval via notification hooks
- C025-C027 implement MCP security per 2025-11-25 spec
- C029 includes kill switch for runaway agents

### LLM07:2025 - System Prompt Leakage (NEW)

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C030 | System Prompt Fragmentation | `prompt::guard` | Planned |
| C031 | Honeytoken Insertion | `prompt::canaries` | Planned |
| C032 | Honeytoken Detection (Egress) | `output::egress` | Planned |
| C033 | Prompt Leakage Incident | `telemetry::incident` | Planned |

**Implementation Notes**:
- C030 splits system prompts across multiple fragments to prevent extraction
- C031 inserts unique UUIDs as honeytokens to detect leakage
- C032 scans all outputs for honeytokens, triggers C033 on detection

### LLM08:2025 - Vector and Embedding Weaknesses (NEW)

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C034 | Vector Store Access Control | `rag::embedding` | Planned |
| C035 | Tenant Isolation (RAG) | `rag::embedding` | Planned |
| C036 | Embedding Inversion Detection | `rag::embedding` | Planned |

**Implementation Notes**:
- C034/C035 enforce ACLs and multi-tenant isolation in SQLite/Redis vector stores
- C036 uses anomaly detection to identify inversion attempts (future research)

### LLM09:2025 - Misinformation

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C037 | Grounding Validator | `output::grounding` | Planned |
| C038 | Citation Generation | `output::grounding` | Planned |
| C039 | Ungrounded Content Warning | `output::grounding` | Planned |

**Implementation Notes**:
- C037 matches LLM claims to retrieved RAG sources
- C038 automatically injects citations into responses
- C039 flags responses that cannot be grounded in context

### LLM10:2025 - Unbounded Consumption

| Control ID | Name | Module | Status |
|------------|------|--------|--------|
| C040 | Multi-Dimensional Rate Limiting | `abuse::rate_limit` | Planned |
| C041 | Token/Cost Budget Tracking | `abuse::cost` | Planned |
| C042 | Recursion Guard | `abuse::recursion` | Planned |
| C043 | Circuit Breaker | `abuse::circuit` | Planned |

**Implementation Notes**:
- C040 tracks limits per user, session, tool, and global dimensions
- C041 monitors token usage and enforces budgets with alerts
- C042 detects infinite loops and runaway agents

---

## NIST AI RMF Function Mapping

### GOVERN (Establish AI governance)
- C008-C012: Supply chain governance (SBOM, audits, signing)
- C044-C046: Policy configuration and fail modes
- C057: Session management
- C062-C068: Documentation and disclosure

### MAP (Identify and contextualize risks)
- C001, C004, C013-C014, C021, C025-C027: Risk identification controls
- C030, C034-C035, C047, C054-C056: Context mapping

### MEASURE (Analyze and track risks)
- C003, C006, C015-C016, C023, C031-C033: Detection and measurement
- C037-C039, C041, C048-C053: Metrics and monitoring
- C059-C061: Testing and validation

### MANAGE (Prioritize and respond to risks)
- C005, C007, C017-C020, C022, C024, C027-C029: Risk mitigation
- C040, C042-C043, C051: Response automation

---

## EU AI Act Article Mapping

| Article | Requirement | Controls | Notes |
|---------|-------------|----------|-------|
| Art 10 | Data Governance | C004-C006 | PII protection, audit trails |
| Art 12 | Record Keeping | C006, C049-C053 | Audit logs, retention policies |
| Art 13 | Transparency | C001-C003, C062-C068 | Documentation, disclosure |
| Art 14 | Human Oversight | C017, C021-C024, C029 | Approval workflows, kill switches |
| Art 15 | Accuracy & Robustness | C008-C009, C037-C039, C062 | Grounding, SBOM, threat model |

**Note**: wg-bastion provides technical controls to support compliance but does not guarantee certification without legal/organizational processes.

---

## Test Coverage Mapping

Every control has at least one associated test ID. Test organization:

- **T-XXX-NNN**: Module-specific tests (XXX = module abbreviation)
- **Integration Tests**: Cross-module scenarios (e.g., full pipeline)
- **Regression Tests**: Prevention of past vulnerabilities
- **Attack Harness**: Adversarial corpus validation

**Example Test Mapping**:
```
C001 (Injection Pattern Scanner) → T-INJ-001
  └─ tests/input/injection.rs::test_known_patterns()
  └─ tests/input/injection.rs::test_adversarial_suffixes()
  └─ tests/regression/llm01_bypasses.rs
```

---

## Gap Analysis

### Current Gaps (Sprint 1)
- **Core implementation**: All modules except `config` and `pipeline` are stubs
- **Testing infrastructure**: Attack harness and corpus not yet built
- **Documentation**: Integration guide and examples pending

### Planned Coverage (Sprint 13)
- **100% OWASP LLM:2025**: All 10 categories with dedicated controls
- **NIST AI RMF alignment**: All four functions addressed
- **EU AI Act hooks**: Technical controls for applicable articles
- **Test coverage**: >95% for security-critical modules

---

## Maintenance Process

### Quarterly Review
1. Update control matrix with new threat intelligence
2. Add controls for emerging attack techniques
3. Review test coverage and address gaps
4. Update compliance mappings for framework changes

### Continuous Updates
- Add test IDs as tests are implemented
- Update status as controls move from Planned → In Progress → Implemented
- Link to code locations once modules are complete

---

## References

- OWASP LLM Top 10 2025: https://owasp.org/www-project-top-10-for-large-language-model-applications/
- NIST AI RMF 1.0: https://www.nist.gov/itl/ai-risk-management-framework
- NIST AI 600-1 (GenAI Profile): https://csrc.nist.gov/pubs/ai/600/1/final
- EU AI Act: https://eur-lex.europa.eu/eli/reg/2024/1689/oj
- wg-bastion Master Plan: [../wg-bastion_plan_v2.md](../../docs/wg-bastion_plan_v2.md)
- Threat Model: [threat_model.md](threat_model.md)

---

*Last updated: January 2026 - Sprint 1 (WS1-02)*
