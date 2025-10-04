use rustc_hash::FxHashMap;

use crate::{
    node::NodePartial,
    reducers::{AddErrors, AddMessages, MapMerge, Reducer, ReducerError},
    state::VersionedState,
    types::ChannelType,
};
use tracing::instrument;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReducerType {
    AddMessages(AddMessages),
    JsonShallowMerge(MapMerge),
    AddErrors(AddErrors),
}

impl ReducerType {
    pub fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        match self {
            ReducerType::AddMessages(r) => r.apply(state, update),
            ReducerType::JsonShallowMerge(r) => r.apply(state, update),
            ReducerType::AddErrors(r) => r.apply(state, update),
        }
    }
}

#[derive(Clone)]
pub struct ReducerRegistry {
    reducer_map: FxHashMap<ChannelType, Vec<ReducerType>>,
}

/// Guard that checks whether a NodePartial actually has meaningful data
/// for the specified channel. This lets the registry skip invoking
/// reducers when there is nothing to do.
fn channel_guard(channel: &ChannelType, partial: &NodePartial) -> bool {
    match channel {
        ChannelType::Message => partial
            .messages
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        ChannelType::Extra => partial
            .extra
            .as_ref()
            .map(|m| !m.is_empty())
            .unwrap_or(false),
        ChannelType::Error => partial
            .errors
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
    }
}

impl Default for ReducerRegistry {
    fn default() -> Self {
        let mut reducer_map: FxHashMap<ChannelType, Vec<ReducerType>> = FxHashMap::default();

        reducer_map.insert(
            ChannelType::Message,
            vec![ReducerType::AddMessages(AddMessages)],
        );

        reducer_map.insert(
            ChannelType::Extra,
            vec![ReducerType::JsonShallowMerge(MapMerge)],
        );

        reducer_map.insert(ChannelType::Error, vec![ReducerType::AddErrors(AddErrors)]);

        ReducerRegistry { reducer_map }
    }
}

impl ReducerRegistry {
    #[instrument(skip(self, state, to_update), err)]
    pub fn try_update(
        &self,
        channel_type: ChannelType,
        state: &mut VersionedState,
        to_update: &NodePartial,
    ) -> Result<(), ReducerError> {
        // Skip if the partial has no applicable data for this channel.
        if !channel_guard(&channel_type, to_update) {
            return Ok(());
        }

        if let Some(reducers) = self.reducer_map.get(&channel_type) {
            for r in reducers {
                r.apply(state, to_update);
            }
            Ok(())
        } else {
            Err(ReducerError::UnknownChannel(channel_type))
        }
    }

    #[instrument(skip(self, state, merged_updates), err)]
    pub fn apply_all(
        &self,
        state: &mut VersionedState,
        merged_updates: &NodePartial,
    ) -> Result<(), ReducerError> {
        // Iterate all registered channels; try_update will skip via guard if no data.
        for channel in self.reducer_map.keys() {
            self.try_update(channel.clone(), state, merged_updates)?;
        }
        Ok(())
    }
}
