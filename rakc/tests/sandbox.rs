//! The capability sandbox, tested as a boundary rather than as a table.
//!
//! The sandbox is the mechanism the language offers for running a script it did
//! not write: something fetched with `oyvey add <user/repo>`, or received from
//! a third party. Its value is entirely in whether it holds, so the tests here
//! are written against the *observable* behaviour — a program that tries to
//! reach outside the sandbox must fail — and not against the shape of the
//! capability table.
//!
//! Several of these are regression tests for holes that were found by running
//! the compiler rather than by reading the code:
//!
//! - `rakc vm --sandbox program.rkbc` enabled no sandbox at all, because the
//!   bytecode branch returned from `main` before the sandbox was switched on.
//!   The same program, run from source, was correctly refused a capability.
//! - `log_init` opened a file for writing and was not in the `fs_write` list,
//!   so a sandboxed script with no capabilities could create files.
//! - `reverse_dns` and `scan_subdomains` did real DNS work and were not in the
//!   `net` list.
//! - `extern_call` was ungated, and `rakc bindgen` generates calls to it.
//! - The `pcap_` gate named `pcap_listen`, a builtin that does not exist,
//!   while the two that do exist were ungated.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The sandbox is process-global state, so the in-process tests take a lock.
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A scratch directory unique to this test binary and this run.
fn scratch(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("rak_sandbox_{}_{}_{}", std::process::id(), n, tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the scratch directory");
    dir
}

/// Write a Rak program into `dir` and return its path.
fn write_program(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).expect("write the program");
    path
}

/// Run `rakc` with `args`, and return `(stdout, stderr)`.
fn rakc(args: &[&str]) -> (String, String) {
    rakc_in(None, args)
}

/// Run `rakc` with `args` in `dir`, and return `(stdout, stderr)`.
///
/// Rak resolves a relative path against the *current directory*, not against
/// the directory the program happens to live in, so a test that uses relative
/// paths has to run the program from the directory it means.
fn rakc_in(dir: Option<&Path>, args: &[&str]) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rakc"));
    cmd.args(args);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd.output().unwrap_or_else(|e| panic!("spawn rakc: {e}"));
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Run a program under the sandbox and return `(stdout, stderr)`.
///
/// The program runs in `dir`, so a relative path in the program means what
/// the test thinks it means.
fn sandboxed(program: &Path, extra: &[&str]) -> (String, String) {
    let mut args: Vec<String> = vec!["run".into(), "--sandbox".into()];
    args.extend(extra.iter().map(|s| s.to_string()));
    args.push(program.to_string_lossy().to_string());
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let dir = program.parent().map(|p| p.to_path_buf());
    rakc_in(dir.as_deref(), &refs)
}

/// Assert that a sandboxed program was refused, and say why it must be.
///
/// The message is checked as well as the outcome, because a program that
/// fails for an unrelated reason — a parse error, a typo — has not been
/// tested at all.
fn assert_blocked(label: &str, stdout: &str, stderr: &str, expect: &str) {
    let combined = format!("{stdout}{stderr}");
    assert!(
        !combined.contains("[DUMP]"),
        "{label}: the program ran to completion, so the sandbox did not stop it:\n{combined}"
    );
    assert!(
        combined.contains(expect),
        "{label}: expected {expect:?} in the output, got:\n{combined}"
    );
}

// ---------------------------------------------------------------------------
// The capability table
// ---------------------------------------------------------------------------

/// With no capability granted, every privileged family is denied.
#[test]
fn every_privileged_family_is_denied_by_default() {
    let _guard = TEST_LOCK.lock().unwrap();
    rakc::caps::enable("");
    for name in [
        // net
        "net_connect",
        "tcp_read",
        "udp_send",
        "http_get_async",
        "ws_connect",
        "dns_query",
        "dns_lookup",
        "reverse_dns",
        "scan_ports",
        "scan_subdomains",
        "whois_lookup",
        "ct_subdomains",
        "fetch",
        // filesystem
        "file_read",
        "file_read_bytes",
        "read",
        "read_lines",
        "stream_csv",
        "stream_jsonl",
        "mmap_open",
        "file_list",
        "file_exists",
        "file_size",
        "zip_list",
        "zip_read",
        "file_write",
        "file_append",
        "file_write_bytes",
        "file_append_bytes",
        "zip_write",
        "mmap_write",
        "file_delete",
        "file_mkdir",
        "file_rename",
        "file_copy",
        "log_init",
        "write",
        "dump:file_write",
        // the rest
        "process_spawn",
        "ffi_load",
        "ffi_call",
        "extern_call",
        "ffi:extern",
        "net_raw_send",
        "net_raw_recv",
        "pcap_open",
        "pcap_next",
        "gui_window",
        "secret_get",
        "env_get",
        "asm",
    ] {
        assert!(
            rakc::caps::check_str(name).is_err(),
            "{name} should be denied with no capabilities granted"
        );
    }
    rakc::caps::disable();
}

