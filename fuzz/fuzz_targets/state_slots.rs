#![no_main]

use libfuzzer_sys::fuzz_target;
use serde::{Deserialize, Serialize};
use serde_json::json;
use weavegraph::node::NodePartial;
use weavegraph::state::{StateKey, VersionedState};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FuzzPayload {
    label: String,
    amount: i64,
    bytes: Vec<u8>,
}

const FUZZ_SLOT: StateKey<FuzzPayload> = StateKey::new("fuzz", "payload", 1);

fn amount_from_bytes(data: &[u8]) -> i64 {
    let mut bytes = [0_u8; 8];
    let len = data.len().min(bytes.len());
    bytes[..len].copy_from_slice(&data[..len]);
    i64::from_le_bytes(bytes)
}

fuzz_target!(|data: &[u8]| {
    let split = data.len().min(32);
    let payload = FuzzPayload {
        label: String::from_utf8_lossy(&data[..split]).to_string(),
        amount: amount_from_bytes(data),
        bytes: data.iter().copied().take(64).collect(),
    };

    let state = VersionedState::builder()
        .with_typed_extra(FUZZ_SLOT, payload.clone())
        .expect("fuzz payload should serialize")
        .build();
    assert_eq!(
        state
            .snapshot()
            .require_typed(FUZZ_SLOT)
            .expect("fuzz payload should deserialize"),
        payload
    );

    let partial = NodePartial::new()
        .with_typed_extra(FUZZ_SLOT, payload)
        .expect("fuzz payload should serialize into partial");
    if let Some(extra) = partial.extra {
        assert!(extra.contains_key(&FUZZ_SLOT.storage_key()));
    }

    let invalid_state = VersionedState::builder()
        .with_extra(&FUZZ_SLOT.storage_key(), json!(String::from_utf8_lossy(data).to_string()))
        .build();
    let _ = invalid_state.snapshot().get_typed::<FuzzPayload>(FUZZ_SLOT);
});
