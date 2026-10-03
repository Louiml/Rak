//! `rakc fuzz` — property-based fuzzing for the parser and the stdlib
//! parsers, on the stable toolchain.
//!
//! The `fuzz/` directory holds 13 `cargo-fuzz` targets that give real
//! coverage-guided fuzzing. Those need nightly and
//! `-fsanitize=fuzzer`, which is not available on `stable-x86_64-pc-windows-msvc`,
//! so nothing runs them automatically on a Windows dev box or in the ordinary
//! CI job. This module covers the same entry points with a deterministic
//! mutation loop that compiles and runs anywhere.
//!
//! It is not a substitute for coverage guidance. It is a cheap regression net
//! that catches a panic, an out-of-bounds read, or an unbounded loop the
//! moment someone changes a parser, which is the failure that actually happens.
//!
//! # Determinism
//!
//! `--seed` makes a run reproducible: the same seed explores the same inputs in
//! the same order, so a crash can be replayed. Crashing input is written to the
//! path given by `--out` (default `fuzz-crash-<target>.bin`).
//!
//! # Panics
//!
//! Each iteration runs inside `catch_unwind`, which needs `panic = "unwind"`.
//! The release profile uses `panic = "abort"` for FFI safety, so run this from
//! a debug build: `cargo run -p rakc -- fuzz <target>`.

use std::io::Write;

/// Deterministic xorshift64* PRNG. No dependency, identical on every platform,
/// so a seed reproduces a run exactly.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // A zero state would be absorbing, so force a nonzero one and step it
        // once so the first output is already mixed.
        let mut r = Rng(seed ^ 0x9E3779B97F4A7C15);
        r.next_u64();
        r
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

/// Bytes that parsers commonly mishandle when they appear where a length field
/// or a magic number is expected.
const INTERESTING: &[u8] = &[
    0x00, 0x01, 0x7f, 0x80, 0xfe, 0xff, 0x0a, 0x0d, 0x1b, b'"', b'\\', b'{', b'}', b'[', b']',
    b',', b':', b'=', b'/', b'\\', b'%', 0x20, 0x09,
];

/// Apply one random mutation to `buf`. Bit flips, byte replacement, insertion,
/// deletion, duplication, and a splice with an "interesting" value. All keep
/// the length roughly stable so the corpus does not drift toward empty.
fn mutate(rng: &mut Rng, buf: &mut Vec<u8>, max_len: usize) {
    // Occasionally grow a very short buffer, so the corpus is not all zeros.
    if buf.is_empty() || rng.below(16) == 0 {
        buf.push(rng.below(256) as u8);
    }
    match rng.below(7) {
        0 => {
            // Flip a single bit.
            let i = rng.below(buf.len());
            buf[i] ^= 1 << rng.below(8);
        }
        1 => {
            // Replace a byte outright.
            let i = rng.below(buf.len());
            buf[i] = rng.below(256) as u8;
        }
        2 => {
            // Splice in a value parsers branch on.
            let i = rng.below(buf.len());
            buf[i] = INTERESTING[rng.below(INTERESTING.len())];
        }
        3 => {
            // Insert.
            if buf.len() < max_len {
                let i = rng.below(buf.len() + 1);
                buf.insert(i, rng.below(256) as u8);
            }
        }
        4 => {
            // Delete a run, but never empty the buffer.
            if buf.len() > 1 {
                let i = rng.below(buf.len());
                let n = 1 + rng.below(4.min(buf.len() - i));
                buf.drain(i..i + n);
            }
        }
        5 => {
            // Duplicate a run, which tends to produce valid-looking structures
            // with inconsistent length prefixes.
            if buf.len() < max_len {
                let i = rng.below(buf.len());
                let n = 1 + rng.below(16.min(buf.len() - i));
                let chunk: Vec<u8> = buf[i..i + n].to_vec();
                let at = rng.below(buf.len() + 1);
                buf.splice(at..at, chunk);
            }
        }
        _ => {
            // Flip a whole 16-bit or 32-bit field at a word boundary-ish offset,
            // which reaches length and count fields.
            let i = rng.below(buf.len());
            let v = if rng.below(2) == 0 { 0xFFFFu16 } else { 0x7FFF } as u8;
            buf[i] = v;
            if i + 1 < buf.len() {
                buf[i + 1] = v;
            }
        }
    }
}

