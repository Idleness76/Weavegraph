# wg-bastion

**Comprehensive security suite for graph-driven LLM applications built on [weavegraph](https://github.com/Idleness76/weavegraph).**

[![Crates.io](https://img.shields.io/crates/v/wg-bastion)](https://crates.io/crates/wg-bastion)
[![Documentation](https://docs.rs/wg-bastion/badge.svg)](https://docs.rs/wg-bastion)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.89%2B-blue.svg)](https://www.rust-lang.org)

---

## Overview

`wg-bastion` provides defense-in-depth security controls for LLM applications, addressing the **OWASP LLM Top 10 (2025)**, **NIST AI RMF**, and modern agentic AI threats. The crate offers opt-in, composable security pipelines with:

- ✅ **Zero-Trust Architecture** – Validate inputs, outputs, tools, and RAG retrievals
- ✅ **Graceful Degradation** – Configurable fail modes (closed/open/log-only)
- ✅ **Minimal Overhead** – <50ms P95 latency target for standard pipelines
- ✅ **Production-Ready** – Structured telemetry, audit logging, incident response

---

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
wg-bastion = "0.1"
weavegraph = "0.1"
```

### Basic Usage

```rust
use wg_bastion::prelude::*;
use weavegraph::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load security policy from configuration
    let policy = PolicyBuilder::new()
        .with_file("config/wg-bastion.toml")?
        .with_env()  // Override with WG_BASTION_* env vars
        .build()?;

    // Integrate with weavegraph (future API)
    let app = GraphBuilder::new()
        .with_security_policy(policy)
        .build()?;

    // Security is now enforced automatically on all requests
    // app.invoke(...).await?;

    Ok(())
}
```

### Configuration Example

Create `wg-bastion.toml`:

```toml
version = "1.0"
enabled = true
fail_mode = "closed"  # Block threats (vs "open" or "log_only")

# Module configurations will be added in subsequent sprints
# [input]
# injection_detection = true
# pii_detection = true

# [output]
# schema_validation = true
# egress_scanning = true
```

---

## Features

### Core Security Modules

| Module | Purpose | OWASP Coverage | Status |
|--------|---------|----------------|--------|
| `config` | Policy management, fail modes | Foundation | ✅ In Progress |
| `pipeline` | Multi-stage security pipeline | Foundation | ✅ In Progress |
| `prompt` | Prompt protection, honeytokens | LLM07 | 🔄 Planned (Sprint 3) |
| `input` | Injection scanning, PII detection | LLM01, LLM02 | 🔄 Planned (Sprint 4) |
| `output` | Schema validation, sanitization | LLM05, LLM09 | 🔄 Planned (Sprint 5-6) |
| `tools` | Tool policies, MCP security | LLM06 | 🔄 Planned (Sprint 6-7) |
| `rag` | RAG ingestion, provenance | LLM04, LLM08 | 🔄 Planned (Sprint 7-8) |
| `agents` | Delegation tracking, boundaries | Agentic AI | 🔄 Planned (Sprint 8-9) |
| `abuse` | Rate limiting, cost monitoring | LLM10 | 🔄 Planned (Sprint 9-10) |
| `telemetry` | Security events, OTLP export | AI RMF Measure | 🔄 Planned (Sprint 10-11) |

### Feature Flags

```toml
[features]
default = ["heuristics"]

# Core functionality
heuristics = []  # Pattern-based detection (no ML deps)
full = ["moderation-onnx", "pii-presidio", "telemetry-otlp"]

# Optional backends
moderation-onnx = ["ort"]           # Local ML classifier
pii-presidio = ["reqwest"]          # Microsoft Presidio API
storage-redis = ["redis"]           # Distributed rate limiting
telemetry-otlp = ["opentelemetry"]  # Full observability

# Development
testing = []
adversarial-corpus = ["testing"]
```

---

## Architecture

```text
SecurityPolicy ─┬─► PolicyBuilder ─► Runtime Policy
                │                     │
                │                     ├─► InputPipeline ──► InjectionScanner, PIIDetector
                │                     ├─► PromptGuard ──► Fragmentation, Honeytokens
                │                     ├─► OutputValidator ──► Schema, Sanitization
                │                     ├─► ToolGuard ──► MCP Security, Approval
                │                     ├─► RagSecurity ──► Ingestion, Provenance
                │                     └─► TelemetrySink ──► Audit, Metrics
                │
                └─► Integration with weavegraph App via hooks and EventBus
```

See [docs/architecture.md](docs/architecture.md) for the complete design.

---

## Documentation

- **[Architecture Guide](docs/architecture.md)** – Module design and integration patterns
- **[Threat Model](docs/threat_model.md)** – Attack trees, actor profiles, playbooks
- **[Control Matrix](docs/control_matrix.md)** – OWASP/NIST/EU AI Act traceability
- **[Master Plan](../docs/wg-bastion_plan_v2.md)** – 13-sprint roadmap and workstreams
- **[Integration Guide](docs/integration_guide.md)** – Step-by-step adoption *(coming soon)*
- **[API Documentation](https://docs.rs/wg-bastion)** – Full API reference

### Attack Playbooks

Incident response procedures for each OWASP category:

- [LLM01: Prompt Injection](docs/attack_playbooks/llm01_prompt_injection.md)
- [LLM02: Data Disclosure](docs/attack_playbooks/llm02_data_disclosure.md) *(planned)*
- [LLM03-LLM10: Additional Playbooks](docs/attack_playbooks/) *(planned)*

---

## Development Status

**Current Sprint**: 1 (WS1 - Foundations & Governance)  
**Release Target**: v0.1.0 (Sprint 13, ~26 weeks from now)

### Sprint 1 Progress (WS1)
- [x] Crate scaffold and workspace integration
- [x] Core `config` module (PolicyBuilder, FailMode)
- [x] Core `pipeline` module (SecurityPipeline, SecurityStage)
- [x] Threat model documentation
- [x] Control matrix (OWASP/NIST/EU AI Act mapping)
- [ ] Governance documentation (README, SECURITY.md, PR template)

### Upcoming Sprints
- **Sprint 2-3**: Prompt protection (fragmentation, honeytokens)
- **Sprint 4**: Input security (injection scanning, PII detection)
- **Sprint 5-6**: Output validation, tool/MCP security
- **Sprint 7-8**: RAG hardening, agentic AI controls

See the [Master Plan](../docs/wg-bastion_plan_v2.md) for the full roadmap.

---

## Performance

**Latency Targets**:
- Heuristic pipeline (default): <50ms P95
- With ML classifier: <100ms P95
- With remote API calls: <200ms P95

**Benchmarks** (coming in Sprint 2):
```bash
cargo bench --package wg-bastion
```

---

## Security

We take security seriously. Please see our [Security Policy](SECURITY.md) for:

- Vulnerability disclosure process
- Supported versions
- Security contact information

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](../CONTRIBUTING.md) for:

- Development setup
- Code standards
- Security review requirements
- PR checklist

---

## License

This project is licensed under the MIT License - see [LICENSE](../LICENSE) for details.

---

## Acknowledgments

This project builds on research and best practices from:

- [OWASP LLM Top 10](https://owasp.org/www-project-top-10-for-large-language-model-applications/)
- [NIST AI Risk Management Framework](https://www.nist.gov/itl/ai-risk-management-framework)
- [Microsoft Presidio](https://github.com/microsoft/presidio)
- [Rebuff Prompt Injection Detector](https://github.com/protectai/rebuff)
- [NVIDIA NeMo Guardrails](https://github.com/NVIDIA/NeMo-Guardrails)

---

## Status Badges

- ✅ **Implemented** – Code complete, tests passing
- 🔄 **In Progress** – Active development
- 📋 **Planned** – Scope defined, not started
- ❌ **Deferred** – Post-v0.1.0 release

---

*Built with ❤️ for secure LLM applications*
