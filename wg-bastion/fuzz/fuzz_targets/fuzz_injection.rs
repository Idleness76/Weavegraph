#![no_main]
use libfuzzer_sys::fuzz_target;
use wg_bastion::input::injection::HeuristicDetector;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(detector) = HeuristicDetector::with_defaults() {
            // Should never panic, produce consistent results
            let result1 = detector.detect(s);
            let result2 = detector.detect(s);
            assert_eq!(result1.len(), result2.len(), "Non-deterministic detection");
        }
    }
});
