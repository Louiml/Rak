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

/// Run the type checker over `src` and return its diagnostics, rendered.
///
/// This mirrors what the `check` subcommand does in `main.rs` -- the checker reports a
/// list and the CLI renders and prints it -- so the tests assert on the same text a user
/// would see. A lexer or parser failure is returned as a one-element list rather than
/// panicking, so a malformed fixture fails an assertion instead of the harness.
fn check_diags(src: &str) -> Vec<String> {
    let tokens = match rakc::lexer::tokenize(src) {
        Ok(t) => t,
        Err(e) => return vec![e.to_string()],
    };
    let ast = match rakc::parser::parse(&tokens, src) {
        Ok(a) => a,
        Err(e) => return vec![e.to_string()],
    };
    let mut tc = rakc::typecheck::TypeChecker::new(src, "test.rak");
    tc.check_module(&ast).iter().map(|d| d.render()).collect()
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

/// `break` with no loop of its own must not break the caller's loop.
///
/// `loop_signal` is interpreter-global. With nothing counting open loops, a `break`
/// in a function stopped whatever loop happened to be running outside it -- `n` came
/// out 0 instead of 3, and the program reported success.
#[test]
fn break_in_a_function_does_not_leak_into_the_callers_loop() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "fn f() { break }\nlet mut n = 0\nfor i in [1,2,3] { f()  n = n + 1 }\ndump n\n",
    );
    assert!(!ok, "a `break` with no loop to leave must fail");
    assert!(
        out.contains("outside a loop"),
        "expected a clear diagnostic, got: {:?}",
        out
    );
}

/// `continue` has the same problem, and it is harder to notice: the loop just stops
/// repeating.
#[test]
fn continue_in_a_function_does_not_leak_into_the_callers_loop() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "fn f() { continue }\nlet mut n = 0\nfor i in [1,2,3] { f()  n = n + 1 }\ndump n\n",
    );
    assert!(!ok, "a `continue` with no loop to continue must fail");
    assert!(
        out.contains("outside a loop"),
        "expected a clear diagnostic, got: {:?}",
        out
    );
}

/// A `break` at the top level has no loop at all.
#[test]
fn break_at_the_top_level_is_an_error() {
    let (out, ok) = run_with_stdin(&["run", "-"], "break\n");
    assert!(!ok);
    assert!(out.contains("outside a loop"), "got: {:?}", out);
}

/// A label that names no open loop must be reported, not resolved to the innermost.
///
/// Silently breaking the wrong loop is worse than an error: the program compiles,
/// runs, and computes something the author never asked for.
#[test]
fn an_unknown_loop_label_is_reported() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "let mut n = 0\nfor i in [1,2,3] { n = n + 1  break 'nope }\ndump n\n",
    );
    assert!(
        !ok,
        "an unknown label must not silently break the innermost loop"
    );
    assert!(out.contains("no loop labelled 'nope'"), "got: {:?}", out);
}

/// The diagnostic is an ordinary runtime error, so `try` catches it.
#[test]
fn an_out_of_loop_break_is_catchable() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "try { break } catch e { dump \"caught\" }\ndump \"alive\"\n",
    );
    assert!(ok, "a caught out-of-loop break should not fail the program");
    assert!(out.contains("[DUMP] caught"), "not caught; got: {:?}", out);
    assert!(out.contains("[DUMP] alive"), "got: {:?}", out);
}

