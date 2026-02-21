# Contributing to Weavegraph

Thank you for your interest in contributing to Weavegraph! This project welcomes contributions from developers of all skill levels. As an early beta framework (targeting v0.2.x), we're actively evolving APIs and architecture based on real-world usage and community feedback.

## 🚀 Getting Started

### Prerequisites

- Rust 1.89 or later
- Basic familiarity with async Rust and the `tokio` runtime
- Understanding of graph-based workflows is helpful but not required

### Development Setup

1. **Clone the repository**:
   ```bash
   git clone https://github.com/Idleness76/weavegraph.git
   cd weavegraph
   ```

2. **Install dependencies and run tests**:
   ```bash
   cargo build
   cargo test --all -- --nocapture
   ```

3. **Run examples to understand the framework**:
   ```bash
   # Start with basic patterns
   cargo run --example basic_nodes

   # Explore advanced features
   cargo run --example advanced_patterns

   # See error handling in action
   cargo run --example errors_pretty
   ```

4. **Set up Ollama for LLM demos** (optional):
   ```bash
   docker-compose up -d ollama
   # LLM-focused demos were removed during the 0.2.0 refactor; start from streaming patterns instead
   cargo run --example streaming_events
   ```

## 🎯 How to Contribute

We welcome various types of contributions:

### 🐛 Bug Reports

- Use the [GitHub issue tracker](https://github.com/Idleness76/weavegraph/issues)
- Include minimal reproduction steps
- Provide system information (OS, Rust version)
- Include relevant log output with `RUST_LOG=debug`

### ✨ Feature Requests

- Describe the use case and motivation
- Consider whether it fits the framework's core philosophy
- Provide examples of how the feature would be used
- Check existing issues for similar requests

### 🔧 Code Contributions

#### Areas We're Particularly Interested In

1. **Persistence Backends**
   - PostgreSQL checkpointer implementation
   - Redis-based state storage
   - Custom persistence adapters

2. **AI/LLM Integration**
   - Enhanced message types for AI workflows
   - Integration with other LLM frameworks beyond Ollama
   - Streaming response handling patterns

3. **Performance Optimizations**
   - Scheduler efficiency improvements
   - Memory usage optimizations
   - Concurrent execution enhancements

4. **Developer Experience**
   - Better error messages and diagnostics
   - Additional convenience methods
   - Documentation improvements

5. **Example Applications**
   - Real-world workflow examples
   - Integration patterns with popular frameworks
   - Performance benchmarking examples

#### Development Guidelines

**Code Style and Guidelines**:
- Follow standard Rust formatting (`cargo fmt`)
- Run Clippy and address warnings (`cargo clippy`)
- Use meaningful variable and function names
- Add comprehensive documentation for public APIs

**Testing**:
- Add unit tests for new functionality
- Include integration tests for complex workflows
- Use property-based testing where appropriate
- Ensure examples continue to work

**Documentation**:
- Update relevant module documentation
- Add or update examples in `lib.rs`
- Include usage examples in function documentation
- Update README if adding major features

**Commit Messages**:
- Use conventional commit format: `type(scope): description`
- Examples:
  - `feat(scheduler): add bounded retry mechanism`
  - `fix(channels): resolve version merge race condition`
  - `docs(message): add role validation examples`

## 🔒 Security Review Requirements (wg-bastion)

For contributions to the **wg-bastion** security crate, additional requirements apply:

### Mandatory Security Review

All PRs touching `wg-bastion/` must pass enhanced security review:

1. **Unsafe Code Policy**
   - `unsafe` blocks require explicit justification in code comments
   - Memory safety analysis must be documented
   - Requires approval from at least **two maintainers**
   - Consider safer alternatives before using `unsafe`

2. **Cryptography**
   - No custom cryptographic implementations (use audited libraries like `ring`)
   - Crypto primitives must be reviewed by maintainer with crypto expertise
   - Use `zeroize` for secret data cleanup
   - Document threat model assumptions

3. **Dependency Review**
   - New dependencies require security justification
   - Check for recent activity and security track record
   - `cargo-deny` must pass (licenses, advisories, duplicates)
   - Prefer well-audited crates with active maintenance

4. **Input Validation**
   - All untrusted input must be validated
   - Add adversarial test cases for injection attacks
   - Document validation logic and assumptions
   - Never trust external data (defense in depth)

5. **Secret Handling**
   - Secrets must be `zeroize`d after use
   - No secrets in logs, errors, or telemetry
   - PII handling must be documented
   - Use structured logging to avoid accidental leakage

6. **Error Messages**
   - User-facing errors must not leak sensitive information
   - No internal paths, credentials, or system details
   - Provide generic security errors with internal logging
   - Review all `Error` types for information disclosure

7. **Testing Requirements**
   - **Unit tests**: Required for all security functions
   - **Adversarial tests**: Required for threat detection logic
   - **Regression tests**: Required when fixing security bugs
   - **Attack harness**: Integration with adversarial corpus

8. **Documentation Requirements**
   - Update threat model (`docs/threat_model.md`) if attack surface changes
   - Update control matrix (`docs/control_matrix.csv`) with test IDs
   - Add attack playbooks for new threat categories
   - Document security assumptions and limitations

### CI Requirements

All wg-bastion PRs must pass:

- `cargo test` – All tests pass
- `cargo clippy` – No warnings
- `cargo fmt --check` – Formatting consistent
- `cargo audit` – No known vulnerabilities
- `cargo deny check` – License/advisory compliance
- Attack harness (when implemented) – >95% detection rate

### Pre-Merge Checklist

Before merging security-critical PRs:

- [ ] Security review by at least two maintainers
- [ ] All adversarial tests pass
- [ ] Threat model updated (if applicable)
- [ ] Control matrix updated with test IDs
- [ ] No sensitive data in logs or errors
- [ ] Performance benchmarks acceptable (<50ms P95 target)
- [ ] Documentation complete and accurate

### Vulnerability Disclosure

If you discover a security vulnerability:

- **DO NOT** open a public issue
- Report privately to `security@example.com` (see `SECURITY.md`)
- Allow time for coordinated disclosure
- Follow responsible disclosure practices

### 📝 Documentation

- Improve existing documentation clarity
- Add more real-world examples
- Create tutorials for common patterns
- Translate documentation (future consideration)

## 💬 Community

- **GitHub Discussions**: For design discussions and questions
- **Issues**: For bug reports and feature requests
- **Pull Requests**: For code contributions

## 🙏 Recognition

Contributors will be recognized in:
- `CHANGELOG.md` for their contributions
- GitHub contributors list
- Release notes for significant features

We appreciate all forms of contribution, from bug reports to major features!

## 📜 Code of Conduct

We are committed to providing a welcoming and inclusive environment. Please be respectful in all interactions:

- Use welcoming and inclusive language
- Be respectful of differing viewpoints and experiences
- Gracefully accept constructive criticism
- Focus on what is best for the community
- Show empathy towards other community members

## ❓ Questions?

If you have questions about contributing:
- Check existing [GitHub issues](https://github.com/Idleness76/weavegraph/issues)
- Open a new issue with the "question" label
- Review the documentation and examples

Thank you for helping make Weavegraph better! 🚀
