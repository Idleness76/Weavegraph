# Migration Guide

This document outlines breaking changes between Weavegraph versions and provides
migration guidance for upgrading your code.

---

## v0.5.0

### Overview

v0.5.0 is the recommended target for the WeaveQuant production feedback work. The changes add new public runtime APIs and a public `RunnerError` variant, so they should not ship as a `0.4.1` patch.

### New Runtime APIs

Use `AppRunner::create_iterative_session(...)` and `AppRunner::invoke_next(...)` when one durable session should process many logical inputs:

```rust
runner
    .create_iterative_session(run_id.clone(), initial_state, NodeKind::Start)
    .await?;

runner
    .invoke_next(&run_id, input_patch, NodeKind::Start)
    .await?;
```

`NodeKind::Start` resolves to the graph's normal Start outgoing frontier. A registered custom node can be supplied for narrower re-entry. `NodeKind::End` now returns `RunnerError::InvalidIterativeEntry` when used as an iterative entry.

When an `AppRunner` event stream is subscribed before iterative execution, each `invoke_next(...)` emits `INVOCATION_END_SCOPE` and keeps the stream open for the next logical input. Call `finish_iterative_session(...)` after the final input to emit the normal `STREAM_END_SCOPE` sentinel and close the stream.

### Typed State Slots

Typed state slots are a thin, JSON-compatible layer over `VersionedState.extra`. Define a reusable key in the domain crate, then read and write typed payloads without hand-rolled `serde_json` calls at every node boundary:

```rust
use serde::{Deserialize, Serialize};
use weavegraph::node::NodePartial;
use weavegraph::state::{StateKey, StateSnapshot};

#[derive(Serialize, Deserialize)]
struct PortfolioState {
    cash_cents: i64,
}

const PORTFOLIO: StateKey<PortfolioState> = StateKey::new("wq", "portfolio", 1);

fn read(snapshot: &StateSnapshot) -> Result<Option<PortfolioState>, weavegraph::state::StateSlotError> {
    snapshot.get_typed(PORTFOLIO)
}

fn write(value: PortfolioState) -> Result<NodePartial, weavegraph::state::StateSlotError> {
    NodePartial::new().with_typed_extra(PORTFOLIO, value)
}
```

The storage key is namespaced and versioned as `namespace:name:v{schema_version}`. Untyped `extra` remains available.

### Deterministic Runtime Clock

Use the existing `Clock` abstraction to inject deterministic time into nodes and emitted node-event metadata:

```rust
use std::sync::Arc;
use weavegraph::runtimes::{AppRunner, CheckpointerType};
use weavegraph::utils::clock::MockClock;

let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory)
    .clock(Arc::new(MockClock::new(1_700_000_000)))
    .build()
    .await;
```

Inside a node, call `ctx.now_unix_ms()` and `ctx.invocation_id()`. `NodeContext::new(...)` is now the easiest way to construct contexts in tests.

### Metadata Helpers

Compiled graphs and runners expose deterministic metadata helpers for audit labels and replay manifests:

```rust
let graph = app.graph_metadata();
let graph_hash = app.graph_definition_hash();
let run = runner.run_metadata();
```

The graph hash includes node kinds, edges, conditional edge registrations, and reducer definition labels. It does not inspect closure bodies for conditional predicates. Custom reducers can override `Reducer::definition_label(...)` when a durable audit label is preferable to the default Rust type path.

### Replay Conformance Helpers

Replay helpers live under `weavegraph::runtimes::replay` and are re-exported from `weavegraph::runtimes`:

```rust
use weavegraph::runtimes::{ReplayRun, compare_replay_runs};

let expected = ReplayRun::new(expected_state, expected_events);
let actual = ReplayRun::new(actual_state, actual_events);

compare_replay_runs(&expected, &actual).assert_matches()?;
```

`normalize_event(...)` strips runtime timestamps. Use `compare_event_sequences_with(...)` or `compare_replay_runs_with(...)` when domain events need semantic normalization.

### Compatibility Notes

