//! Runtime configuration types for controlling event bus, sinks, and diagnostics.
use std::sync::Arc;

use crate::event_bus::{EventBus, EventSink, MemorySink, StdOutSink};

use super::Checkpointer;

/// Configuration for a single [`AppRunner`](crate::runtimes::runner::AppRunner) instance.
#[derive(Clone)]
pub struct RuntimeConfig {
    /// Optional session ID to use; a new UUID is generated if `None`.
    pub session_id: Option<String>,
    /// Custom [`Checkpointer`] to use instead of the built-in types.
    pub checkpointer_custom: Option<Arc<dyn Checkpointer>>,
    /// SQLite database file name; defaults to `SQLITE_DB_NAME` env var or `weavegraph.db`.
    pub sqlite_db_name: Option<String>,
    /// Event bus configuration used to build the [`EventBus`].
    pub event_bus: EventBusConfig,
}

impl std::fmt::Debug for RuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeConfig")
            .field("session_id", &self.session_id)
            .field("checkpointer_custom", &self.checkpointer_custom.is_some())
            .field("sqlite_db_name", &self.sqlite_db_name)
            .field("event_bus", &self.event_bus)
            .finish()
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            session_id: None,
            checkpointer_custom: None,
            sqlite_db_name: Self::resolve_sqlite_db_name(None),
            event_bus: EventBusConfig::default(),
        }
    }
}

impl RuntimeConfig {
    fn resolve_sqlite_db_name(provided: Option<String>) -> Option<String> {
        if let Some(name) = provided {
            return Some(name);
        }
        dotenvy::dotenv().ok();
        Some(std::env::var("SQLITE_DB_NAME").unwrap_or_else(|_| "weavegraph.db".to_string()))
    }

    /// Create a new `RuntimeConfig` with the given session ID and optional SQLite DB name.
    pub fn new(session_id: Option<String>, sqlite_db_name: Option<String>) -> Self {
        Self {
            session_id,
            checkpointer_custom: None,
            sqlite_db_name: Self::resolve_sqlite_db_name(sqlite_db_name),
            event_bus: EventBusConfig::default(),
        }
    }

    #[must_use]
    /// Set a custom [`Checkpointer`] for this configuration.
    pub fn checkpointer_custom(mut self, checkpointer: Arc<dyn Checkpointer>) -> Self {
        self.checkpointer_custom = Some(checkpointer);
        self
    }

    #[must_use]
    /// Return the custom checkpointer if one has been set.
    pub fn custom_checkpointer(&self) -> Option<Arc<dyn Checkpointer>> {
        self.checkpointer_custom.clone()
    }

    #[must_use]
    /// Replace the event bus configuration for this runtime.
    pub fn with_event_bus(mut self, event_bus: EventBusConfig) -> Self {
        self.event_bus = event_bus;
        self
    }

    #[must_use]
    /// Configure the runtime with a stdout-only event bus.
    pub fn with_stdout_event_bus(self) -> Self {
        self.with_event_bus(EventBusConfig::with_stdout_only())
    }

    #[must_use]
    /// Configure the runtime with an in-memory event bus (useful for testing).
    pub fn with_memory_event_bus(self) -> Self {
        self.with_event_bus(EventBusConfig::with_memory_sink())
    }
}

/// Selects the output target for an [`EventBusConfig`] sink entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SinkConfig {
    /// Write events to standard output.
    StdOut,
    /// Capture events in memory (useful for testing).
    Memory,
}

/// Configuration for building the [`EventBus`] used by a runtime.
#[derive(Clone, Debug)]
pub struct EventBusConfig {
    /// Broadcast channel capacity; events are dropped when the buffer is full.
    pub buffer_capacity: usize,
    /// Ordered list of sink targets that will receive events.
    pub sinks: Vec<SinkConfig>,
    diagnostics: DiagnosticsConfig,
}

impl EventBusConfig {
    /// Default broadcast channel capacity.
    pub const DEFAULT_BUFFER_CAPACITY: usize = 1024;

