#![no_main]

use libfuzzer_sys::fuzz_target;
use weavegraph::event_bus::{Event, NodeEvent};
use weavegraph::runtimes::normalize_event;

fuzz_target!(|data: &[u8]| {
    if let Ok(event) = serde_json::from_slice::<Event>(data) {
        let _ = event.scope_label();
        let _ = event.message();
        let _ = event.to_json_value();
        let _ = event.to_json_string();
        let _ = event.to_json_pretty();
        let _ = normalize_event(&event);
    }

    if let Ok(node_event) = serde_json::from_slice::<NodeEvent>(data) {
        let event = Event::Node(node_event);
        let _ = event.to_json_value();
        let _ = normalize_event(&event);
    }
});
