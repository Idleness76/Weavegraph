# Fuzz Testing for wg-bastion

## Prerequisites

Install cargo-fuzz (requires nightly):
```bash
cargo install cargo-fuzz
```

## Running Fuzz Targets

```bash
cd wg-bastion

# Template parsing (60 seconds)
cargo +nightly fuzz run fuzz_template -- -max_total_time=60

# Injection detection (60 seconds)
cargo +nightly fuzz run fuzz_injection -- -max_total_time=60

# Normalization (60 seconds)
cargo +nightly fuzz run fuzz_normalization -- -max_total_time=60
```

## Targets

| Target | What it fuzzes | Key properties |
|--------|---------------|----------------|
| `fuzz_template` | `SecureTemplate::compile()` + `render()` | No panics, no OOM |
| `fuzz_injection` | `HeuristicDetector::detect()` | No panics, deterministic |
| `fuzz_normalization` | `NormalizationStage::evaluate()` | No panics on any UTF-8 input |