- `App::invoke(...)`, `AppRunner::create_session(...)`, and `AppRunner::run_until_complete(...)` keep their existing behavior.
- `RunnerError` is an exhaustive public enum. Code that matches every variant must handle `InvalidIterativeEntry` after upgrading.
- `GraphMetadata`, `RunMetadata`, `ReplayRun`, `NodeContext`, and `SchedulerRunContext` are `#[non_exhaustive]`; use provided constructors/builders instead of external struct literals.
- `Reducer` gains a default `definition_label(...)` method for graph metadata. Existing reducer implementations do not need to change unless they want a custom stable label.
- `RuntimeConfig` gains a public `clock` field. Code using struct literals should add `clock: None` or switch to `RuntimeConfig::default()` / builder-style methods.
- `NodeContext` gains `clock` and `invocation_id` fields. Tests should prefer `NodeContext::new(...)` over struct literals.
- Direct calls to `Scheduler::superstep(...)` must pass the optional clock and invocation ID arguments.
- Iterative sessions keep step numbers monotonic across invocations and reload checkpoints through the existing checkpointer path.

---

## v0.4.0

### Overview

v0.4.0 is the **API freeze** release. All items deprecated in v0.2.0 and v0.3.0
have been removed. No new public APIs were added. If you are already on v0.3.0
with no deprecation warnings, upgrading requires only the signature change to
`RuntimeConfig::new()`.

### Breaking Changes

#### 1. `Message::new(role: &str, content: &str)` removed

**Removed in:** v0.4.0 (deprecated since v0.3.0)

Use the typed constructors instead:

```rust
// Before
let m = Message::new("user", "hello");

// After — typed Role enum
let m = Message::with_role(Role::User, "hello");

// Or use the convenience constructors
let m = Message::user("hello");
let m = Message::assistant("reply");
let m = Message::system("you are a helpful assistant");
```

---

#### 2. `RuntimeConfig::new()` signature changed

**Removed in:** v0.4.0

The `checkpointer: Option<CheckpointerType>` middle parameter is removed.

```rust
// Before (v0.3.0)
let config = RuntimeConfig::new(
    Some("session-id".into()),
    Some(CheckpointerType::InMemory),
    None,
);

// After (v0.4.0) — two parameters only
let config = RuntimeConfig::new(
    Some("session-id".into()),
    None, // sqlite_db_name
);
```

Set the checkpointer type via `AppRunner::builder()`:

```rust
AppRunner::builder()
    .app_arc(app)
    .checkpointer(CheckpointerType::SQLite)
    .build()
    .await?;
```

---

#### 3. `RuntimeConfig.checkpointer` field, `with_checkpointer()`, and `checkpointer_type()` removed

**Removed in:** v0.4.0

Configure the checkpointer exclusively through `AppRunner::builder()`:

```rust
// Before — field on RuntimeConfig
let config = RuntimeConfig { checkpointer: Some(CheckpointerType::Postgres), ..Default::default() };
// or
let config = RuntimeConfig::default().with_checkpointer(CheckpointerType::Postgres);

// After — builder method on AppRunner
AppRunner::builder()
    .app_arc(app)
    .checkpointer(CheckpointerType::Postgres)
    .build()
    .await?;

// For a fully custom checkpointer — still on RuntimeConfig
let config = RuntimeConfig::new(None, None)
    .checkpointer_custom(Arc::new(my_checkpointer));
```

---

#### 4. Legacy `AppRunner` constructors removed

**Removed in:** v0.4.0 (deprecated since v0.2.0)

All free-standing constructors have been removed. Use `AppRunner::builder()` exclusively:

| Removed | Replacement |
|---------|-------------|
| `AppRunner::new(app)` | `AppRunner::builder().app(app).build().await` |
| `AppRunner::from_arc(app)` | `AppRunner::builder().app_arc(app).build().await` |
| `AppRunner::with_options(app, config)` | `AppRunner::builder().app(app)` + config methods |
| `AppRunner::with_options_arc(app, config)` | `AppRunner::builder().app_arc(app)` + config methods |
| `AppRunner::with_options_and_bus(app, config, bus)` | `AppRunner::builder().app(app).event_bus(bus)` |
| `AppRunner::with_options_arc_and_bus(app, config, bus)` | `AppRunner::builder().app_arc(app).event_bus(bus)` |

```rust
// Before
let runner = AppRunner::with_options_and_bus(app, config, bus).await?;

// After
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory)
    .event_bus(bus)
    .build()
    .await?;
```

---

#### 5. `LadderError` type alias removed

**Removed in:** v0.4.0 (deprecated since v0.3.0)

```rust
// Before
use weavegraph::channels::errors::LadderError;
fn my_fn() -> Result<(), LadderError> { ... }

// After
use weavegraph::channels::errors::WeaveError;
fn my_fn() -> Result<(), WeaveError> { ... }
```

