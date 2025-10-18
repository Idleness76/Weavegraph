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
use futures_util::StreamExt;
use serde_json::json;
use std::sync::Arc;

use weavegraph::{
    channels::Channel,
    event_bus::Event,
    graphs::GraphBuilder,
    message::Message,
    node::{Node, NodeContext, NodeError, NodePartial},
    state::{StateSnapshot, VersionedState},
    types::NodeKind,
};

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
    println!("=== Streaming Events Example ===\n");

    // 1. Build the workflow graph (compile once, reuse many times)
    println!("Building workflow graph...");
    let graph = Arc::new(
        GraphBuilder::new()
            .add_node(NodeKind::Custom("Processor".into()), ProcessingNode)
            .add_edge(NodeKind::Start, NodeKind::Custom("Processor".into()))
            .add_edge(NodeKind::Custom("Processor".into()), NodeKind::End)
            .compile()?,
    );

    // 2. Kick off the workflow using invoke_streaming and grab the live event stream
    println!("Setting up event stream...\n");
    let initial_state = VersionedState::new_with_user_message("Process my data");
    let (workflow_handle, event_stream) = graph.invoke_streaming(initial_state).await;

    // 3. Spawn a task to render each event as JSON (similar to forwarding over SSE)
    let printer = tokio::spawn(async move {
        println!("📡 Streaming events (these could be sent to a web client):\n");
        let mut stream = event_stream.into_async_stream().boxed();
        while let Some(event) = stream.next().await {
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

            match serde_json::to_string_pretty(&json_payload) {
                Ok(pretty) => println!("📨 Stream event: {pretty}"),
                Err(err) => eprintln!("failed to render event as json: {err}"),
            }
        }
    });

    // 4. Wait for the workflow to finish and surface the result
    let final_state = match workflow_handle.await {
        Ok(Ok(state)) => state,
        Ok(Err(err)) => return Err(err.into()),
        Err(err) => return Err(err.into()),
    };

    let _ = printer.await;

    println!("\n=== Example Complete ===");
    let final_snapshot = final_state.messages.snapshot();
    if let Some(msg) = final_snapshot.last() {
        println!("Final assistant message: {}", msg.content);
    }

    Ok(())
}
