#![no_main]

use libfuzzer_sys::fuzz_target;

// RFC 6455 WebSocket frame parser.
fuzz_target!(|data: &[u8]| {
    let _ = rak_stdlib::websocket::parse_frame(data);
    // Handshake response derivation must not panic on arbitrary sec-websocket-key.
    let key = String::from_utf8_lossy(&data[..data.len().min(64)]);
    let _ = rak_stdlib::websocket::accept_value(&key);
});