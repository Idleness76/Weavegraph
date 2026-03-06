# Security Policy

## Supported Versions

We actively support the following versions of Weavegraph with security updates:

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| 0.1.x   | :x:                |
| < 0.1.0 | :x:                |

## Reporting a Vulnerability

We take security vulnerabilities seriously and appreciate your efforts to responsibly disclose your findings.

### How to Report

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report them by:

1. Opening a security advisory on our [GitHub Security Advisories page](https://github.com/Idleness76/weavegraph/security/advisories/new)
2. Or emailing the maintainers directly (contact information available in project metadata)

### What to Include

Please include the following information in your report:

- **Description of the vulnerability**: A clear description of the issue
- **Steps to reproduce**: Detailed steps to reproduce the vulnerability
- **Potential impact**: Your assessment of the potential impact
- **Suggested fix**: If you have a fix or mitigation in mind, please share it
- **Affected versions**: Which versions of Weavegraph are affected
- **Environment details**: Operating system, Rust version, and any relevant configuration

### Response Timeline

- **Initial response**: Within 72 hours of receiving your report
- **Status updates**: We will provide regular updates (at least weekly) on our progress
- **Resolution timeline**: 
  - **Critical vulnerabilities**: We aim to release a patch within 7 days
  - **High severity**: Within 30 days
  - **Medium/Low severity**: Within 90 days

### What to Expect

1. **Acknowledgment**: We will acknowledge receipt of your vulnerability report
2. **Validation**: We will validate the vulnerability and determine its severity
3. **Fix development**: We will work on a fix, potentially requesting your input
4. **Coordinated disclosure**: We will coordinate disclosure timing with you
5. **Credit**: We will credit you in the security advisory (unless you prefer to remain anonymous)

## Security Best Practices

When using Weavegraph:

### Checkpointer Security

- **SQLite**: Ensure database files have appropriate file permissions (`chmod 600`)
- **PostgreSQL**: Use strong passwords, TLS connections, and principle of least privilege for database users
- **Never commit** connection strings or credentials to version control

### Event Bus & Logging

- **Sensitive data**: Avoid logging sensitive information (credentials, PII, etc.) in node outputs
- **Event sinks**: Ensure event sinks (file, network) have appropriate access controls
- **JSON Lines logs**: Rotate and protect log files containing event streams

### LLM Integration

- **API keys**: Store API keys securely (environment variables, secret managers)
- **Prompt injection**: Sanitize user inputs before passing to LLM nodes
- **Rate limiting**: Implement appropriate rate limiting for LLM API calls

### State Management

- **Input validation**: Always validate user inputs before adding to state
- **State snapshots**: Be cautious about serializing/deserializing state from untrusted sources

## Known Security Considerations

### Dependencies

We use `cargo-deny` in CI to check for known vulnerabilities in dependencies. Current advisories we track:

- See [deny.toml](deny.toml) for our advisory ignore list and rationale

### Async Runtime

Weavegraph depends on Tokio for async execution. Follow [Tokio security best practices](https://tokio.rs/).

## Disclosure Policy

When we receive a security vulnerability report, we will:

1. Work with the reporter to validate and fix the issue
2. Create a security advisory on GitHub
3. Release a patched version
4. Publish the advisory after the patch is available
5. Credit the reporter (with their permission)

We follow a **90-day disclosure timeline**: we aim to release fixes within this period, and will disclose the vulnerability 90 days after the initial report (or sooner if a patch is available).

## Security Updates

Subscribe to security advisories via:

- [GitHub Security Advisories](https://github.com/Idleness76/weavegraph/security/advisories)
- Watch the repository for releases
- Follow the project for announcements

## Questions

If you have questions about this security policy, please open a discussion on GitHub or contact the maintainers.

---

**Last updated**: 2026-03-06
