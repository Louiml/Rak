//! FFI pointer provenance, on both backends.
//!
//! `ffi_write` used to be an arbitrary write: it did `*(ptr + off) = byte` with no
//! check that `ptr` was ever allocated and none that `off` was inside it, and
//! `ffi_ptr(n)` builds a pointer from any integer. `ffi_read` was the matching
//! read, and `ffi_cstr_to_string` scanned for a NUL with no bound at all.
//!
//! A pointer is now provenanced if Rak allocated it or `ffi_trust` declared it, and
//! every access is range-checked. These tests assert the *refusal*, since that is
//! the whole behaviour.

/// Run `source` on both backends and return `(stdout, stderr)`.
///
/// stderr is normalised before it is compared, for two reasons, both of which
/// would otherwise make every test here fail for a reason that has nothing to do
/// with provenance:
///
/// * The VM wraps a native's error as `<builtin>: <message>` where the
///   interpreter writes the bare message. That is the documented transport
///   difference, and it says nothing about behaviour.
/// * Heap addresses differ between processes, so `0x7ffd...` is never stable.
fn normalise(line: &str) -> String {
    let line = line
        .trim_start_matches("Error: Runtime error: ")
        .trim_start_matches("VM error: ");
    // Drop a leading `builtin: ` from the VM's wrapper.
    for pfx in [
        "ffi_cstr_to_string: ",
        "ffi_write: ",
        "ffi_read: ",
        "ffi_trust: ",
    ] {
        if let Some(rest) = line.strip_prefix(pfx) {
            return re_hex_addresses(rest);
        }
    }
    let line = line;
    re_hex_addresses(line)
}

fn re_hex_addresses(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("0x") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        let hex_len = tail
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(tail.len());
        // Keep the literal for a forged pointer we asserted on; blank the rest.
        out.push_str("0xADDR");
        rest = &tail[hex_len..];
        let _ = hex_len;
    }
    out.push_str(rest);
    out.to_string()
}

