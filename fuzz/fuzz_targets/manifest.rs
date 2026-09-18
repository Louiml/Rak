#![no_main]

use libfuzzer_sys::fuzz_target;

// Pakcage-manifest parser: fuzz `parse_manifest_str` with arbitrary text to find
// panics / infinite loops / overflows in manifest parsing.
fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = rakpkg::parse_manifest_str(&text);
});