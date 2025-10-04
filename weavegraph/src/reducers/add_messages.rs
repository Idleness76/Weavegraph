use super::Reducer;
use crate::{channels::Channel, node::NodePartial, state::VersionedState};

#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub struct AddMessages;

impl Reducer for AddMessages {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        if let Some(msgs) = &update.messages {
            // Append new messages and bump version
            state.messages.get_mut().extend(msgs.clone());
        }
    }
}
