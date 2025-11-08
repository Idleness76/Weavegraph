# Weavegraph Documentation Index

Complete documentation for building workflows with Weavegraph.

## Core Documentation

- **[GUIDE.md](GUIDE.md)** - Developer guide covering core concepts
  - [Messages](GUIDE.md#messages): Message construction, roles, and usage patterns
  - [State Management](GUIDE.md#state): Versioned state, initialization, and snapshots
  - [Graph Building](GUIDE.md#graphs): Workflow definition, conditional edges, and routing

- **[OPERATIONS.md](OPERATIONS.md)** - Runtime operations and deployment
  - [Event Streaming](OPERATIONS.md#event-streaming): Sinks, patterns, diagnostics, and observability
  - [Persistence](OPERATIONS.md#persistence): SQLite checkpointing and in-memory mode
  - [Testing](OPERATIONS.md#testing): Test strategies, event capture, and property-based testing
  - [Error Handling](OPERATIONS.md#errors): Diagnostics and troubleshooting
  - [Production](OPERATIONS.md#production): Performance, monitoring, and deployment

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Core architecture, module overview, and custom reducers

## Additional Resources

- [Examples](../weavegraph/examples/) - Runnable code for all major patterns
- [STREAMING_QUICKSTART.md](../weavegraph/examples/STREAMING_QUICKSTART.md) - Event streaming quickstart guide
