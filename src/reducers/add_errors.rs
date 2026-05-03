//! Reducer that appends incoming [`ErrorEvent`](crate::channels::errors::ErrorEvent) entries to the errors channel.
use super::Reducer;
use crate::{channels::Channel, node::NodePartial, state::VersionedState};

/// Reducer that appends error events from a [`NodePartial`](crate::node::NodePartial) to the state errors channel.
#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub struct AddErrors;

impl Reducer for AddErrors {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        if let Some(error_events) = &update.errors
            && !error_events.is_empty()
        {
            state.errors.get_mut().extend_from_slice(error_events);
        }
    }
}
