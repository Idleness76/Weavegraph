# Security Policy

## Supported Versions

| Version | Supported          | Notes |
| ------- | ------------------ | ----- |
| 0.1.x   | :white_check_mark: | Current development version |
| < 0.1.0 | :x:                | Pre-release, not supported |

**Note**: wg-bastion is currently in active development and is pre-1.0. Security support is provided for versions 0.1.0 and later (see table above); earlier pre-release versions (< 0.1.0) are not supported.

---

## Reporting a Vulnerability

We take security vulnerabilities in wg-bastion seriously. If you discover a security issue, please follow responsible disclosure practices:

### 1. **DO NOT** Open a Public Issue

Security vulnerabilities should **not** be reported via GitHub issues, discussions, or pull requests. Public disclosure before a fix is available puts all users at risk.

### 2. Report Privately

Submit vulnerability reports via the GitHub **"Report a vulnerability"** feature on this repository's **Security** tab (GitHub Security Advisories). This keeps the report private and lets maintainers coordinate a fix before public disclosure.

Include in your report:
- Description of the vulnerability
- Steps to reproduce (PoC if possible)
- Potential impact and affected versions
- Any suggested mitigations or patches

### 3. Encryption (Optional)

For highly sensitive disclosures, you may encrypt your report using our PGP key:

```
-----BEGIN PGP PUBLIC KEY BLOCK-----
(PGP key will be published on first stable release)
-----END PGP PUBLIC KEY BLOCK-----
```

### 4. Response Timeline

- **Initial Response**: Within 48 hours (acknowledgment of receipt)
- **Assessment**: Within 7 days (severity classification and timeline)
- **Fix Development**: Varies by severity (see table below)
- **Public Disclosure**: Coordinated with reporter after patch release

| Severity | Fix Timeline | Disclosure Delay |
|----------|-------------|------------------|
| Critical | 7 days      | 14 days post-patch |
| High     | 14 days     | 30 days post-patch |
| Medium   | 30 days     | 60 days post-patch |
| Low      | 90 days     | 90 days post-patch |

### 5. Coordinated Disclosure

We follow responsible disclosure practices:

- We will work with you to understand and validate the issue
- We will develop and test a fix before public announcement
- We will credit you in release notes (unless you prefer anonymity)
- We request you do not publicly disclose until coordinated date

---

## Security Features

wg-bastion is designed to provide security controls for LLM applications. Key features include:

- **Input Validation**: Prompt injection detection, PII scanning
- **Output Security**: Schema validation, egress scanning, sanitization
- **Tool Security**: MCP protocol security, approval workflows
- **RAG Security**: Ingestion sanitization, provenance tracking
- **Telemetry**: Structured security events, audit logging
- **Incident Response**: Automated detection and response automation

### Security Boundaries

wg-bastion is a **defense-in-depth** library that:

- ✅ Provides guardrails against common LLM attacks (OWASP LLM Top 10 2025)
- ✅ Enables observability and incident detection
- ✅ Supports compliance requirements (NIST AI RMF, EU AI Act)

However, it:

- ❌ Does NOT guarantee 100% attack prevention (security is layered)
- ❌ Does NOT replace application logic validation
- ❌ Does NOT provide legal/compliance certification (technical controls only)

**Users are responsible for**:
- Configuring policies appropriate to their risk tolerance
- Testing controls against their specific threat model
- Monitoring security telemetry and responding to incidents
- Keeping wg-bastion and dependencies updated

---

## Security Audits

- **Internal Audits**: Continuous security review during development
- **External Audits**: Planned for v1.0 release
- **Penetration Testing**: Red team exercises planned quarterly post-v0.1.0
- **Dependency Audits**: Automated via `cargo-audit` and `cargo-deny` in CI

---

## Known Limitations

### Current Development Phase

As of January 2026, wg-bastion is in active development (Sprint 1):

- Most security modules are **planned but not yet implemented**
- No formal security audit has been completed
- API is subject to breaking changes before v1.0
- Documentation is evolving

**Do NOT use in production until v0.1.0 stable release.**

### Architectural Limitations

- **Detection vs. Prevention**: Some controls are detective (alerts) rather than preventive (blocks)
- **False Positives**: Heuristic detection may flag legitimate content
- **Adversarial ML**: ML-based classifiers can be evaded by sophisticated attackers
- **Performance Trade-offs**: Enabling all features may impact latency
- **Dependency Trust**: Relies on upstream crates (Presidio, ONNX Runtime, etc.)

---

## Security Best Practices

When using wg-bastion:

### Configuration
- ✅ Use `FailMode::Closed` in production (block threats)
- ✅ Enable audit logging and retention policies
- ✅ Set appropriate rate limits and budgets
- ✅ Rotate honeytokens periodically
- ⚠️ Never disable security controls in production

### Deployment
- ✅ Keep wg-bastion and dependencies updated
- ✅ Monitor security events via OTLP/SIEM integration
- ✅ Test security controls with adversarial corpus
- ✅ Review and update threat model quarterly
- ⚠️ Implement network-level controls (WAF, rate limiting)

### Incident Response
- ✅ Have an incident response plan (see attack playbooks)
- ✅ Test incident workflows with simulations
- ✅ Monitor for honeytoken leakage (critical alerts)
- ✅ Review audit logs regularly for anomalies

---

## Supply Chain Security

wg-bastion follows secure development practices:

- **SBOM Generation**: CycloneDX SBOM published with each release
- **Dependency Audits**: `cargo-audit` runs nightly in CI
- **License Compliance**: `cargo-deny` enforces allowlist
- **Signed Releases**: Git tags signed with maintainer PGP keys
- **Pinned Dependencies**: Lock file committed to repository

---

## Contact

- **Security Issues**: security@example.com *(update with actual contact)*
- **General Questions**: GitHub Discussions
- **Maintainers**: See [CONTRIBUTING.md](CONTRIBUTING.md)

---

## Hall of Fame

We recognize security researchers who responsibly disclose vulnerabilities:

*(This section will be populated as researchers contribute)*

---

**Last Updated**: January 2026  
**Next Review**: Post-v0.1.0 release