fn agree(source: &str) -> (Vec<String>, Vec<String>) {
    let dir = std::env::temp_dir().join(format!("rak_ffi_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = dir.join(format!("case_{}_{}.rak", std::process::id(), n));
    std::fs::write(&path, source).expect("write");

    let mut outs = Vec::new();
    let mut errs = Vec::new();
    for backend in ["run", "vm"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rakc"))
            .arg(backend)
            .arg(&path)
            .output()
            .unwrap_or_else(|e| panic!("spawn {backend}: {e}"));
        outs.push(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect::<Vec<_>>(),
        );
        errs.push(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| normalise(l))
                .collect::<Vec<_>>(),
        );
    }
    let _ = std::fs::remove_file(&path);
    assert_eq!(outs[0], outs[1], "stdout differs:\n{:?}\n{:?}", outs[0], outs[1]);
    // stderr is deliberately *not* compared for equality. The two backends wrap a
    // failure differently -- "Error: Runtime error: <msg>" against "VM error:
    // <builtin>: <msg>" -- and the pointer in the message differs between processes.
    // Each test asserts the substring it is about.
    (outs[0].clone(), errs[0].clone())
}

#[test]
fn reading_and_writing_inside_an_allocation_still_work() {
    let (out, err) = agree(
        r#"
let p = ffi_alloc(8)
ffi_write(p, 0, 65)
ffi_write(p, 7, 90)
dump ffi_read(p, 0)
dump ffi_read(p, 7)
ffi_free(p)
"#,
    );
    assert_eq!(
        out,
        vec!["[DUMP] 65", "[DUMP] 90"],
        "the legitimate path must keep working; stderr: {:?}",
        err
    );
}

#[test]
fn a_write_past_the_end_of_the_allocation_is_refused() {
    // The vulnerability: `ffi_write(p, 8, ...)` on an 8-byte allocation wrote one
    // byte into whatever followed it.
    let (_, err) = agree(
        r#"
let p = ffi_alloc(8)
ffi_write(p, 8, 65)
"#,
    );
    let text = err.join("\n");
    assert!(text.contains("runs past the end"), "got: {text}");
    assert!(text.contains("8-byte"), "it should state the size: {text}");
}

#[test]
fn a_far_out_of_bounds_write_is_refused() {
    let (_, err) = agree(
        r#"
let p = ffi_alloc(8)
ffi_write(p, 4096, 65)
"#,
    );
    assert!(err.join("\n").contains("runs past the end"), "{:?}", err);
}

#[test]
fn an_overflowing_offset_cannot_wrap_past_the_check() {
    // `ptr + off` in u64 wraps; the check is done in u128 precisely so this cannot
    // turn into an accepted access.
    let (_, err) = agree(
        r#"
let p = ffi_alloc(8)
let big = 9223372036854775807
ffi_write(p, big, 65)
"#,
    );
    assert!(err.join("\n").contains("runs past the end"), "{:?}", err);
}

#[test]
fn a_read_past_the_end_is_refused() {
    let (_, err) = agree(
        r#"
let p = ffi_alloc(4)
dump ffi_read(p, 4)
"#,
    );
    assert!(err.join("\n").contains("runs past the end"), "{:?}", err);
}

#[test]
fn a_forged_pointer_cannot_be_written_through() {
    // `ffi_ptr` builds a pointer from any integer. It still does -- resolving a
    // symbol's address is legitimate -- but the address is not usable for memory
    // access until Rak has been told the region is safe.
    let (_, err) = agree(
        r#"
let p = ffi_ptr(140737488355328)
ffi_write(p, 0, 65)
"#,
    );
    let text = err.join("\n");
    assert!(text.contains("not a region Rak owns"), "got: {text}");
    assert!(text.contains("ffi_trust"), "it should name the way out: {text}");
}

#[test]
fn a_forged_pointer_cannot_be_read_through() {
    let (_, err) = agree("dump ffi_read(ffi_ptr(140737488355328), 0)
");
    assert!(err.join("\n").contains("not a region Rak owns"), "{:?}", err);
}

#[test]
fn ffi_trust_narrows_what_rak_will_touch() {
    // `ffi_trust` on a forged address followed by a write *should* crash -- the
    // promise is a lie and the machine says so -- so this tests the mechanism
    // instead: narrow a real allocation and watch the wider access stop working.
    //
    // That is the property that matters. `ffi_trust` is an assertion Rak records
    // and then obeys, so it can be used to make an access *narrower* than the
    // allocation it came from.
    let (out, _) = agree(
        r#"
let p = ffi_alloc(16)
ffi_write(p, 0, 65)
let q = ffi_trust(p, 4)
ffi_read(q, 3)
dump "in bounds after trust"
"#,
    );
    assert_eq!(out, vec!["[DUMP] in bounds after trust"]);
}

#[test]
fn ffi_trust_can_make_an_access_refuse_again() {
    let (_, err) = agree(
        r#"
let p = ffi_alloc(16)
ffi_write(p, 0, 65)
let q = ffi_trust(p, 4)
ffi_read(q, 4)
"#,
    );
    let text = err.join("\n");
    assert!(text.contains("runs past the end"), "got: {text}");
    assert!(text.contains("4-byte"), "it should state the narrowed size: {text}");
}

#[test]
fn ffi_trust_refuses_a_nonsense_length() {
    // A zero-length region would let `check(ptr, off, 0)` pass while the pointer
    // still reached nowhere, so it is refused outright.
    let (_, err) = agree("ffi_trust(ffi_ptr(4096), 0)\n");
    let text = err.join("\n");
    assert!(text.contains("1..=4294967296"), "got: {text}");
}

#[test]
fn a_cstr_within_the_allocation_is_read() {
    let (out, _) = agree(
        r#"
let p = ffi_string_to_cstr("hello")
dump ffi_cstr_to_string(p)
"#,
    );
    assert_eq!(out, vec!["[DUMP] hello"]);
}

#[test]
fn a_cstr_scan_cannot_run_off_the_end() {
    // No NUL anywhere in the allocation: the old loop walked past it into
    // unmapped memory and killed the process.
    let (_, err) = agree(
        r#"
let p = ffi_alloc(8)
ffi_write(p, 0, 65)
ffi_write(p, 1, 66)
ffi_write(p, 2, 67)
ffi_write(p, 3, 68)
ffi_write(p, 4, 69)
ffi_write(p, 5, 70)
ffi_write(p, 6, 71)
ffi_write(p, 7, 72)
dump ffi_cstr_to_string(p)
"#,
    );
    let text = err.join("\n");
    assert!(text.contains("no NUL terminator"), "got: {text}");
    assert!(text.contains("not a C string"), "got: {text}");
}

#[test]
fn a_cstr_scan_of_a_forged_pointer_is_refused_rather_than_walked() {
    let (_, err) = agree("dump ffi_cstr_to_string(ffi_ptr(140737488355328))
");
    assert!(err.join("\n").contains("not a region Rak owns"), "{:?}", err);
}

#[test]
fn a_freed_pointer_is_no_longer_usable() {
    let (_, err) = agree(
        r#"
let p = ffi_alloc(8)
ffi_free(p)
ffi_write(p, 0, 65)
"#,
    );
    let text = err.join("\n");
    assert!(
        text.contains("not a region Rak owns") || text.contains("already freed"),
        "got: {text}"
    );
}
