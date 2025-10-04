//! Shared testing utilities for the Weavegraph framework.
//!
//! This module provides common test nodes and utilities to eliminate
//! code duplication across test modules and maintain consistency
//! in test implementations.

use crate::message::Message;
use crate::node::{Node, NodeContext, NodeError, NodePartial};
use crate::state::StateSnapshot;
use crate::types::NodeKind;
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// A minimal test node that returns a marker message for validation.
///
/// This node is useful for testing basic scheduler functionality
/// without complex logic or side effects.
#[derive(Debug, Clone)]
pub struct TestNode {
    pub name: &'static str,
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
        })
    }
}

/// A test node that sleeps for a configured duration.
///
/// Useful for testing concurrent execution and completion ordering
/// in scheduler tests.
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
        })
    }
}

/// A test node that always fails with a specific error.
///
/// Used for testing error propagation and handling in the
/// scheduler and runtime systems.
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

/// A test node that produces both messages and extra data.
///
/// Useful for testing channel updates and barrier application.
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
        })
    }
}

/// Creates a standard test node registry for scheduler testing.
///
/// Returns a registry with nodes "A", "B", and "END" for common
/// test scenarios.
pub fn make_test_registry() -> FxHashMap<NodeKind, Arc<dyn Node>> {
    let mut registry = FxHashMap::default();
    registry.insert(
        NodeKind::Other("A".into()),
        Arc::new(TestNode { name: "A" }) as Arc<dyn Node>,
    );
    registry.insert(
        NodeKind::Other("B".into()),
        Arc::new(TestNode { name: "B" }) as Arc<dyn Node>,
    );
    registry.insert(
        NodeKind::End,
        Arc::new(TestNode { name: "END" }) as Arc<dyn Node>,
    );
    registry
}

/// Creates a test node registry with delayed nodes for concurrency testing.
///
/// Useful for testing concurrent execution and completion ordering.
pub fn make_delayed_registry() -> FxHashMap<NodeKind, Arc<dyn Node>> {
    let mut registry = FxHashMap::default();
    registry.insert(
        NodeKind::Other("A".into()),
        Arc::new(DelayedNode {
            name: "A",
            delay_ms: 30,
        }) as Arc<dyn Node>,
    );
    registry.insert(
        NodeKind::Other("B".into()),
        Arc::new(DelayedNode {
            name: "B",
            delay_ms: 1,
        }) as Arc<dyn Node>,
    );
    registry
}

/// Creates a StateSnapshot with specified version numbers for testing.
///
/// This utility function reduces boilerplate in scheduler tests.
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
