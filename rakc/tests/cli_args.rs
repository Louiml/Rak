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

/// Run `rakc` with `source` piped to stdin, returning the combined output
/// and the exit code.
///
/// The code is returned rather than a success flag because a non-zero exit
/// is not necessarily a failure: `fn main`'s return value *is* the exit code,
/// so a test about that plumbing cannot otherwise tell "exited 7 on purpose"
/// from "crashed".
///
/// Output is combined because the assertions care about what the user sees,
/// and `rakc` sends program output to stdout but diagnostics to stderr. A test
/// that only reads stdout sees an empty string for every failing case, which is
/// exactly the assertion that would have silently passed.
fn run_with_stdin_code(args: &[&str], source: &str) -> (String, Option<i32>) {
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
    (text, out.status.code())
}

/// `run_with_stdin` in terms of `run_with_stdin_code`, for the assertions that
/// only care whether the process succeeded.
fn run_with_stdin(args: &[&str], source: &str) -> (String, bool) {
    let (text, code) = run_with_stdin_code(args, source);
    (text, code == Some(0))
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

/// Output written inside `fn main` must reach the user.
///
/// This is the regression test for the snapshot-before-`main` bug: `eval_cli`
/// returned the output collected before `main` ran, so a program whose logic
/// lives in `main` printed nothing at all.
#[test]
fn output_from_inside_main_is_printed() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "fn main(argv) -> int {\n  dump \"from main\"\n  return 0\n}\n",
    );
    assert!(ok, "the program should succeed");
    assert!(
        out.contains("[DUMP] from main"),
        "output written inside `fn main` was discarded; got: {:?}",
        out
    );
}

/// Top-level output and `main`'s output must both appear, in order.
///
/// A fix that only collected output after `main` would pass the test above while
/// dropping everything the top level printed, so both directions are pinned.
#[test]
fn top_level_and_main_output_both_appear() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "dump \"top\"\nfn main(argv) -> int {\n  dump \"main\"\n  return 0\n}\n",
    );
    assert!(ok, "the program should succeed");
    let top = out
        .find("[DUMP] top")
        .unwrap_or_else(|| panic!("top-level output was lost; got: {:?}", out));
    let from_main = out
        .find("[DUMP] main")
        .unwrap_or_else(|| panic!("main output was lost; got: {:?}", out));
    assert!(
        top < from_main,
        "output should stay in execution order: {:?}",
        out
    );
}

/// An uncaught `raise` inside `main` must report why.
///
/// `run_main` collapsed the error to a bare exit code and dropped the message,
/// so a program that died inside `main` exited 1 with nothing on stderr.
#[test]
fn an_error_inside_main_is_reported() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "fn main(argv) -> int {\n  raise \"boom\"\n  return 0\n}\n",
    );
    assert!(!ok, "an uncaught raise should fail the program");
    assert!(
        out.contains("boom"),
        "the error message from `main` was swallowed; got: {:?}",
        out
    );
}

/// `main`'s return value is the process exit code, and the output fix must not
/// have disturbed it.
#[test]
fn main_return_value_is_still_the_exit_code() {
    let (out, code) = run_with_stdin_code(
        &["run", "-"],
        "fn main(argv) -> int {\n  dump \"x\"\n  return 7\n}\n",
    );
    assert!(out.contains("[DUMP] x"), "got: {:?}", out);
    assert_eq!(
        code,
        Some(7),
        "`fn main`'s return value should still be the process exit code; got {:?}",
        code
    );
}

/// `--sandbox` must actually be switched on by `run`.
///
/// It was not: the argument scanner stripped every `-` token out of the list the
/// sandbox parser was then handed, so `caps::enable` was never reached and the
/// program ran with no sandbox at all while the flag sat in plain sight. Writes
/// went through untouched.
#[test]
fn sandbox_blocks_a_gated_builtin() {
    let (out, ok) = run_with_stdin(
        &["run", "-", "--sandbox"],
        "file_write(\"probe-sandbox.txt\", \"x\")\n",
    );
    assert!(!ok, "a gated builtin should fail under the sandbox");
    assert!(
        out.contains("sandbox: active"),
        "the sandbox was never enabled; got: {:?}",
        out
    );
    assert!(
        out.contains("fs_write"),
        "the error should name the missing capability; got: {:?}",
        out
    );
    assert!(
        !std::path::Path::new("probe-sandbox.txt").exists(),
        "the sandbox reported a block but the file was still created"
    );
}

