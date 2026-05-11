# Changelog

All notable changes to Weavegraph will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - 2026-05-11

### Added

#### WG-006 — Invocation-scoped state lifecycle + normalization profiles

- `StateLifecycle` enum (`Durable` / `InvocationScoped`) in `weavegraph::state`.
- `StateKey<T>::invocation_scoped()` const builder — marks a key as invocation-scoped without changing its identity (equality / hash exclude the lifecycle field).
- `StateKey<T>::lifecycle()` getter returning the stored `StateLifecycle`.
- `StateNormalizeProfile` in `weavegraph::runtimes::replay` — fluent builder for specifying which state keys to ignore during replay comparison. Supports both typed (`ignore_key<T>(StateKey<T>)`) and raw-string (`ignore_extra_keys(impl IntoIterator<Item = &str>)`) forms. Panics at construction time if two registrations conflict on lifecycle annotation.
- `normalize_state_with(state, profile)` — normalizes a `VersionedState` snapshot to a `serde_json::Value` after dropping ignored keys.
- `compare_final_state_with(left, right, profile)` — variant of `compare_final_state` accepting a `StateNormalizeProfile`.
- `compare_replay_runs_with_profile(left, right, profile, event_normalizer)` — variant of `compare_replay_runs_with` accepting a `StateNormalizeProfile`.
- `NodePartial::clear_extra_keys(keys)` — **deletes** the given raw keys from state on the next barrier application. Uses JSON Merge Patch semantics: `MapMerge` now removes keys whose incoming value is `null` (RFC 7396). No separate cleanup reducer is needed.
- `NodePartial::clear_typed_extra_key(key)` — typed companion to `clear_extra_keys`; uses the `StateKey`'s storage key.

#### WG-007 — Runtime observability hooks + metrics adapter

- `RuntimeObserver` trait in `weavegraph::runtimes::observer` — zero-cost (no allocation, no virtual dispatch when unused), always compiled, no feature gate. All methods have default no-op bodies; implementors override only what they need.
- Hook methods: `on_invocation_start`, `on_invocation_finish`, `on_node_finish`, `on_checkpoint_load`, `on_checkpoint_save`, `on_event_bus_emit`.
- Metadata structs (all `#[non_exhaustive]`): `InvocationStartMeta`, `InvocationFinishMeta`, `NodeFinishMeta`, `CheckpointLoadMeta`, `CheckpointSaveMeta`, `EventBusEmitMeta`.
- Outcome enums (all `#[non_exhaustive]`): `InvocationOutcome`, `NodeOutcome`.
- `AppRunnerBuilder::observer(Arc<dyn RuntimeObserver>)` — attaches an observer; no-op overhead when not set.
- Observer panics are caught via `std::panic::catch_unwind` and logged as `tracing::warn!` — a misbehaving observer cannot abort a workflow.
- `ObservingEmitter` (private) — wraps the event bus emitter to fire `on_event_bus_emit` for every emitted event when an observer is attached.
- `MetricsObserver` in `weavegraph::runtimes::metrics_observer` — a `RuntimeObserver` impl that emits standard counters and histograms via the `metrics` crate facade. Available under the `metrics` feature flag.
  - Counters: `weavegraph.node.invocations` (labels: `node`, `outcome`), `weavegraph.invocation.count` (`outcome`), `weavegraph.checkpoint.saves` (`backend`), `weavegraph.checkpoint.loads` (`backend`), `weavegraph.event_bus.emits` (`scope`).
  - Histograms: `weavegraph.node.step_duration_ms` (`node`), `weavegraph.invocation.duration_ms`, `weavegraph.checkpoint.save_duration_ms` (`backend`).

### Changed (breaking)

- `RunnerError`, `NodeError`, `CheckpointerError`, `StateSlotError`, and `ReplayConformanceError` are now `#[non_exhaustive]`. Exhaustive `match` arms on these types must add a wildcard `_` arm.
  - Migration: replace `_ => unreachable!()` with `_ => { /* handle future variants */ }` where appropriate.
- **`MapMerge` reducer now deletes keys whose incoming value is `null`** (JSON Merge Patch / RFC 7396). Previously a `null` was written into state as-is. Any code that deliberately stored `serde_json::Value::Null` via `with_extra` should use a sentinel value instead (e.g. a JSON object with an `absent: true` field).

