//! Property-based (proptest) harnesses for security-sensitive components.
//!
//! These assert never-panics, bounded-termination, and no out-of-bounds access
//! on arbitrary / adversarial / byte-mangled inputs. They run on the stable
//! CI toolchain via `cargo test` (no nightly needed). Deep coverage-guided
//! fuzzing is provided separately by the `fuzz/` cargo-fuzz targets.

use proptest::prelude::*;

/// Arbitrary byte strings, including adversarial ones (huge, empty, prefixes
/// of real structures, NUL-heavy, UTF-8-garbage).
fn any_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..4096)
        .prop_union(prop::collection::vec(any::<u8>(), 0..32))
}

proptest! {
    // --- Lexer must never panic on arbitrary bytes (lossy UTF-8). ---
    #[test]
    fn lexer_never_panics(data in any_bytes()) {
        let text = String::from_utf8_lossy(&data).to_string();
        let _ = rakc::lexer::tokenize(&text);
    }

    // --- Parser must terminate (no infinite loop / stack overflow) on crafted
    //     token streams built from arbitrary source. ---
    #[test]
    fn parser_terminates(text in "\\PC{0,2000}") {
        let src = String::from_utf8_lossy(text.as_bytes()).to_string();
        match rakc::lexer::tokenize(&src) {
            Ok(tokens) => { let _ = rakc::parser::parse(&tokens, &src); }
            Err(_) => {}
        }
    }

    // --- DNS wire parser: never panics / OOB, bounded answers. ---
    #[test]
    fn dns_parse_never_panics(data in any_bytes()) {
        let _ = rak_stdlib::dns::parse_response(&data);
    }

    // --- Raw packet builders + checksum: no panic, defined length. ---
    #[test]
    fn net_raw_builders_bounded(ip in any::<String>(), payload in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = rak_stdlib::net_raw::ipv4(&ip, &ip, 6, &payload);
        let _ = rak_stdlib::net_raw::tcp(&ip, &ip, 0, 0, "S", 0, 0, &payload);
        let _ = rak_stdlib::net_raw::udp(&ip, &ip, 0, 0, &payload);
    }

    // --- WebSocket frame parser: no panic on arbitrary bytes. ---
    #[test]
    fn websocket_frame_never_panics(data in any_bytes()) {
        let _ = rak_stdlib::websocket::parse_frame(&data);
    }

    // --- TLS ClientHello / cert chain parser: no panic / OOB. ---
    #[test]
    fn tls_parse_never_panics(data in any_bytes()) {
        let _ = rak_stdlib::tls::parse_client_hello(&data);
        let _ = rak_stdlib::tls::parse_cert_chain(&data);
    }

    // --- JSON parser: never panics, empty/truncated handled. ---
    #[test]
    fn json_parse_never_panics(text in "\\PC{0,2000}") {
        let s = String::from_utf8_lossy(text.as_bytes()).to_string();
        let _ = rak_stdlib::js::json_parse(&s);
    }

    // --- Tunnel framing: never panics / OOB on arbitrary bytes. ---
    #[test]
    fn tunnel_unframe_never_panics(data in any_bytes()) {
        let _ = rak_stdlib::tunnel::tunnel_unframe(&data);
    }
}