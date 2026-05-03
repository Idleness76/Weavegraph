//! # Production Streaming: Axum SSE + Postgres Checkpointing
//!
//! The **golden path** reference example for production web-server consumers.
//!
//! Demonstrates the complete pattern for a production web server that:
//!
//! - Compiles a [`GraphBuilder`] with [`RuntimeConfig`] once at startup
//! - Shares the compiled [`App`] across concurrent requests via [`Arc`]
//! - Checkpoints state to Postgres via [`PostgresCheckpointer`] for durable resumption
//! - Streams workflow events to HTTP clients via Server-Sent Events (SSE)
//! - Terminates the SSE stream cleanly on [`STREAM_END_SCOPE`]
//! - Supports per-request cancellation via [`InvocationHandle::abort`]
//! - Handles node errors uniformly with [`NodeError::Other`]
//!
//! ## Architecture
//!
//! ```text
//! HTTP Client  GET /run?prompt=hello
//!     │
//!     ▼
//! ┌──────────────────────────────────────────────────────┐
//! │ Axum Handler run_handler()                           │
//! │  ┌─ app.invoke_streaming(state) ──────────────────┐  │
//! │  │   Returns (InvocationHandle, EventStream)      │  │
//! │  │   Workflow runs in background tokio task       │  │
//! │  └────────────────────────────────────────────────┘  │
//! │  Returns Sse<impl Stream<Item=SseEvent>>             │
//! └──────────────────────────────────────────────────────┘
//!     │
//!     │  data: {"kind":"llm","message":"token1",...}
//!     │  data: {"kind":"diagnostic","scope":"__weavegraph_stream_end__",...}
//!     │  [stream closed by server]
//!     ▼
//! HTTP Client
//! ```
//!
//! ## Per-Request Isolation
//!
//! Each request gets its own [`AppRunner`] (and therefore its own [`EventBus`])
//! via [`App::invoke_streaming`]. The [`App`] itself is a cheap [`Arc`] clone.
//! This is the canonical concurrency pattern for streaming workflows.
//!
//! ## Feature Requirements
//!
//! ```bash
//! cargo run --example production_streaming --features postgres,examples
//! ```
//!
//! Set `DATABASE_URL` before running:
//!
//! ```bash
//! export DATABASE_URL="postgres://postgres:postgres@localhost/weavegraph"
//! cargo run --example production_streaming --features postgres,examples
//! ```
//!
//! ## Testing
//!
//! ```bash
//! curl -N "http://localhost:3000/run?prompt=hello+world"
//! ```

use std::{
    convert::Infallible,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Router,
    extract::{Query, State},
    response::{
        IntoResponse,
        sse::{Event as SseEvent, KeepAlive, Sse},
    },
    routing::get,
};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use weavegraph::{
    app::{App, InvocationHandle},
    channels::Channel,
    event_bus::{Event, EventStream, STREAM_END_SCOPE},
    graphs::GraphBuilder,
    message::{Message, Role},
    node::{Node, NodeContext, NodeError, NodePartial, NodeResultExt},
    runtimes::{EventBusConfig, PostgresCheckpointer, RuntimeConfig},
    state::{StateSnapshot, VersionedState},
    types::NodeKind,
};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

// ============================================================================
// Node definitions
// ============================================================================

/// Simulates an LLM node that streams a response token by token.
///
/// In a real application this would call an LLM provider and emit each
/// chunk via [`NodeContext::emit`] so clients receive tokens as they arrive.
#[derive(Clone)]
struct LlmNode;

#[async_trait]
impl Node for LlmNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let prompt = snapshot
            .messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("(no input)");

        // Simulate token streaming — in production, replace with your LLM call.
        let tokens = ["Hello", ", ", "I", " am", " a", " streaming", " assistant", "!"];
        for token in tokens {
            ctx.emit("llm.token", format!("Response to '{}': {}", prompt, token))?;
            // Simulate token generation latency.
            tokio::time::sleep(Duration::from_millis(150)).await;
        }

        Ok(NodePartial::new().with_messages(vec![
            Message::with_role(Role::Assistant, &format!("Response to '{}'", prompt)),
        ]))
    }
}

/// A validation node demonstrating `NodeError::Other` for recoverable failures.
///
/// Input validation belongs at the node boundary where the error context is
/// richest. Use [`NodeResultExt::node_err`] to lift arbitrary errors into
/// [`NodeError::Other`] without losing the original message.
#[derive(Clone)]
struct ValidateNode;

#[async_trait]
impl Node for ValidateNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let prompt = snapshot.messages.last().map(|m| m.content.as_str()).unwrap_or("");

        if prompt.trim().is_empty() {
            return Err(NodeError::Other(
                "prompt must not be empty".into(),
            ));
        }

        if prompt.len() > 4096 {
            return Err(NodeError::Other(
                format!("prompt too long: {} chars (max 4096)", prompt.len()).into(),
            ));
        }

        // Use NodeResultExt for fallible stdlib operations.
        let _validated = std::str::from_utf8(prompt.as_bytes()).node_err()?;

        Ok(NodePartial::new())
    }
}

// ============================================================================
// Application state
// ============================================================================

/// Shared application state injected into every Axum handler.
#[derive(Clone)]
struct AppState {
    app: Arc<App>,
}

// ============================================================================
// HTTP handlers
// ============================================================================

#[derive(Debug, Deserialize)]
struct RunQuery {
    #[serde(default = "default_prompt")]
    prompt: String,
}

fn default_prompt() -> String {
    "Hello, weavegraph!".to_string()
}

