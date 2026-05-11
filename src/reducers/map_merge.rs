//! Reducer that shallow-merges incoming extra key-value pairs into the extras channel.
//!
//! Follows the [JSON Merge Patch](https://www.rfc-editor.org/rfc/rfc7396) convention:
//! an incoming `null` value **removes** the key from state rather than setting it to null.
//! This is what makes [`NodePartial::clear_extra_keys`](crate::node::NodePartial::clear_extra_keys)
//! and [`NodePartial::clear_typed_extra_key`](crate::node::NodePartial::clear_typed_extra_key)
//! functional without requiring a separate cleanup reducer.
use super::Reducer;
use crate::{channels::Channel, node::NodePartial, state::VersionedState};

/// Reducer that merges extra key-value pairs from a [`NodePartial`](crate::node::NodePartial) into the state extras channel.
///
/// Uses JSON Merge Patch semantics (RFC 7396): an incoming `null` value **removes** the
/// key from state rather than writing a null entry. This means
/// [`NodePartial::clear_extra_keys`](crate::node::NodePartial::clear_extra_keys) and
/// [`NodePartial::clear_typed_extra_key`](crate::node::NodePartial::clear_typed_extra_key)
/// fully delete the key — no separate cleanup reducer is needed.
#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub struct MapMerge;
impl Reducer for MapMerge {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        if let Some(extras_update) = &update.extra
            && !extras_update.is_empty()
        {
            let state_map = state.extra.get_mut();
            for (k, v) in extras_update.iter() {
                if v.is_null() {
                    state_map.remove(k);
                } else {
                    state_map.insert(k.clone(), v.clone());
                }
            }
        }
    }
}
