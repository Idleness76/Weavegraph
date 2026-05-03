//! Channel types that form the typed state slots of a workflow's [`VersionedState`](crate::state::VersionedState).
use crate::types::ChannelType;

/// Error event and scope types for structured workflow error capture.
pub mod errors;
mod errors_channel;
mod extras;
mod messages;

pub use errors::*;
pub use errors_channel::ErrorsChannel;
pub use extras::ExtrasChannel;
pub use messages::MessagesChannel;

/// Core trait for a typed, versioned workflow state channel.
///
/// Each implementing type wraps a value of type `T` with a version counter
/// used by the scheduler for change-detection gating.
pub trait Channel<T>: Sync + Send {
    /// Returns the [`ChannelType`] discriminant for this channel.
    fn get_channel_type(&self) -> ChannelType;
    /// Returns a clone of the current channel value.
    fn snapshot(&self) -> T;
    /// Returns the number of items in the channel.
    fn len(&self) -> usize;
    /// Returns `true` if the channel contains no items.
    fn is_empty(&self) -> bool;
    /// Returns the current version counter.
    fn version(&self) -> u32;
    /// Sets the version counter to the given value.
    fn set_version(&mut self, version: u32) -> ();
    /// Returns a mutable reference to the underlying value.
    fn get_mut(&mut self) -> &mut T;
    /// Returns `true` if this channel's data should be persisted across steps.
    fn persistent(&self) -> bool;
}
