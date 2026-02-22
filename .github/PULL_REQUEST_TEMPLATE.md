## Security Checklist

Please review the following before submitting your pull request:

### Code Quality
- [ ] All tests pass (`cargo test`)
- [ ] No new compiler warnings (`cargo clippy`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] Documentation is updated (if applicable)

### Security Review

**⚠️ Required for ALL PRs touching wg-bastion crate:**

- [ ] **Threat Model**: Does this change affect the threat model or attack surface?
  - [ ] If yes, `docs/threat_model.md` updated
- [ ] **Control Matrix**: Does this implement or modify a security control?
  - [ ] If yes, `docs/control_matrix.csv` updated with test ID and status
- [ ] **Cryptography**: Does this use cryptographic primitives (`ring`, `zeroize`)?
  - [ ] If yes, reviewed by maintainer with crypto expertise
  - [ ] No custom crypto implementations (use audited libraries)
- [ ] **Unsafe Code**: Does this PR include `unsafe` blocks?
  - [ ] If yes, thorough justification provided in code comments
  - [ ] Memory safety analysis documented
  - [ ] Reviewed by at least two maintainers
- [ ] **Dependencies**: Does this add new dependencies?
  - [ ] If yes, dependency justified (not available in std/existing deps)
  - [ ] Dependency has recent activity and security track record
  - [ ] `cargo-deny` passes (licenses, advisories, duplicates)
- [ ] **Input Validation**: Does this handle untrusted input?
  - [ ] If yes, validation logic is present
  - [ ] Adversarial test cases added
- [ ] **Secrets/PII**: Does this handle sensitive data?
  - [ ] If yes, secrets are zeroized after use
  - [ ] No secrets logged or exposed in errors
  - [ ] PII handling documented
- [ ] **Error Messages**: Do error messages avoid leaking sensitive information?
  - [ ] No internal paths, credentials, or system details in user-facing errors
- [ ] **Performance**: Does this impact latency-critical paths?
  - [ ] If yes, benchmarks run and results documented
  - [ ] P95 latency target (<50ms baseline) met

### Testing

- [ ] **Unit Tests**: All new functions have unit tests
- [ ] **Integration Tests**: Cross-module scenarios covered (if applicable)
- [ ] **Regression Tests**: If fixing a bug, regression test added
- [ ] **Adversarial Tests**: If security-critical, attack scenarios tested
- [ ] **Edge Cases**: Boundary conditions and error paths tested

### Documentation

- [ ] **Code Comments**: Complex logic has explanatory comments
- [ ] **Doc Comments**: Public APIs have `///` documentation with examples
- [ ] **Changelog**: `CHANGELOG.md` updated (if user-facing change)
- [ ] **Migration Guide**: Breaking changes documented (if applicable)

---

## Description

<!-- Describe your changes in detail. What problem does this solve? -->

## Related Issues

<!-- Link to related issues/discussions. Use keywords like "Fixes #123" or "Closes #456" -->

## Breaking Changes

<!-- If this introduces breaking changes, describe the migration path for users -->

## Screenshots (if applicable)

<!-- Add screenshots for UI changes or visual examples -->

## Checklist for Maintainers

<!-- For maintainer use during review -->

- [ ] Security review completed
- [ ] Changes approved by at least one other maintainer
- [ ] CI passes (all platforms)
- [ ] Documentation builds successfully
- [ ] Performance benchmarks acceptable (if applicable)
