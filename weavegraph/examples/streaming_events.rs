//! # Streaming Events Example
//!
//! This example demonstrates how to stream events from a Weavegraph workflow
//! using `App::invoke_streaming`. This pattern is the foundation for building
//! real-time web dashboards, SSE endpoints, or WebSocket connections without
//! wiring `AppRunner` by hand.
//!
//! ## What This Example Shows
//!
//! 1. **Invoking the workflow** - `App::invoke_streaming(initial_state)`
//! 2. **Consuming events** - Convert `EventStream` into an async iterator
//! 3. **Forwarding to clients** - Serialize events to JSON/SSE/WebSocket frames
//! 4. **Awaiting completion** - Join the workflow handle for the final state
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │  Workflow Node  │ ──ctx.emit()──┐
//! └─────────────────┘                │
//!                                    ▼
//! ┌────────────────────────────────────────┐
//! │  EventHub (broadcasts to all streams)  │
//! └─────────┬──────────────────────────────┘
//!           │
//!           ├─────────────┐
//!           ▼             ▼
//!     StdOutSink     EventStream ──→ Your Code / SSE / WebSocket
//! ```
//!
//! ## Web Integration
//!
//! For Axum/HTTP integration, convert the `EventStream` returned by
//! `invoke_streaming` into SSE frames (see `demo7_axum_sse.rs`).
//!
//! ```ignore
//! use axum::response::sse::{Event as SseEvent, Sse};
//! use futures_util::StreamExt;
//!
//! async fn stream_handler(State(app): State<Arc<App>>) -> Sse<_> {
//!     let (workflow, events) = app.invoke_streaming(initial_state).await;
//!
//!     tokio::spawn(async move {
//!         if let Err(err) = workflow.await.and_then(|res| res) {
//!             tracing::error!("workflow failed: {err}");
//!         }
//!     });
//!
//!     let sse_stream = events.into_async_stream().map(|event| {
//!         Ok(SseEvent::default().json_data(event).unwrap())
//!     });
//!     Sse::new(sse_stream)
//! }
//! ```
//!
//! **Key Points:**
//! - `App::invoke_streaming` handles the `AppRunner` boilerplate for you
//! - `EventStream::into_async_stream` is ideal for SSE/WebSocket integrations
//! - Drop-in convenience methods (`invoke_with_channel`, `invoke_with_sinks`) remain for simple scripts
//!
//! ## Run This Example
//!
//! ```bash
//! cargo run --example streaming_events
//! ```

use async_trait::async_trait;
use flume;
use miette::{self, IntoDiagnostic, Result};
use serde_json::json;
use std::sync::Arc;
use tokio::signal;

use weavegraph::{
    channels::Channel,
    event_bus::Event,
    event_bus::STREAM_END_SCOPE,
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
async fn main() -> Result<()> {
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
            serde_json::to_string_pretty(&json_payload).into_diagnostic()?
        );

        // Break on completion event
        if event.message().contains("completed") {
            break;
        }
    }

    // Wait for workflow to finish
    workflow_task.await.into_diagnostic()?;

    info!("\n=== Example Complete ===");
    info!("\n💡 Next Steps:");
    info!("   - Use this pattern with Axum for SSE endpoints");
    info!("   - Add multiple ChannelSinks for different clients");
    info!("   - Filter events by scope before streaming");

    Ok(())
}
