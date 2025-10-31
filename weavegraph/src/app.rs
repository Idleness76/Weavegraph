use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::channels::errors::{ErrorEvent, ErrorScope};
use crate::channels::Channel;
use crate::control::FrontierCommand;
use crate::event_bus::{ChannelSink, EventBus, EventStream};
use crate::message::*;
use crate::node::*;
use crate::reducers::ReducerRegistry;
use crate::runtimes::runner::RunnerError;
use crate::runtimes::{AppRunner, CheckpointerType, RuntimeConfig, SessionInit};
use crate::state::*;
use crate::types::*;
use crate::utils::collections::new_extra_map;
use crate::utils::id_generator::IdGenerator;
use futures_util::stream::BoxStream;
use thiserror::Error;
use tokio::task::JoinHandle;
use tracing::instrument;

/// Orchestrates graph execution and applies reducers at barriers.
///
/// `App` is the central coordination point for workflow execution, managing:
/// - Node graph topology (nodes, edges, conditional routing)
/// - State reduction through configurable reducers
/// - Runtime configuration and checkpointing
///
/// # Examples
///
/// ```rust,no_run
/// use weavegraph::graphs::GraphBuilder;
/// use weavegraph::runtimes::CheckpointerType;
/// use weavegraph::state::VersionedState;
/// use weavegraph::types::NodeKind;
/// use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
/// use async_trait::async_trait;
///
/// # struct MyNode;
/// # #[async_trait]
/// # impl Node for MyNode {
/// #     async fn run(&self, _: weavegraph::state::StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
/// #         Ok(NodePartial::default())
/// #     }
/// # }
/// #
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let app = GraphBuilder::new()
///     .add_node(NodeKind::Custom("process".into()), MyNode)
///     .add_edge(NodeKind::Start, NodeKind::Custom("process".into()))
///     .add_edge(NodeKind::Custom("process".into()), NodeKind::End)
///     .compile()?;
///
/// let initial_state = VersionedState::new_with_user_message("Hello");
/// let final_state = app.invoke(initial_state).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct App {
    nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    conditional_edges: Vec<crate::graphs::ConditionalEdge>,
    reducer_registry: ReducerRegistry,
    runtime_config: RuntimeConfig,
}

/// Combined handle exposing the configured event bus and a single subscription.
///
/// Obtained from [`App::event_stream()`], it lets callers attach additional sinks
/// before execution starts or choose how to consume the broadcast feed (async
/// stream, blocking iterator, or timed polling).
pub struct AppEventStream {
    event_bus: EventBus,
    event_stream: Option<EventStream>,
}

/// Errors returned when accessing an [`AppEventStream`] after its subscription
/// has already been consumed.
#[derive(Debug, Error)]
pub enum AppEventStreamError {
    #[error("event stream has already been taken")]
    AlreadyTaken,
}

type AppEventStreamResult<T> = Result<T, AppEventStreamError>;

/// Handle for a streaming workflow invocation.
///
/// Dropping the handle aborts the workflow task. Use [`join`](InvocationHandle::join)
/// to await graceful completion; the paired event stream will emit a diagnostic with
/// scope [`STREAM_END_SCOPE`](crate::event_bus::STREAM_END_SCOPE) before closing.
pub struct InvocationHandle {
    join_handle: Option<JoinHandle<Result<VersionedState, RunnerError>>>,
}

/// Result of applying node partials at a barrier.
///
/// The outcome aggregates channel and error information in a deterministic
/// order so downstream consumers (runner, checkpointers, tests) observe stable
/// behaviour across executions.
#[derive(Debug, Clone, Default)]
pub struct BarrierOutcome {
    /// Channel identifiers that were updated during the barrier.
    pub updated_channels: Vec<&'static str>,
    /// Aggregated error events emitted by nodes in the superstep.
    pub errors: Vec<ErrorEvent>,
    /// Frontier manipulation commands emitted during the barrier.
    pub frontier_commands: Vec<(NodeKind, FrontierCommand)>,
}

impl AppEventStream {
    fn new(event_bus: EventBus, event_stream: EventStream) -> Self {
        Self {
            event_bus,
            event_stream: Some(event_stream),
        }
    }

    /// Access the bus to add sinks before execution begins.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Mutable access to the underlying broadcast subscription.
    ///
    /// Returns an error if the stream was already consumed by another accessor.
    pub fn event_stream(&mut self) -> AppEventStreamResult<&mut EventStream> {
        self.event_stream
            .as_mut()
            .ok_or(AppEventStreamError::AlreadyTaken)
    }

