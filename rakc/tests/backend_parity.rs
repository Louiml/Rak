//! Backend parity: the interpreter and the bytecode VM are two implementations
//! of one language, so every builtin and every language feature must exist
//! twice.
//!
//! v8.0.0 shipped nineteen builtins that had been registered only in the VM.
//! `rakc run` — the default backend — failed with `Unknown function` for every
//! one of them, and the existing test suite passed, because it exercised each
//! backend separately and nothing covered the new builtins on the interpreter
//! side at all. The unit tests could not have caught it.
//!
//! These tests compare the two backends against each other instead. There are
//! two kinds:
//!
//! * `registrations_match` — a structural check. It reads the registration
//!   sites in both backends and requires the name sets to be identical. A
//!   builtin added to one side and forgotten on the other fails here, at
//!   compile-and-test time, with the name in the message.
//! * `parity_*` — behavioural checks. They run a program on both backends and
//!   require identical output, which catches not just missing builtins but any
//!   case where the two implementations disagree.

use std::collections::BTreeSet;

/// The interpreter's builtin registrations, read from source.
///
/// The tree walker dispatches with one large `match name` in `eval_builtin`,
/// plus the extension tables in `ext_batteries` and `ext_osint` which
/// dispatch through per-pack sub-functions. Both shapes reduce to a string
/// literal followed by `=>`, so one pattern covers them.
///
/// The interpreter scan is deliberately limited to the body of `eval_builtin`.
/// A whole-file scan also picks up the *method* arms on `regex` values —
///
/// ```ignore
/// "find" => { /* regex.find(), not the builtin */ }
/// ```
///
/// — and a gate that reports those as missing builtins is a gate people learn
/// to ignore. `eval_builtin` is delimited by its signature and by the
/// `Unknown function` arm that terminates its match, so the slice is exact
/// rather than heuristic.
fn interpreter_registrations() -> BTreeSet<String> {
    let mut names = BTreeSet::new();

    let body = include_str!("../src/interpreter.rs");
    let start = body.find("fn eval_builtin").expect("eval_builtin exists");
    let end = body[start..]
        .find("Unknown function")
        .map(|i| start + i)
        .expect("eval_builtin terminates with an Unknown function arm");
    collect_arms(&body[start..end], &mut names);

    // The extension tables are separate `match name` blocks reached from
    // `try_interp`. They are scanned whole; every `"name" =>` arm in those
    // files is a builtin dispatch, not a method arm.
    for src in [
        include_str!("../src/ext_batteries.rs"),
        include_str!("../src/ext_osint.rs"),
    ] {
        collect_arms(src, &mut names);
    }
    names
}

/// Collect `"some_name" =>` match arms from `src` into `names`.
///
/// Only a leading quote, a plain-identifier name, and an immediate `=>` count,
/// so call sites, assertions and string data are not mistaken for arms.
fn collect_arms(src: &str, names: &mut BTreeSet<String>) {
    for line in src.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('"') {
            continue;
        }
        let rest = &trimmed[1..];
        let Some(end) = rest.find('"') else { continue };
        let name = &rest[..end];
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        if rest[end + 1..].trim_start().starts_with("=>") {
            names.insert(name.to_string());
        }
    }
}

/// The VM's native registrations, read from source.
///
/// Two shapes: `self.insert_native("name", ...)` in `register_natives`, and
/// `("name", vm_fn)` entries in the extension tables.
fn vm_registrations() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let insert_native = "insert_native(\"";
    let table_entry = "(\"";
    for path in VM_NATIVE_SOURCES {
        let src: &str = match *path {
            "../src/vm.rs" => include_str!("../src/vm.rs"),
            "../src/ext_batteries.rs" => include_str!("../src/ext_batteries.rs"),
            "../src/ext_osint.rs" => include_str!("../src/ext_osint.rs"),
            "../src/ext_stdlib.rs" => include_str!("../src/ext_stdlib.rs"),
            other => panic!("add {} to this match when adding a native table", other),
        };
        let mut rest = src;
        while let Some(at) = rest.find(insert_native) {
            let after = &rest[at + insert_native.len()..];
            if let Some(end) = after.find('"') {
                names.insert(after[..end].to_string());
            }
            rest = &rest[at + insert_native.len()..];
        }
        // Extension tables:  ("time_now", vm_time_now),
        // Only collect entries that name a `vm_`-prefixed function, which is
        // the convention every table entry follows. That keeps this from
        // scraping unrelated two-string tuples out of the same files.
    let mut rest = src;
    while let Some(at) = rest.find(table_entry) {
        let after = &rest[at + table_entry.len()..];
        let Some(end) = after.find('"') else {
            rest = &rest[at + table_entry.len()..];
            continue;
        };
        let entry = &after[..end];
        let after_name = &after[end + 1..];
        let looks_like_table = after_name
            .trim_start()
            .trim_start_matches(',')
            .trim_start()
            .starts_with("vm_");
        if looks_like_table {
            names.insert(entry.to_string());
        }
        rest = &rest[at + table_entry.len()..];
        }
    }
    names
}

