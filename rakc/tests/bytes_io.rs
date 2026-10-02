//! Binary file I/O and byte mutation, on both backends.
//!
//! Everything here was impossible before this change, which is why the tests are
//! written as round trips rather than as unit assertions:
//!
//! * `file_read` is `fs::read_to_string`, which **fails** on any file containing
//!   a byte sequence that is not valid UTF-8. `rak_stdlib::file::read_bytes`
//!   existed and was registered nowhere, so there was no way to open a binary file
//!   at all.
//! * `write` coerces through UTF-8, so a buffer holding `0xFF` came back as U+FFFD.
//!   There was no byte-exact write either.
//! * `mmap_open(path, "rw")` produced a writable mapping that Rak could not write
//!   through.
//! * `bytes([1, 2])` produced the *text* `[1, 2]`; `b[0] = v` and `b1 + b2` were
//!   rejected; `for b in buf` was rejected on the interpreter.
//!
//! ## Hex literals are avoided on purpose
//!
//! `0x00` and `0` are different values in Rak, and the two backends disagree about
//! how a `Hex` renders and compares - `Hex(0, 64)` prints as 16 hex digits on the
//! VM and as `0x0` on the interpreter. That is pre-existing and unrelated to
//! binary I/O, so using hex literals here would make these tests fail for a reason
//! that has nothing to do with what they check. `docs/V8-KNOWN-ISSUES.md` records
//! it; the fix is separate.
//!
//! ## Error text is not asserted
//!
//! The VM wraps every native error as `<builtin>: <message>`, so the wording differs
//! by a transport prefix that says nothing about behaviour. These tests assert
//! that an operation *fails*, not how it says so.

use std::path::PathBuf;

