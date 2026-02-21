#![no_main]
use libfuzzer_sys::fuzz_target;
use wg_bastion::input::normalization::NormalizationStage;
use wg_bastion::pipeline::content::Content;
use wg_bastion::pipeline::stage::{GuardrailStage, SecurityContext};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let stage = NormalizationStage::with_defaults();
        let content = Content::Text(s.to_string());
        let ctx = SecurityContext::default();
        // Should never panic on any UTF-8 input
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = rt.block_on(stage.evaluate(&content, &ctx));
    }
});
