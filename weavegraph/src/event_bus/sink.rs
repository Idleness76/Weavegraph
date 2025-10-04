use std::io::{self, Result as IoResult, Stdout, Write};
use std::sync::{Arc, Mutex};

/// Abstraction over an output device that can consume rendered event lines.
pub trait EventSink: Sync + Send {
    fn write(&mut self, message: &str) -> IoResult<()>;
}

/// Implementation of EventSink that writes to process stdout.
pub struct StdOutSink {
    handle: Stdout,
}

impl Default for StdOutSink {
    fn default() -> Self {
        Self {
            handle: io::stdout(),
        }
    }
}

impl EventSink for StdOutSink {
    fn write(&mut self, message: &str) -> IoResult<()> {
        self.handle.write_all(message.as_bytes())?;
        self.handle.flush()
    }
}

/// Simple in-memory sink useful for tests and instrumentation snapshots.
#[derive(Clone, Default)]
pub struct MemorySink {
    entries: Arc<Mutex<Vec<String>>>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.entries.lock().unwrap().clone()
    }
}

impl EventSink for MemorySink {
    fn write(&mut self, message: &str) -> IoResult<()> {
        self.entries.lock().unwrap().push(message.to_string());
        Ok(())
    }
}
