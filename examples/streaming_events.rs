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
//! `invoke_streaming` into SSE frames.
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
use futures_util::StreamExt;
use serde_json::json;

use weavegraph::{
    channels::Channel,
    event_bus::{Event, STREAM_END_SCOPE},
    graphs::GraphBuilder,
    message::{Message, Role},
    node::{Node, NodeContext, NodeError, NodePartial},
    state::{StateSnapshot, VersionedState},
    types::NodeKind,
};

use tracing::info;
use tracing_error::ErrorLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

type ExampleResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

        Ok(NodePartial::new().with_messages(vec![Message::with_role(
            Role::Assistant,
            "Processing finished successfully.",
        )]))
    }
}

#[tokio::main]
async fn main() -> ExampleResult<()> {
    init_tracing();

    info!("=== Streaming Events Example ===\n");

    // 1. Build the workflow graph (compile once, reuse many times)
    info!("Building workflow graph...");
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("Processor".into()), ProcessingNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("Processor".into()))
        .add_edge(NodeKind::Custom("Processor".into()), NodeKind::End)
        .compile()?;

    let initial_state = VersionedState::new_with_user_message("Process my data");
    let (invocation, event_stream) = app.invoke_streaming(initial_state).await;

    // 2. Consume streamed events as they arrive
    info!("📡 Streaming events (these could be sent to a web client):\n");

    let events_task = tokio::spawn(async move {
        let mut count = 0usize;
        let mut events = event_stream.into_async_stream();
        while let Some(event) = events.next().await {
            count += 1;
            let json_payload = json!({
                "type": match &event {
                    Event::Node(_) => "node",
                    Event::Diagnostic(_) => "diagnostic",
                    Event::LLM(_) => "llm",
                },
                "scope": event.scope_label(),
                "message": event.message(),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });

            info!(
                "📨 Stream event: {}",
                serde_json::to_string_pretty(&json_payload)?
            );

            if event.scope_label() == Some(STREAM_END_SCOPE) {
                info!("✅ Received STREAM_END_SCOPE sentinel; closing stream");
                break;
            }
        }
        Ok::<usize, serde_json::Error>(count)
    });

    let final_state = invocation.join().await?;
    let _event_count = events_task
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;

    info!(
        "🧾 Final state contains {} message(s)",
        final_state.messages.snapshot().len()
    );

    info!("\n=== Example Complete ===");
    info!("\n💡 Next Steps:");
    info!("   - Use this pattern with Axum for SSE endpoints");
    info!("   - Use `invoke_with_channel` when you need a flume receiver");
    info!("   - Filter events by scope before streaming");

    Ok(())
}