/// Every real loop form still allows `break` and `continue`.
///
/// The counter is only worth having if it does not reject the cases that are supposed
/// to work, so all five forms are pinned here.
#[test]
fn real_loops_still_allow_break_and_continue() {
    for (src, want) in [
        (
            "let mut n = 0\nfor i in [1,2,3] { if i == 2 { break }  n = n + 1 }\ndump n",
            "[DUMP] 1",
        ),
        (
            "let mut n = 0\nfor i in [1,2,3] { if i == 2 { continue }  n = n + 1 }\ndump n",
            "[DUMP] 2",
        ),
        (
            "let mut n = 0\n'outer: for i in [1,2,3] { for j in [1,2,3] { if j == 2 { break 'outer }  n = n + 1 } }\ndump n",
            "[DUMP] 1",
        ),
        (
            "let mut i = 0\nwhile i < 10 { i = i + 1  if i == 3 { break } }\ndump i",
            "[DUMP] 3",
        ),
        (
            "let mut i = 0\nloop { i = i + 1  if i == 4 { break } }\ndump i",
            "[DUMP] 4",
        ),
        (
            "let mut i = 0\ndo { i = i + 1  if i == 5 { break } } while i < 100\ndump i",
            "[DUMP] 5",
        ),
        (
            "let mut n = 0\nfor i in [1,2] { for j in [1,2] { n = n + 1  break 2 } }\ndump n",
            "[DUMP] 1",
        ),
        (
            "fn f() { let mut i = 0  while i < 5 { i = i + 1  if i == 3 { break } }  return i }\ndump f()",
            "[DUMP] 3",
        ),
    ] {
        let (out, ok) = run_with_stdin(&["run", "-"], &format!("{}\n", src));
        assert!(ok, "{:?} should succeed; got: {:?}", src, out);
        assert!(
            out.contains(want),
            "{:?} should print {:?}; got: {:?}",
            src,
            want,
            out
        );
    }
}

/// A caught error inside a loop must not leave the depth stale.
///
/// The depth is restored on the error path precisely so this holds: a leaked count
/// would make a later `break` look like it had a loop to leave, or make a legitimate
/// one look out of scope.
#[test]
fn a_caught_error_in_a_loop_does_not_corrupt_the_depth() {
    let (out, ok) = run_with_stdin(
        &["run", "-"],
        "fn boom() { raise \"x\" }\nlet mut n = 0\nfor i in [1,2,3] { try { boom() } catch e { }  n = n + 1 }\ndump n\n",
    );
    assert!(ok, "got: {:?}", out);
    assert!(out.contains("[DUMP] 3"), "got: {:?}", out);
}

/// An `impl` that misspells a trait method is reported at both ends.
///
/// The typo leaves the real method unimplemented, so a single error naming the stray
/// method is not enough -- the required one has to be named too, or the reader has to
/// connect the two themselves.
#[test]
fn trait_typo_is_reported() {
    let src = r#"
trait Greet { fn hi(self) -> string; }
struct P { v: int }
impl Greet for P { fn h1(self) -> string { return "x" } }
"#;
    let diags = check_diags(src);
    assert!(
        diags.iter().any(|d| d.contains("is not a method of trait")),
        "expected the stray `h1` to be reported, got: {:#?}",
        diags
    );
    assert!(
        diags.iter().any(|d| d.contains("does not implement `hi`")),
        "expected the missing `hi` to be reported, got: {:#?}",
        diags
    );
    assert!(
        diags.iter().any(|d| d.contains("did you mean `hi`?")),
        "expected a suggestion for the typo, got: {:#?}",
        diags
    );
}

/// A trait that is never implemented is reported where the `impl` is.
#[test]
fn trait_missing_method_is_reported() {
    let diags = check_diags(
        r#"
trait Greet { fn hi(self) -> string; }
struct P { v: int }
impl Greet for P { }
"#,
    );
    assert!(
        diags.iter().any(|d| d.contains("does not implement `hi`")),
        "got: {:#?}",
        diags
    );
}

/// Implementing a trait that was never declared is an error, not a silent no-op.
#[test]
fn undeclared_trait_is_reported() {
    let diags = check_diags(
        r#"
struct P { v: int }
impl NotATrait for P { fn hi(self) -> string { return "x" } }
"#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("trait `NotATrait` is not declared")),
        "got: {:#?}",
        diags
    );
}