/// The flag written *before* the filename is the order the usage text documents,
/// and it hit the same bug through a different path: the scanner had already
/// removed it from the list the sandbox parser saw.
#[test]
fn sandbox_works_before_the_filename() {
    let (out, ok) = run_with_stdin(
        &["run", "--sandbox", "-"],
        "file_write(\"probe-sandbox-2.txt\", \"x\")\n",
    );
    assert!(!ok, "a gated builtin should fail under the sandbox");
    assert!(
        out.contains("sandbox: active"),
        "the sandbox was never enabled; got: {:?}",
        out
    );
    assert!(
        !std::path::Path::new("probe-sandbox-2.txt").exists(),
        "the sandbox reported a block but the file was still created"
    );
}

/// Every filesystem mutator belongs behind `fs_write`.
///
/// The capability table named four builtins that do not exist -- `append_file`,
/// `mkdir`, `remove_file`, `report_write` -- while omitting five that do:
/// `write`, `file_delete`, `file_mkdir`, `file_copy`, `file_rename`. So the
/// sandbox blocked writing a file and cheerfully allowed deleting one.
#[test]
fn sandbox_covers_every_filesystem_mutator() {
    for builtin in [
        "file_write(\"p.txt\", \"x\")",
        "write(\"p.txt\", \"x\")",
        "file_delete(\"p.txt\")",
        "file_mkdir(\"probe-dir\")",
        "file_rename(\"p.txt\", \"p2.txt\")",
        "file_copy(\"p.txt\", \"p2.txt\")",
    ] {
        let (out, ok) = run_with_stdin(&["run", "-", "--sandbox"], &format!("{}\n", builtin));
        assert!(!ok, "{} should be blocked under the sandbox", builtin);
        assert!(
            out.contains("blocked"),
            "{} was not blocked; got: {:?}",
            builtin,
            out
        );
    }
    assert!(
        !std::path::Path::new("probe-dir").exists(),
        "file_mkdir ran despite the sandbox"
    );
}

/// The sandbox is a restriction, not a switch that stops the program running.
#[test]
fn sandbox_still_allows_ordinary_computation() {
    let (out, ok) = run_with_stdin(&["run", "-", "--sandbox"], "dump 6 * 7\n");
    assert!(ok, "pure computation should still run; got: {:?}", out);
    assert!(out.contains("[DUMP] 42"), "got: {:?}", out);
}

/// Consuming `--sandbox` must not cost the script its own arguments.
///
/// The sandbox flags are now parsed separately from the script's argv precisely
/// so that fixing the sandbox cannot change what a program is handed.
#[test]
fn sandbox_does_not_consume_script_arguments() {
    let (out, ok) = run_with_stdin(
        &["run", "-", "--sandbox", "alpha", "beta"],
        "fn main(argv) -> int {\n  dump argv\n  return 0\n}\n",
    );
    assert!(ok, "the program should still run; got: {:?}", out);
    assert!(
        out.contains("[alpha, beta]"),
        "the script lost its arguments; got: {:?}",
        out
    );
}

/// A negative `substr` bound must be an ordinary catchable error.
///
/// It used to abort the process. `overflow-checks = true` and `panic = "abort"`
/// are both set in release, so the overflow in `start + length` was not a
/// catchable panic -- `try` around it did not help, and any script could kill the
/// host. This test spawns the binary deliberately: an abort inside the test
/// harness would look like a crashed runner rather than a failing assertion.
#[test]
fn substr_with_a_negative_start_does_not_abort() {
    let (out, ok) = run_with_stdin(&["run", "-"], "dump substr(\"abc\", -1, 2)\n");
    assert!(
        !ok,
        "a negative start should be an error, not a silent success"
    );
    assert!(
        out.contains("must not be negative"),
        "expected a clear error, got: {:?}",
        out
    );
    assert!(
        !out.contains("panicked") && !out.contains("overflow"),
        "the process aborted instead of reporting an error: {:?}",
        out
    );
}