    /// Consume the handle and return the raw event stream.
    ///
    /// Subsequent calls will error with [`AppEventStreamError::AlreadyTaken`].
    pub fn into_stream(mut self) -> AppEventStreamResult<EventStream> {
        self.event_stream
            .take()
            .ok_or(AppEventStreamError::AlreadyTaken)
    }

    /// Consume the handle and return the event bus.
    pub fn into_event_bus(self) -> EventBus {
        self.event_bus
    }

    /// Split the handle into the bus and event stream.
    pub fn split(mut self) -> AppEventStreamResult<(EventBus, EventStream)> {
        let stream = self
            .event_stream
            .take()
            .ok_or(AppEventStreamError::AlreadyTaken)?;
        Ok((self.event_bus, stream))
    }

    /// Consume and convert the stream into a blocking iterator.
    ///
    /// Fails if the stream was already taken through another accessor.
    pub fn into_blocking_iter(self) -> AppEventStreamResult<crate::event_bus::BlockingEventIter> {
        Ok(self.into_stream()?.into_blocking_iter())
    }

    /// Consume and convert the stream into an async iterator.
    ///
    /// Fails if the stream was already taken through another accessor.
    pub fn into_async_stream(
        self,
    ) -> AppEventStreamResult<BoxStream<'static, crate::event_bus::Event>> {
        Ok(self.into_stream()?.into_async_stream())
    }

    /// Await the next event with a timeout, skipping lag notifications.
    ///
    /// Fails if the stream was already taken through another accessor.
    pub async fn next_timeout(
        &mut self,
        duration: std::time::Duration,
    ) -> AppEventStreamResult<Option<crate::event_bus::Event>> {
        Ok(self.event_stream()?.next_timeout(duration).await)
    }
}

impl InvocationHandle {
    /// Abort the underlying workflow task. `join` will return a join error afterwards.
    ///
    /// Equivalent to dropping the handle explicitly.
    pub fn abort(&self) {
        if let Some(handle) = &self.join_handle {
            handle.abort();
        }
    }

    /// Returns true if the underlying workflow task has completed or aborted.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.join_handle
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(true)
    }

    /// Await the workflow result.
    pub async fn join(mut self) -> Result<VersionedState, RunnerError> {
        let handle = self
            .join_handle
            .take()
            .expect("join_handle already awaited");
        match handle.await {
            Ok(result) => result,
            Err(err) => Err(RunnerError::Join(err)),
        }
    }
}

impl App {
    /// Internal (crate) factory to build an App while keeping nodes/edges private.
    pub(crate) fn from_parts(
        nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
        edges: FxHashMap<NodeKind, Vec<NodeKind>>,
        conditional_edges: Vec<crate::graphs::ConditionalEdge>,
        runtime_config: RuntimeConfig,
    ) -> Self {
        App {
            nodes,
            edges,
            conditional_edges,
            reducer_registry: ReducerRegistry::default(),
            runtime_config,
        }
    }

    /// Returns a reference to the conditional edges in this graph.
    ///
    /// Conditional edges enable dynamic routing based on runtime state,
    /// allowing workflows to branch based on computed conditions. Predicates
    /// return a String which is interpreted as the next target node:
    /// - "End" and "Start" are recognized as virtual endpoints
    /// - any other string is treated as the name of a custom node
    ///
    /// At runtime, targets are validated before being pushed to the frontier.
    /// Unknown custom targets are skipped with a warning, preserving progress.
    ///
    /// # Returns
    /// A slice of conditional edge specifications.
    #[must_use]
    pub fn conditional_edges(&self) -> &Vec<crate::graphs::ConditionalEdge> {
        &self.conditional_edges
    }

    /// Returns a reference to the nodes registry.
    ///
    /// Provides access to all registered node implementations in the graph.
    /// Nodes are keyed by their `NodeKind` identifier.
    ///
    /// # Returns
    /// A map from `NodeKind` to the corresponding `Node` implementation.
    #[must_use]
    pub fn nodes(&self) -> &FxHashMap<NodeKind, Arc<dyn Node>> {
        &self.nodes
    }