---

#### 6. `llm` feature flag alias removed

**Removed in:** v0.4.0 (deprecated since v0.3.0)

```toml
# Before
weavegraph = { version = "0.3", features = ["llm"] }

# After
weavegraph = { version = "0.4", features = ["rig"] }
```

---

### New in v0.4.0

- `DIAGNOSTIC_SCOPE` constant exported from `weavegraph::event_bus` — use to
  identify internal diagnostic events when filtering the event stream.
- `#![warn(missing_docs)]` is now enforced — all public API items are documented.
- `examples/production_streaming.rs` — golden-path reference for Axum + SSE +
  Postgres checkpointing (requires `--features postgres,examples`).

---

## v0.3.0 (Upcoming)

### Breaking Changes

#### 1. `Message.role` is now `Role` (High Impact)

**What changed:**
`Message.role` changed from `String` to typed [`Role`](weavegraph::message::Role).

Serialization remains wire-compatible: roles still encode as plain JSON strings
(`"user"`, `"assistant"`, etc.) and decode from plain strings.

**Before (v0.2.x):**
```rust
use weavegraph::message::{Message, Role};

if msg.role == "user" {
    // ...
}

let role = msg.role_type();
if msg.is_role(Role::Assistant) {
    // ...
}
```

**After (v0.3.0):**
```rust
use weavegraph::message::{Message, Role};

if msg.role == Role::User {
    // ...
}

let role = msg.role.clone();
let role_str = msg.role.as_str();
```

**Migration steps:**
1. Replace string comparisons like `msg.role == "user"` with `msg.role == Role::User`
2. Replace `msg.is_role(Role::X)` with `msg.role == Role::X`
3. Replace `msg.role_type()` with `msg.role` (or `msg.role.clone()`)
4. For string interop, use `msg.role.as_str()`

#### 2. Role helper removals and deprecations (High Impact)

**Removed in v0.3.0:**
- `Message::role_type()`
- `Message::is_role(...)`
- `Message::has_role(...)`
- `Message::USER`, `Message::ASSISTANT`, `Message::SYSTEM`

**Deprecated in v0.3.0 (removed in v0.4.0):**
- `Message::new(role: &str, content: &str)`

**Replacement guidance:**
- Use `Message::with_role(Role::..., ...)` for typed construction
- Use `Message::user(...)`, `Message::assistant(...)`, `Message::system(...)`, `Message::tool(...)` for common roles
- Use `Message::with_role(Role::Custom("name".into()), ...)` for custom roles

#### 3. Error System Redesign (`0.3.2-alt`) (High Impact)

**What changed:**
- `NodeError` remains a structured public enum (library-friendly and matchable)
- `NodeError::Anyhow(...)` was removed from the public API
- `NodeError::Other(Box<dyn Error + Send + Sync>)` remains the generic fallback
- Rich diagnostics are now optional via `diagnostics` feature
- New ergonomic helper: `NodeResultExt::node_err()` for natural `?` propagation
- **All public error types now follow a uniform architecture** (see below)

This keeps public APIs typed and introspectable while reducing dependency pressure.

**Uniform Error Architecture (0.3.2-alt):**

All public error enums in Weavegraph now follow this pattern for consistency and feature-gating:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum MyError {
    #[error("user-facing description")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(
            code(weavegraph::module::variant),
            help("Optional help text for debugging")
        )
    )]
    VariantName(/* fields */),
}
```

This pattern applies to:
- `NodeError` & `NodeContextError` (node execution)
- `RunnerError` & `SchedulerError` (workflow runtime)
- `CheckpointerError` (state persistence)
- `PersistenceError` (serialization/deserialization)
- `GraphCompileError` (graph validation)
- `JsonError` & `CollectionError` (data operations)
- `IdError` (ID generation)
- `EmitterError` (event bus)
- `AppEventStreamError` (event stream lifecycle)
- `ReducerError` (state reduction)

**Before (v0.2.x / early v0.3 drafts):**
```rust
return Err(NodeError::Provider {
    provider: "mcp",
    message: err.to_string(),
});
```

**After (v0.3.0):**
```rust
use weavegraph::node::{NodeError, NodeResultExt};

// Keep Provider for real provider identity.
return Err(NodeError::Provider {
    provider: "mcp",
    message: "upstream rejected request".to_string(),
});