/// The error must be catchable, and the program must survive it.
///
/// The point of the fix is that a bad bound is a runtime error like any other,
/// not a process-level abort that `try` cannot intercept.
#[test]
fn a_negative_substr_bound_is_catchable() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "try { dump substr(\"abc\", -1, 2) } catch e { dump \"caught\" }\ndump \"still alive\"\n",
    );
    assert!(ok, "the program should survive; got: {:?}", out);
    assert!(out.contains("[DUMP] caught"), "not caught; got: {:?}", out);
    assert!(
        out.contains("[DUMP] still alive"),
        "execution did not continue past the error; got: {:?}",
        out
    );
}

/// A negative length is the same hazard by a different route.
#[test]
fn substr_with_a_negative_length_is_an_error() {
    let (out, ok) = run_with_stdin(&["run", "-"], "dump substr(\"abc\", 0, -1)\n");
    assert!(!ok, "a negative length should be an error");
    assert!(
        out.contains("must not be negative"),
        "expected a clear error, got: {:?}",
        out
    );
}

/// Ordinary bounds must be unaffected, including the clamping cases that sit
/// either side of the new checks.
#[test]
fn substr_with_valid_bounds_is_unchanged() {
    for (src, want) in [
        ("dump substr(\"hello\", 1, 3)", "[DUMP] ell"),
        ("dump substr(\"hello\", 0, 99)", "[DUMP] hello"),
        ("dump substr(\"abc\", 0, 0)", "[DUMP] "),
        ("dump substr(\"abc\", 10, 2)", "[DUMP] "),
    ] {
        let (out, ok) = run_with_stdin(&["run", "-"], &format!("{}\n", src));
        assert!(ok, "{} should succeed; got: {:?}", src, out);
        assert!(
            out.contains(want),
            "{} should print {:?}; got: {:?}",
            src,
            want,
            out
        );
    }
}

/// The program source used by the separator tests: it reports the argv it got.
const ECHO_ARGV: &str = "fn main(argv) -> int {\n  dump argv\n  return 0\n}\n";

/// Everything after `--` must reach the program verbatim.
///
/// There was no separator, so `--verbose` was claimed as a rakc flag wherever it
/// appeared and the program could never see one. The conventional spelling is a
/// literal `--`, so that is what is honoured here.
#[test]
fn separator_delivers_flag_shaped_arguments() {
    let (out, ok) = run_with_stdin(
        &["run", "-", "--", "--verbose", "--port", "8080"],
        ECHO_ARGV,
    );
    assert!(ok, "the program should run; got: {:?}", out);
    assert!(
        out.contains("[--verbose, --port, 8080]"),
        "arguments after `--` must arrive untouched; got: {:?}",
        out
    );
}

/// The separator itself is consumed, not forwarded.
#[test]
fn separator_is_not_forwarded_as_an_argument() {
    let (out, ok) = run_with_stdin(&["run", "-", "--", "alpha"], ECHO_ARGV);
    assert!(ok, "the program should run; got: {:?}", out);
    assert!(
        out.contains("[alpha]"),
        "the `--` should not appear in argv; got: {:?}",
        out
    );
}

/// Plain words after the file keep working, with or without a separator.
#[test]
fn plain_arguments_still_reach_the_program() {
    for argv in [
        vec!["run", "-", "alpha", "beta"],
        vec!["run", "-", "--", "alpha", "beta"],
    ] {
        let (out, ok) = run_with_stdin(&argv, ECHO_ARGV);
        assert!(ok, "{:?} should run; got: {:?}", argv, out);
        assert!(
            out.contains("[alpha, beta]"),
            "{:?} lost the program's arguments; got: {:?}",
            argv,
            out
        );
    }
}