## [0.5.0] - 2026-05-08

### Added
- `AppRunner::create_iterative_session(...)` and `AppRunner::invoke_next(...)` for repeated graph invocations under one durable session lineage.
- `RunnerError::InvalidIterativeEntry` for invalid iterative entry nodes.
- Typed state-slot helpers: `StateKey<T>`, `StateSnapshot::get_typed(...)`, `StateSnapshot::require_typed(...)`, `VersionedState::add_typed_extra(...)`, `VersionedStateBuilder::with_typed_extra(...)`, and `NodePartial::with_typed_extra(...)`.
- Runtime clock injection through `RuntimeConfig::with_clock(...)`, `AppRunnerBuilder::clock(...)`, and `NodeContext::now_unix_ms()`.
- Optional node event metadata for `invocation_id` and `now_unix_ms` when runtime metadata is configured.
- `INVOCATION_END_SCOPE` and `AppRunner::finish_iterative_session(...)` for long-lived iterative event streams.
- Graph and run metadata helpers: `App::graph_metadata()`, `App::graph_definition_hash()`, `RuntimeConfig::config_hash()`, and `AppRunner::run_metadata()`.
- `Reducer::definition_label(...)` so graph metadata can distinguish reducer implementations, not only reducer counts.
- Replay conformance helpers in `weavegraph::runtimes::replay` for normalized event comparison, final-state comparison, and reusable replay assertions.

### Notes
- This feedback package ships as `0.5.0` rather than `0.4.1` because it changes the public runtime surface, adds public error enum variants/types, and extends public structs.
- New public metadata/context structs are marked `#[non_exhaustive]` where they are expected to grow before v1.

## [0.4.0] - 2026-04-01

### Added
- `DIAGNOSTIC_SCOPE` constant exported from `weavegraph::event_bus` for identifying internal diagnostic events
- `examples/production_streaming.rs` — golden-path reference for Axum + SSE + Postgres checkpointing
- `[[example]]` entry with `required-features = ["postgres", "examples"]` for `production_streaming`
- `#![warn(missing_docs)]` now enforced — all 228 previously undocumented public API items are documented

### Changed
- `RuntimeConfig::new()` signature changed: removed middle `checkpointer: Option<CheckpointerType>` parameter; now takes `(session_id: Option<String>, sqlite_db_name: Option<String>)`
- Feature flags table in crate-level docs updated to remove the removed `llm` alias
- `docs/MIGRATION.md` updated with v0.3.0 → v0.4.0 migration guide

### Removed
- **BREAKING**: `Message::new(role: &str, content: &str)` removed (deprecated since v0.3.0) — use `Message::with_role(Role::..., ...)` or convenience constructors
- **BREAKING**: `RuntimeConfig.checkpointer` field removed — configure checkpointer via `AppRunner::builder().checkpointer(...)` 
- **BREAKING**: `RuntimeConfig::with_checkpointer()` and `RuntimeConfig::checkpointer_type()` removed
- **BREAKING**: `AppRunner::new()`, `from_arc()`, `with_options()`, `with_options_arc()`, `with_options_and_bus()`, `with_options_arc_and_bus()` removed (deprecated since v0.2.0) — use `AppRunner::builder()`
- **BREAKING**: `LadderError` type alias removed (deprecated since v0.3.0) — use `WeaveError` directly
- **BREAKING**: `llm` feature flag alias removed (deprecated since v0.3.0) — use `features = ["rig"]`

## [0.3.0] - 2026-03-07

### Added
- Custom checkpointer support via `RuntimeConfig::checkpointer_custom` field
- `NodeError::Other` variant for extensible error handling

### Changed
- **BREAKING**: `RuntimeConfig` struct gains new `checkpointer_custom` field (breaking for struct literal construction)
- **BREAKING**: `RuntimeConfig` no longer implements `UnwindSafe` and `RefUnwindSafe` auto traits
- **BREAKING**: `NodeError` enum gains `Other` variant (exhaustive enum)

### Removed
- **BREAKING**: Cargo features `rmcp` and `rig-core` removed
- **BREAKING**: `Message::USER`, `Message::ASSISTANT`, `Message::SYSTEM` constants removed (use `Role` enum)
- **BREAKING**: `Message::role_type()`, `Message::is_role()`, `Message::has_role()` methods removed

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
