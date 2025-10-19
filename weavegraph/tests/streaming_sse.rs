use std::{convert::Infallible, sync::Arc, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use axum::{
    extract::State,
    response::sse::{Event as SseEvent, Sse},
    routing::get,
    Router,
};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::{net::TcpListener, time::timeout};
use weavegraph::event_bus::STREAM_END_SCOPE;
use weavegraph::graphs::GraphBuilder;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

struct TestNode;

#[async_trait]
impl Node for TestNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        if let Some(msg) = snapshot.messages.first() {
            ctx.emit("sse", format!("processing: {}", msg.content))?;
        }
        ctx.emit("sse", "stream-complete")?;
        Ok(NodePartial::new().with_messages(vec![]))
    }
}

async fn handler(
    State(app): State<Arc<weavegraph::app::App>>,
) -> Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>> {
    let (invocation, events) = app
        .invoke_streaming(VersionedState::new_with_user_message("ping"))
        .await;

    tokio::spawn(async move {
        if let Err(err) = invocation.join().await {
            tracing::error!("test workflow failed: {err:?}");
        }
    });

    let mut stream = events.into_async_stream();
    let sse_stream = stream! {
        while let Some(event) = stream.next().await {
            let payload = json!({
                "scope": event.scope_label(),
                "message": event.message(),
            });
            yield Ok(SseEvent::default().json_data(payload).unwrap());
            if event.scope_label() == Some(STREAM_END_SCOPE) {
                break;
            }
        }
    };

    Sse::new(sse_stream)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn axum_sse_example_streams_until_completion() -> Result<(), Box<dyn std::error::Error>> {
    let app = Arc::new(
        GraphBuilder::new()
            .add_node(NodeKind::Custom("test".into()), TestNode)
            .add_edge(NodeKind::Start, NodeKind::Custom("test".into()))
            .add_edge(NodeKind::Custom("test".into()), NodeKind::End)
            .compile()?,
    );

    let router = Router::new().route("/stream", get(handler)).with_state(app);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, router.into_make_service()).await {
            tracing::error!("axum server error: {err:?}");
        }
    });

    let client = Client::builder().build()?;
    let response = client.get(format!("http://{addr}/stream")).send().await?;
    let mut body = response.bytes_stream();
    let mut saw_end = false;

    while let Some(chunk_result) = timeout(Duration::from_secs(1), body.next()).await? {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);
        if text.contains(STREAM_END_SCOPE) {
            saw_end = true;
            break;
        }
    }

    assert!(saw_end, "stream should include termination event");

    server.abort();
    Ok(())
}
