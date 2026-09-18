#![no_main]

use libfuzzer_sys::fuzz_target;

// gzip/zlib decompression of arbitrary bytes. Decompression bombs are guarded by
// libFuzzer's RSS limit; this verifies no panic on malformed/truncated streams.
fuzz_target!(|data: &[u8]| {
    let _ = rak_stdlib::stream_io::gzip_decompress(data);
    let _ = rak_stdlib::stream_io::deflate_decompress(data);
});