mod add_errors;
mod add_messages;
mod map_merge;
mod reducer_registry;

pub use add_errors::AddErrors;
pub use add_messages::AddMessages;
pub use map_merge::MapMerge;
pub use reducer_registry::*;

use crate::node::NodePartial;
use crate::state::VersionedState;
use crate::types::ChannelType;
use std::fmt;

/// Unified reducer trait: every reducer mutates VersionedState using a NodePartial delta.
/// Channels currently implemented: messages (append) and extra (shallow JSON map merge).
pub trait Reducer: Send + Sync {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial);
}

#[derive(Debug)]
pub enum ReducerError {
    UnknownChannel(ChannelType),

    Apply {
        channel: ChannelType,
        message: String,
    },
}

impl fmt::Display for ReducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReducerError::UnknownChannel(channel) => {
                write!(f, "no reducers registered for channel: {channel:?}")
            }
            ReducerError::Apply { channel, message } => {
                write!(f, "reducer apply failed for channel {channel:?}: {message}")
            }
        }
    }
}

impl std::error::Error for ReducerError {}
