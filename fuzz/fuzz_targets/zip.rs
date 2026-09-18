#![no_main]

use libfuzzer_sys::fuzz_target;

// ZIP archive listing / extraction of arbitrary bytes.
fuzz_target!(|data: &[u8]| {
    if let Ok(names) = rak_stdlib::stream_io::zip_list(data) {
        for n in names.iter().take(8) {
            let _ = rak_stdlib::stream_io::zip_extract(data, n);
        }
    }
});