//! # Weavegraph: Graph-driven Agent Workflow Framework
//!
//! Weavegraph is a framework for building concurrent, stateful workflows using graph-based
//! execution with versioned state management and deterministic barrier merges.
//!
//! ## Core Concepts
//!
//! - **Nodes**: Async units of work that process state snapshots
//! - **Messages**: Communication primitives with role-based typing
//! - **State**: Versioned, channel-based state management
//! - **Graph**: Declarative workflow definition with conditional edges
//! - **Scheduler**: Concurrent execution with dependency tracking
//!
//! ## Quick Start
//!
//! ### Working with Messages
//!
//! Messages are the primary communication primitive. Use convenience constructors:
//!
//! ```
//! use weavegraph::message::{Message, roles};
//!
//! // Preferred: Use convenience constructors
//! let user_msg = Message::user("What's the weather like?");
//! let assistant_msg = Message::assistant("It's sunny and 75°F!");
//! let system_msg = Message::system("You are a helpful assistant.");
//!
//! // For custom roles, use the general constructor
//! let function_msg = Message::new("function", "Processing complete");
//!
//! // For complex cases, use the builder pattern
//! let complex_msg = Message::builder()
//!     .role("custom_agent")
//!     .content("Task completed with status: OK")
//!     .build();
//!
//! // Validate messages
//! assert!(user_msg.is_valid());
//! assert!(user_msg.is_user());
//! assert!(!user_msg.is_assistant());
//! ```
//!
//! ### Building a Simple Workflow
//!
//! ```
//! use weavegraph::{
//!     graph::GraphBuilder,
//!     node::{Node, NodeContext, NodePartial},
//!     message::Message,
//!     state::VersionedState,
//!     types::NodeKind,
//! };
//! use async_trait::async_trait;
//!
//! // Define a simple node
//! struct GreetingNode;
//!
//! #[async_trait]
//! impl Node for GreetingNode {
//!     async fn run(
//!         &self,
//!         snapshot: weavegraph::state::StateSnapshot,
//!         _ctx: NodeContext,
//!     ) -> Result<NodePartial, weavegraph::node::NodeError> {
//!         // Use convenience constructor instead of verbose struct syntax
//!         let greeting = Message::assistant("Hello! How can I help you today?");
//!         
//!         Ok(NodePartial {
//!             messages: Some(vec![greeting]),
//!             extra: None,
//!             errors: None,
//!         })
//!     }
//! }
//! ```
//!
//! ### State Management
//!
//! ```
//! use weavegraph::state::VersionedState;
//! use weavegraph::message::Message;
//!
//! // Create initial state with user message
//! let state = VersionedState::new_with_user_message("Hello, system!");
//!
//! // Or use the builder pattern for complex initialization
//! let complex_state = VersionedState::builder()
//!     .with_user_message("What's the weather?")
//!     .with_system_message("You are a weather assistant")
//!     .with_extra("location", serde_json::json!("San Francisco"))
//!     .build();
//! ```
//!
//! ## Best Practices
//!
//! ### Message Construction
//!
//! ```
//! use weavegraph::message::{Message, roles};
//!
//! // ✅ GOOD: Use convenience constructors
//! let user_msg = Message::user("Hello");
//! let assistant_msg = Message::assistant("Hi there!");
//! let system_msg = Message::system("You are helpful");
//!
//! // ✅ GOOD: Use role constants for consistency
//! let custom_msg = Message::new(roles::USER, "Custom content");
//!
//! // ✅ GOOD: Use builder for complex cases
//! let complex_msg = Message::builder()
//!     .role("function")
//!     .content("Result: success")
//!     .build();
//!
//! // ❌ AVOID: Direct struct construction (verbose and error-prone)
//! // let verbose_msg = Message {
//! //     role: "user".to_string(),
//! //     content: "Hello".to_string(),
//! // };
//! ```
//!
//! ### Error Handling
//!
//! The framework uses comprehensive error types with detailed context:
//!
//! ```
//! use weavegraph::node::{NodeError, NodeContext};
//!
//! // Errors are automatically traced and can be emitted to the event bus
//! fn example_error_handling(ctx: &NodeContext) -> Result<(), NodeError> {
//!     ctx.emit("validation", "Checking input parameters")?;
//!     
//!     // Framework provides rich error types
//!     Err(NodeError::MissingInput {
//!         what: "user_id",
//!     })
//! }
//! ```
//!
//! ## Module Guide
//!
//! - [`message`] - Message types and construction utilities
//! - [`state`] - Versioned state management and snapshots  
//! - [`node`] - Node trait and execution primitives
//! - [`graph`] - Workflow graph definition and compilation
//! - [`schedulers`] - Concurrent execution and dependency resolution
//! - [`runtimes`] - High-level execution runtime and checkpointing
//! - [`channels`] - Channel-based state storage and versioning
//! - [`reducers`] - State merge strategies and conflict resolution

pub mod app;
pub mod channels;
pub mod event_bus;
pub mod graph;
pub mod message;
pub mod node;
pub mod reducers;
pub mod runtimes;
pub mod schedulers;
pub mod state;
pub mod telemetry;
pub mod types;
pub mod utils;
