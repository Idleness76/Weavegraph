# Migration Guide

This document outlines breaking changes between Weavegraph versions and provides
migration guidance for upgrading your code.

---

## v0.2.0 (Upcoming)

### Breaking Changes

#### 1. Message Role Enum (High Impact)

**What changed:**  
The `Message.role` field changed from `String` to the new `Role` enum.

**Before (v0.1.x):**
```rust
// Old: role was a String
let msg = Message {
    role: "user".to_string(),
    content: "Hello".to_string(),
    ..Default::default()
};

// Checking roles
if msg.role == "user" { ... }
```

**After (v0.2.0):**
```rust
use weavegraph::message::{Message, Role};

// New: use Role enum variants
let msg = Message::user("Hello");

// Or construct manually with the enum
let msg = Message {
    role: Role::User,
    content: "Hello".to_string(),
    ..Default::default()
};

// Checking roles
if msg.role == Role::User { ... }
// Or use the matches helper
if msg.role.matches("user") { ... }
```

**Migration steps:**
1. Replace `role: "user".to_string()` with `role: Role::User`
2. Replace `role: "assistant".to_string()` with `role: Role::Assistant`
3. Replace `role: "system".to_string()` with `role: Role::System`
4. Replace `role: "tool".to_string()` with `role: Role::Tool`
5. For custom roles: `role: Role::Custom("my_role".to_string())`
6. Replace string comparisons with enum comparisons or `role.matches("...")`

**Convenience constructors (recommended):**
```rust
// These create messages with the correct role already set
let user_msg = Message::user("User input");
let assistant_msg = Message::assistant("AI response");
let system_msg = Message::system("System prompt");
let tool_msg = Message::tool("Tool output");
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
let runner = AppRunner::with_arc_and_bus(app_arc, checkpointer, bus).await;
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
    .checkpointer(CheckpointerType::Sqlite(path))
    .event_sink(my_sink)
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
| `ROLE_USER` constant | `Role::User` enum variant |
| `ROLE_ASSISTANT` constant | `Role::Assistant` enum variant |
| `ROLE_SYSTEM` constant | `Role::System` enum variant |
| `ROLE_TOOL` constant | `Role::Tool` enum variant |
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
    .checkpointer(CheckpointerType::Sqlite("./data/checkpoints.db".into()))
    .event_sink(JsonLinesSink::new(file))
    .autosave(true)
    .max_concurrent_nodes(4)
    .build()
    .await?;
```

#### Graph API Enhancements
New iteration methods inspired by petgraph:
```rust
// Iterate over all nodes
for node_kind in app.graph().nodes() {
    println!("Node: {:?}", node_kind);
}

// Iterate over all edges
for (from, to) in app.graph().edges() {
    println!("Edge: {:?} -> {:?}", from, to);
}

// Topological ordering for deterministic traversal
for node in app.graph().topological_sort()? {
    println!("Order: {:?}", node);
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
