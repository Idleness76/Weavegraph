use super::Channel;
use super::errors::ErrorEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorsChannel {
    value: Vec<ErrorEvent>,
    version: u32,
}

impl ErrorsChannel {
    pub fn new(events: Vec<ErrorEvent>, version: u32) -> Self {
        Self {
            value: events,
            version,
        }
    }
}

impl Default for ErrorsChannel {
    fn default() -> Self {
        Self {
            value: Vec::new(),
            version: 1,
        }
    }
}

impl Channel<Vec<ErrorEvent>> for ErrorsChannel {
    fn get_channel_type(&self) -> crate::types::ChannelType {
        crate::types::ChannelType::Error
    }

    fn snapshot(&self) -> Vec<ErrorEvent> {
        self.value.clone()
    }

    fn len(&self) -> usize {
        self.value.len()
    }

    fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    fn version(&self) -> u32 {
        self.version
    }

    fn set_version(&mut self, version: u32) {
        self.version = version;
    }

    fn get_mut(&mut self) -> &mut Vec<ErrorEvent> {
        &mut self.value
    }

    fn persistent(&self) -> bool {
        true
    }
}
