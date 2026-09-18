#![no_main]

use libfuzzer_sys::fuzz_target;

// CSV + JSONL per-line parsers.
fuzz_target!(|data: &[u8]| {
    let line = String::from_utf8_lossy(data);
    let _ = rak_stdlib::stream_io::parse_csv_line(&line, ',');
    let _ = rak_stdlib::stream_io::parse_csv_line(&line, ';');
    let _ = rak_stdlib::stream_io::parse_jsonl_line(&line);
});