/// Seed corpora, so the first iterations start from structured input rather
/// than random noise. Random bytes mostly produce an immediate parse error,
/// which explores very little.
fn seed_corpus(target: &str) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    match target {
        "lex" | "parse" | "eval" => {
            out.push(b"fn main() { dump 1 }".to_vec());
            out.push(b"let x = [1, 2, 3]\nfor i in x { dump i }".to_vec());
            out.push(b"let s = f\"a{b}c\"\ndump s".to_vec());
            out.push(b"match 1 { 1 => { dump 1 } _ => { dump 0 } }".to_vec());
            out.push(b"struct P { x: int }\nlet p = P { x: 1 }".to_vec());
        }
        "dns" => {
            out.push(vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
            out.push(vec![0, 0, 0x84, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1]);
        }
        "tls" => {
            out.push(vec![0x16, 0x03, 0x01, 0x00, 0x2a, 0x01, 0x00, 0x00, 0x26]);
            out.push(vec![0x30, 0x82, 0x01, 0x0a, 0x02, 0x82, 0x01, 0x01]);
        }
        "json" => out.push(br#"{"a":[1,2,{"b":"c"}],"d":null}"#.to_vec()),
        "websocket" => {
            out.push(vec![0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d]);
            out.push(b"dGhlIHNhbXBsZSBub25jZQ==".to_vec());
        }
        "tunnel" => {
            out.push(vec![0x52, 0x41, 0x4b, 0x21, 0, 0, 0, 1]);
            out.push(b"passphrase".to_vec());
        }
        "netraw" => {
            out.push(b"10.0.0.5".to_vec());
            out.push(b"aa:bb:cc:dd:ee:ff".to_vec());
        }
        "csv" => out.push(b"a,b,\"c,d\",e\r\n1,2,3,4".to_vec()),
        "gzip" => out.push(vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0x03]),
        "zip" => out.push(b"PK\x03\x04\x14\x00\x00\x00\x00\x00".to_vec()),
        _ => {}
    }
    out
}

/// The body under test. Every one of these must return normally for arbitrary
/// input; an `Err` is a perfectly good answer, a panic is a bug.
fn exercise(target: &str, data: &[u8]) {
    let lossy = String::from_utf8_lossy(data).to_string();
    let head = &data[..data.len().min(64)];
    match target {
        "lex" => {
            let _ = crate::lexer::tokenize(&lossy);
        }
        "parse" => {
            if let Ok(toks) = crate::lexer::tokenize(&lossy) {
                let _ = crate::parser::parse(&toks, &lossy);
            }
        }
        "eval" => {
            // Cap the input so a deeply nested expression cannot make the tree
            // walker blow the Rust stack. Same bound the cargo-fuzz target uses.
            if data.len() <= 2048 {
                let _ = crate::eval(&lossy);
            }
        }
        "dns" => {
            let _ = rak_stdlib::dns::parse_response(data);
            let _ = rak_stdlib::dns::parse_response(&data[..data.len().min(12)]);
        }
        "tls" => {
            let _ = rak_stdlib::tls::parse_client_hello(data);
            let _ = rak_stdlib::tls::parse_cert_chain(data);
        }
        "json" => {
            if let Ok(v) = rak_stdlib::js::json_parse(&lossy) {
                let pretty = rak_stdlib::js::json_stringify(&v);
                let _ = rak_stdlib::js::json_keys(&pretty);
                let _ = rak_stdlib::js::json_get(&pretty, "0");
                let _ = rak_stdlib::js::json_path(&pretty, "a.b");
                let _ = rak_stdlib::js::json_find_all(&pretty, "x");
            }
        }
        "websocket" => {
            let _ = rak_stdlib::websocket::parse_frame(data);
            let _ = rak_stdlib::websocket::accept_value(&String::from_utf8_lossy(head));
        }
        "tunnel" => {
            let _ = rak_stdlib::tunnel::tunnel_unframe(data);
            let key = &data[..data.len().min(32)];
            let nonce = &data[..data.len().min(12)];
            let _ = rak_stdlib::tunnel::x25519_keypair(key);
            let _ = rak_stdlib::tunnel::chacha20_decrypt(key, nonce, b"aad", data);
            let _ = rak_stdlib::tunnel::psk_derive(&lossy, b"salt", 2, 32);
        }
        "netraw" => {
            let ip = String::from_utf8_lossy(&data[..data.len().min(16)]);
            let payload = &data[..data.len().min(256)];
            let _ = rak_stdlib::net_raw::csum16(data);
            let _ = rak_stdlib::net_raw::ipv4(&ip, &ip, 6, payload);
            let _ = rak_stdlib::net_raw::tcp(&ip, &ip, 1234, 80, "S", 1, 0, payload);
            let _ = rak_stdlib::net_raw::udp(&ip, &ip, 1234, 53, payload);
            let _ = rak_stdlib::net_raw::tcp_syn(&ip, &ip, 1234, 80);
            let _ = rak_stdlib::net_raw::icmp_echo(&ip, &ip, 1, 1, payload);
            let _ = rak_stdlib::net_raw::arp_request(&ip, &ip, &ip);
            let _ = rak_stdlib::net_raw::arp_parse(data);
        }
        "csv" => {
            let line = String::from_utf8_lossy(data);
            let _ = rak_stdlib::stream_io::parse_csv_line(&line, ',');
            let _ = rak_stdlib::stream_io::parse_csv_line(&line, ';');
            let _ = rak_stdlib::stream_io::parse_jsonl_line(&line);
        }
        "gzip" => {
            let _ = rak_stdlib::stream_io::gzip_decompress(data);
            let _ = rak_stdlib::stream_io::deflate_decompress(data);
        }
        "zip" => {
            if let Ok(names) = rak_stdlib::stream_io::zip_list(data) {
                for n in names.iter().take(8) {
                    let _ = rak_stdlib::stream_io::zip_extract(data, n);
                }
            }
        }
        _ => {}
    }
}

