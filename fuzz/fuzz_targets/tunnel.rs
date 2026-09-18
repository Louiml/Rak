#![no_main]

use libfuzzer_sys::fuzz_target;

// Tunnel framing + AEAD helpers: never panic / OOB on arbitrary key/sizes.
fuzz_target!(|data: &[u8]| {
    let _ = rak_stdlib::tunnel::tunnel_unframe(data);
    let key = &data[..data.len().min(32)];
    let nonce = &data[..data.len().min(12)];
    let _ = rak_stdlib::tunnel::x25519_keypair(key);
    let _ = rak_stdlib::tunnel::chacha20_decrypt(key, nonce, b"aad", data);
    // derived key for arbitrary passphrase
    let _ = rak_stdlib::tunnel::psk_derive(&String::from_utf8_lossy(data), b"salt", 2, 32);
});