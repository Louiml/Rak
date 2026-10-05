//! Argument handling for the `rakc` CLI.
//!
//! These exercise the `rakc` binary itself rather than the library, because
//! the bugs they pin down live in `main.rs`'s argument scanner -- a layer no
//! `rakc::` API reaches.
//!
//! The one that matters most: the usage text has always advertised
//! "Use - for file to read from stdin", and `read_source` has always
//! implemented it, but the scanner classified every argument beginning with
//! `-` as a flag. `-` begins with `-`. So `rakc run -` consumed the only
//! filename candidate and died with "Missing file argument (only flags were
//! given)" -- the documented stdin path could not be used at all, which also
//! made it impossible to pipe a script into `run`, `vm`, `check`, `lint` or
//! `fmt` for testing.

use std::io::Write;
use std::process::{Command, Stdio};

/// Run `rakc` with `source` piped to stdin, returning (combined output,
/// success).
///
/// Output is combined because the assertions care about what the user sees,
/// and `rakc` sends program output to stdout but diagnostics to stderr. A test
/// that only reads stdout sees an empty string for every failing case, which is
/// exactly the assertion that would have silently passed.
fn run_with_stdin(args: &[&str], source: &str) -> (String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rakc"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rakc");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(source.as_bytes())
        .expect("write stdin");
    let mut out = child.wait_with_output().expect("wait for rakc");
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.success())
}

#[test]
fn run_reads_a_script_from_stdin() {
    let (out, ok) = run_with_stdin(&["run", "-"], "dump \"from stdin\"\n");
    assert!(ok, "rakc run - should succeed");
    assert!(
        out.contains("[DUMP] from stdin"),
        "expected the program to run, got: {:?}",
        out
    );
}

#[test]
fn vm_reads_a_script_from_stdin() {
    let (out, ok) = run_with_stdin(&["vm", "-"], "dump 6 * 7\n");
    assert!(ok, "rakc vm - should succeed");
    assert!(out.contains("[DUMP] 42"), "got: {:?}", out);
}

#[test]
fn check_reads_a_script_from_stdin() {
    let (out, ok) = run_with_stdin(&["check", "-"], "let x: i32 = 1\n");
    assert!(ok, "rakc check - should succeed");
    assert!(out.contains("no errors"), "got: {:?}", out);
}

#[test]
fn lint_reads_a_script_from_stdin() {
    let (out, ok) = run_with_stdin(&["lint", "-"], "let unused = 1\n");
    assert!(ok, "rakc lint - should succeed");
    assert!(
        out.contains("unused-var"),
        "expected the lint to fire on the unused binding, got: {:?}",
        out
    );
}

/// A flag written alongside `-` must still be recognised as a flag, or the
/// scanner fix would just have traded one bug for another: `-` would be
/// treated as a filename and `--max-depth` would be handed to the script.
#[test]
fn flags_still_parse_alongside_the_stdin_marker() {
    // Deep recursion with a tight cap: without `--max-depth` reaching the
    // script, the program runs and the cap is ignored.
    let (out, ok) = run_with_stdin(
        &["run", "-", "--max-depth", "32"],
        "fn down(n) { return down(n + 1) }\ndump down(0)\n",
    );
    assert!(!ok, "recursion should trip the cap and fail");
    assert!(
        out.contains("recursion depth exceeded"),
        "--max-depth should have been consumed by rakc, not passed to the script; got: {:?}",
        out
    );
}

/// `--max-depth` before the filename is the order the usage text documents,
/// and both orders have to work.
#[test]
fn flags_before_the_stdin_marker_also_parse() {
    let (_, ok) = run_with_stdin(
        &["run", "--max-depth", "16", "-"],
        "fn down(n) { return down(n + 1) }\ndump down(0)\n",
    );
    assert!(!ok, "recursion should trip the cap and fail");
}