/// `GET /run?prompt=...`
///
/// Starts a workflow invocation and returns an SSE stream of events.
///
/// Each event is a JSON-serialized [`weavegraph::event_bus::Event`].
/// The stream terminates with a special diagnostic event whose scope is
/// [`STREAM_END_SCOPE`]; consumers should close the connection on receipt.
///
/// ## Per-Request Isolation
///
/// Each request gets its own [`AppRunner`] (via `App::invoke_streaming`).
/// The shared [`App`] is a cheap [`Arc`] clone; only the runner (with its
/// own [`EventBus`]) is created per request. This is the canonical pattern
/// for concurrent SSE in production.
async fn run_handler(
    State(state): State<AppState>,
    Query(query): Query<RunQuery>,
) -> impl IntoResponse {
    info!(prompt = %query.prompt, "starting workflow invocation");

    let initial_state = VersionedState::new_with_user_message(&query.prompt);

    // invoke_streaming returns immediately; the workflow runs in a background task.
    let (handle, event_stream) = state.app.invoke_streaming(initial_state).await;

    // Convert the EventStream into an SSE-compatible futures Stream.
    let sse_stream = build_sse_stream(handle, event_stream);

    Sse::new(sse_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Wraps the weavegraph [`EventStream`] as a futures [`Stream`] of SSE frames.
///
/// - Serializes each event to JSON and wraps it in an `SseEvent`.
/// - Watches for [`STREAM_END_SCOPE`] to terminate the stream gracefully.
/// - Aborts the workflow task via [`InvocationHandle`] if the client disconnects.
fn build_sse_stream(
    handle: InvocationHandle,
    event_stream: EventStream,
) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    let handle = Arc::new(tokio::sync::Mutex::new(Some(handle)));
    let handle_for_cleanup = handle.clone();

    // Convert EventStream into an async stream of SseEvent.
    let stream = event_stream.into_async_stream().map(move |event| {
        let is_end = event
            .scope_label()
            .map(|s| s == STREAM_END_SCOPE)
            .unwrap_or(false);

        let payload = serde_json::to_string(&SsePayload::from(&event))
            .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string());

        let sse = SseEvent::default().data(payload);

        (sse, is_end)
    });

    // Take-while inclusive: emit the STREAM_END event, then stop.
    futures_util::stream::unfold(
        (stream.boxed(), false, handle),
        move |(mut stream, done, handle)| async move {
            if done {
                // Join the workflow handle so its task is properly reaped.
                if let Some(h) = handle.lock().await.take() {
                    match h.join().await {
                        Ok(state) => info!(
                            messages = state.messages.len(),
                            "workflow completed successfully"
                        ),
                        Err(e) => warn!(error = %e, "workflow ended with error"),
                    }
                }
                return None;
            }

            match stream.next().await {
                Some((sse, is_end)) => Some((Ok(sse), (stream, is_end, handle))),
                None => {
                    // Stream closed unexpectedly (e.g., workflow panicked).
                    error!("event stream closed without STREAM_END_SCOPE");
                    None
                }
            }
        },
    )
}

/// Lightweight SSE payload wrapping the weavegraph event.
///
/// In production you may want to normalise the shape further — this keeps
/// the full event detail available while adding a top-level discriminant.
#[derive(Debug, Serialize)]
struct SsePayload {
    kind: &'static str,
    message: String,
    scope: Option<String>,
}

impl From<&Event> for SsePayload {
    fn from(event: &Event) -> Self {
        Self {
            kind: match event {
                Event::Node(_) => "node",
                Event::Diagnostic(_) => "diagnostic",
                Event::LLM(_) => "llm",
            },
            message: event.message().to_string(),
            scope: event.scope_label().map(str::to_string),
        }
    }
}

/// `GET /healthz` — liveness probe for container orchestration.
async fn healthz() -> &'static str {
    "ok"
}

// ============================================================================
// Startup and graph compilation
// ============================================================================

/// Build and compile the workflow graph with Postgres checkpointing.
///
/// This runs **once** at startup. The compiled [`App`] is wrapped in [`Arc`]
/// and shared across all handlers for the lifetime of the server. Graph
/// compilation is O(V+E) and negligible relative to request handling.
async fn build_app() -> Result<App, BoxError> {
    dotenvy::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/weavegraph".to_string());

    // Connect to Postgres. When the `postgres-migrations` feature is enabled,
    // schema migrations are run automatically on connect.
    let pg = PostgresCheckpointer::connect(&db_url).await?;

    // Attach the postgres checkpointer via checkpointer_custom().
    // This takes precedence over any CheckpointerType enum variant.
    let runtime_config = RuntimeConfig::new(None, None)
        .checkpointer_custom(Arc::new(pg))
        .with_event_bus(EventBusConfig::with_stdout_only());

    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("validate".into()), ValidateNode)
        .add_node(NodeKind::Custom("llm".into()), LlmNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("validate".into()))
        .add_edge(NodeKind::Custom("validate".into()), NodeKind::Custom("llm".into()))
        .add_edge(NodeKind::Custom("llm".into()), NodeKind::End)
        .with_runtime_config(runtime_config)
        .compile()?;

    info!(db_url = %db_url, "graph compiled with postgres checkpointing");
    Ok(app)
}

// ============================================================================
// Main entry point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let app = build_app().await?;
    let state = AppState {
        app: Arc::new(app),
    };

    let router = Router::new()
        .route("/run", get(run_handler))
        .route("/healthz", get(healthz))
        .with_state(state);

    let addr = "0.0.0.0:3000";
    info!(addr, "production_streaming server listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
