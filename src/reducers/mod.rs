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
    fn apply(&self, state: &mut VersionedState, update: &NodePartial);
}

#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum ReducerError {
    #[error("no reducers registered for channel: {0:?}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::reducers::unknown_channel))
    )]
    UnknownChannel(ChannelType),

    #[error("reducer apply failed for channel {channel:?}: {message}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::reducers::apply_failed))
    )]
    Apply {
        channel: ChannelType,
        message: String,
    },
}
