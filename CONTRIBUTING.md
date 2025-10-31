# Contributing to Weavegraph

Thank you for your interest in contributing to Weavegraph! This project welcomes contributions from developers of all skill levels.

## 🎓 Project Origins

Weavegraph began as a capstone project for a Rust online course, developed by contributors with backgrounds in Python and experience with LangGraph and LangChain. The goal was to bring similar graph-based workflow capabilities to the Rust ecosystem with modern concurrency, type safety, and performance benefits.

While the project started in an educational context, **it has grown beyond the classroom** and continues active development as a production-ready framework. We're committed to maintaining and expanding Weavegraph well beyond the course completion.

## 🚀 Getting Started

### Prerequisites

- Rust 1.91 or later
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
   cargo run --example demo3
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

**Code Style**:
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

### 📝 Documentation

- Improve existing documentation clarity
- Add more real-world examples
- Create tutorials for common patterns
- Translate documentation (future consideration)

## 🔄 Pull Request Process

1. **Fork the repository** and create a feature branch
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes** following the guidelines above

3. **Test thoroughly**:
   ```bash
   cargo test --all
   cargo clippy
   cargo fmt --check
   ```

4. **Update documentation** as needed

5. **Submit the pull request**:
   - Provide a clear description of changes
   - Reference any related issues
   - Include examples of the new functionality
   - Ensure CI passes

6. **Respond to feedback** and iterate as needed

## 🏗️ Architecture Guidelines

When contributing, keep these architectural principles in mind:

### Core Principles

- **Composability**: Components should work well together
- **Type Safety**: Leverage Rust's type system for correctness
- **Performance**: Efficient concurrent execution without sacrificing safety
- **Observability**: Rich tracing and event streaming throughout
- **Error Handling**: Comprehensive, actionable error reporting

### Module Organization

- **`message`**: Keep message types simple and role-focused
- **`state`**: Maintain versioning and snapshot isolation
- **`node`**: Async-first design with clear error propagation
- **`graph`**: Declarative graph definition with conditional routing
- **`schedulers`**: Concurrent execution with dependency resolution
- **`runtimes`**: High-level orchestration and persistence
- **`channels`**: Efficient state storage and versioning
- **`reducers`**: Deterministic merge strategies
- **`event_bus`**: Non-blocking event streaming

### Design Patterns

- **Builder Pattern**: For complex object construction
- **Convenience Constructors**: Reduce boilerplate for common cases
- **Rich Error Types**: Provide context and suggestions
- **Instrument Everything**: Add tracing spans for observability

## 🧪 Testing Philosophy

- **Unit Tests**: Test individual components in isolation
- **Integration Tests**: Test component interactions
- **Property Tests**: Use `proptest` for edge case discovery
- **Example Tests**: Ensure all examples compile and run
- **Performance Tests**: Benchmark critical paths

## 📋 Project Roadmap

### Short Term (Next 3 months)
- Enhanced LLM integration patterns
- Additional persistence backends
- Performance optimizations
- More comprehensive examples

### Medium Term (6 months)
- Distributed execution capabilities
- Enhanced monitoring and metrics
- Plugin system for custom components
- WebAssembly support exploration

### Long Term (1+ years)
- Multi-language bindings
- Cloud-native deployment patterns
- Enterprise features (audit trails, compliance)
- Integration with major ML/AI platforms

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
