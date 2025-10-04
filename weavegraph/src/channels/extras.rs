use rustc_hash::FxHashMap;

use super::Channel;
use crate::types::ChannelType;

type ChannelValue = FxHashMap<String, serde_json::Value>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtrasChannel {
    value: ChannelValue,
    version: u32,
}

impl ExtrasChannel {
    pub fn new(extras: ChannelValue, version: u32) -> Self {
        Self {
            value: extras,
            version,
        }
    }
}

impl Channel<ChannelValue> for ExtrasChannel {
    fn get_channel_type(&self) -> ChannelType {
        ChannelType::Extra
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

impl Default for ExtrasChannel {
    fn default() -> Self {
        Self {
            value: FxHashMap::default(),
            version: 1,
        }
    }
}
