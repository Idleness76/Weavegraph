use crate::utils::id_generator;

use super::CheckpointerType;

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub session_id: Option<String>,
    pub checkpointer: Option<CheckpointerType>,
    pub sqlite_db_name: Option<String>,
    pub event_bus: EventBusConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            session_id: Some(id_generator::IdGenerator::new().generate_run_id()),
            checkpointer: Some(CheckpointerType::InMemory),
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

    pub fn new(
        session_id: Option<String>,
        checkpointer: Option<CheckpointerType>,
        sqlite_db_name: Option<String>,
    ) -> Self {
        Self {
            session_id,
            checkpointer,
            sqlite_db_name: Self::resolve_sqlite_db_name(sqlite_db_name),
            event_bus: EventBusConfig::default(),
        }
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
        }
    }

    #[must_use]
    pub fn with_stdout_only() -> Self {
        Self::new(Self::DEFAULT_BUFFER_CAPACITY, vec![SinkConfig::StdOut])
    }

    #[must_use]
    pub fn with_memory_sink() -> Self {
        Self::new(
            Self::DEFAULT_BUFFER_CAPACITY,
            vec![SinkConfig::StdOut, SinkConfig::Memory],
        )
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
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self::with_stdout_only()
    }
}
