//! Reducer that appends incoming messages to the messages channel.
use super::Reducer;
use crate::{channels::Channel, node::NodePartial, state::VersionedState};

/// Reducer that appends messages from a [`NodePartial`](crate::node::NodePartial) to the state messages channel.
#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub struct AddMessages;

impl Reducer for AddMessages {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        if let Some(msgs) = &update.messages {
            // Append new messages without cloning the entire vector
            state.messages.get_mut().extend_from_slice(msgs);
        }
    }
}