/// The pure builders must stay reachable, or the sandbox is unusable.
///
/// These cannot touch the outside world: they construct bytes, parse strings
/// the script already has, or compute over data it already holds. Gating them
/// pushes users towards granting `net` for everything, which is the exact
/// opposite of what the sandbox is for.
#[test]
fn the_pure_builders_stay_reachable() {
    let _guard = TEST_LOCK.lock().unwrap();
    rakc::caps::enable("");
    for name in [
        "net_raw_ipv4",
        "net_raw_tcp",
        "net_raw_udp",
        "net_raw_tcp_syn",
        "net_raw_csum",
        "net_raw_icmp",
        "net_raw_arp_request",
        "net_raw_arp_reply",
        "net_raw_arp_parse",
        "dns_build",
        "dns_parse",
        "whois_parse",
        "subdomain_enum",
        "tunnel_frame",
        "tunnel_unframe",
        "tunnel_nonce",
        "tunnel_preshared_key",
        "kdf_next",
        "tls_parse_client_hello",
        "sha256",
        "md5",
        "hmac_sha256",
        "aes_gcm_encrypt",
        "ed25519_sign",
        "ct_eq",
        "ct_eq_hex",
        "ct_select",
        "yara_scan",
        "hex_encode",
        "base64_encode",
        "json_parse",
        "csv_parse",
        "sql_escape",
        "shell_escape",
        "html_escape",
        "regex_new",
        "len",
        "sort",
        "abs",
    ] {
        assert!(
            rakc::caps::check_str(name).is_ok(),
            "{name} should stay reachable inside a sandbox: it cannot touch the outside world"
        );
    }
    rakc::caps::disable();
}

/// Granting one capability must not grant another.
#[test]
fn granting_one_capability_does_not_grant_another() {
    let _guard = TEST_LOCK.lock().unwrap();
    rakc::caps::enable("fs_read");
    assert!(rakc::caps::check_str("file_read").is_ok());
    assert!(rakc::caps::check_str("file_write").is_err());
    assert!(rakc::caps::check_str("net_connect").is_err());
    assert!(rakc::caps::check_str("secret_get").is_err());
    rakc::caps::disable();
}

// ---------------------------------------------------------------------------
// End to end: a program that tries to leave the sandbox
// ---------------------------------------------------------------------------

/// A program that tries to open a network connection is refused.
#[test]
fn a_network_builtin_is_refused() {
    let dir = scratch("net");
    let program = write_program(&dir, "net.rak", "dump net_connect(\"example.com\", 80)\n");
    let (out, err) = sandboxed(&program, &[]);
    assert_blocked("run", &out, &err, "sandbox: builtin 'net_connect' blocked");
}

/// The same program, compiled to bytecode, is refused too.
///
/// This is the regression that matters: the bytecode path used to return from
/// `main` before the sandbox was enabled, so compiling the program first was
/// enough to switch the sandbox off. And `.rkbc` is the format Rak ships, so
/// that was the *likely* path rather than an edge case.
#[test]
fn a_compiled_program_is_refused_just_like_the_source() {
    let dir = scratch("rkbc");
    let program = write_program(&dir, "net.rak", "dump net_connect(\"example.com\", 80)\n");

    // Compile it to bytecode first.
    let (cout, cerr) = rakc_in(
        Some(&dir),
        &["build", "--emit=bytecode", &program.to_string_lossy()],
    );
    assert!(
        cout.contains("Emitted") || cerr.contains("Emitted"),
        "the bytecode build did not report an artifact:\n{cout}\n{cerr}"
    );
    let rkbc = program.with_extension("rkbc");
    assert!(rkbc.exists(), "expected {} to exist", rkbc.display());

    // Source: refused.
    let (out, err) = sandboxed(&program, &[]);
    assert_blocked(
        "source",
        &out,
        &err,
        "sandbox: builtin 'net_connect' blocked",
    );

    // Bytecode: refused, the same way.
    let (out, err) = rakc_in(Some(&dir), &["vm", "--sandbox", &rkbc.to_string_lossy()]);
    assert_blocked(
        "bytecode",
        &out,
        &err,
        "sandbox: builtin 'net_connect' blocked",
    );
}

