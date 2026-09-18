#![no_main]

use libfuzzer_sys::fuzz_target;

// DNS wire-format parser: arbitrary bytes must never panic / read out of bounds.
fuzz_target!(|data: &[u8]| {
    let _ = rak_stdlib::dns::parse_response(data);
    let _ = rak_stdlib::dns::parse_response(&data[..data.len().min(12)]);
});