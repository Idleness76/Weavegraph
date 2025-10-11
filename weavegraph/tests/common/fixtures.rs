use serde_json::Value;
use weavegraph::channels::Channel;
use weavegraph::state::{StateSnapshot, VersionedState};

pub fn empty_snapshot() -> StateSnapshot {
    VersionedState::builder().build().snapshot()
}

pub fn empty_state() -> VersionedState {
    VersionedState::builder().build()
}

pub fn state_with_user(msg: &str) -> VersionedState {
    VersionedState::new_with_user_message(msg)
}

pub fn state_with_extra(pairs: &[(&str, Value)]) -> VersionedState {
    let mut st = empty_state();
    for (k, v) in pairs {
        st.extra.get_mut().insert((*k).into(), v.clone());
    }
    st
}