// Generic external errors use Other.
let parsed = std::fs::read_to_string("config.json").node_err()?;
```

**Optional diagnostics metadata:**
```toml
[features]
diagnostics = ["dep:miette"]
```

Enable `diagnostics` when you want `miette::Diagnostic` metadata on error enums.

**Migration steps:**
1. Keep `NodeError::Provider` only for true provider/service errors
2. Replace generic wrapping with `NodeError::other(...)` or `.node_err()?`
3. Remove use of `NodeError::Anyhow` and the `anyhow` crate feature
4. Enable `diagnostics` only where rich terminal diagnostics are desired
5. All error types are now matchable enums — use pattern matching instead of `.downcast_ref()`

#### 4. LLM Abstraction + Rig Feature Rename (`0.3.3` + `0.3.5`) (High Impact)

**What changed:**
- Added framework-agnostic traits under `weavegraph::llm` (`LlmProvider`, `LlmStreamProvider`, `LlmResponse`)
- Added dedicated Rig adapter module under `weavegraph::llm::rig_adapter` (gated by `rig` feature)
- Renamed feature flag from `llm` to `rig`
- Kept `llm` as backward-compatible alias to `rig` for 0.3.x
- Added both conversion impls:
    - `From<weavegraph::message::Message> for rig::completion::message::Message`
    - `From<rig::completion::message::Message> for weavegraph::message::Message`

**Why this matters:**
Weavegraph no longer treats a specific LLM SDK as part of its core API contract.
Consumers can keep using Rig via feature-gated adapters while retaining a stable,
framework-neutral integration surface.

**Feature migration:**
```toml
# Before
weavegraph = { version = "0.2", features = ["llm"] }

# After (preferred)
weavegraph = { version = "0.3", features = ["rig"] }

# 0.3.x compatibility path (still works)
weavegraph = { version = "0.3", features = ["llm"] }
```

**Message conversion migration:**
```rust
use weavegraph::message::Message;

// weavegraph -> rig
let rig_messages: Vec<rig::completion::message::Message> =
        history.clone().into_iter().map(Into::into).collect();

// rig -> weavegraph
let wg_messages: Vec<Message> = rig_messages.into_iter().map(Into::into).collect();
```

**Role-mapping caveats:**
- Rig completion history is user/assistant-oriented.
- `Role::System`, `Role::Tool`, and `Role::Custom(_)` map to Rig user messages.
- Reverse conversion cannot reconstruct original non-native roles from Rig message history.

**Migration steps:**
1. Prefer `features = ["rig"]` in `Cargo.toml`
2. Keep `llm` only as a temporary alias while rolling upgrades
3. Replace bespoke conversion boilerplate with `Into::into` impls
4. If your workflow depends on preserving system/tool/custom roles across Rig round-trips, carry role metadata out-of-band

#### 5. Checkpointer Custom Escape Hatch + Precedence (`0.3.4`) (Medium Impact)

**What changed:**
- Added `AppRunner::builder().checkpointer_custom(Arc<dyn Checkpointer>)`
- Added `RuntimeConfig::checkpointer_custom(Arc<dyn Checkpointer>)`
- Kept enum convenience route (`CheckpointerType`) for in-memory/SQLite/Postgres
- Added deterministic precedence when both are present: custom checkpointer wins
- Marked `RuntimeConfig.checkpointer` field as deprecated for planned removal in `0.4.0`

**Precedence rules:**
1. If a custom checkpointer is set, it is always used
2. Otherwise, enum-based `CheckpointerType` is used
3. If neither is set, runtime falls back to `CheckpointerType::InMemory`

**Before (enum only):**
```rust
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory)
    .build()
    .await;
```

**After (custom override):**
```rust
use std::sync::Arc;
use weavegraph::runtimes::{AppRunner, Checkpointer, CheckpointerType};

let custom: Arc<dyn Checkpointer> = Arc::new(MyCheckpointer::new());

let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory) // convenience default
    .checkpointer_custom(custom) // takes precedence
    .build()
    .await;
```

**RuntimeConfig migration:**
```rust
use std::sync::Arc;
use weavegraph::runtimes::{CheckpointerType, RuntimeConfig};

let cfg = RuntimeConfig::new(None, Some(CheckpointerType::InMemory), None)
    .checkpointer_custom(Arc::new(MyCheckpointer::new()));
