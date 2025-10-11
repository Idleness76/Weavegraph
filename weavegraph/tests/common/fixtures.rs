use weavegraph::state::{StateSnapshot, VersionedState};

pub fn empty_snapshot() -> StateSnapshot {
    VersionedState::builder().build().snapshot()
}
