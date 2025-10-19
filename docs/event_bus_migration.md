# Event Bus Migration Guide

This guide helps upgrade applications from the pre-refactor event bus to the broadcast-based architecture introduced in Weavegraph 0.1.0-alpha.5.

## Summary of Breaking Changes

- **`EventBus::get_sender` removed:** nodes now emit through an `Arc<dyn EventEmitter>` injected into `NodeContext`.
- **New event variant:** `Event::LLM(LLMStreamingEvent)` carries streaming chunks with metadata and timestamps.
- **Runtime configuration:** event routing is configured through `RuntimeConfig::event_bus` (`EventBusConfig`).
- **Event streams close with a sentinel:** the final diagnostic uses scope `STREAM_END_SCOPE` so subscribers can detect completion.

## Migration Checklist

1. **Update NodeContext usage**
   - Replace direct channel sends with `ctx.emit("scope", "message")?`.
   - Remove manual cloning of `flume::Sender` handles.

2. **Adopt the new streaming APIs**
   - Prefer `App::invoke_streaming` for SSE/WebSocket integrations.
   - Legacy helpers (`invoke_with_channel`, `invoke_with_sinks`) now wrap the same broadcast hub; no code changes required.

3. **Handle the LLM variant**
   - When matching on `Event`, add a branch for `Event::LLM(llm)`.
   - Use `llm.is_final()` to detect the final chunk.

4. **Propagate sentinel events**
   - Treat events whose `scope_label() == Some(STREAM_END_SCOPE)` as end-of-stream notifications.
   - Close sockets / progress indicators after emitting the sentinel to clients.

5. **Configure sinks declaratively**
   - Use `RuntimeConfig::with_event_bus(EventBusConfig::with_memory_sink())` or `EventBusConfig::new(capacity, sinks)` to specify sinks.
   - Built-in sink identifiers: `SinkConfig::StdOut`, `SinkConfig::Memory`.

6. **Monitor lag warnings**
   - Slow subscribers trigger `weavegraph::event_bus` warnings (`event stream lagged; dropped events`).
   - If warnings appear, increase the buffer (`EventBusConfig::new`) or speed up consumers.

## Example: Migrating an SSE Handler

**Before (flume-based):**

```rust
let (tx, rx) = flume::unbounded();
let bus = EventBus::with_sinks(vec![Box::new(ChannelSink::new(tx))]);
let mut runner = AppRunner::with_options_and_bus(app, checkpointer, false, bus, true).await;
let session_id = uuid::Uuid::new_v4().to_string();
runner.create_session(session_id.clone(), state).await?;

let sse_stream = rx.into_stream().map(|event| SseEvent::default().json_data(event).unwrap());
Sse::new(sse_stream)
```

**After (broadcast-based):**

```rust
let (invocation, events) = app.invoke_streaming(state).await;
let sse_stream = async_stream::stream! {
    let mut stream = events.into_async_stream();
    while let Some(event) = stream.next().await {
        yield Ok::<SseEvent, Infallible>(
            SseEvent::default().json_data(event.clone()).unwrap()
        );
        if event.scope_label() == Some(STREAM_END_SCOPE) {
            break;
        }
    }
};
Sse::new(sse_stream)
```

## Further Reading

- `README.md` – updated streaming documentation and examples.
- `STREAMING_QUICKSTART.md` – pattern comparison table and buffer tuning tips.
- `examples/demo7_axum_sse.rs` – reference HTTP streaming integration with graceful cancellation.
