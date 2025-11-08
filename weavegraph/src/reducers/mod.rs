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
use miette::Diagnostic;
use thiserror::Error;

/// Unified reducer trait: every reducer mutates VersionedState using a NodePartial delta.
/// Channels currently implemented: messages (append) and extra (shallow JSON map merge).
pub trait Reducer: Send + Sync {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial);
}

#[derive(Debug, Error, Diagnostic)]
pub enum ReducerError {
    #[error("no reducers registered for channel: {0:?}")]
    #[diagnostic(
        code(weavegraph::reducers::unknown_channel),
        help("Use GraphBuilder::with_reducer() to register a reducer for {0:?}")
    )]
    UnknownChannel(ChannelType),

    #[error("reducer apply failed for channel {channel:?}: {message}")]
    #[diagnostic(
        code(weavegraph::reducers::apply),
        help("Check that your reducer implementation correctly handles the NodePartial structure")
    )]
    Apply {
        channel: ChannelType,
        message: String,
    },
}
