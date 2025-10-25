//! ```text
//! GraphBuilder ─┬─► App::compile ─► AppRunner
//!               │                   │
//!               │                   ├─► Scheduler ─► Nodes ─► NodePartial
//!               │                   │                         │
//!               │                   │                         ├─► Reducers → VersionedState
//!               │                   │                         └─► EventBus (diagnostics / LLM)
//!               │                   │
//!               │                   └─► Checkpointer (SQLite / InMemory)
//!               │
//!               └─► RuntimeConfig & reducers wire behaviour end-to-end
//! ```
//!
//! Weavegraph is a framework for building concurrent, stateful workflows with graph-based
//! execution, versioned state, and structured observability. Consult the workspace
//! `ARCHITECTURE.md` for a complete module guide, authoring patterns, and execution flow.

pub mod app;
pub mod channels;
pub mod event_bus;
pub mod graphs;
pub mod message;
pub mod node;
pub mod reducers;
pub mod runtimes;
pub mod schedulers;
pub mod state;
pub mod telemetry;
pub mod types;
pub mod utils;