/// Every target `rakc fuzz` accepts.
pub const TARGETS: &[&str] = &[
    "lex",
    "parse",
    "eval",
    "dns",
    "tls",
    "json",
    "websocket",
    "tunnel",
    "netraw",
    "csv",
    "gzip",
    "zip",
    "all",
];

/// Run one target for `runs` iterations. Returns the crashing input, if any.
pub fn fuzz_target(target: &str, runs: u32, seed: u64, max_len: usize) -> Option<Vec<u8>> {
    let mut rng = Rng::new(seed);
    let mut corpus: Vec<Vec<u8>> = seed_corpus(target);
    for i in 0..runs {
        // Round-robin the corpus so every seed gets mutated, rather than
        // hammering whichever one happens to be first.
        let mut input = if corpus.is_empty() {
            Vec::new()
        } else {
            corpus[i as usize % corpus.len()].clone()
        };
        // A few mutations per iteration explores faster than one.
        for _ in 0..1 + rng.below(4) {
            mutate(&mut rng, &mut input, max_len);
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            exercise(target, &input);
        }));
        if result.is_err() {
            return Some(input);
        }
        // Keep a bounded, deduplicated corpus so later iterations start from
        // the most interesting inputs seen rather than only the originals.
        if corpus.len() < 256 {
            if !corpus.contains(&input) {
                corpus.push(input);
            }
        }
    }
    None
}

