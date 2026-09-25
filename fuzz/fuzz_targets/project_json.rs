#![no_main]

use aria_models::Project;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = Project::load_from_string(text);
    }
});