```

**Migration steps:**
1. Keep enum configuration for standard backends
2. Use `checkpointer_custom(...)` when injecting custom storage backends
3. Treat `RuntimeConfig.checkpointer` field as deprecated and migrate call sites to `RuntimeConfig::with_checkpointer(...)`/`checkpointer_custom(...)`
4. If both are configured, **custom always wins** (add tests for your expected resume behavior)

#### 6. Examples and Guide Renames (`0.3.7`) (Low Impact)

**What changed:**
- `examples/demo1.rs` -> `examples/graph_execution.rs`
- `examples/demo2.rs` -> `examples/scheduler_fanout.rs`
- `examples/STREAMING_QUICKSTART.md` moved to `docs/STREAMING.md`
- `docs/QUICKSTART.md` now replaces the old guide entrypoint
- `examples/README.md` was reduced to a lean runnable index

**Migration steps:**
1. Update local scripts and docs that run `cargo run --example demo1` to `cargo run --example graph_execution`
2. Update local scripts and docs that run `cargo run --example demo2` to `cargo run --example scheduler_fanout`
3. Update links from old streaming/example docs paths to `docs/STREAMING.md`
4. Update guide links to `docs/QUICKSTART.md`

#### 7. `LadderError` renamed to `WeaveError` (`0.3.8`) (Medium Impact)

**What changed:**
- Canonical error type in `channels::errors` is now `WeaveError`
- A 0.3.x compatibility alias remains:
    `#[deprecated] pub type LadderError = WeaveError;`
- Alias removal is planned for `0.4.0`

**Before:**
```rust
use weavegraph::channels::errors::{ErrorEvent, LadderError};

let event = ErrorEvent::app(LadderError::msg("startup failed"));
```

**After (preferred):**
```rust
use weavegraph::channels::errors::{ErrorEvent, WeaveError};

let event = ErrorEvent::app(WeaveError::msg("startup failed"));
```

**Migration steps:**
1. Replace imports of `LadderError` with `WeaveError`
2. Replace explicit type annotations (`LadderError`) with `WeaveError`
3. If you consume JSON schema names directly, update references from `LadderError` to `WeaveError`

---

## v0.2.0 (Upcoming)

### Breaking Changes

#### 1. Message Role Helpers + `Role` Enum (High Impact)

**What changed:**  
Weavegraph introduced a typed [`Role`](weavegraph::message::Role) enum and helper APIs.

For backward compatibility, `Message.role` remains a `String` (it still serializes cleanly to JSON), but you should treat roles as typed via `Role`, `Message::with_role`, `Message::role_type()`, and `Message::is_role()`.

**Before (v0.1.x):**
```rust
// Old: role was a String
let msg = Message::new("user", "Hello");

// Checking roles
if msg.role == "user" { ... }
```

**After (v0.2.0):**
```rust
use weavegraph::message::{Message, Role};

// New: use Role enum variants
let msg = Message::with_role(Role::User, "Hello");

// Or construct explicitly with a typed Role
let msg = Message::with_role(Role::User, "Hello");

// Checking roles (type-safe)
if msg.is_role(Role::User) {
    // ...
}
```

**Migration steps:**
1. Prefer `Message::with_role(Role::..., ...)` (typed roles)
2. Replace string comparisons like `msg.role == "user"` with `msg.is_role(Role::User)`
3. For custom roles, prefer `Message::with_role(Role::Custom("my_role".into()), ...)`
4. If you must keep string roles (interop), use `msg.role_type()` when branching

**Convenience constructors (recommended):**
```rust
// These create messages with the correct role already set
let user_msg = Message::with_role(Role::User, "User input");
let assistant_msg = Message::with_role(Role::Assistant, "AI response");
let system_msg = Message::with_role(Role::System, "System prompt");
let tool_msg = Message::with_role(Role::Tool, "Tool output");
```

---

#### 2. AppRunner Constructor Consolidation (Medium Impact)

**What changed:**  
Multiple `AppRunner` constructors have been consolidated into a builder pattern.

**Before (v0.1.x):**
```rust
// Various constructors
let runner = AppRunner::new(app, CheckpointerType::InMemory).await;
let runner = AppRunner::with_options(app, checkpointer, event_bus).await;
let runner = AppRunner::with_options_and_bus(app, checkpointer, event_bus).await;
```

**After (v0.2.0):**
```rust
// Use the builder pattern
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::InMemory)
    .build()
    .await;

// With event bus
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::SQLite)
    .event_bus(bus)
    .autosave(true)
    .build()
    .await;
```

