//! State reducers that apply [`NodePartial`] updates to [`VersionedState`].
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
use thiserror::Error;

/// Unified reducer trait: every reducer mutates VersionedState using a NodePartial delta.
/// Channels currently implemented: messages (append) and extra (shallow JSON map merge).
pub trait Reducer: Send + Sync {
    /// Apply the partial update `update` to `state`, mutating it in place.
    fn apply(&self, state: &mut VersionedState, update: &NodePartial);
}

/// Errors that can occur when applying reducers to workflow state.
#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum ReducerError {
    /// No reducer is registered for the specified channel type.
    #[error("no reducers registered for channel: {0:?}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::reducers::unknown_channel))
    )]
    UnknownChannel(ChannelType),

    /// A reducer failed while applying an update to a channel.
    #[error("reducer apply failed for channel {channel:?}: {message}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::reducers::apply_failed))
    )]
    Apply {
        /// The channel type for which the reducer failed.
        channel: ChannelType,
        /// Human-readable description of the failure.
        message: String,
    },
}
