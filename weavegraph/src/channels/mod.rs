use crate::types::ChannelType;

pub mod errors;
mod errors_channel;
mod extras;
mod messages;
#[cfg(test)]
mod tests;

pub use errors::*;
pub use errors_channel::ErrorsChannel;
pub use extras::ExtrasChannel;
pub use messages::MessagesChannel;

pub trait Channel<T>: Sync + Send {
    fn get_channel_type(&self) -> ChannelType;
    fn snapshot(&self) -> T;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn version(&self) -> u32;
    fn set_version(&mut self, version: u32) -> ();
    fn get_mut(&mut self) -> &mut T;
    fn persistent(&self) -> bool;
}
