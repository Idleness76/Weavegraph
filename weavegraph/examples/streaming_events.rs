//! # Streaming Events Example
//!
//! This example demonstrates how to stream events from a Weavegraph workflow
//! using the `ChannelSink`. This pattern is the foundation for building real-time
//! web dashboards, SSE endpoints, or WebSocket connections.
//!
//! ## What This Example Shows
//!
//! 1. **Creating a streaming channel** - Using `tokio::sync::mpsc`
//! 2. **Registering a ChannelSink** - Forwarding events to the channel
//! 3. **Running a workflow** - While capturing events in real-time
//! 4. **Consuming the stream** - Processing events as they arrive
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │  Workflow Node  │ ──ctx.emit()──┐
//! └─────────────────┘                │
//!                                    ▼
//! ┌────────────────────────────────────────┐
//! │  EventBus (broadcasts to all sinks)    │
//! └─────────┬──────────────────────────────┘
//!           │
//!           ├─────────────┐
//!           ▼             ▼
//!     StdOutSink     ChannelSink ──→ mpsc::channel ──→ Your Code
//!     (terminal)     (streaming)
//! ```
//!
//! ## Web Integration
//!
//! For Axum/HTTP integration, create the EventBus with ChannelSink and pass it
//! to the AppRunner:
//!
//! ```ignore
//! // With Axum (requires adding axum dependency):
//! use axum::response::sse::{Event as SseEvent, Sse};
//! use tokio_stream::wrappers::UnboundedReceiverStream;
//!
//! async fn stream_handler(
//!     State(graph): State<Arc<App>>
//! ) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
//!     let (tx, rx) = mpsc::unbounded_channel();
//!     
//!     // Create EventBus with ChannelSink for this client
//!     let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
//!     
//!     // Run workflow with custom EventBus using AppRunner
//!     let graph_clone = graph.clone();
//!     tokio::spawn(async move {
//!         let mut runner = AppRunner::with_options_and_bus(
//!             Arc::try_unwrap(graph_clone).unwrap_or_else(|arc| (*arc).clone()),
//!             CheckpointerType::InMemory,
//!             false,
//!             bus,
//!             true,
//!         ).await;
//!         
//!         let session_id = format!("client-{}", uuid::Uuid::new_v4());
//!         let initial_state = VersionedState::new_with_user_message("...");
//!         runner.create_session(session_id.clone(), initial_state).await.ok();
//!         runner.run_until_complete(&session_id).await
//!     });
//!     
//!     // Stream events as Server-Sent Events
//!     let stream = UnboundedReceiverStream::new(rx).map(|event| {
//!         Ok(SseEvent::default().json_data(event).unwrap())
//!     });
//!     Sse::new(stream)
//! }
//! ```
//!
//! **Key Points:**
//! - Each client connection gets its own `ChannelSink` and `EventBus`
//! - Use `AppRunner::with_options_and_bus()` to inject the custom EventBus
//! - The workflow runs in a background task while the stream is returned immediately
//! - See `STREAMING_IMPLEMENTATION.md` for complete Axum examples
//!
//! ## Run This Example
//!
//! ```bash
//! cargo run --example streaming_events
//! ```

use async_trait::async_trait;
use flume;
use miette;
use serde_json::json;
use std::sync::Arc;

use weavegraph::{
    event_bus::{ChannelSink, Event, EventBus},
    graphs::GraphBuilder,
    message::Message,
    node::{Node, NodeContext, NodeError, NodePartial},
    state::{StateSnapshot, VersionedState},
    types::NodeKind,
};

use tracing::info;
use tracing_error::ErrorLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false)
                .compact(),
        )
        .with(
            EnvFilter::from_default_env()
                .add_directive("weavegraph=info".parse().unwrap())
                .add_directive("streaming_events=info".parse().unwrap()),
        )
        .with(ErrorLayer::default())
        .init();
}

fn init_miette() {
    miette::set_panic_hook();
}

/// Demo node that emits several events during execution.
/// This simulates a real workflow that produces incremental updates.
struct ProcessingNode;

#[async_trait]
impl Node for ProcessingNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let query = snapshot
            .messages
            .first()
            .map(|m| m.content.as_str())
            .unwrap_or("default query");

        // Emit events at each step (these will be streamed to clients)
        ctx.emit("processing", format!("Starting to process: {}", query))?;
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        ctx.emit("processing", "Step 1/3: Analyzing input")?;
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        ctx.emit("processing", "Step 2/3: Computing result")?;
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        ctx.emit("processing", "Step 3/3: Formatting output")?;
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        ctx.emit("processing", "Processing complete!")?;

        Ok(NodePartial::new().with_messages(vec![Message::assistant(
            "Processing finished successfully.",
        )]))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    init_miette();

    info!("=== Streaming Events Example ===\n");

    // 1. Build the workflow graph (compile once, reuse many times)
    info!("Building workflow graph...");
    let graph = Arc::new(
        GraphBuilder::new()
            .add_node(NodeKind::Custom("Processor".into()), ProcessingNode)
            .add_edge(NodeKind::Start, NodeKind::Custom("Processor".into()))
            .add_edge(NodeKind::Custom("Processor".into()), NodeKind::End)
            .compile()?,
    );

    // 2. Create streaming channel (one per client/request in production)
    info!("Setting up event stream...\n");
    let (tx, rx) = flume::unbounded();

    // 3. Create EventBus with custom sink
    let bus = EventBus::with_sinks(vec![
        Box::new(weavegraph::event_bus::StdOutSink::default()),
        Box::new(ChannelSink::new(tx.clone())),
    ]);

    // 4. Run workflow in background with custom event bus
    let graph_clone = graph.clone();
    let workflow_task = tokio::spawn(async move {
        use weavegraph::runtimes::{runner::AppRunner, CheckpointerType};

        let initial_state = VersionedState::new_with_user_message("Process my data");

        // Create runner with our custom EventBus (don't auto-start listener)
        let mut runner = AppRunner::with_options_and_bus(
            Arc::try_unwrap(graph_clone).unwrap_or_else(|arc| (*arc).clone()),
            CheckpointerType::InMemory,
            false,
            bus,
            true, // start_listener=true
        )
        .await;

        // Create session and run
        let session_id = "stream-example-session".to_string();
        runner
            .create_session(session_id.clone(), initial_state)
            .await
            .ok();

        match runner.run_until_complete(&session_id).await {
            Ok(_) => {
                let _ = tx.send(Event::diagnostic("workflow", "Workflow completed"));
            }
            Err(e) => {
                let _ = tx.send(Event::diagnostic("workflow", format!("Error: {e}")));
            }
        }
    });

    // 6. Consume streamed events as they arrive
    info!("📡 Streaming events (these could be sent to a web client):\n");

    while let Ok(event) = rx.recv_async().await {
        // Convert event to JSON (like you would for SSE or WebSocket)
        let json_payload = json!({
            "type": match event {
                Event::Node(_) => "node",
                Event::Diagnostic(_) => "diagnostic",
            },
            "scope": event.scope_label(),
            "message": event.message(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        // In production, you'd send this to a web client
        info!(
            "📨 Stream event: {}",
            serde_json::to_string_pretty(&json_payload)?
        );

        // Break on completion event
        if event.message().contains("completed") {
            break;
        }
    }

    // Wait for workflow to finish
    workflow_task.await?;

    info!("\n=== Example Complete ===");
    info!("\n💡 Next Steps:");
    info!("   - Use this pattern with Axum for SSE endpoints");
    info!("   - Add multiple ChannelSinks for different clients");
    info!("   - Filter events by scope before streaming");

    Ok(())
}