/// Arity is compared, and the receiver counts toward it.
///
/// `fn hi(self, x)` is arity 2, so an impl declaring `fn hi(self)` is a mismatch. A
/// wrong parameter *name* at the same arity is not -- only the count is compared, since
/// impl methods are stored as closures.
#[test]
fn trait_method_arity_is_checked() {
    let diags = check_diags(
        r#"
trait Greet { fn hi(self, x: int) -> string; }
struct P { v: int }
impl Greet for P { fn hi(self) -> string { return "x" } }
"#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("takes 1 parameter(s) here but the trait expects 2")),
        "got: {:#?}",
        diags
    );
}

/// Valid programs must stay clean -- including the shapes that could plausibly break.
///
/// An inherent `impl` names no trait and so has nothing to validate; an `impl` may
/// precede its `trait` because traits are collected in a pre-pass; and a trait with more
/// than one method must accept a complete impl.
#[test]
fn valid_trait_declarations_stay_clean() {
    for (label, src) in [
        (
            "well-formed trait impl",
            r#"
trait Greet { fn hi(self) -> string; }
struct P { v: int }
impl Greet for P { fn hi(self) -> string { return "hello" } }
"#,
        ),
        (
            "inherent impl names no trait",
            r#"
struct P { v: int }
impl P { fn hi(self) -> string { return "hello" } }
"#,
        ),
        (
            "impl before trait",
            r#"
struct P { v: int }
impl Greet for P { fn hi(self) -> string { return "hello" } }
trait Greet { fn hi(self) -> string; }
"#,
        ),
        (
            "multi-method trait, all implemented",
            r#"
trait Both { fn a(self) -> int; fn b(self) -> int; }
struct P { v: int }
impl Both for P { fn a(self) -> int { return 1 } fn b(self) -> int { return 2 } }
"#,
        ),
        (
            "wrong parameter name at the same arity is not an error",
            r#"
trait Greet { fn hi(self) -> string; }
struct P { v: int }
impl Greet for P { fn hi(me) -> string { return "x" } }
"#,
        ),
    ] {
        let diags = check_diags(src);
        assert!(
            diags.is_empty(),
            "{}: unexpected diagnostics {:#?}",
            label,
            diags
        );
    }
}

/// A trait impl has to actually dispatch, not merely type-check.
///
/// The trait and impl both declare `hi`, and the call goes through `p.hi()` with the
/// receiver passed implicitly -- which is why every method above spells its receiver.
#[test]
fn trait_impl_dispatches_at_runtime() {
    let out = rakc::eval(
        r#"
trait Greet { fn hi(self) -> string; }
struct P { v: int }
impl Greet for P { fn hi(self) -> string { return "hello" } }
dump P { v: 1 }.hi()
"#,
    )
    .expect("a well-formed trait impl must run");
    assert!(
        out.iter().any(|l| l.contains("hello")),
        "expected the trait method to run, got: {:?}",
        out
    );
}

/// A struct literal is checked against its declaration, not just its annotation.
///
/// The three failures this covers were all silent. Reporting them needs a field map, and
/// this asserts the codes so a future refactor that drops the check fails here.
#[test]
fn a_struct_literal_is_checked_against_its_declaration() {
    for (label, src, code) in [
        (
            "unknown field",
            "struct P { v: int }\nlet p = P { v: 1, nope: 2 }\n",
            "E0420",
        ),
        (
            "missing field",
            "struct P { v: int }\nlet p = P { }\n",
            "E0421",
        ),
        (
            "wrong field type",
            "struct P { v: int }\nlet p = P { v: \"s\" }\n",
            "E0422",
        ),
    ] {
        let diags = check_diags(src);
        assert!(
            diags.iter().any(|d| d.contains(code)),
            "{}: expected {}, got: {:#?}",
            label,
            code,
            diags
        );
    }
}

/// An unknown field is reported *and* a suggestion is offered.
///
/// The suggester is worth pinning because the obvious implementation suggests the field
/// that was just named, which reads as a non sequitur.
#[test]
fn an_unknown_struct_field_is_suggested() {
    let diags = check_diags("struct P { v: int }\nlet p = P { vee: 1 }\n");
    assert!(
        diags
            .iter()
            .any(|d| d.contains("has no field `vee`") && d.contains("did you mean `v`?")),
        "got: {:#?}",
        diags
    );
}

