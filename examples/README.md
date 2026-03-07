# Examples Index

Runnable examples for core Weavegraph patterns.

## Run an example

```bash
cargo run --example <name>
```

## Core workflow examples

- `basic_nodes` - Minimal node and graph setup.
- `graph_execution` - End-to-end graph execution and state updates.
- `scheduler_fanout` - Dependency fan-out and scheduler behavior.
- `advanced_patterns` - Conditional routing and richer orchestration patterns.

## Streaming and observability examples

- `convenience_streaming` - `invoke_with_channel` and `invoke_with_sinks` helpers.
- `streaming_events` - Runtime/event bus streaming pattern for services.
- `event_backpressure` - Handling lag and drop behavior under load.
- `json_serialization` - Emitting machine-readable event payloads.

## Error handling example

- `errors_pretty` - Structured error collection and pretty output.

## Related docs

- `docs/QUICKSTART.md`
- `docs/STREAMING.md`
- `docs/OPERATIONS.md`
- `docs/MIGRATION.md`
