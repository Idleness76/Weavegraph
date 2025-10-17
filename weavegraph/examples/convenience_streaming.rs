//! # Convenience Streaming Example
//!
//! This example demonstrates the new convenience helpers for event streaming:
//! - `App::invoke_with_channel()` - Simple streaming with a channel
//! - `App::invoke_with_sinks()` - Multiple custom sinks
//!
//! These methods simplify the common case while the full `AppRunner::with_options_and_bus()`
//! pattern remains available for advanced use cases like web servers.
//!
//! ## When to Use Each Pattern
//!
//! ### `invoke_with_channel()` - CLI Tools & Scripts
//! - Simple one-off executions
//! - Want events streamed to a single channel
//! - Don't need per-request isolation
//!
//! ### `invoke_with_sinks()` - Multiple Destinations
//! - Need events in multiple places (stdout + file + metrics)
//! - Single execution with custom event routing
//! - More control than `invoke_with_channel()`
//!
//! ### `AppRunner::with_options_and_bus()` - Web Servers
//! - Per-request event isolation required
//! - SSE or WebSocket streaming
//! - Multiple concurrent clients
//!
//! ## Run This Example
//!
//! ```bash
//! cargo run --example convenience_streaming
//! ```

use async_trait::async_trait;
use weavegraph::{
    channels::Channel,
    event_bus::{ChannelSink, StdOutSink},
    graphs::GraphBuilder,
    message::Message,
    node::{Node, NodeContext, NodeError, NodePartial},
    state::{StateSnapshot, VersionedState},
    types::NodeKind,
};

/// A node that simulates work with progress updates
#[derive(Debug, Clone)]
struct ProgressNode {
    steps: usize,
}

impl ProgressNode {
    fn new(steps: usize) -> Self {
        Self { steps }
    }
}

#[async_trait]
impl Node for ProgressNode {
    async fn run(&self, _: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        for i in 1..=self.steps {
            ctx.emit(
                "progress",
                format!("Step {}/{}: Processing...", i, self.steps),
            )?;
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        Ok(NodePartial::new().with_messages(vec![Message::assistant("Complete!")]))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Convenience Streaming Examples ===\n");
    println!("This example demonstrates two new convenience methods for event streaming:\n");

    // Build graph once (can be reused)
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("progress".into()), ProgressNode::new(3))
        .add_edge(NodeKind::Start, NodeKind::Custom("progress".into()))
        .add_edge(NodeKind::Custom("progress".into()), NodeKind::End)
        .compile()?;

    // ============================================================================
    // Example 1: invoke_with_channel() - Simple channel streaming
    // ============================================================================
    println!("## Example 1: invoke_with_channel()");
    println!("   Use case: CLI tools, simple progress monitoring\n");

    let (result, events) = app
        .invoke_with_channel(VersionedState::new_with_user_message("Start task 1"))
        .await;

    // Spawn task to handle events (simulating progress bar or logging)
    let event_handler = tokio::spawn(async move {
        let mut count = 0;
        println!("   📡 Listening for events...");

        // Use timeout to avoid hanging if events stop
        let timeout = tokio::time::Duration::from_millis(100);
        loop {
            match tokio::time::timeout(timeout, events.recv_async()).await {
                Ok(Ok(event)) => {
                    count += 1;
                    println!("      Event {}: {}", count, event.message());
                }
                Ok(Err(_)) => {
                    println!("   ✅ Channel closed (workflow complete)");
                    break;
                }
                Err(_) => {
                    println!("   ⏱️  No more events (timeout)");
                    break;
                }
            }
        }
        count
    });

    // Wait for workflow
    let final_state = result?;
    println!(
        "   ✅ Workflow completed with {} messages",
        final_state.messages.len()
    );

    // Wait for event collection
    let event_count = event_handler.await?;
    println!("   📊 Received {} events total\n", event_count);

    // Give some time before next example
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // ============================================================================
    // Example 2: invoke_with_sinks() - Multiple destinations
    // ============================================================================
    println!("## Example 2: invoke_with_sinks()");
    println!("   Use case: Events to multiple destinations (stdout + channel + file)\n");

    let (tx, rx) = flume::unbounded();

    println!("   🔧 Configured sinks:");
    println!("      • StdOutSink (you'll see events below)");
    println!("      • ChannelSink (collecting in background)\n");

    // Spawn background collector for channel
    let channel_collector = tokio::spawn(async move {
        let mut events = Vec::new();
        let timeout = tokio::time::Duration::from_millis(100);
        loop {
            match tokio::time::timeout(timeout, rx.recv_async()).await {
                Ok(Ok(event)) => events.push(event),
                Ok(Err(_)) | Err(_) => break,
            }
        }
        events
    });

    // Execute with multiple sinks
    let final_state = app
        .invoke_with_sinks(
            VersionedState::new_with_user_message("Start task 2"),
            vec![
                Box::new(StdOutSink::default()),
                Box::new(ChannelSink::new(tx)),
            ],
        )
        .await?;

    println!(
        "\n   ✅ Workflow completed with {} messages",
        final_state.messages.len()
    );

    // Get channel events
    let channel_events = channel_collector.await?;
    println!("   📊 Channel received {} events", channel_events.len());
    println!("   📊 Events were also printed to stdout above\n");

    // ============================================================================
    // Summary
    // ============================================================================
    println!("=== Summary ===\n");
    println!("✅ invoke_with_channel():");
    println!("   • Returns (Result, Receiver)");
    println!("   • Perfect for CLI tools");
    println!("   • Simple single-channel streaming\n");

    println!("✅ invoke_with_sinks():");
    println!("   • Takes Vec<Box<dyn EventSink>>");
    println!("   • Events go to multiple destinations");
    println!("   • More flexible than channel-only\n");

    println!("💡 For web servers with per-request isolation:");
    println!("   Use AppRunner::with_options_and_bus() instead");
    println!("   (See examples/streaming_events.rs)\n");

    Ok(())
}
