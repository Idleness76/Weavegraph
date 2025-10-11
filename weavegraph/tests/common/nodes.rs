#![allow(dead_code)]

use async_trait::async_trait;
use weavegraph::message::Message;
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
        Ok(NodePartial::new().with_messages(vec![Message::assistant(self.msg)]))
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
