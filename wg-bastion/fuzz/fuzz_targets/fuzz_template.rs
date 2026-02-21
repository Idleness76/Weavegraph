#![no_main]
use libfuzzer_sys::fuzz_target;
use wg_bastion::prompt::template::SecureTemplate;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Fuzz compile — should never panic
        if let Ok(template) = SecureTemplate::compile(s) {
            // Fuzz render with the template's own placeholders as keys
            let values: Vec<(String, String)> = template
                .placeholders()
                .iter()
                .map(|p| (p.name().to_string(), "test_value".to_string()))
                .collect();
            let _ = template.render(values);
        }
    }
});
