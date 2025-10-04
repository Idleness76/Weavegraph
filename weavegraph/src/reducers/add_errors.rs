use super::Reducer;
use crate::{channels::Channel, node::NodePartial, state::VersionedState};

#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub struct AddErrors;

impl Reducer for AddErrors {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        if let Some(error_events) = &update.errors {
            if !error_events.is_empty() {
                state.errors.get_mut().extend(error_events.iter().cloned());
            }
        }
    }
}
