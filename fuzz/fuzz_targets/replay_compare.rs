#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::json;
use weavegraph::event_bus::Event;
use weavegraph::runtimes::{
    ReplayRun, compare_event_sequences, compare_event_sequences_with, compare_replay_runs,
};
use weavegraph::state::VersionedState;

fn events_from_bytes(data: &[u8]) -> Vec<Event> {
    data.chunks(8)
        .take(32)
        .enumerate()
        .map(|(index, chunk)| {
            Event::diagnostic(
                format!("scope-{index}"),
                String::from_utf8_lossy(chunk).to_string(),
            )
        })
        .collect()
}

fuzz_target!(|data: &[u8]| {
    let midpoint = data.len() / 2;
    let left_events = events_from_bytes(&data[..midpoint]);
    let right_events = events_from_bytes(&data[midpoint..]);

    compare_event_sequences(&left_events, &left_events)
        .assert_matches()
        .expect("event comparison must be reflexive");
    let _ = compare_event_sequences(&left_events, &right_events);
    let _ = compare_event_sequences_with(&left_events, &right_events, |_| json!("ignored"));

    let left_state = VersionedState::builder()
        .with_extra("bytes", json!(data.len()))
        .build();
    let right_state = VersionedState::builder()
        .with_extra("bytes", json!(midpoint))
        .build();
    let left_run = ReplayRun::new(left_state.clone(), left_events.clone());
    let same_run = ReplayRun::new(left_state, left_events);
    let right_run = ReplayRun::new(right_state, right_events);

    compare_replay_runs(&left_run, &same_run)
        .assert_matches()
        .expect("replay comparison must be reflexive");
    let _ = compare_replay_runs(&left_run, &right_run);
});
