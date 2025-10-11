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
