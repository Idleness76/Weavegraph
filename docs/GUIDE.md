# Developer Guide

A comprehensive guide to Weavegraph's core concepts and APIs.

## Messages {#messages}

Weavegraph uses a type-safe message system for agent and workflow communication. Messages have roles (user, assistant, system, etc.) and are constructed with ergonomic helpers.

### Constructors

```rust
use weavegraph::message::Message;

// Recommended: convenience constructors
let user_msg = Message::user("What's the weather like?");
let assistant_msg = Message::assistant("It's sunny and 75°F!");
let system_msg = Message::system("You are a helpful assistant.");

// Custom roles
let function_msg = Message::new("function", "Processing complete");

// Typed role helpers
use weavegraph::message::Role;
let tool_msg = Message::with_role(Role::Tool, "Tool output");
assert!(tool_msg.is_role(Role::Tool));
```

## State Management {#state}

Weavegraph provides versioned state management with channel isolation and snapshot consistency.

### Initialization

```rust
use weavegraph::state::VersionedState;

// Simple initialization
let state = VersionedState::new_with_user_message("Hello!");

// Builder for richer state
let state = VersionedState::builder()
    .with_user_message("What's the weather?")
    .with_system_message("You are a weather assistant")
    .with_extra("location", serde_json::json!("San Francisco"))
    .build();
```

### State Snapshots

State snapshots provide immutable views of workflow state at specific points in execution. Nodes receive snapshots and return partial updates that are merged via reducers.

## Graph Building & Conditional Edges {#graphs}

Define workflows declaratively with unconditional and conditional routing.

### Basic Graph

```rust
use weavegraph::graphs::GraphBuilder;
use weavegraph::types::NodeKind;

let app = GraphBuilder::new()
    .add_node(NodeKind::Custom("process".into()), ProcessNode)
    .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
    .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
    .compile()?;
```

### Conditional Edges

A conditional edge predicate must return a `Vec<String>` of target node names (or virtual endpoints) using the helper `NodeKind::as_target()` (or `NodeKind::start_target()/end_target()` for endpoints).

```rust
use weavegraph::graphs::{GraphBuilder, EdgePredicate};
use weavegraph::types::NodeKind;
use std::sync::Arc;

// Predicate decides next hop based on snapshot extra data
let escalate_or_respond: EdgePredicate = Arc::new(|snap| {
    if snap.extra.contains_key("needs_escalation") {
        vec![NodeKind::Custom("escalate".into()).as_target()]
    } else {
        vec![NodeKind::Custom("respond".into()).as_target()]
    }
});

let app = GraphBuilder::new()
    .add_node(NodeKind::Custom("analyze".into()), AnalyzeNode)
    .add_node(NodeKind::Custom("respond".into()), RespondNode)
    .add_node(NodeKind::Custom("escalate".into()), EscalateNode)
    .add_edge(NodeKind::Start, NodeKind::Custom("analyze".into()))
    .add_conditional_edge(NodeKind::Custom("analyze".into()), escalate_or_respond)
    .add_edge(NodeKind::Custom("respond".into()), NodeKind::End)
    .add_edge(NodeKind::Custom("escalate".into()), NodeKind::End)
    .compile()?;
```

**Important notes:**
- Return only registered custom node names or the virtual endpoints (`Start`, `End`)
- Unknown targets are ignored with a warning
- For multiple fan-out routes, push several target strings into the returned `Vec<String>`

**Troubleshooting:**
- If execution stops unexpectedly after a conditional edge, verify predicate outputs match registered node names
- Unit test predicates directly with `StateSnapshot` to validate branching logic

### Virtual Endpoints

`NodeKind::Start` and `NodeKind::End` are virtual structural endpoints. You never register them with `add_node`; attempts to do so are ignored with a warning. Define only your executable (custom) nodes and connect them with edges from `Start` and to `End`.

See also: [Operations Guide](OPERATIONS.md#event-streaming), [Architecture](ARCHITECTURE.md)
