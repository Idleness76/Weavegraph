# Changelog

All notable changes to Weavegraph will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-02-11

### Added
- **PostgreSQL checkpointer** with indexed JSONB queries for concurrent checkpoint management
- **AppRunner builder pattern** for more flexible runtime configuration
- **Type-safe message roles** via `Role` enum with compile-time safety
- **Graph iteration API** (`nodes()`, `edges()`, `topological_sort()`) inspired by petgraph
- **petgraph compatibility layer** (feature-gated) for visualization and analysis
- **JSON event schemas** with examples for all event types
- **JSON Lines sink** for structured event logging
- Property-based tests ensuring conditional edges never target unregistered nodes

### Changed
- **BREAKING**: Message role API refactored - prefer `Message::with_role(Role::...)` over string roles
- **BREAKING**: `AppRunner` constructors consolidated into builder pattern
- Runner module decomposed into focused sub-modules (session, execution, streaming)
- Replaced `parking_lot` locks with `std::sync` for simpler dependencies
- Improved error context - scheduler errors now include frontier state snapshots
- Enhanced tracing spans for schedule, barrier, and frontier operations
- Postgres checkpointer maintains "latest" snapshot in application code for correctness
- SQLite imports and checkpoint patterns refactored for consistency

### Deprecated
- `Message::USER`, `Message::ASSISTANT`, `Message::SYSTEM` constants (use `Role` enum)
- `AppRunner::new()` and `AppRunner::with_options()` (use builder pattern)

### Removed
- Production code no longer uses `.expect()` - all operations return `Result` types
- Unused `FrontierContext` error wrapper (simplified to direct scheduler errors)

### Fixed
- Concurrent checkpoint writes now maintain monotonic "latest" pointer
- Out-of-order step writes handled correctly via JSONB containment queries
- Scheduler error propagation improved with proper `?` operator usage

## [0.1.3] - 2026-01-14

### Added
- PostgreSQL checkpointer implementation with migration support
- Indexed JSONB queries for performant step history lookups
- rig-core upgraded to 0.28 with improved LLM integration

### Changed
- Checkpointer implementations refactored for better separation of concerns
- Test suite expanded with concurrency and out-of-order write scenarios

### Fixed
- Clippy warnings resolved for unused field false positives in error types
- Port configuration alignment in test suite

## [0.1.2] - 2025-12-20

### Added
- Enhanced telemetry with schedule, barrier, and frontier tracing spans
- Helper methods for checkpoint management and session completion checks

### Changed
- Runner error context improved with scheduler state snapshots
- Refactored `run_one_superstep` into focused helper methods
- `InMemoryCheckpointer` properly synchronized for concurrent access

### Fixed
- Consistent use of synchronization primitives across checkpointer implementations
- Examples updated to run without prior setup requirements

## [0.1.1] - 2025-11-28

### Added
- Graph validation: cycle detection, unreachable node detection, duplicate edge detection
- Missing End reachability validation with actionable error diagnostics
- JSON event schemas (`event.json`, `llm_event.json`, `error_event.json`) with examples
- `JsonLinesSink` for structured event output
- Property-based tests for graph compilation edge cases
- `From` trait implementations for idiomatic conversions

### Changed
- `GraphBuilder::compile()` now returns `Result` with comprehensive validation
- Builder documentation updated with error handling patterns

### Fixed
- Helper converters added to avoid typo-prone string literals in edge definitions

## [0.1.0] - 2025-11-15

Initial stable release. Core features:

- Graph-based workflow execution with concurrent node scheduling
- Versioned state management with snapshot isolation
- Type-safe message passing between nodes
- Event bus with streaming support and multiple sink types
- SQLite and in-memory checkpointing
- Conditional routing with state inspection
- LLM integration via rig-core
- Comprehensive test suite with property-based testing

---

[unreleased]: https://github.com/Idleness76/weavegraph/compare/weavegraph-v0.2.0...HEAD
[0.2.0]: https://github.com/Idleness76/weavegraph/compare/weavegraph-v0.1.3...weavegraph-v0.2.0
[0.1.3]: https://github.com/Idleness76/weavegraph/compare/weavegraph-0.1.2...weavegraph-v0.1.3
[0.1.2]: https://github.com/Idleness76/weavegraph/compare/weavegraph-v0.1.1...weavegraph-0.1.2
[0.1.1]: https://github.com/Idleness76/weavegraph/compare/v0.1.0-alpha.7...weavegraph-v0.1.1
[0.1.0]: https://github.com/Idleness76/weavegraph/releases/tag/v0.1.0-alpha.7