/// `log_init` opens a file for appending. Under the sandbox it must not, and
/// the file must not appear.
#[test]
fn log_init_cannot_write_a_file_under_the_sandbox() {
    let dir = scratch("log");
    let program = write_program(&dir, "log.rak", "log_init(\"proof.txt\")\n");
    let (out, err) = sandboxed(&program, &[]);
    assert_blocked("run", &out, &err, "sandbox: builtin 'log_init' blocked");
    assert!(
        !dir.join("proof.txt").exists(),
        "the sandbox blocked the call but the file was still created"
    );
}

/// A filesystem write is refused.
#[test]
fn a_filesystem_write_is_refused() {
    let dir = scratch("write");
    let program = write_program(&dir, "w.rak", "file_write(\"out.txt\", \"x\")\n");
    let (out, err) = sandboxed(&program, &[]);
    assert_blocked("run", &out, &err, "sandbox: builtin 'file_write' blocked");
    assert!(!dir.join("out.txt").exists());
}

/// Reading a file needs its own capability, separate from writing one.
#[test]
fn reading_a_file_needs_the_fs_read_capability() {
    let dir = scratch("read");
    std::fs::write(dir.join("secret.txt"), "classified").unwrap();
    let program = write_program(&dir, "r.rak", "dump file_read(\"secret.txt\")\n");

    // Denied by default.
    let (out, err) = sandboxed(&program, &[]);
    assert_blocked("run", &out, &err, "sandbox: builtin 'file_read' blocked");

    // Granted, it reads.
    let (out, _err) = sandboxed(&program, &["--allow", "fs_read"]);
    assert!(
        out.contains("classified"),
        "with fs_read granted the read should succeed, got:\n{out}"
    );
}

/// `--fs-root` confines the program to one directory.
#[test]
fn the_fs_root_confines_a_read() {
    let dir = scratch("root");
    let inside = dir.join("work");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(inside.join("ok.txt"), "inside").unwrap();
    std::fs::write(dir.join("outside.txt"), "outside").unwrap();

    let read_inside = write_program(&dir, "in.rak", "dump file_read(\"work/ok.txt\")\n");
    let read_outside = write_program(&dir, "out.rak", "dump file_read(\"outside.txt\")\n");

    // Inside the root: allowed.
    let (out, err) = sandboxed(&read_inside, &["--allow", "fs_read", "--fs-root", "work"]);
    assert!(
        out.contains("inside"),
        "a read inside the scope should be allowed, got:\n{out}\n{err}"
    );

    // Outside the root: refused.
    let (out, err) = sandboxed(&read_outside, &["--allow", "fs_read", "--fs-root", "work"]);
    assert_blocked("run", &out, &err, "outside the filesystem scope");
}

/// `..` must not be a way out of the scope.
#[test]
fn the_fs_root_refuses_a_dot_dot_escape() {
    let dir = scratch("escape");
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(dir.join("outside.txt"), "outside").unwrap();

    let program = write_program(&dir, "esc.rak", "dump file_read(\"work/../outside.txt\")\n");
    let (out, err) = sandboxed(&program, &["--allow", "fs_read", "--fs-root", "work"]);
    assert_blocked("run", &out, &err, "outside the filesystem scope");
}

/// Without `--fs-root` there is no scope, so the scope must not get in the
/// way of an ordinary sandboxed program.
#[test]
fn no_fs_root_means_no_scope() {
    let dir = scratch("noscope");
    std::fs::write(dir.join("f.txt"), "content").unwrap();
    let program = write_program(&dir, "r.rak", "dump file_read(\"f.txt\")\n");
    let (out, _err) = sandboxed(&program, &["--allow", "fs_read"]);
    assert!(
        out.contains("content"),
        "without --fs-root a granted read should work, got:\n{out}"
    );
}

/// `--allow all` has to grant the filesystem-read capability too, or it is a
/// weaker sandbox than it reads.
#[test]
fn allow_all_grants_everything() {
    let _guard = TEST_LOCK.lock().unwrap();
    rakc::caps::enable("all");
    for name in [
        "net_connect",
        "file_read",
        "file_write",
        "log_init",
        "process_spawn",
        "ffi_call",
        "extern_call",
        "pcap_open",
        "secret_get",
        "asm",
    ] {
        assert!(
            rakc::caps::check_str(name).is_ok(),
            "--allow all should grant {name}"
        );
    }
    rakc::caps::disable();
}

/// The sandbox is off unless it is asked for. A program that is not sandboxed
/// is not sandboxed — that is the contract, and it is what makes `--sandbox`
/// meaningful rather than decorative.
#[test]
fn the_sandbox_is_off_unless_asked_for() {
    let _guard = TEST_LOCK.lock().unwrap();
    rakc::caps::disable();
    assert!(rakc::caps::check_str("net_connect").is_ok());
    assert!(rakc::caps::check_str("file_write").is_ok());
    assert!(!rakc::caps::is_active());
}
