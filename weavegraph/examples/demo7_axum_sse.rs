//! Demo 7: Axum SSE streaming with Weavegraph
//!
//! This example shows how to expose Weavegraph events over Server-Sent Events (SSE)
//! using Axum. Each HTTP request spins up an isolated workflow run whose events are
//! streamed back to the client in real time.
//!
//! ## Key ideas
//! - `App::invoke_streaming(initial_state)` hides the `AppRunner` plumbing and returns a
//!   workflow join handle alongside an `EventStream`.
//! - `EventStream::into_async_stream()` adapts the broadcast-backed stream into an async iterator
//!   that plugs directly into Axum's SSE response type.
//! - Because `invoke_streaming` launches the workflow on a Tokio task, the HTTP handler can
//!   return immediately while the workflow continues running in the background.
//!
//! ## Run it
//! ```bash
//! cargo run --example demo7_axum_sse
//! curl -N http://127.0.0.1:3000/stream
//! ```

use async_trait::async_trait;
use axum::{
    Router,
    extract::State,
    response::sse::{Event as SseEvent, Sse},
    routing::get,
};
use futures_util::StreamExt;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, time::sleep};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt};

use weavegraph::{
    event_bus::STREAM_END_SCOPE,
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

        ctx.emit(
            "mcp stream".to_string(),
            format!("Starting work on: {request}"),
        )?;
        sleep(Duration::from_millis(300)).await;

        for step in 1..=3 {
            let mut metadata = FxHashMap::default();
            metadata.insert("content_type".into(), json!("reasoning"));
            ctx.emit_llm_chunk(
                Some("lads".to_owned()),
                Some("mcp stream".to_string()),
                format!("Processing step {step}/3..."),
                Some(metadata),
            )?;
            sleep(Duration::from_millis(400)).await;
        }

        ctx.emit_llm_final(
            Some("lads".to_owned()),
            Some("mcp stream".to_string()),
            "Finalizing response".to_string(),
            None,
        )?;
        sleep(Duration::from_millis(300)).await;

        Ok(NodePartial::new()
            .with_messages(vec![Message::assistant("Workflow completed successfully!")]))
    }
}

async fn stream_workflow(
    State(app): State<Arc<weavegraph::app::App>>,
) -> Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>> {
    let initial_state =
        VersionedState::new_with_user_message("Stream this workflow's progress over HTTP.");
    let (invocation, event_stream) = app.invoke_streaming(initial_state).await;
    let invocation = Arc::new(tokio::sync::Mutex::new(Some(invocation)));

    {
        let invocation = Arc::clone(&invocation);
        tokio::spawn(async move {
            if let Some(handle) = invocation.lock().await.take() {
                match handle.join().await {
                    Ok(_) => tracing::info!("workflow completed"),
                    Err(err) => tracing::error!("workflow error: {err:?}"),
                }
            }
        });
    }

    let sse_stream = async_stream::stream! {
        let mut stream = event_stream.into_async_stream();
        while let Some(event) = stream.next().await {
            let scope = event.scope_label().map(|s| s.to_string());
            if let weavegraph::event_bus::Event::LLM(llm) = &event {
                tracing::debug!(
                    stream = %llm.stream_id().unwrap_or("default"),
                    final_chunk = llm.is_final(),
                    "forwarding LLM token"
                );
            }
            let payload = event.to_json_value();
            let event = SseEvent::default()
                .json_data(payload)
                .expect("serialise SSE payload");
            yield Ok::<SseEvent, Infallible>(event);
            if scope.as_deref() == Some(STREAM_END_SCOPE) {
                break;
            }
        }
    };

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