**Migration steps:**
1. Replace `AppRunner::new(app, checkpointer)` with `AppRunner::builder().app(app).checkpointer(checkpointer).build()`
2. Replace `AppRunner::with_options(...)` with the equivalent builder calls
3. The old constructors are deprecated but still available; update at your convenience

---

#### 3. Runner Module Decomposition (Low Impact - Internal)

**What changed:**  
The `runtimes/runner.rs` module was split into focused sub-modules:
- `runtimes/session.rs` - Session lifecycle management
- `runtimes/execution.rs` - Step execution logic
- `runtimes/streaming.rs` - Event stream management
- `runtimes/runner.rs` - Slim coordinator

**Impact:**  
This is primarily an internal refactoring. Public API remains stable. If you were
importing internal types directly from the runner module, update your imports:

```rust
// Before (if using internal imports)
use weavegraph::runtimes::runner::SessionState;

// After
use weavegraph::runtimes::session::SessionState;
```

---

#### 4. Removed `.expect()` Calls (Low Impact)

**What changed:**  
Production code no longer uses `.expect()`. Methods that previously panicked now
return `Result` types.

**Affected methods:**
- `AppRunner` internal checkpoint operations now propagate errors
- Clock timestamp operations use safe fallbacks

**Impact:**  
If you were relying on panics for error handling, you'll need to handle `Result`
types explicitly. This improves reliability in production deployments.

---

### Deprecations

The following items are deprecated and will be removed in v0.3.0:

| Deprecated | Replacement |
|-----------|-------------|
| `Message::USER` constant | `Role::User` + `Message::with_role(...)` |
| `Message::ASSISTANT` constant | `Role::Assistant` + `Message::with_role(...)` |
| `Message::SYSTEM` constant | `Role::System` + `Message::with_role(...)` |
| `AppRunner::new()` | `AppRunner::builder()...build()` |
| `AppRunner::with_options()` | `AppRunner::builder()...build()` |

---

### New Features

#### Type-Safe Message Roles
The new `Role` enum provides compile-time safety for message roles:
```rust
use weavegraph::message::Role;

match msg.role {
    Role::User => handle_user_input(),
    Role::Assistant => handle_ai_response(),
    Role::System => handle_system_prompt(),
    Role::Tool => handle_tool_result(),
    Role::Custom(ref name) => handle_custom(name),
}
```

#### Builder Pattern for AppRunner
More flexible and self-documenting runner construction:
```rust
let runner = AppRunner::builder()
    .app(app)
    .checkpointer(CheckpointerType::SQLite)
    .event_bus(EventBus::with_sinks(vec![Box::new(JsonLinesSink::new(file))]))
    .autosave(true)
    .build()
    .await;
```

#### Graph API Enhancements
New iteration methods inspired by petgraph:
```rust
let builder = GraphBuilder::new()
    .add_node(NodeKind::Custom("A".into()), MyNode)
    .add_node(NodeKind::Custom("B".into()), MyNode)
    .add_edge(NodeKind::Start, NodeKind::Custom("A".into()))
    .add_edge(NodeKind::Custom("A".into()), NodeKind::Custom("B".into()))
    .add_edge(NodeKind::Custom("B".into()), NodeKind::End);

for node_kind in builder.nodes() {
    println!("Node: {node_kind}");
}

for (from, to) in builder.edges() {
    println!("Edge: {from} -> {to}");
}

for node in builder.topological_sort() {
    println!("Topo: {node}");
}
```

---

## v0.1.x Releases

### v0.1.3
- Added `VersionedState::new_with_user_message()` convenience constructor
- Fixed edge case in conditional edge routing with empty predicate results
- Improved event bus backpressure handling
- Added Postgres checkpointing

### v0.1.2
- Initial public release
- Graph-driven workflow execution
- SQLite and in-memory checkpointing
- Event bus with multiple sink types
- Property-based test coverage

---

## Getting Help

If you encounter issues during migration:

1. Check the [examples](examples/) for updated usage patterns
2. Review the [ARCHITECTURE.md](docs/ARCHITECTURE.md) for design context
3. Open an issue on [GitHub](https://github.com/Idleness76/weavegraph/issues)

---

## Version Compatibility Matrix

| Weavegraph | Rust MSRV | rig-core | tokio |
|------------|-----------|----------|-------|
| 0.3.x      | 1.90.0    | 0.30.x   | 1.x   |
| 0.2.x      | 1.89.0    | 0.28+    | 1.x   |
| 0.1.x      | 1.89.0    | 0.28+    | 1.x   |