/// A scratch file that removes itself, so a failed test leaves nothing behind and
/// a re-run cannot read the previous run's leftovers.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("rak_bytesio_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch { dir }
    }

    /// A path with forward slashes, because it is interpolated into a Rak string
    /// literal where a backslash is an escape: `\Users` is not a valid escape and
    /// the path stops naming the file. Windows accepts `/`, so this is portable.
    fn path(&self, name: &str) -> String {
        self.dir
            .join(name)
            .to_str()
            .expect("utf-8 path")
            .replace('\\', "/")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Run `source` on both backends with `path` as the file it writes, requiring
/// byte-identical output.
fn agree_with_file(tag: &str, scratch: &Scratch, path: &str, source: &str) -> Vec<String> {
    let full = source.replace("SCRATCH_PATH", &scratch.path(path));
    let parity = rakc::run_on_both(&full, scratch.dir.to_str().expect("utf-8 path"));
    if let Some(why) = parity.divergence() {
        panic!("backend divergence:\n{}\n--- source ---\n{}", why, full);
    }
    match parity {
        rakc::BackendParity::Agree(out) => out,
        other => panic!("both backends failed, so nothing was compared: {:?}", other),
    }
}

#[test]
fn a_buffer_can_be_built_from_numbers_and_written_byte_exactly() {
    let scratch = Scratch::new("roundtrip");
    let out = agree_with_file(
        "roundtrip",
        &scratch,
        "out.bin",
        r#"
let buf = bytes([0, 255, 65, 66, 10, 13])
dump hex_encode(buf)
dump file_write_bytes("SCRATCH_PATH", buf)
dump file_size("SCRATCH_PATH")
let back = file_read_bytes("SCRATCH_PATH")
dump hex_encode(back)
dump back == buf
"#,
    );
    // `0x00` and `0xFF` survive, which is the whole point: through UTF-8 they would
    // have become U+0000 and U+FFFD.
    assert_eq!(
        out,
        vec!["[DUMP] 00ff41420a0d", "[DUMP] true", "[DUMP] 6", "[DUMP] 00ff41420a0d", "[DUMP] true"]
    );
}

#[test]
fn append_is_byte_exact_too() {
    let scratch = Scratch::new("append");
    let out = agree_with_file(
        "append",
        &scratch,
        "out.bin",
        r#"
file_write_bytes("SCRATCH_PATH", bytes([1, 2]))
dump file_append_bytes("SCRATCH_PATH", bytes([255, 0]))
dump hex_encode(file_read_bytes("SCRATCH_PATH"))
"#,
    );
    assert_eq!(out, vec!["[DUMP] true", "[DUMP] 0102ff00"]);
}

#[test]
fn a_mapping_can_be_written_through() {
    // The mapping is the file, so this needs no flush and no second handle - which
    // is the reason to write through the mapping rather than seek-and-write a path.
    let scratch = Scratch::new("mmapwrite");
    let out = agree_with_file(
        "mmapwrite",
        &scratch,
        "out.bin",
        r#"
file_write_bytes("SCRATCH_PATH", bytes([0, 1, 2, 3]))
let m = mmap_open("SCRATCH_PATH", "rw")
mmap_write(m, 1, 255)
mmap_write(m, 3, 128)
mmap_close(m)
dump hex_encode(file_read_bytes("SCRATCH_PATH"))
"#,
    );
    // The write is visible through a *fresh* read of the file, so it reached the
    // file and not just the process's view of it.
    assert_eq!(out, vec!["[DUMP] 00ff0280"]);
}

#[test]
fn a_write_through_a_mapping_is_visible_to_the_mapping() {
    let scratch = Scratch::new("mmapsee");
    let out = agree_with_file(
        "mmapsee",
        &scratch,
        "out.bin",
        r#"
file_write_bytes("SCRATCH_PATH", bytes([9, 9, 9, 9]))
let m = mmap_open("SCRATCH_PATH", "rw")
mmap_write(m, 0, 1)
dump m[0]
dump hex_encode(mmap_slice(m, 0, 4))
mmap_close(m)
"#,
    );
    assert_eq!(out, vec!["[DUMP] 1", "[DUMP] 01090909"]);
}

#[test]
fn a_write_out_of_range_changes_nothing() {
    let scratch = Scratch::new("range");
    let out = agree_with_file(
        "range",
        &scratch,
        "out.bin",
        r#"
file_write_bytes("SCRATCH_PATH", bytes([1, 2]))
let m = mmap_open("SCRATCH_PATH", "rw")
try {
    mmap_write(m, 99, 255)
    dump "refused: no"
} catch e {
    dump "refused: yes"
}
mmap_close(m)
dump hex_encode(file_read_bytes("SCRATCH_PATH"))
"#,
    );
    // All-or-nothing: the rejected write did not partially apply.
    assert_eq!(out, vec!["[DUMP] refused: yes", "[DUMP] 0102"]);
}

#[test]
fn a_read_only_mapping_refuses_a_write() {
    let scratch = Scratch::new("ro");
    let out = agree_with_file(
        "ro",
        &scratch,
        "out.bin",
        r#"
file_write_bytes("SCRATCH_PATH", bytes([1, 2]))
let m = mmap_open("SCRATCH_PATH", "r")
try {
    mmap_write(m, 0, 255)
    dump "refused: no"
} catch e {
    dump "refused: yes"
}
mmap_close(m)
dump hex_encode(file_read_bytes("SCRATCH_PATH"))
"#,
    );
    assert_eq!(out, vec!["[DUMP] refused: yes", "[DUMP] 0102"]);
}

#[test]
fn a_byte_buffer_can_be_built_mutated_and_concatenated() {
    let out = agree_with_file(
        "mutate",
        &Scratch::new("mutate"),
        "unused.bin",
        r#"
let mut buf = bytes([1, 2, 3])
dump len(buf)
buf[0] = 10
dump hex_encode(buf)
buf[1] = 'Z'
dump hex_encode(buf)
buf[2] = 0xFF
dump hex_encode(buf)
dump hex_encode(bytes([9]) + buf)
dump hex_encode(bytes([9]) + buf + bytes([8]))
let mut sum = 0
for b in buf { sum = sum + b }
dump sum
"#,
    );
    assert_eq!(
        out,
        vec![
            "[DUMP] 3",
            "[DUMP] 0a0203",
            "[DUMP] 0a5a03",
            "[DUMP] 0a5aff",
            "[DUMP] 090a5aff",
            "[DUMP] 090a5aff08",
            "[DUMP] 355",
        ]
    );
}

#[test]
fn a_read_only_file_fails_to_read_rather_than_returning_junk() {
    // The old `file_read` is `fs::read_to_string`, so it *fails* on a binary file.
    // That behaviour is unchanged and still correct for `file_read`; what changed is
    // that `file_read_bytes` is the way to ask. Pinned so the distinction does not
    // quietly change: a program that reads a binary file must not start getting a
    // lossy string.
    let scratch = Scratch::new("utf8fail");
    let path = scratch.path("bin.dat");
    std::fs::write(&path, [0x00, 0xFF, 0x41]).expect("write fixture");
    let source = format!(
        r#"
try {{
    dump file_read("{}")
    dump "read-as-text: yes"
}} catch e {{
    dump "read-as-text: no"
}}
"#,
        path
    );
    match rakc::run_on_both(&source, scratch.dir.to_str().expect("utf-8 path")) {
        rakc::BackendParity::Agree(lines) => {
            assert_eq!(lines, vec!["[DUMP] read-as-text: no"]);
        }
        other => panic!("the two backends disagreed about reading a binary file as text: {:?}", other),
    }
}

#[test]
fn a_buffer_of_bytes_can_be_sliced_and_rejoined() {
    // The round trip a hex editor leans on: read a window, split it, put it back.
    let scratch = Scratch::new("slices");
    let out = agree_with_file(
        "slices",
        &scratch,
        "out.bin",
        r#"
let all = bytes([0, 1, 2, 3, 4, 5, 6, 7])
dump hex_encode(all[2..5])
dump hex_encode(all[..2] + all[6..])
dump hex_encode(all[0..8])
"#,
    );
    assert_eq!(out, vec!["[DUMP] 020304", "[DUMP] 00010607", "[DUMP] 0001020304050607"]);
}