    #[must_use]
    /// Create an `EventBusConfig` with the given capacity and sinks.
    pub fn new(buffer_capacity: usize, sinks: Vec<SinkConfig>) -> Self {
        Self {
            buffer_capacity: if buffer_capacity == 0 {
                Self::DEFAULT_BUFFER_CAPACITY
            } else {
                buffer_capacity
            },
            sinks,
            diagnostics: DiagnosticsConfig::default_with_capacity(buffer_capacity),
        }
    }

    #[must_use]
    /// Create an `EventBusConfig` with a single stdout sink at the default capacity.
    pub fn with_stdout_only() -> Self {
        Self::new(Self::DEFAULT_BUFFER_CAPACITY, vec![SinkConfig::StdOut])
    }

    #[must_use]
    /// Create an `EventBusConfig` with a single in-memory sink (silent stdout) at the default capacity.
    pub fn with_memory_sink() -> Self {
        // Memory sink intentionally omits stdout so callers get a silent capture by default.
        Self::new(Self::DEFAULT_BUFFER_CAPACITY, vec![SinkConfig::Memory])
    }

    #[must_use]
    /// Add a sink to this configuration, ignoring duplicates.
    pub fn add_sink(mut self, sink: SinkConfig) -> Self {
        if !self.sinks.contains(&sink) {
            self.sinks.push(sink);
        }
        self
    }

    /// Returns the configured broadcast buffer capacity.
    pub fn buffer_capacity(&self) -> usize {
        self.buffer_capacity
    }

    /// Returns the configured sink list.
    pub fn sinks(&self) -> &[SinkConfig] {
        &self.sinks
    }

    #[must_use]
    /// Override the diagnostics configuration for this event bus.
    pub fn with_diagnostics(mut self, diagnostics: DiagnosticsConfig) -> Self {
        self.diagnostics = diagnostics.with_default_capacity(self.buffer_capacity);
        self
    }

    #[must_use]
    /// Build and return the configured [`EventBus`].
    pub fn build_event_bus(&self) -> EventBus {
        let mut sinks: Vec<Box<dyn EventSink>> = if self.sinks.is_empty() {
            vec![Box::new(StdOutSink::default())]
        } else {
            self.sinks
                .iter()
                .map(|sink| match sink {
                    SinkConfig::StdOut => Box::new(StdOutSink::default()) as Box<dyn EventSink>,
                    SinkConfig::Memory => Box::new(MemorySink::new()) as Box<dyn EventSink>,
                })
                .collect()
        };
        if sinks.is_empty() {
            sinks.push(Box::new(StdOutSink::default()));
        }
        EventBus::with_capacity_and_diag(
            sinks,
            self.buffer_capacity(),
            self.diagnostics.effective_capacity(self.buffer_capacity()),
            self.diagnostics.enabled,
            self.diagnostics.emit_to_events,
        )
    }
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self::with_stdout_only()
    }
}

/// Configuration controlling the diagnostics (sink health) broadcast channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticsConfig {
    /// Whether sink diagnostics are enabled.
    pub enabled: bool,
    /// Optional override for the diagnostics channel capacity; falls back to the event bus capacity.
    pub buffer_capacity: Option<usize>,
    /// Whether diagnostics should also be forwarded into the main event stream.
    pub emit_to_events: bool,
}

impl DiagnosticsConfig {
    fn normalize_capacity(capacity: usize) -> usize {
        capacity.max(1)
    }

    /// Create a default `DiagnosticsConfig` with the given event bus capacity.
    pub fn default_with_capacity(event_bus_capacity: usize) -> Self {
        Self {
            enabled: true,
            buffer_capacity: Some(Self::normalize_capacity(event_bus_capacity)),
            emit_to_events: false,
        }
    }

    /// Fill in the buffer capacity from `event_bus_capacity` if not already set.
    pub fn with_default_capacity(mut self, event_bus_capacity: usize) -> Self {
        if self.buffer_capacity.is_none() {
            self.buffer_capacity = Some(Self::normalize_capacity(event_bus_capacity));
        }
        self
    }

    /// Return the effective diagnostics channel capacity, falling back to `event_bus_capacity`.
    pub fn effective_capacity(&self, event_bus_capacity: usize) -> usize {
        self.buffer_capacity
            .unwrap_or_else(|| Self::normalize_capacity(event_bus_capacity))
    }
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            buffer_capacity: None,
            emit_to_events: false,
        }
    }
}