/// `rakc fuzz <target> [--runs N] [--seed S] [--max-len N] [--out FILE] [--list]`
pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--list" || a == "-l") {
        println!("targets:");
        for t in TARGETS {
            println!("  {}", t);
        }
        return 0;
    }
    let mut target = String::new();
    let mut runs: u32 = 20_000;
    let mut seed: u64 = 0x5241_4B5F; // "RAK_"
    let mut max_len = 4096usize;
    let mut out = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--runs" | "-n" => {
                i += 1;
                runs = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(runs);
            }
            "--seed" | "-s" => {
                i += 1;
                seed = args
                    .get(i)
                    .and_then(|v| {
                        let v = v.trim_start_matches("0x");
                        u64::from_str_radix(v, 16).ok().or_else(|| v.parse().ok())
                    })
                    .unwrap_or(seed);
            }
            "--max-len" => {
                i += 1;
                max_len = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(max_len);
            }
            "--out" | "-o" => {
                i += 1;
                out = args.get(i).cloned().unwrap_or_default();
            }
            other if !other.starts_with('-') && target.is_empty() => {
                target = other.to_string();
            }
            _ => {}
        }
        i += 1;
    }

    if target.is_empty() {
        eprintln!("fuzz: no target given");
        eprintln!(
            "Usage: rakc fuzz <target> [--runs N] [--seed HEX] [--max-len N] [--out FILE] [--list]"
        );
        eprintln!("Targets: {}", TARGETS.join(", "));
        return 2;
    }
    if target != "all" && !TARGETS.contains(&target.as_str()) {
        eprintln!("fuzz: unknown target '{}'", target);
        eprintln!("Targets: {}", TARGETS.join(", "));
        return 2;
    }

    // A panic anywhere prints a backtrace note to stderr by default. Keep it:
    // the message is the whole point of the run.
    let targets: Vec<&str> = if target == "all" {
        TARGETS.iter().copied().filter(|t| *t != "all").collect()
    } else {
        vec![target.as_str()]
    };

    let mut failed = false;
    for t in targets {
        eprint!("fuzz {:<10} {} runs (seed 0x{:X}) ... ", t, runs, seed);
        let _ = std::io::stderr().flush();
        match fuzz_target(t, runs, seed, max_len) {
            None => {
                println!("ok");
            }
            Some(input) => {
                failed = true;
                println!("CRASH");
                let path = if out.is_empty() {
                    format!("fuzz-crash-{}.bin", t)
                } else {
                    out.clone()
                };
                match std::fs::write(&path, &input) {
                    Ok(_) => eprintln!("  wrote {} bytes to {}", input.len(), path),
                    Err(e) => eprintln!("  could not write '{}': {}", path, e),
                }
                eprintln!(
                    "  replay: rakc fuzz {} --seed 0x{:X} --out {}",
                    t, seed, path
                );
            }
        }
        // Vary the seed per target so `all` does not replay the same inputs
        // against every parser.
        seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    }
    if failed {
        eprintln!("fuzz: at least one target crashed");
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_for_a_seed() {
        let a: Vec<u64> = (0..5).map(|_| Rng::new(42).next_u64()).collect();
        // Same seed, same first value, every time.
        for v in &a {
            assert_eq!(*v, a[0]);
        }
        let mut r = Rng::new(7);
        let seq1: Vec<u64> = (0..8).map(|_| r.next_u64()).collect();
        let mut r2 = Rng::new(7);
        let seq2: Vec<u64> = (0..8).map(|_| r2.next_u64()).collect();
        assert_eq!(seq1, seq2);
        assert_ne!(seq1[0], 0, "xorshift must not start at zero");
    }

    #[test]
    fn below_handles_zero_without_panicking() {
        let mut r = Rng::new(1);
        assert_eq!(r.below(0), 0);
        for _ in 0..100 {
            assert!(r.below(5) < 5);
        }
    }

    #[test]
    fn mutation_keeps_the_buffer_bounded() {
        let mut r = Rng::new(99);
        let mut buf = vec![1u8, 2, 3, 4];
        for _ in 0..2000 {
            mutate(&mut r, &mut buf, 64);
            assert!(buf.len() <= 64 + 16, "buffer grew to {}", buf.len());
        }
    }

    #[test]
    fn mutation_actually_changes_the_input() {
        let mut r = Rng::new(5);
        let mut buf = vec![0u8; 32];
        let mut changed = 0;
        for _ in 0..200 {
            let before = buf.clone();
            mutate(&mut r, &mut buf, 64);
            if buf != before {
                changed += 1;
            }
        }
        assert!(
            changed > 150,
            "only {} of 200 mutations changed anything",
            changed
        );
    }

    #[test]
    fn every_target_survives_random_input() {
        // A cheap smoke run so a panic in any harness shows up in `cargo test`
        // rather than only under `rakc fuzz`.
        for t in TARGETS.iter().filter(|t| **t != "all") {
            assert!(
                fuzz_target(t, 200, 0xC0FFEE, 512).is_none(),
                "target {} crashed",
                t
            );
        }
    }

    #[test]
    fn seed_corpus_is_not_empty_for_every_target() {
        for t in TARGETS.iter().filter(|t| **t != "all") {
            assert!(!seed_corpus(t).is_empty(), "no seed corpus for {}", t);
        }
    }
}
