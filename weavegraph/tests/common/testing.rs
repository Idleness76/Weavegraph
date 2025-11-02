#![allow(dead_code)]

use async_trait::async_trait;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::StateSnapshot;
use weavegraph::types::NodeKind;

#[derive(Debug, Clone)]
pub struct TestNode {
    pub name: &'static str,
}

// Example usage to avoid dead_code warning
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_testnode_construction() {
        let node = TestNode { name: "example" };
        let bus = weavegraph::event_bus::EventBus::default();
        let ctx = NodeContext {
            node_id: "test_node".to_string(),
            step: 1,
            event_emitter: bus.get_emitter(),
        };
        let snapshot = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: FxHashMap::default(),
            extra_version: 1,
            errors: vec![],
            errors_version: 1,
        };
        let result = node.run(snapshot, ctx).await;
        assert!(result.is_ok());
    }
}

#[async_trait]
impl Node for TestNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Ok(NodePartial {
            messages: Some(vec![Message::assistant(&format!(
                "ran:{}:step:{}",
                self.name, ctx.step
            ))]),
            extra: None,
            errors: None,
            frontier: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct DelayedNode {
    pub name: &'static str,
    pub delay_ms: u64,
}

#[async_trait]
impl Node for DelayedNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        sleep(Duration::from_millis(self.delay_ms)).await;
        Ok(NodePartial {
            messages: Some(vec![Message::assistant(&format!(
                "ran:{}:step:{}",
                self.name, ctx.step
            ))]),
            extra: None,
            errors: None,
            frontier: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FailingNode {
    pub error_message: &'static str,
}

impl Default for FailingNode {
    fn default() -> Self {
        Self {
            error_message: "test_key",
        }
    }
}

#[async_trait]
impl Node for FailingNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Err(NodeError::MissingInput {
            what: self.error_message,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RichNode {
    pub name: &'static str,
    pub produce_extra: bool,
}

#[async_trait]
impl Node for RichNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let messages = Some(vec![Message::assistant(&format!(
            "{}:step:{}",
            self.name, ctx.step
        ))]);

        let extra = if self.produce_extra {
            let mut map = FxHashMap::default();
            map.insert(format!("{}_executed", self.name), json!(true));
            map.insert("step".to_string(), json!(ctx.step));
            Some(map)
        } else {
            None
        };

        Ok(NodePartial {
            messages,
            extra,
            errors: None,
            frontier: None,
        })
    }
}

pub fn make_test_registry() -> FxHashMap<NodeKind, Arc<dyn Node>> {
    let mut registry = FxHashMap::default();
    registry.insert(
        NodeKind::Custom("A".into()),
        Arc::new(TestNode { name: "A" }) as Arc<dyn Node>,
    );
    registry.insert(
        NodeKind::Custom("B".into()),
        Arc::new(TestNode { name: "B" }) as Arc<dyn Node>,
    );
    registry.insert(
        NodeKind::End,
        Arc::new(TestNode { name: "END" }) as Arc<dyn Node>,
    );
    registry
}

pub fn make_delayed_registry() -> FxHashMap<NodeKind, Arc<dyn Node>> {
    let mut registry = FxHashMap::default();
    registry.insert(
        NodeKind::Custom("A".into()),
        Arc::new(DelayedNode {
            name: "A",
            delay_ms: 30,
        }) as Arc<dyn Node>,
    );
    registry.insert(
        NodeKind::Custom("B".into()),
        Arc::new(DelayedNode {
            name: "B",
            delay_ms: 1,
        }) as Arc<dyn Node>,
    );
    registry
}

pub fn create_test_snapshot(messages_version: u32, extra_version: u32) -> StateSnapshot {
    StateSnapshot {
        messages: vec![],
        messages_version,
        extra: FxHashMap::default(),
        extra_version,
        errors: vec![],
        errors_version: 1,
    }
}