    /// Returns a reference to the unconditional edges in this graph.
    ///
    /// Unconditional edges define the static topology of the workflow graph,
    /// specifying which nodes can transition to which other nodes.
    ///
    /// # Returns
    /// A map from source `NodeKind` to a list of destination `NodeKind`s.
    #[must_use]
    pub fn edges(&self) -> &FxHashMap<NodeKind, Vec<NodeKind>> {
        &self.edges
    }

    /// Returns a reference to the runtime configuration.
    ///
    /// Runtime configuration includes checkpointer settings, session IDs,
    /// and other execution parameters.
    ///
    /// # Returns
    /// The current runtime configuration.
    #[must_use]
    pub fn runtime_config(&self) -> &RuntimeConfig {
        &self.runtime_config
    }

    /// Create a subscription to the configured event bus without starting execution.
    ///
    /// This is the low-level entry point when you want to inspect the stream or
    /// register additional sinks before running the workflow (e.g. in tests or
    /// fully-custom server integrations).
    ///
    /// ```no_run
    /// use futures_util::StreamExt;
    /// use weavegraph::event_bus::{Event, MemorySink};
    /// use weavegraph::runtimes::{AppRunner, CheckpointerType};
    ///
    /// # async fn example(app: weavegraph::app::App, state: weavegraph::state::VersionedState) -> miette::Result<()> {
    /// let mut handle = app.event_stream();
    /// handle.event_bus().add_sink(MemorySink::new());
    /// let (event_bus, event_stream) = handle
    ///     .split()
    ///     .expect("fresh event stream handle should still own the stream");
    ///
    /// let mut runner = AppRunner::with_options_and_bus(
    ///     app.clone(),
    ///     CheckpointerType::InMemory,
    ///     false,
    ///     event_bus,
    ///     true,
    /// ).await;
    ///
    /// tokio::spawn(async move {
    ///     let mut stream = event_stream.into_async_stream();
    ///     while let Some(event) = stream.next().await {
    ///         if matches!(event, Event::LLM(llm) if llm.is_final()) {
    ///             tracing::info!("final streaming chunk delivered");
    ///         }
    ///     }
    /// });
    ///
    /// runner.create_session("demo".to_string(), state).await?;
    /// runner.run_until_complete("demo").await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn event_stream(&self) -> AppEventStream {
        let event_bus = self.runtime_config.event_bus.build_event_bus();
        let event_stream = event_bus.subscribe();
        AppEventStream::new(event_bus, event_stream)
    }

    fn resolve_checkpointer(&self, override_config: Option<CheckpointerType>) -> CheckpointerType {
        override_config
            .or_else(|| self.runtime_config.checkpointer.clone())
            .unwrap_or(CheckpointerType::InMemory)
    }

    /// Internal helper that centralises runner setup for the public `invoke*` helpers.
    ///
    /// - `R` represents any auxiliary handle the caller wants to extract alongside the run
    ///   result (for example, a `flume::Receiver<Event>` when wiring a channel).
    /// - `F` is a closure that is invoked exactly once to construct the `EventBus`
    ///   together with that auxiliary handle. Using `FnOnce` lets the closure move
    ///   ownership of channels or sink vectors.
    ///
    /// The helper resolves the effective checkpointer configuration, spins up an
    /// `AppRunner`, executes the session, and returns both the workflow result and the
    /// caller-provided handle so wrappers can keep their surface area small and
    /// consistent.
    async fn invoke_with_bus_builder<R, F>(
        &self,
        initial_state: VersionedState,
        autosave: bool,
        checkpointer_override: Option<CheckpointerType>,
        build_event_bus: F,
    ) -> (Result<VersionedState, RunnerError>, R)
    where
        F: FnOnce() -> (EventBus, R),
    {
        let (event_bus, output) = build_event_bus();
        let checkpointer_type = self.resolve_checkpointer(checkpointer_override);

        let runner = AppRunner::with_options_and_bus(
            self.clone(),
            checkpointer_type,
            autosave,
            event_bus,
            true,
        )
        .await;

        let session_id = self.next_session_id();
        let result = Self::run_session(runner, session_id, initial_state).await;

        (result, output)
    }

    /// Invoke the workflow asynchronously while streaming events to the caller.
    ///
    /// Returns a join handle for the workflow outcome and an [`EventStream`] that yields
    /// every event emitted during execution. The stream closes after emitting a
    /// diagnostic with scope [`STREAM_END_SCOPE`](crate::event_bus::STREAM_END_SCOPE).
    /// Any sinks configured on the runtime event bus continue to receive events.
    ///
    /// # Cancellation
    ///
    /// Dropping the [`InvocationHandle`] (or calling [`InvocationHandle::abort`]) stops
    /// the workflow immediately. Dropping the event stream does **not** cancel the run;
    /// use the handle if you want to interrupt execution when the client disconnects.
    ///
    /// ```no_run
    /// use futures_util::StreamExt;
    /// use tokio::time::{sleep, Duration};
    /// use weavegraph::event_bus::STREAM_END_SCOPE;
    /// # async fn run(app: weavegraph::app::App, state: weavegraph::state::VersionedState) -> miette::Result<()> {
    /// let (handle, events) = app.invoke_streaming(state).await;
    /// let mut handle_slot = Some(handle);
    ///
    /// let mut events = events.into_async_stream();
    /// tokio::spawn(async move {
    ///     while let Some(event) = events.next().await {
    ///         if event.scope_label() == Some(STREAM_END_SCOPE) {
    ///             tracing::info!("workflow finished");
    ///         }
    ///     }
    /// });
    ///
    /// tokio::select! {
    ///     result = async {
    ///         handle_slot
    ///             .take()
    ///             .expect("join branch must own the handle")
    ///             .join()
    ///             .await
    ///     } => {
    ///         if let Err(err) = result {
    ///             tracing::error!("workflow failed: {err}");
    ///         }
    ///     }
    ///     _ = sleep(Duration::from_secs(30)) => {
    ///         tracing::warn!("cancelling run after timeout");
    ///         if let Some(handle) = handle_slot.as_ref() {
    ///             handle.abort();
    ///         }
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// See `examples/streaming_events.rs` for a CLI integration and
    /// `examples/demo7_axum_sse.rs` for an Axum streaming server that reacts to client cancellation.
    pub async fn invoke_streaming(
        &self,
        initial_state: VersionedState,
    ) -> (InvocationHandle, EventStream) {
        let checkpointer_type = self.resolve_checkpointer(None);

        let event_handle = self.event_stream();
        let (event_bus, event_stream) = event_handle
            .split()
            .expect("fresh App::event_stream() should yield an unused event stream");

        let runner =
            AppRunner::with_options_and_bus(self.clone(), checkpointer_type, true, event_bus, true)
                .await;

        let session_id = self.next_session_id();
        let join = tokio::spawn(Self::run_session(runner, session_id, initial_state));

        (
            InvocationHandle {
                join_handle: Some(join),
            },
            event_stream,
        )
    }

    /// Execute the entire workflow until completion or no nodes remain.
    ///
    /// This is the primary entry point for simple workflow execution. It creates an
    /// `AppRunner` with the runtime-configured event bus (stdout sink by default),
    /// manages session state, and coordinates execution through to completion.
    ///
    /// # Event Handling
    ///
    /// This method uses the **EventBus defined on the `RuntimeConfig`**. Out of the box
    /// that means a stdout sink only, but you can customise the configuration when
    /// building the app.
    ///
    /// For streaming-first scenarios consider [`invoke_streaming`](Self::invoke_streaming),
    /// [`invoke_with_channel`](Self::invoke_with_channel), or
    /// [`invoke_with_sinks`](Self::invoke_with_sinks). Drop down to
    /// [`AppRunner::with_options_and_bus`](crate::runtimes::runner::AppRunner::with_options_and_bus)
    /// when you need per-request isolation or bespoke runner lifecycle management.
    ///
    /// See [`AppRunner::with_options_and_bus()`](crate::runtimes::runner::AppRunner::with_options_and_bus)
    /// for streaming events to custom sinks.
    ///
    /// # Parameters
    /// * `initial_state` - The starting state for workflow execution
    ///
    /// # Returns
    /// * `Ok(VersionedState)` - The final state after workflow completion
    /// * `Err(RunnerError)` - If execution fails due to node errors,
    ///   checkpointer issues, or other runtime problems
    ///
    /// # Examples
    ///
    /// ## Simple Execution (Default EventBus)
    ///
    /// ```rust,no_run
    /// use weavegraph::state::VersionedState;
    /// use weavegraph::channels::Channel;
    /// # use weavegraph::app::App;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    /// let initial = VersionedState::new_with_user_message("Start workflow");
    /// let final_state = app.invoke(initial).await?;
    /// println!("Workflow completed with {} messages", final_state.messages.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Custom Event Streaming (Use AppRunner)
    ///
    /// For streaming events to web clients, use `AppRunner` with a custom EventBus:
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::{EventBus, ChannelSink};
    /// use weavegraph::runtimes::{AppRunner, CheckpointerType};
    /// use weavegraph::state::VersionedState;
    /// # use weavegraph::app::App;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    /// // Create channel for streaming events
    /// let (tx, rx) = flume::unbounded();
    ///
    /// // Create EventBus with custom sink
    /// let bus = EventBus::with_sinks(vec![
    ///     Box::new(ChannelSink::new(tx))
    /// ]);
    ///
    /// // Use AppRunner with custom EventBus
    /// let mut runner = AppRunner::with_options_and_bus(
    ///     app,
    ///     CheckpointerType::InMemory,
    ///     false,
    ///     bus,
    ///     true,
    /// ).await;
    ///
    /// let session_id = "my-session".to_string();
    /// let initial = VersionedState::new_with_user_message("Process this");
    /// runner.create_session(session_id.clone(), initial).await?;
    ///
    /// // Events now stream to the channel
    /// tokio::spawn(async move {
    ///     while let Ok(event) = rx.recv_async().await {
    ///         println!("Event: {:?}", event);
    ///     }
    /// });
    ///
    /// runner.run_until_complete(&session_id).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Workflow Lifecycle
    /// 1. Creates an `AppRunner` with the configured checkpointer and event bus
    /// 2. Initializes or resumes a session
    /// 3. Executes supersteps until End nodes or empty frontier
    /// 4. Returns the final accumulated state
    #[instrument(skip(self, initial_state), err)]
    pub async fn invoke(
        &self,
        initial_state: VersionedState,
    ) -> Result<VersionedState, RunnerError> {
        self.invoke_with_bus_builder(
            initial_state,
            true,
            self.runtime_config.checkpointer.clone(),
            || (self.runtime_config.event_bus.build_event_bus(), ()),
        )
        .await
        .0
    }

    /// Execute workflow with event streaming to a channel.
    ///
    /// This is a convenience method that combines `AppRunner::with_options_and_bus()`
    /// with channel creation and management. It's ideal for simple use cases where
    /// you want to stream events without manually managing the EventBus.
    ///
    /// # When to Use This
    ///
    /// - Simple scripts or CLI tools that need event streaming
    /// - Single-execution scenarios (not web servers)
    /// - You want both the final state AND the event stream
    ///
    /// # When NOT to Use This
    ///
    /// - Web servers with per-request streaming (use `AppRunner::with_options_and_bus()`)
    /// - Need multiple EventSinks beyond ChannelSink (use `invoke_with_sinks()`)
    /// - Need fine-grained control over EventBus lifecycle
    ///
    /// The runtime-configured sinks remain active; this helper simply appends a channel
    /// sink so you can consume events alongside any existing logging destinations.
    ///
    /// # Returns
    ///
    /// Returns a tuple of:
    /// - `Result<VersionedState, RunnerError>` - Final workflow state
    /// - `flume::Receiver<Event>` - Stream of events from workflow execution
    ///
    /// # Examples
    ///
    /// ## Basic Usage
    ///
    /// ```rust,no_run
    /// use weavegraph::state::VersionedState;
    /// # use weavegraph::app::App;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    /// // Execute with streaming
    /// let (result, events) = app.invoke_with_channel(
    ///     VersionedState::new_with_user_message("Process this")
    /// ).await;
    ///
    /// // Process events in parallel with execution
    /// tokio::spawn(async move {
    ///     while let Ok(event) = events.recv_async().await {
    ///         println!("Event: {:?}", event);
    ///     }
    /// });
    ///
    /// let final_state = result?;
    /// println!("Workflow completed!");
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## With Structured Event Processing
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::Event;
    /// use weavegraph::state::VersionedState;
    /// # use weavegraph::app::App;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    /// let (result_future, events) = app.invoke_with_channel(
    ///     VersionedState::new_with_user_message("Analyze data")
    /// ).await;
    ///
    /// // Collect all events
    /// let event_collector = tokio::spawn(async move {
    ///     let mut collected = Vec::new();
    ///     while let Ok(event) = events.recv_async().await {
    ///         match &event {
    ///             Event::Node(ne) => {
    ///                 if let Some(node_id) = ne.node_id() {
    ///                     println!("Node {}: {}", node_id, ne.message());
    ///                 }
    ///             }
    ///             Event::Diagnostic(de) => {
    ///                 println!("Diagnostic: {}", de.message());
    ///             }
    ///             Event::LLM(llm) => {
    ///                 println!(
    ///                     "LLM stream {}: {}",
    ///                     llm.stream_id().unwrap_or("default"),
    ///                     llm.chunk()
    ///                 );
    ///             }
    ///         }
    ///         collected.push(event);
    ///     }
    ///     collected
    /// });
    ///
    /// let final_state = result_future?;
    /// let all_events = event_collector.await?;
    /// println!("Captured {} events", all_events.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Architecture
    ///
    /// This method internally:
    /// 1. Creates a `flume::unbounded()` channel
    /// 2. Builds an EventBus from the runtime configuration and appends a `ChannelSink`
    /// 3. Uses `AppRunner::with_options_and_bus()` with the custom EventBus
    /// 4. Returns both the execution result and receiver
    ///
    /// # See Also
    ///
    /// - [`invoke_with_sinks()`](Self::invoke_with_sinks) - For multiple EventSinks
    /// - [`invoke_streaming()`](Self::invoke_streaming) - Async `EventStream` helper
    /// - [`AppRunner::with_options_and_bus()`](crate::runtimes::runner::AppRunner::with_options_and_bus) - For web servers
    /// - [`invoke()`](Self::invoke) - Simple execution without streaming
    #[instrument(skip(self, initial_state))]
    pub async fn invoke_with_channel(
        &self,
        initial_state: VersionedState,
    ) -> (
        Result<VersionedState, RunnerError>,
        flume::Receiver<crate::event_bus::Event>,
    ) {
        self.invoke_with_bus_builder(
            initial_state,
            false,
            self.runtime_config.checkpointer.clone(),
            || {
                let (tx, rx) = flume::unbounded();
                let event_bus = self.runtime_config.event_bus.build_event_bus();
                event_bus.add_sink(ChannelSink::new(tx));
                (event_bus, rx)
            },
        )
        .await
    }

    /// Execute workflow with custom EventSinks for advanced streaming patterns.
    ///
    /// This convenience method allows you to specify multiple EventSinks while
    /// still maintaining the simplicity of a single method call. Use this when
    /// you need more control over event handling than `invoke_with_channel()`
    /// provides, but don't need the full flexibility of `AppRunner`.
    ///
    /// # When to Use This
    ///
    /// - Need multiple sinks (e.g., stdout + channel + file)
    /// - Want to configure EventBus but don't need per-request isolation
    /// - Building a CLI tool with rich event handling
    ///
    /// # When NOT to Use This
    ///
    /// - Web servers with per-request streaming (use `AppRunner::with_options_and_bus()`)
    /// - Need to create EventBus instances per HTTP request
    /// - Require fine-grained control over runner lifecycle
    ///
    /// Sinks configured on the `RuntimeConfig` remain active; the provided collection is
    /// appended so you can layer additional destinations without rebuilding the app.
    ///
    /// # Parameters
    ///
    /// - `initial_state` - Starting state for workflow execution
    /// - `sinks` - Vector of boxed EventSink implementations
    ///
    /// # Returns
    ///
    /// Final workflow state after completion
    ///
    /// # Examples
    ///
    /// ## Multiple Sinks
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::{ChannelSink, StdOutSink};
    /// use weavegraph::state::VersionedState;
    /// # use weavegraph::app::App;
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    /// let (tx, rx) = flume::unbounded();
    ///
    /// let final_state = app.invoke_with_sinks(
    ///     VersionedState::new_with_user_message("Process data"),
    ///     vec![
    ///         Box::new(StdOutSink::default()),    // Server logs
    ///         Box::new(ChannelSink::new(tx)),     // Client stream
    ///     ],
    /// ).await?;
    ///
    /// // Process events from channel
    /// tokio::spawn(async move {
    ///     while let Ok(event) = rx.recv_async().await {
    ///         println!("Client sees: {:?}", event);
    ///     }
    /// });
    ///
    /// println!("Workflow completed!");
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Custom Sink Implementation
    ///
    /// ```rust,no_run
    /// use weavegraph::event_bus::{EventSink, Event};
    /// # use weavegraph::app::App;
    /// # use weavegraph::state::VersionedState;
    ///
    /// struct MetricsSink;
    ///
    /// impl EventSink for MetricsSink {
    ///     fn handle(&mut self, event: &Event) -> std::io::Result<()> {
    ///         // Send to metrics system
    ///         Ok(())
    ///     }
    /// }
    ///
    /// # async fn example(app: App) -> Result<(), Box<dyn std::error::Error>> {
    /// let final_state = app.invoke_with_sinks(
    ///     VersionedState::new_with_user_message("Monitored workflow"),
    ///     vec![Box::new(MetricsSink)],
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # See Also
    ///
    /// - [`invoke_with_channel()`](Self::invoke_with_channel) - Simpler channel-only variant
    /// - [`invoke_streaming()`](Self::invoke_streaming) - Async `EventStream` without channels
    /// - [`AppRunner::with_options_and_bus()`](crate::runtimes::runner::AppRunner::with_options_and_bus) - Full control
    #[instrument(skip(self, initial_state, sinks), err)]
    pub async fn invoke_with_sinks(
        &self,
        initial_state: VersionedState,
        sinks: Vec<Box<dyn crate::event_bus::EventSink>>,
    ) -> Result<VersionedState, RunnerError> {
        self.invoke_with_bus_builder(
            initial_state,
            false,
            self.runtime_config.checkpointer.clone(),
            move || {
                let event_bus = self.runtime_config.event_bus.build_event_bus();
                for sink in sinks {
                    event_bus.add_boxed_sink(sink);
                }
                (event_bus, ())
            },
        )
        .await
        .0
    }

    /// Generate the session identifier for the next invocation.
    ///
    /// Prefers an explicit session id from the runtime configuration and
    /// falls back to a randomly generated identifier when none is supplied.
    /// Consolidating this logic helps keep new entry points from accidentally
    /// reusing the same hard-coded id.
    fn next_session_id(&self) -> String {
        self.runtime_config
            .session_id
            .clone()
            .unwrap_or_else(|| IdGenerator::new().generate_run_id())
    }

    /// Drive a workflow session to completion, resuming from checkpoints when available.
    async fn run_session(
        mut runner: AppRunner,
        session_id: String,
        initial_state: VersionedState,
    ) -> Result<VersionedState, RunnerError> {
        let init_state = runner
            .create_session(session_id.clone(), initial_state)
            .await?;

        if let SessionInit::Resumed { checkpoint_step } = init_state {
            tracing::info!(
                session = %session_id,
                checkpoint_step,
                "Resuming session from checkpoint"
            );
        }

        runner.run_until_complete(&session_id).await
    }

    /// Merge node outputs and apply state reductions after a superstep.
    ///
    /// This method coordinates the barrier synchronization phase of workflow
    /// execution, where all node outputs from a superstep are collected,
    /// merged, and applied to the global state via registered reducers. The
    /// returned [`BarrierOutcome`] captures channel updates, aggregated errors,
    /// and frontier commands in a stable order so downstream consumers can rely
    /// on deterministic behaviour.
    ///
    /// # Parameters
    /// * `state` - Mutable reference to the current versioned state
    /// * `run_ids` - Slice of node kinds that executed in this superstep
    /// * `node_partials` - Vector of partial updates from each executed node
    ///
    /// # Returns
    /// * `Ok(Vec<&'static str>)` - Names of channels that were updated
    /// * `Err(Box<dyn Error>)` - If reducer application fails
    ///
    /// # State Management
    /// - Aggregates messages, extra data, and errors from all nodes
    /// - Applies registered reducers to merge updates into global state
    /// - Intelligently bumps version numbers only when content changes
    /// - Preserves deterministic merge behavior for reproducible execution
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use weavegraph::app::App;
    /// # use weavegraph::node::NodePartial;
    /// # use weavegraph::state::VersionedState;
    /// # use weavegraph::types::NodeKind;
    /// # use weavegraph::message::Message;
    /// # async fn example(app: App, state: &mut VersionedState) -> Result<(), String> {
    /// let partials = vec![NodePartial {
    ///     messages: Some(vec![Message::assistant("test")]),
    ///     ..Default::default()
    /// }];
    /// let outcome = app.apply_barrier(state, &[NodeKind::Custom("process".into())], partials).await
    ///     .map_err(|e| format!("Error: {}", e))?;
    /// println!("Updated channels: {:?}", outcome.updated_channels);
    /// println!("Errors emitted: {}", outcome.errors.len());
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(skip(self, state, run_ids, node_partials), err)]
    pub async fn apply_barrier(
        &self,
        state: &mut VersionedState,
        run_ids: &[NodeKind],
        node_partials: Vec<NodePartial>,
    ) -> Result<BarrierOutcome, Box<dyn std::error::Error + Send + Sync>> {
        let mut msgs_all: Vec<Message> = Vec::new();
        let mut extra_all = new_extra_map();
        let mut errors_all: Vec<ErrorEvent> = Vec::new();
        let mut frontier_commands: Vec<(NodeKind, FrontierCommand)> = Vec::new();

        for (i, p) in node_partials.iter().enumerate() {
            let fallback = NodeKind::Custom("?".to_string());
            let nid = run_ids.get(i).unwrap_or(&fallback);

            if let Some(ms) = &p.messages {
                if !ms.is_empty() {
                    tracing::debug!(node = ?nid, count = ms.len(), "Node produced messages");
                    msgs_all.extend(ms.clone());
                }
            }

            if let Some(ex) = &p.extra {
                if !ex.is_empty() {
                    tracing::debug!(node = ?nid, keys = ex.len(), "Node produced extra data");
                    // Sort keys to keep the merged map deterministic across runs.
                    let mut sorted_pairs: Vec<_> = ex.iter().collect();
                    sorted_pairs.sort_by(|(left, _), (right, _)| left.cmp(right));
                    for (k, v) in sorted_pairs {
                        extra_all.insert(k.clone(), v.clone());
                    }
                }
            }

            if let Some(errs) = &p.errors {
                if !errs.is_empty() {
                    tracing::debug!(node = ?nid, count = errs.len(), "Node produced errors");
                    errors_all.extend(errs.clone());
                }
            }

            if let Some(command) = &p.frontier {
                frontier_commands.push((nid.clone(), command.clone()));
            }
        }

        fn scope_sort_key(scope: &ErrorScope) -> (u8, &str, u64) {
            match scope {
                ErrorScope::Node { kind, step } => (0, kind.as_str(), *step),
                ErrorScope::Scheduler { step } => (1, "", *step),
                ErrorScope::Runner { session, step } => (2, session.as_str(), *step),
                ErrorScope::App => (3, "", 0),
            }
        }

        // Sort aggregated errors so downstream consumers observe a stable order.
        errors_all.sort_by(|a, b| {
            let key_a = scope_sort_key(&a.scope);
            let key_b = scope_sort_key(&b.scope);
            key_a
                .cmp(&key_b)
                .then_with(|| a.when.cmp(&b.when))
                .then_with(|| a.error.message.cmp(&b.error.message))
        });

        let errors_for_state = if errors_all.is_empty() {
            None
        } else {
            Some(errors_all.clone())
        };

        let merged_updates = NodePartial {
            messages: if msgs_all.is_empty() {
                None
            } else {
                Some(msgs_all)
            },
            extra: if extra_all.is_empty() {
                None
            } else {
                Some(extra_all)
            },
            errors: errors_for_state,
            frontier: None,
        };

        // Record before-states for version bump decisions
        let msgs_before_len = state.messages.len();
        let msgs_before_ver = state.messages.version();
        let extra_before = state.extra.snapshot();
        let extra_before_ver = state.extra.version();

        // Apply reducers (they do NOT bump versions)
        self.reducer_registry
            .apply_all(&mut *state, &merged_updates)?;

        // Detect changes & bump versions responsibly
        let mut updated: Vec<&'static str> = Vec::new();

        let msgs_changed = state.messages.len() != msgs_before_len;
        if msgs_changed {
            state
                .messages
                .set_version(msgs_before_ver.saturating_add(1));
            tracing::info!(
                target: "weavegraph::app",
                channel = "messages",
                before_count = msgs_before_len,
                after_count = state.messages.len(),
                before_version = msgs_before_ver,
                after_version = state.messages.version(),
                "channel updated"
            );
            updated.push("messages");
        }

        let extra_after = state.extra.snapshot();
        let extra_changed = extra_after != extra_before;
        if extra_changed {
            state.extra.set_version(extra_before_ver.saturating_add(1));
            tracing::info!(
                target: "weavegraph::app",
                channel = "extra",
                before_count = extra_before.len(),
                after_count = extra_after.len(),
                before_version = extra_before_ver,
                after_version = state.extra.version(),
                "channel updated"
            );
            updated.push("extra");
        }

        Ok(BarrierOutcome {
            updated_channels: updated,
            errors: errors_all,
            frontier_commands,
        })
    }
}
