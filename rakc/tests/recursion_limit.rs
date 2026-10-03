//! Recursion must fail with an error, not take the process with it.
//!
//! Before this, `fn down(n) { 1 + down(n-1) }` killed `rakc run` *and* `rakc vm` with
//! `STATUS_STACK_OVERFLOW` (exit `0xC00000FD`) at a few hundred frames. The check
//! already existed in the interpreter -- `Limits::max_depth` -- but `Limits`
//! derives `Default`, so every field was `None`, meaning unlimited, so the check
//! never fired. Only `rakc verify` set a limit. The VM had no accounting at all.
//!
//! These tests assert the *error*, and that the limit is adjustable, because a
//! limit you cannot raise is a bug report waiting to happen.

use std::path::PathBuf;

/// Windows STATUS_STACK_OVERFLOW as a wait status, which is what a native stack
/// overflow surfaces as. It does not fit in i32.
const STACK_OVERFLOW: i32 = i32::MIN + 1;  // 0xC00000FD as a signed wait status

/// Write `source` to a uniquely-named file and run it under `mode`.
///
/// Unique per call: the tests run in parallel threads in one binary, and a shared
/// filename means each test executes whichever sibling wrote last.
fn run(mode: &str, source: &str, extra: &[&str]) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("rak_depth_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path: PathBuf = dir.join(format!("case_{}_{}.rak", std::process::id(), n));
    std::fs::write(&path, source).expect("write the case");

    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_rakc"));
    cmd.arg(mode).arg(&path);
    for e in extra {
        cmd.arg(e);
    }
    let out = cmd.output().unwrap_or_else(|e| panic!("spawn {mode}: {e}"));
    let _ = std::fs::remove_file(&path);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (text, out.status.code().unwrap_or(-1))
}

fn recursive(depth: u32) -> String {
    format!(
        "fn down(n) {{\n  if n <= 0 {{ return 0 }}\n  return 1 + down(n - 1)\n}}\ndump down({depth})\n"
    )
}

#[test]
fn the_interpreter_reports_the_limit_instead_of_dying() {
    let (text, code) = run("run", &recursive(5000), &[]);
    assert_ne!(
        code,
        STACK_OVERFLOW,
        "the process was killed by a stack overflow rather than reporting an error:\n{text}"
    );
    assert!(text.contains("depth exceeded"), "got: {text}");
    assert!(text.contains("256"), "the message should name the limit: {text}");
}

#[test]
fn the_vm_reports_the_limit_instead_of_dying() {
    let (text, code) = run("vm", &recursive(5000), &[]);
    assert_ne!(
        code,
        STACK_OVERFLOW,
        "the VM overflowed the stack rather than reporting an error:\n{text}"
    );
    assert!(text.contains("depth exceeded"), "got: {text}");
}

#[test]
fn recursion_within_the_limit_still_works_on_both_backends() {
    for mode in ["run", "vm"] {
        let (text, code) = run(mode, &recursive(100), &[]);
        assert_eq!(code, 0, "{mode} failed: {text}");
        assert!(text.contains("[DUMP] 100"), "{mode} got: {text}");
    }
}

#[test]
fn the_limit_can_be_raised_from_the_command_line() {
    // After the filename, which is the order the usage text documents.
    for mode in ["run", "vm"] {
        let (text, code) = run(mode, &recursive(300), &["--max-depth", "4000"]);
        assert_eq!(code, 0, "{mode} with --max-depth failed: {text}");
        assert!(text.contains("[DUMP] 300"), "{mode} got: {text}");
    }
}

#[test]
fn the_limit_can_also_be_given_before_the_filename() {
    // Both orders, because the scanner strips flags from the script's argv and a
    // flag written after the file is the one that was being dropped.
    let (text, code) = run("run", &recursive(300), &[]);
    assert_ne!(code, 0, "precondition: the default should clamp 300");
    let dir = std::env::temp_dir().join(format!("rak_depth_pre_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("pre.rak");
    std::fs::write(&path, recursive(300)).expect("write");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rakc"))
        .arg("run")
        .arg("--max-depth")
        .arg("4000")
        .arg(&path)
        .output()
        .expect("spawn");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(text.contains("[DUMP] 300"), "got: {text}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_nonsense_limit_is_reported_rather_than_ignored() {
    let (text, _) = run("run", &recursive(300), &["--max-depth", "abc"]);
    assert!(
        text.contains("--max-depth needs a positive number"),
        "a bad flag should say so: {text}"
    );
}
