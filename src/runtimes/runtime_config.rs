use std::sync::Arc;

use crate::event_bus::{EventBus, EventSink, MemorySink, StdOutSink};

use super::{Checkpointer, CheckpointerType};

#[derive(Clone)]
pub struct RuntimeConfig {
    pub session_id: Option<String>,
    #[deprecated(
        since = "0.3.4",
        note = "Use RuntimeConfig::with_checkpointer(...) for enum convenience or RuntimeConfig::checkpointer_custom(...) for custom backends; field will be removed in 0.4.0"
    )]
    pub checkpointer: Option<CheckpointerType>,
    pub checkpointer_custom: Option<Arc<dyn Checkpointer>>,
    pub sqlite_db_name: Option<String>,
    pub event_bus: EventBusConfig,
}

impl std::fmt::Debug for RuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeConfig")
            .field("session_id", &self.session_id)
            .field("checkpointer", &self.checkpointer_type())
            .field("checkpointer_custom", &self.checkpointer_custom.is_some())
            .field("sqlite_db_name", &self.sqlite_db_name)
            .field("event_bus", &self.event_bus)
            .finish()
    }
}

impl Default for RuntimeConfig {
    #[allow(deprecated)]
    fn default() -> Self {
        Self {
            // Generate session identifiers lazily so helpers can pick a fresh id per run.
            session_id: None,
            checkpointer: Some(CheckpointerType::InMemory),
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

    #[allow(deprecated)]
    pub fn new(
        session_id: Option<String>,
        checkpointer: Option<CheckpointerType>,
        sqlite_db_name: Option<String>,
    ) -> Self {
        Self {
            session_id,
            checkpointer,
            checkpointer_custom: None,
            sqlite_db_name: Self::resolve_sqlite_db_name(sqlite_db_name),
            event_bus: EventBusConfig::default(),
        }
    }

    #[allow(deprecated)]
    #[must_use]
    pub fn with_checkpointer(mut self, checkpointer: Option<CheckpointerType>) -> Self {
        self.checkpointer = checkpointer;
        self
    }

    #[must_use]
    pub fn checkpointer_type(&self) -> Option<CheckpointerType> {
        #[allow(deprecated)]
        {
            self.checkpointer.clone()
        }
    }

    #[must_use]
    pub fn checkpointer_custom(mut self, checkpointer: Arc<dyn Checkpointer>) -> Self {
        self.checkpointer_custom = Some(checkpointer);
        self
    }

    #[must_use]
    pub fn custom_checkpointer(&self) -> Option<Arc<dyn Checkpointer>> {
        self.checkpointer_custom.clone()
    }

    #[must_use]
    pub fn with_event_bus(mut self, event_bus: EventBusConfig) -> Self {
        self.event_bus = event_bus;
        self
    }

    #[must_use]
    pub fn with_stdout_event_bus(self) -> Self {
        self.with_event_bus(EventBusConfig::with_stdout_only())
    }

    #[must_use]
    pub fn with_memory_event_bus(self) -> Self {
        self.with_event_bus(EventBusConfig::with_memory_sink())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SinkConfig {
    StdOut,
    Memory,
}

#[derive(Clone, Debug)]
pub struct EventBusConfig {
    pub buffer_capacity: usize,
    pub sinks: Vec<SinkConfig>,
    diagnostics: DiagnosticsConfig,
}

impl EventBusConfig {
    pub const DEFAULT_BUFFER_CAPACITY: usize = 1024;

    #[must_use]
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
    pub fn with_stdout_only() -> Self {
        Self::new(Self::DEFAULT_BUFFER_CAPACITY, vec![SinkConfig::StdOut])
    }

    #[must_use]
    pub fn with_memory_sink() -> Self {
        // Memory sink intentionally omits stdout so callers get a silent capture by default.
        Self::new(Self::DEFAULT_BUFFER_CAPACITY, vec![SinkConfig::Memory])
    }

    #[must_use]
    pub fn add_sink(mut self, sink: SinkConfig) -> Self {
        if !self.sinks.contains(&sink) {
            self.sinks.push(sink);
        }
        self
    }

    pub fn buffer_capacity(&self) -> usize {
        self.buffer_capacity
    }

    pub fn sinks(&self) -> &[SinkConfig] {
        &self.sinks
    }

    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: DiagnosticsConfig) -> Self {
        self.diagnostics = diagnostics.with_default_capacity(self.buffer_capacity);
        self
    }

    #[must_use]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticsConfig {
    pub enabled: bool,
    pub buffer_capacity: Option<usize>,
    pub emit_to_events: bool,
}

impl DiagnosticsConfig {
    fn normalize_capacity(capacity: usize) -> usize {
        capacity.max(1)
    }

    pub fn default_with_capacity(event_bus_capacity: usize) -> Self {
        Self {
            enabled: true,
            buffer_capacity: Some(Self::normalize_capacity(event_bus_capacity)),
            emit_to_events: false,
        }
    }

    pub fn with_default_capacity(mut self, event_bus_capacity: usize) -> Self {
        if self.buffer_capacity.is_none() {
            self.buffer_capacity = Some(Self::normalize_capacity(event_bus_capacity));
        }
        self
    }

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