/// A flag after `--` belongs to the program, so rakc must not act on it.
///
/// `--sandbox` is the sharpest case: if rakc claimed it here it would silently
/// switch the sandbox on for a program that merely asked to receive the word.
#[test]
fn rakc_flags_after_the_separator_are_not_acted_on() {
    let (out, _ok) = run_with_stdin(&["run", "-", "--", "--sandbox"], ECHO_ARGV);
    assert!(
        out.contains("[--sandbox]"),
        "the program should receive the flag; got: {:?}",
        out
    );
    assert!(
        !out.contains("sandbox: active"),
        "rakc acted on a flag meant for the program; got: {:?}",
        out
    );
}

/// rakc's own flags must still be claimed, on either side of the filename.
///
/// The separator is an addition, not a replacement: everything before it is
/// scanned exactly as before.
#[test]
fn rakc_flags_before_the_separator_still_apply() {
    // A gated builtin, which only fails when the sandbox is genuinely on.
    let (out, ok) = run_with_stdin(
        &["run", "--sandbox", "-"],
        "file_write(\"probe-sep.txt\", \"x\")\n",
    );
    assert!(!ok, "the sandbox should be active and block the write");
    assert!(
        out.contains("sandbox: active") && out.contains("blocked"),
        "--sandbox before the separator was not applied; got: {:?}",
        out
    );
    assert!(
        !std::path::Path::new("probe-sep.txt").exists(),
        "the write went through despite the sandbox"
    );
}

/// A rakc flag written after the file but before the separator is still rakc's.
///
/// This is the case that made the sandbox look broken in the first place, and the
/// one `--max-depth` already had to be special-cased for.
#[test]
fn rakc_flags_after_the_file_but_before_the_separator_still_apply() {
    let (out, ok) = run_with_stdin(
        &["run", "-", "--sandbox"],
        "file_write(\"probe-sep2.txt\", \"x\")\n",
    );
    assert!(!ok, "the sandbox should be active and block the write");
    assert!(
        out.contains("blocked"),
        "--sandbox was not applied; got: {:?}",
        out
    );
    assert!(
        !std::path::Path::new("probe-sep2.txt").exists(),
        "the write went through despite the sandbox"
    );
}

/// With no file left to run, the separator must not be mistaken for one.
#[test]
fn separator_alone_is_not_treated_as_the_filename() {
    let (_, ok) = run_with_stdin(&["run", "--"], ECHO_ARGV);
    assert!(!ok, "`--` is not a file, so this should be rejected");
}

/// `rakc vm` must call `fn main` and honour its return value as the exit code.
///
/// It did neither. The VM had no notion of an entry point, so `rakc vm` ran only
/// top-level statements and exited 0: every program whose logic lived in `main`
/// did nothing under `vm` while working perfectly under `run`.
#[test]
fn vm_invokes_main_and_uses_its_exit_code() {
    let (out, ok) = run_with_stdin(
        &["vm", "-"],
        "fn main(argv) -> int {\n  dump \"vm main ran\"\n  return 7\n}\n",
    );
    assert!(
        out.contains("[DUMP] vm main ran"),
        "`rakc vm` did not run `fn main`; got: {:?}",
        out
    );
    assert!(
        !ok,
        "`main` returned 7, so the process should exit non-zero"
    );
}

/// A lex/parse/compile failure under `vm` must exit non-zero.
///
/// The handlers only printed the diagnostic and `main` returns `()`, so a program
/// that failed to compile exited 0 -- which in CI reads as a pass for a program
/// that never ran.
#[test]
fn vm_compile_failure_exits_non_zero() {
    let (out, ok) = run_with_stdin(
        &["vm", "-"],
        "fn f(x) { return x } ensures result > 0\ndump f(5)\n",
    );
    assert!(!ok, "a compile error must not exit 0");
    assert!(
        out.contains("does not support postconditions"),
        "expected the unsupported-construct message, got: {:?}",
        out
    );
}
