# Quickstart

Build and run a minimal workflow in Weavegraph, then layer in routing and runtime controls.

## Graph Building {#graphs}

```rust,no_run
use async_trait::async_trait;
use weavegraph::graphs::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

struct Echo;

#[async_trait]
impl Node for Echo {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let content = snapshot
            .messages
            .last()
            .map(|m| format!("echo: {}", m.content))
            .unwrap_or_else(|| "echo: (empty input)".to_string());

        Ok(NodePartial::new().with_messages(vec![Message::assistant(&content)]))
    }
}

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let app = GraphBuilder::new()
    .add_node(NodeKind::Custom("echo".into()), Echo)
    .add_edge(NodeKind::Start, NodeKind::Custom("echo".into()))
    .add_edge(NodeKind::Custom("echo".into()), NodeKind::End)
    .compile()?;

let initial = VersionedState::new_with_user_message("hello");
let final_state = app.invoke(initial).await?;
assert!(!final_state.snapshot().messages.is_empty());
# Ok(())
# }
```

### Conditional Routing

```rust,no_run
use std::sync::Arc;
use weavegraph::graphs::{EdgePredicate, GraphBuilder};
use weavegraph::types::NodeKind;

let route: EdgePredicate = Arc::new(|snapshot| {
    if snapshot.extra.contains_key("needs_review") {
        vec![NodeKind::Custom("review".into()).as_target()]
    } else {
        vec![NodeKind::Custom("publish".into()).as_target()]
    }
});

# struct MyNode;
# #[async_trait::async_trait]
# impl weavegraph::node::Node for MyNode {
#   async fn run(&self, _: weavegraph::state::StateSnapshot, _: weavegraph::node::NodeContext)
#     -> Result<weavegraph::node::NodePartial, weavegraph::node::NodeError> {
#       Ok(weavegraph::node::NodePartial::default())
#   }
# }
let _app = GraphBuilder::new()
    .add_node(NodeKind::Custom("analyze".into()), MyNode)
    .add_node(NodeKind::Custom("review".into()), MyNode)
    .add_node(NodeKind::Custom("publish".into()), MyNode)
    .add_edge(NodeKind::Start, NodeKind::Custom("analyze".into()))
    .add_conditional_edge(NodeKind::Custom("analyze".into()), route)
    .add_edge(NodeKind::Custom("review".into()), NodeKind::End)
    .add_edge(NodeKind::Custom("publish".into()), NodeKind::End)
    .compile();
```

## Messages {#messages}

```rust
use weavegraph::message::{Message, Role};

let user = Message::with_role(Role::User, "input");
let assistant = Message::assistant("output");
assert_eq!(user.role, Role::User);
assert_eq!(assistant.role, Role::Assistant);
```

## State Management {#state}

```rust
use weavegraph::state::VersionedState;

let state = VersionedState::builder()
    .with_user_message("hello")
    .with_extra("request_id", serde_json::json!("req-1"))
    .build();

let snapshot = state.snapshot();
assert!(!snapshot.messages.is_empty());
```

## Execution Modes

- `App::invoke(...)`: simplest one-shot execution.
- `App::invoke_streaming(...)`: get an `EventStream` for SSE/WebSocket/observers.
- `AppRunner::builder()`: full runtime control (checkpointer, event bus, autosave, listener).

## Next Docs

- `docs/STREAMING.md`: production streaming patterns and diagnostics.
- `docs/OPERATIONS.md`: persistence, testing, and deployment guidance.
- `docs/ARCHITECTURE.md`: module boundaries and runtime execution model.
- `docs/MIGRATION.md`: upgrade guidance by release.
