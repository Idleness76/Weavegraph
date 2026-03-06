# Contributing to Weavegraph

Thank you for your interest in contributing to Weavegraph! This project welcomes contributions from developers of all skill levels. Weavegraph is in active development (v0.2.x released, targeting v0.3.0 API stabilization) with ongoing improvements based on real-world usage and community feedback.

## Getting Started

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
   cargo run --example basic_nodes
   cargo run --example advanced_patterns
   cargo run --example streaming_events
   ```

4. **Set up local services** (optional):
   ```bash
   # Start Ollama and PostgreSQL for integration testing
   docker compose up -d
   ```

## Running CI Locally

Before submitting a PR, run local CI checks to catch issues early:

```bash
# Quick checks (fmt, clippy, test, doc)
./scripts/ci-quick.sh

# Full CI suite (includes MSRV 1.89, deny, machete)
./scripts/ci-local.sh
```

Both scripts mirror the GitHub Actions CI pipeline and should pass before pushing.

## How to Contribute

### Bug Reports

Use the issue templates on GitHub. Include:
- Minimal reproduction steps
- System information (OS, Rust version, Weavegraph version)
- Relevant log output with `RUST_LOG=debug`

### Feature Requests

Use the feature request template. Describe:
- The use case and motivation
- How it fits the framework's philosophy
- Examples of proposed usage
- Whether it would be a breaking change

### Code Contributions

#### Areas of Interest

1. **Persistence & Backends**
   - Custom checkpointer implementations
   - Performance optimizations for existing backends

2. **AI/LLM Integration**
   - Enhanced message types for AI workflows
   - Integration with other LLM frameworks
   - Streaming response handling patterns

3. **Performance Optimizations**
   - Scheduler efficiency improvements
   - Memory usage optimizations

4. **Developer Experience**
   - Better error messages and diagnostics
   - Additional convenience methods
   - Documentation improvements

5. **Example Applications**
   - Real-world workflow examples
   - Integration patterns with popular frameworks

#### Development Guidelines

**Code Style**:
- Follow standard Rust formatting (`cargo fmt`)
- Run Clippy and address warnings (`cargo clippy`)
- Add comprehensive documentation for public APIs

**Testing**:
- Add unit tests for new functionality
- Include integration tests for complex workflows
- Ensure examples continue to work

**Documentation**:
- Update relevant module documentation
- Include usage examples in function documentation
- Update README for major features

**Commit Messages**:
- Use conventional commit format: `type(scope): description`
- Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `chore`

## Release Process

Weavegraph follows semantic versioning. Releases use the `v{VERSION}` tag convention:

- **Tag format**: `weavegraph-v{MAJOR}.{MINOR}.{PATCH}` (e.g., `weavegraph-v0.3.0`)
- **Changelog**: All user-facing changes documented in `CHANGELOG.md`
- **Breaking changes**: Require minor version bump in 0.x, major version bump in 1.x+
- **CI validation**: All checks must pass before release

Contributors don't need to worry about versioning—maintainers handle releases.

## Community

- **GitHub Discussions**: Design discussions and questions
- **Issues**: Bug reports and feature requests (use templates)
- **Pull Requests**: Code contributions

## Recognition

Contributors are recognized in:
- `CHANGELOG.md` for their contributions
- GitHub contributors list
- Release notes for significant features

We appreciate all forms of contribution!

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code. Please report unacceptable behavior via GitHub issues or by contacting the maintainers.

## Questions?

- Check [GitHub Discussions](https://github.com/Idleness76/weavegraph/discussions)
- Review existing [issues](https://github.com/Idleness76/weavegraph/issues)
- Consult the [documentation](https://docs.rs/weavegraph)

Thank you for helping make Weavegraph better!