/// A struct literal must keep its own name as its inferred type.
///
/// It used to infer as the literal string `Custom("struct")`, which is why the annotated
/// case worked only by accident and the unannotated one did not work at all. This asserts
/// the behaviour through the annotation the user writes by hand.
#[test]
fn a_struct_literal_keeps_its_name_as_its_type() {
    assert!(check_diags("struct P { v: int }\nlet p = P { v: 1 }\n").is_empty());
    assert!(
        check_diags("struct P { v: int }\nlet p: P = P { v: 1 }\n").is_empty(),
        "the annotated form must keep working"
    );
    // Two different struct names must not satisfy each other.
    let diags = check_diags("struct P { v: int }\nstruct Q { v: int }\nlet p: P = Q { v: 1 }\n");
    assert!(
        diags.iter().any(|d| d.contains("type mismatch")),
        "P and Q are different types, got: {:#?}",
        diags
    );
}

/// An enum variant construction is checked: variant, arity, payload types.
///
/// Bare paths are included, since `C::N` with no values is exactly how a payload variant
/// gets reached by mistake -- and a unit variant, whose correct arity is zero, has to
/// stay valid.
#[test]
fn an_enum_variant_construction_is_checked() {
    for (label, src, code) in [
        (
            "unknown variant",
            "enum C { R, G, B }\nlet c = C::P\n",
            "E0423",
        ),
        (
            "payload variant used bare",
            "enum C { N(int) }\nlet c = C::N\n",
            "E0424",
        ),
        (
            "too many values",
            "enum C { N(int) }\nlet c = C::N(1, 2)\n",
            "E0424",
        ),
        (
            "value given to a unit variant",
            "enum C { R, G, B }\nlet c = C::R(1)\n",
            "E0424",
        ),
        (
            "wrong payload type",
            "enum C { N(int, string) }\nlet c = C::N(1, 2)\n",
            "E0425",
        ),
    ] {
        let diags = check_diags(src);
        assert!(
            diags.iter().any(|d| d.contains(code)),
            "{}: expected {}, got: {:#?}",
            label,
            code,
            diags
        );
    }
}

/// Literals that are fine must stay clean.
///
/// The interesting ones are the shapes that could plausibly be mistaken for the bad
/// cases: a two-segment module path is not an enum construction, an untyped field has
/// nothing to check against, an undeclared struct is left to the runtime, a unit variant
/// is a bare path with the correct arity, and an int literal still satisfies an `f64`
/// field (the numeric arm of `compatible`, which is the same rule annotations use).
#[test]
fn valid_literals_stay_clean() {
    for (label, src) in [
        (
            "module path is not an enum construction",
            "mod m { pub fn thing(n: int) -> int { return n } }\nlet x = m::thing(1)\n",
        ),
        (
            "undeclared struct is left to the runtime",
            "let p = Q { v: 1 }\n",
        ),
        (
            "untyped field has nothing to check against",
            "struct P { v }\nlet p = P { v: \"s\" }\n",
        ),
        (
            "unit variant",
            "enum C { R, G, B }\nlet c = C::G\n",
        ),
        (
            "payload variant with its values",
            "enum C { N(int, string) }\nlet c = C::N(1, \"a\")\n",
        ),
        (
            "int literal satisfies an f64 field",
            "struct P { v: f64 }\nlet p = P { v: 1 }\n",
        ),
        (
            "enum construction in a match arm",
            "enum C { R, G, B }\nlet c = C::G\nmatch c { C::R => { dump 1 } C::G => { dump 2 } C::B => { dump 3 } }\n",
        ),
    ] {
        let diags = check_diags(src);
        assert!(
            diags.is_empty(),
            "{}: unexpected diagnostics {:#?}",
            label,
            diags
        );
    }
}
