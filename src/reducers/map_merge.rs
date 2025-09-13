use super::Reducer;
use crate::{channels::Channel, node::NodePartial, state::VersionedState};

#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub struct MapMerge;
impl Reducer for MapMerge {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        if let Some(extras_update) = &update.extra {
            if extras_update.len() > 0 {
                for (k, v) in extras_update {
                    state.extra.get_mut().insert(k.clone(), v.clone());
                }
            }
        }
    }
}
