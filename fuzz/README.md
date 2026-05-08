# Fuzz Targets

These targets are intended for `cargo-fuzz` and are kept outside the normal crate build.

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run event_json
cargo +nightly fuzz run replay_compare
cargo +nightly fuzz run state_slots
```

The targets cover event JSON decoding/normalization, replay comparison helpers, and typed state slot serialization boundaries.
