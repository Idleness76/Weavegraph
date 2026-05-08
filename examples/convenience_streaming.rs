//! # Convenience Streaming Example
//!
//! This example demonstrates the new convenience helpers for event streaming:
//! - `App::invoke_with_channel()` - Simple streaming with a channel
//! - `App::invoke_with_sinks()` - Multiple custom sinks
//!
//! These methods simplify the common case while the `AppRunner::builder()`
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
//! ### `AppRunner::builder()` - Web Servers
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
    message::{Message, Role},
    node::{Node, NodeContext, NodeError, NodePartial},
    runtimes::RuntimeConfig,
    state::{StateSnapshot, VersionedState},
    types::NodeKind,
};

use tracing::info;
use tracing_error::ErrorLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

type ExampleResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

        Ok(
            NodePartial::new()
                .with_messages(vec![Message::with_role(Role::Assistant, "Complete!")]),
        )
    }
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(
            EnvFilter::from_default_env()
                .add_directive("weavegraph=info".parse().unwrap())
                .add_directive("convenience_streaming=info".parse().unwrap()),
        )
        .with(ErrorLayer::default())
        .init();
}

#[tokio::main]
async fn main() -> ExampleResult<()> {
    init_tracing();

    info!("=== Convenience Streaming Examples ===\n");
    info!("This example demonstrates two new convenience methods for event streaming:\n");

    // Build graph once (can be reused)
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("progress".into()), ProgressNode::new(3))
        .add_edge(NodeKind::Start, NodeKind::Custom("progress".into()))
        .add_edge(NodeKind::Custom("progress".into()), NodeKind::End)
        .with_runtime_config(RuntimeConfig::new(None, None).with_memory_event_bus())
        .compile()?;

    // ============================================================================
    // Example 1: invoke_with_channel() - Simple channel streaming
    // ============================================================================
    info!("## Example 1: invoke_with_channel()");
    info!("   Use case: CLI tools, simple progress monitoring\n");

    let (result, events) = app
        .invoke_with_channel(VersionedState::new_with_user_message("Start task 1"))
        .await;

    // Spawn task to handle events (simulating progress bar or logging)
    let event_handler = tokio::spawn(async move {
        let mut count = 0;
        info!("   📡 Listening for events...");

        // Use timeout to avoid hanging if events stop
        let timeout = tokio::time::Duration::from_millis(100);
        loop {
            match tokio::time::timeout(timeout, events.recv_async()).await {
                Ok(Ok(event)) => {
                    count += 1;
                    info!("      Event {}: {}", count, event.message());
                }
                Ok(Err(_)) => {
                    info!("   ✅ Channel closed (workflow complete)");
                    break;
                }
                Err(_) => {
                    info!("   ⏱️  No more events (timeout)");
                    break;
                }
            }
        }
        count
    });

    // Wait for workflow
    let final_state = result?;
    info!(
        "   ✅ Workflow completed with {} messages",
        final_state.messages.len()
    );

    // Wait for event collection
    let event_count = event_handler
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    info!("   📊 Received {} events total\n", event_count);

    // Give some time before next example
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // ============================================================================
    // Example 2: invoke_with_sinks() - Multiple destinations
    // ============================================================================
    info!("## Example 2: invoke_with_sinks()");
    info!("   Use case: Events to multiple destinations (stdout + channel)\n");

    let (tx, rx) = flume::unbounded();

    info!("   🔧 Configured sinks:");
    info!("      • StdOutSink (you'll see events below)");
    info!("      • ChannelSink (collecting in background)\n");

    // Spawn background collector for channel
    let channel_collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Ok(event) = rx.recv_async().await {
            events.push(event);
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

    info!(
        "\n   ✅ Workflow completed with {} messages",
        final_state.messages.len()
    );

    // Get channel events
    let channel_events = channel_collector
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    info!("   📊 Channel received {} events", channel_events.len());
    info!("   📊 Events were also printed to stdout above\n");

    // ============================================================================
    // Summary
    // ============================================================================
    info!("=== Summary ===\n");
    info!("✅ invoke_with_channel():");
    info!("   • Returns (Result, Receiver)");
    info!("   • Perfect for CLI tools");
    info!("   • Simple single-channel streaming\n");

    info!("✅ invoke_with_sinks():");
    info!("   • Takes Vec<Box<dyn EventSink>>");
    info!("   • Events go to multiple destinations");
    info!("   • More flexible than channel-only\n");

    info!("💡 For web servers with per-request isolation:");
    info!("   Use AppRunner::builder() with .event_bus() instead");
    info!("   (See examples/streaming_events.rs)\n");

    Ok(())
}
