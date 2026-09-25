#![no_main]

use aria_core::Rational;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<Rational>(data);
});