/// Every source file that registers a native for the VM.
///
/// This list has to be kept in step with the `for (n, f) in ...::vm_natives()`
/// loops in `Vm::register_natives`. A registration table that is not listed
/// here would make the gate blind to that whole family — the failure mode is
/// silent, so the reminder is a compile-time checked constant rather than a
/// comment.
const VM_NATIVE_SOURCES: &[&str] = &[
    "../src/vm.rs",
    "../src/ext_batteries.rs",
    "../src/ext_osint.rs",
    "../src/ext_stdlib.rs",
];

#[test]
fn registrations_match() {
    let interp = interpreter_registrations();
    let vm = vm_registrations();

    // A handful of names are deliberately not builtins on both sides. Each
    // entry is a name and why it is allowed to differ, so that adding to this
    // list is a deliberate act rather than a way to silence a regression.
    const INTERP_ONLY: &[(&str, &str)] = &[];
    const VM_ONLY: &[(&str, &str)] = &[];

    let interp_only: Vec<&String> = interp
        .iter()
        .filter(|n| !vm.contains(*n) && !INTERP_ONLY.iter().any(|(k, _)| *k == n.as_str()))
        .collect();
    let vm_only: Vec<&String> = vm
        .iter()
        .filter(|n| !interp.contains(*n) && !VM_ONLY.iter().any(|(k, _)| *k == n.as_str()))
        .collect();

    assert!(
        interp_only.is_empty() && vm_only.is_empty(),
        "the two backends do not register the same builtins.\n\
         \n  interpreter only ({}): {}\n  \n  VM only ({}): {}\n  \n\
         A builtin that exists on one backend only means `rakc run` and `rakc vm` \
         are different languages. Add the missing registration, or if the divergence \
         is intentional and documented, record it in INTERP_ONLY / VM_ONLY above \
         with a reason.",
        interp_only.len(),
        interp_only
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        vm_only.len(),
        vm_only
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
}

/// The parity gap is large enough that it needs its own budget and its own
/// trend line, not just a pass/fail assertion.
///
/// `registrations_match` is the gate: once the gap is closed it stays closed.
/// While it is open, this test records exactly what is still missing so the
/// backlog is measurable and cannot quietly grow, and so closing a family is
/// visible in the diff.
#[test]
#[ignore = "backlog tracker; run explicitly with --ignored while the gap is open"]
fn parity_backlog_report() {
    let interp = interpreter_registrations();
    let vm = vm_registrations();
    let interp_only: Vec<&str> = interp
        .iter()
        .filter(|n| !vm.contains(*n))
        .map(|s| s.as_str())
        .collect();
    let vm_only: Vec<&str> = vm
        .iter()
        .filter(|n| !interp.contains(*n))
        .map(|s| s.as_str())
        .collect();

    println!("interpreter-only: {}", interp_only.len());
    println!("vm-only: {}", vm_only.len());
    println!("interpreter-only names: {}", interp_only.join(" "));
    println!("vm-only names: {}", vm_only.join(" "));
}

// ---------------------------------------------------------------------------
// Behavioural parity
// ---------------------------------------------------------------------------

/// Run a program on both backends and require identical output.
///
/// This is the check that would have caught the v8.0.0 regression: the
/// builtins below are simply not callable on the interpreter in that release,
/// so `assert_backends_agree` fails on the interpreter side.
fn agree(label: &str, src: &str) -> Vec<String> {
    rakc::assert_backends_agree(label, src)
}

#[test]
fn parity_v8_crypto_builtins() {
    agree(
        "constant-time helpers",
        r#"
dump ct_eq(b"k", b"k")
dump ct_eq(b"k", b"j")
dump ct_eq_hex("DEADBEEF", "deadbeef")
dump ct_select(1, b"\xAA\xAA", b"\xBB\xBB")
dump ct_select(0, b"\xAA\xAA", b"\xBB\xBB")
dump zeroize(b"\x01\x02\x03\x04")
"#,
    );
}

#[test]
fn parity_v8_rsa() {
    // Round-trip through RSA. Key generation is seeded from the OS RNG in the
    // VM, so the program is written to print only facts that hold for any key.
    agree(
        "rsa round trip",
        r#"
let (pk, sk) = rsa_keypair(2048)
let msg = b"rak v8.1"
let sig = rsa_sign(sk, msg)
dump rsa_verify(pk, sig, msg)
dump rsa_verify(pk, sig, b"tampered")
let ct = rsa_encrypt(pk, b"topsecret", b"")
dump rsa_decrypt(sk, ct, b"")
"#,
    );
}

#[test]
fn parity_v8_ecdsa() {
    agree(
        "ecdsa round trip",
        r#"
let (pk, sk) = ecdsa_keypair()
let msg = b"rak v8.1"
dump ecdsa_verify(pk, ecdsa_sign(sk, msg), msg)
dump ecdsa_verify(pk, ecdsa_sign(sk, msg), b"other")
"#,
    );
}

#[test]
fn parity_v8_packet_builders() {
    // These only construct bytes and open no socket, so they run identically
    // on both backends without privileges.
    agree(
        "icmp and arp builders",
        r#"
let ping = net_raw_icmp_ping("10.0.0.5", "10.0.0.10", 1, 1)
dump len(ping)
dump ping[0]
let reply = net_raw_arp_reply("aa:bb:cc:dd:ee:ff", "10.0.0.10", "11:22:33:44:55:66", "10.0.0.5")
let parsed = net_raw_arp_parse(reply)
dump parsed["opcode"]
dump parsed["sender_mac"]
dump parsed["target_ip"]
"#,
    );
}

#[test]
fn parity_ext_batteries() {
    agree(
        "extension battery natives",
        r#"
dump time_parts(0)["year"]
dump len(rand_bytes(16))
dump sha256("abc")
dump md5("abc")
"#,
    );
}

#[test]
fn parity_core_language() {
    agree(
        "core language surface",
        r#"
fn add(a, b) { return a + b } requires a >= 0 ensures result == a + b
dump add(2, 3)
let xs = [1, 2, 3]
dump xs[0]
dump len(xs)
for i in [1, 2, 3] { dump i }
dump match 3 { 1 => "one", _ => "other" }
let m = {"a": 1}
dump m["a"]
if add(1, 1) > 1 { dump "ab" + "c" }
"#,
    );
}

/// The stdlib surface both backends share today.
///
/// This is `#[ignore]`d rather than passing because it is not yet true. The VM
/// is missing 158 of the interpreter's builtins, so most of the standard
/// library simply does not exist under `rakc vm` — `to_string` is the smallest
/// example, and it is a builtin every language is expected to have.
///
/// Un-ignore this as families land. `registrations_match` is the gate that
/// fails until the whole list is closed; this test is the behavioural
/// counterpart, so that closing the gap in the registry is not enough — the
/// implementations have to agree too.
#[test]
#[ignore = "VM is missing 158 interpreter builtins; see registrations_match"]
fn parity_stdlib_surface() {
    agree(
        "stdlib surface",
        r#"
dump to_string(42)
dump abs(0 - 7)
dump max(1, 2)
dump min(1, 2)
dump pow(2, 10)
dump sqrt(16.0)
dump "a,b,c" |> split(",") |> join("|")
dump "hello" |> replace("l", "L")
dump "hello" |> starts_with("he")
dump "hello" |> substr(1, 3)
dump [3, 1, 2] |> sort()
dump [1, 2] |> reverse()
dump ord("A")
dump chr(66)
"#,
    );
}

/// Stream builtins, which are spec 7A.4 and are interpreter-only today.
#[test]
#[ignore = "spec 7A.4: VM has no Value::Stream and no stream natives"]
fn parity_streams() {
    agree(
        "streams",
        r#"
let s = stream_from_array([1, 2, 3, 4])
dump collect(filter(s, fn(x) { x % 2 == 0 }))
"#,
    );
}

/// Encrypted UDP, which is spec 7A.5 and is interpreter-only today.
#[test]
#[ignore = "spec 7A.5: VM has no Value::UdpTransport and no udp_* natives"]
fn parity_udp() {
    agree(
        "udp bind and local address",
        r#"
let (t, addr) = udp_bind("127.0.0.1:0")
dump len(addr) > 0
dump udp_local_addr(t) == addr
"#,
    );
}

/// GUI, which is interpreter-only today. See the GUI rewrite.
#[test]
#[ignore = "VM has no GUI natives; rakc vm reports Undefined: gui_open"]
fn parity_gui_surface() {
    // GUI cannot open a real window in a test, so this asserts the *registry*
    // instead: the same builtin names must exist on both backends. A VM that
    // cannot open a window should still fail the same way the interpreter
    // fails when no display is available, rather than reporting a missing
    // builtin, which is a different bug wearing the same coat.
    agree("gui registry", "gui_wait()");
}
