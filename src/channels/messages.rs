use super::Channel;
use crate::{message::Message, types::ChannelType};

type ChannelValue = Vec<Message>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessagesChannel {
    value: ChannelValue,
    version: u32,
}

impl MessagesChannel {
    pub fn new(messages: ChannelValue, version: u32) -> Self {
        Self {
            value: messages,
            version,
        }
    }
}

impl Channel<ChannelValue> for MessagesChannel {
    fn get_channel_type(&self) -> ChannelType {
        ChannelType::Message
    }

    fn snapshot(&self) -> ChannelValue {
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

    fn get_mut(&mut self) -> &mut ChannelValue {
        &mut self.value
    }

    fn set_version(&mut self, version: u32) {
        self.version = version
    }

    fn persistent(&self) -> bool {
        true
    }
}

impl Default for MessagesChannel {
    fn default() -> Self {
        Self {
            value: Vec::new(),
            version: 1,
        }
    }
}
