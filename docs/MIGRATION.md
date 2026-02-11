# Migration Guide

This document outlines breaking changes between Weavegraph versions and provides
migration guidance for upgrading your code.

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

1. Check the [examples](weavegraph/examples/) for updated usage patterns
2. Review the [ARCHITECTURE.md](docs/ARCHITECTURE.md) for design context
3. Open an issue on [GitHub](https://github.com/Idleness76/weavegraph/issues)

---

## Version Compatibility Matrix

| Weavegraph | Rust MSRV | rig-core | tokio |
|------------|-----------|----------|-------|
| 0.2.x      | 1.89.0    | 0.28+    | 1.x   |
| 0.1.x      | 1.89.0    | 0.28+    | 1.x   |
