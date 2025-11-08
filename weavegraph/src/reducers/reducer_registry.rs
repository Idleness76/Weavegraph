use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::{
    node::NodePartial,
    reducers::{AddErrors, AddMessages, MapMerge, Reducer, ReducerError},
    state::VersionedState,
    types::ChannelType,
};
use tracing::instrument;

#[derive(Clone)]
pub struct ReducerRegistry {
    reducer_map: FxHashMap<ChannelType, Vec<Arc<dyn Reducer>>>,
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
        let mut registry = Self::new();
        registry
            .register(ChannelType::Message, Arc::new(AddMessages))
            .register(ChannelType::Extra, Arc::new(MapMerge))
            .register(ChannelType::Error, Arc::new(AddErrors));
        registry
    }
}

impl ReducerRegistry {
    /// Creates a new empty reducer registry.
    pub fn new() -> Self {
        Self {
            reducer_map: FxHashMap::default(),
        }
    }

    /// Registers a reducer for a specific channel type.
    ///
    /// This method allows dynamic registration of reducers at runtime.
    /// Multiple reducers can be registered for the same channel and will
    /// be applied in registration order.
    ///
    /// # Parameters
    /// - `channel`: The channel type to register the reducer for
    /// - `reducer`: The reducer implementation wrapped in Arc
    ///
    /// # Returns
    /// A mutable reference to self for method chaining
    pub fn register(&mut self, channel: ChannelType, reducer: Arc<dyn Reducer>) -> &mut Self {
        self.reducer_map.entry(channel).or_default().push(reducer);
        self
    }

    /// Builder-style method for registering a reducer.
    ///
    /// This is a convenience method that consumes self and returns it,
    /// enabling fluent API usage when constructing a ReducerRegistry.
    ///
    /// # Parameters
    /// - `channel`: The channel type to register the reducer for
    /// - `reducer`: The reducer implementation wrapped in Arc
    ///
    /// # Returns
    /// Self for method chaining
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    /// use weavegraph::reducers::{ReducerRegistry, AddMessages};
    /// use weavegraph::types::ChannelType;
    ///
    /// let registry = ReducerRegistry::new()
    ///     .with_reducer(ChannelType::Message, Arc::new(AddMessages));
    /// ```
    pub fn with_reducer(mut self, channel: ChannelType, reducer: Arc<dyn Reducer>) -> Self {
        self.register(channel, reducer);
        self
    }

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
            for reducer in reducers {
                reducer.apply(state, to_update);
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
