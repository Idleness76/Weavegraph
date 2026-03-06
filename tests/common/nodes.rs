#![allow(dead_code)]

use async_trait::async_trait;
use weavegraph::message::{Message, Role};
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::StateSnapshot;

#[derive(Debug, Clone)]
pub struct SimpleMessageNode {
    pub msg: &'static str,
}

impl SimpleMessageNode {
    pub fn new(msg: &'static str) -> Self {
        Self { msg }
    }
}

#[async_trait]
impl Node for SimpleMessageNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Ok(NodePartial::new().with_messages(vec![Message::with_role(Role::Assistant, self.msg)]))
    }
}

#[derive(Debug, Clone)]
pub struct NoopNode;

#[async_trait]
impl Node for NoopNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Ok(NodePartial::default())
    }
}

#[derive(Debug, Clone)]
pub struct EmitterNode;

#[async_trait]
impl Node for EmitterNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("test", "First event")?;
        ctx.emit("test", "Second event")?;
        ctx.emit("test", "Third event")?;
        Ok(NodePartial::new()
            .with_messages(vec![Message::with_role(Role::Assistant, "Done emitting")]))
    }
}

// Example usage to avoid dead_code warning
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_message_node_construction() {
        let _node = SimpleMessageNode::new("Hello, world!");
    }

    #[test]
    fn test_noop_node_construction() {
        let _node = NoopNode;
    }
}
