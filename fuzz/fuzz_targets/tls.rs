#![no_main]

use libfuzzer_sys::fuzz_target;

// TLS ClientHello + DER cert-chain parsers.
fuzz_target!(|data: &[u8]| {
    let _ = rak_stdlib::tls::parse_client_hello(data);
    let _ = rak_stdlib::tls::parse_cert_chain(data);
});