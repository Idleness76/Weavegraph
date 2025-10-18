//! Demo 7: Axum SSE streaming with Weavegraph
//!
//! This example shows how to expose Weavegraph events over Server-Sent Events (SSE)
//! using Axum. Each HTTP request spins up an isolated workflow run whose events are
//! streamed to the client in real time.
//!
//! Run with:
//!   cargo run --example demo7_axum_sse
//!
//! Then, in another terminal:
//!   curl -N http://127.0.0.1:3000/stream

use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    extract::State,
    response::sse::{Event as SseEvent, Sse},
    routing::get,
    Router,
};
use futures_util::StreamExt;
use serde_json::json;
use tokio::{net::TcpListener, time::sleep};
use tracing::Level;
use tracing_subscriber::{fmt, EnvFilter};

use weavegraph::{
    graphs::GraphBuilder,
    message::Message,
    node::{Node, NodeContext, NodeError, NodePartial},
    runtimes::{EventBusConfig, RuntimeConfig},
    state::{StateSnapshot, VersionedState},
    types::NodeKind,
};

/// Simple node that emits progress updates and returns a final assistant message.
struct StreamingNode;

#[async_trait]
impl Node for StreamingNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let request = snapshot
            .messages
            .first()
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "No request provided".to_string());

        ctx.emit("stream", format!("Starting work on: {request}"))?;
        sleep(Duration::from_millis(300)).await;

        for step in 1..=3 {
            ctx.emit("stream", format!("Processing step {step}/3..."))?;
            sleep(Duration::from_millis(400)).await;
        }

        ctx.emit("stream", "Finalizing response")?;
        sleep(Duration::from_millis(300)).await;

        Ok(NodePartial::new()
            .with_messages(vec![Message::assistant("Workflow completed successfully!")]))
    }
}

async fn stream_workflow(
    State(app): State<Arc<weavegraph::app::App>>,
) -> Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>> {
    let initial_state =
        VersionedState::new_with_user_message("Stream this workflow's progress over SSE.");
    let (join_handle, event_stream) = app.invoke_streaming(initial_state).await;

    tokio::spawn(async move {
        match join_handle.await {
            Ok(Ok(_)) => tracing::info!("workflow completed"),
            Ok(Err(err)) => tracing::error!("workflow error: {err:?}"),
            Err(err) => tracing::error!("workflow task panicked: {err:?}"),
        }
    });

    let sse_stream = event_stream.into_async_stream().map(|event| {
        let event_type = match &event {
            weavegraph::event_bus::Event::Node(_) => "node",
            weavegraph::event_bus::Event::Diagnostic(_) => "diagnostic",
            weavegraph::event_bus::Event::LLM(_) => "llm",
        };
        let payload = json!({
            "type": event_type,
            "scope": event.scope_label(),
            "message": event.message(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        Ok(SseEvent::default().json_data(payload).unwrap())
    });

    Sse::new(sse_stream)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Simple tracing setup so emitted events are visible in the server logs.
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .init();

    let app_graph = GraphBuilder::new()
        .add_node(NodeKind::Custom("streamer".into()), StreamingNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("streamer".into()))
        .add_edge(NodeKind::Custom("streamer".into()), NodeKind::End)
        .with_runtime_config(
            RuntimeConfig::default().with_event_bus(EventBusConfig::with_stdout_only()),
        )
        .compile()?;

    let router = Router::new()
        .route("/stream", get(stream_workflow))
        .with_state(Arc::new(app_graph));

    let addr: SocketAddr = "127.0.0.1:3000".parse()?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Serving SSE stream on http://{addr}/stream");
    axum::serve(listener, router.into_make_service()).await?;

    Ok(())
}
