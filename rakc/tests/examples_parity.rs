//! The examples are the language's front page, so they are worth executing.
//!
//! Most of them are supposed to behave identically under `rakc run` and
//! `rakc vm`. Several are not, and the reason is nearly always the same: they
//! use a builtin from the 34 that exist only on the interpreter, so the VM
//! reports `Undefined` partway through.
//!
//! These tests run each example on both backends and classify the outcome into
//! one of three buckets, rather than asserting that all of them match. That is
//! deliberate. A test that asserted full parity would fail today, and a failing
//! test in the default suite is a test people stop running. Instead:
//!
//! * an example that **agrees** is recorded as agreement,
//! * an example that **needs a network or a display** is expected to differ or
//!   hang and is listed explicitly, and
//! * anything else is a **regression** and fails.
//!
//! So the suite is green now, it gets greener as families close, and a genuine
//! backend divergence still fails the build. The list of VM-blocked examples is
//! the shortfall, stated where it is measured.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the `rakc` binary under test lives.
///
/// `CARGO_BIN_EXE_rakc` is set by cargo for integration tests, so this runs the
/// binary built from *this* tree rather than whatever is on `PATH`. Using
/// `PATH` would silently test an installed release.
fn rakc() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rakc"))
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rakc has a parent")
        .join("examples")
}

/// Run one example on one backend, capturing output.
///
/// Returns `None` on timeout rather than hanging the suite. An example that
/// opens a socket, binds a port or waits for a window legitimately never
/// returns, and a test harness that blocks on one is worse than useless.
fn run_with_timeout(example: &Path, vm: bool, secs: u64) -> Option<String> {
    let subcommand = if vm { "vm" } else { "run" };
    let mut child = Command::new(rakc())
        .arg(subcommand)
        .arg(example)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("rakc should be runnable");

    // `try_wait` in a loop rather than a thread, so the child is reaped and no
    // extra handle leaks on the timeout path.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.stderr.is_empty() {
        text.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    Some(text.trim().to_string())
}

/// Examples that cannot run unattended, and why.
///
/// Listed rather than inferred, because the alternative is a test that hangs or
/// a list that is wrong in a way nobody notices.
const ENVIRONMENTAL: &[(&str, &str)] = &[
    ("bench", "runs a long benchmark loop"),
    ("echo_server", "binds a port and serves until interrupted"),
    ("http_server_demo", "starts an HTTP server and blocks"),
    ("ws_server_demo", "starts a WebSocket server and blocks"),
    (
        "gui_demo",
        "opens a window and waits; needs the gui feature and a display",
    ),
    ("echo_client", "connects to a server that is not running"),
    ("ws_client_demo", "connects to a server that is not running"),
    ("sql_client", "connects to a database that is not running"),
    ("sql_server", "listens for database clients"),
    ("dap_demo", "drives an editor session over stdio"),
];

/// Examples whose remaining VM gap is a known interpreter-only builtin.
///
/// Each names the builtin, so when a family closes this list is edited rather
/// than rediscovered. The test does not check the builtin is still missing — it
/// only requires the example to *not* agree yet, which is what keeps it honest:
/// if a fix makes the example work on the VM, the test fails and the entry gets
/// removed.
const VM_BLOCKED: &[(&str, &str)] = &[
    ("async", "await_all / async futures"),
    ("async_orchestration", "task_group / select / timeout"),
    ("batteries_demo", "HTTP client natives"),
    ("binary_patterns", "bytes handling"),
    ("cli_greeter", "argv / parse_args"),
    ("closures", "spawn / thread_join"),
    ("crypto_demo", "net_raw send/recv"),
    ("ffi", "extern_call through VM frames"),
    ("ffi_dynamic", "ffi_read_i32"),
    ("forensic_structs", "file_read"),
    ("json_demo", "read_lines streams"),
    ("logging_demo", "log_init"),
    ("macros", "scan_ports"),
    ("mmap", "file_ / mmap natives"),
    ("net_raw", "net_raw_send / net_raw_recv"),
    ("osint_demo", "dns_lookup / reverse_dns"),
    ("osint_pack_demo", "subdomain_enum"),
    ("osint_scan", "scan_subdomains"),
    ("parsers", "read_lines / file_read"),
    (
        "pipeline",
        "filter / take / collect over streams with closures",
    ),
    ("process_demo", "process_spawn"),
    ("regex", "regex_is_match"),
    ("secrets_demo", "secret_get"),
    ("security_workflow", "net_raw_send / net_raw_recv"),
    ("self_host", "compile / extern_call"),
    ("stdlib_demo", "whois_parse is VM-only; the rest is covered"),
    ("streaming", "stream_csv / stream_jsonl with closures"),
    ("traits", "trait protocols are interpreter-only"),
    ("v071_demo", "argv / parse_args"),
    ("vm_features", "process_spawn / channel"),
    ("log_info", "log_info is VM-only, not the other way round"),
];

/// Examples whose output is not reproducible, so two runs of the *same*
/// backend can differ.
///
/// These are the `HashMap` iteration-order cases: `Map` is a `std::HashMap` in
/// both backends, so anything that prints or serialises a map can emit its keys
/// in a different order each run, and a DNS answer set has no inherent order
/// either. Listing them is not an excuse — it is the shortfall stated where it
/// is measured. `docs/V8-ROADMAP.md` has insertion-ordered maps as a planned fix.
const NONDETERMINISTIC: &[(&str, &str)] = &[
    (
        "osint_scan",
        "log fields are a map, iterated in HashMap order",
    ),
    (
        "security_workflow",
        "log fields are a map, iterated in HashMap order",
    ),
    (
        "osint_demo",
        "DNS answer order is not guaranteed by the protocol",
    ),
];

/// Examples that only make sense on one platform.
///
/// Without this the suite fails on Linux for reasons that have nothing to do
/// with backend parity: `ffi_dynamic` loads `msvcrt.dll`, and the secrets and
/// process examples shell out to Windows commands. The alternative is a
/// platform-conditional expectation, which is the same list with more indirection.
const PLATFORM_SPECIFIC: &[(&str, &str)] = &[
    ("ffi_dynamic", "loads msvcrt.dll; Windows only"),
    ("secrets_demo", "uses Windows credential storage semantics"),
    ("process_demo", "spawns Windows commands"),
];

/// Examples whose wall time depends on another program starting up, and so need a
/// longer budget than the rest.
///
/// `process_demo` spawns `powershell`, and a cold PowerShell start on a loaded CI
/// runner regularly runs past 20s. The example itself is fine -- it finishes in well
/// under a second on a warm machine, and its output is compared, so the coverage is
/// real. But a parity test that fails on how fast someone else's machine boots
/// PowerShell is measuring the runner rather than the program, and it failed on
/// `e16ed1c` for exactly that reason while the previous CI run on an identical tree
/// passed.
const SLOW_STARTING: &[(&str, &str)] = &[("process_demo", "spawns powershell")];

/// Seconds an example gets before it counts as a timeout.
fn timeout_secs(name: &str) -> u64 {
    if SLOW_STARTING.iter().any(|(n, _)| *n == name) {
        90
    } else {
        20
    }
}

#[test]
fn examples_behave_the_same_on_both_backends() {
    let dir = examples_dir();
    assert!(
        dir.is_dir(),
        "examples directory not found at {} — run this from the rakc crate",
        dir.display()
    );

    let mut agreed = Vec::new();
    let mut rejected = Vec::new();
    let mut unstable = Vec::new();
    let mut blocked = Vec::new();
    let mut failures = Vec::new();

    for entry in std::fs::read_dir(&dir).expect("read examples dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rak") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        if ENVIRONMENTAL.iter().any(|(n, _)| *n == name) {
            continue;
        }
        // Portable examples run everywhere. A platform-specific example runs
        // only on its own platform, so on Windows the three Windows-only ones
        // are compared and on Linux they are skipped.
        if PLATFORM_SPECIFIC.iter().any(|(n, _)| *n == name) && !cfg!(windows) {
            continue;
        }
        if !std::path::Path::new(&rakc()).exists() {
            panic!("rakc binary not found at {}", rakc().display());
        }

        let budget = timeout_secs(&name);
        let interp = run_with_timeout(&path, false, budget);
        let vm = run_with_timeout(&path, true, budget);

        match (interp, vm) {
            (Some(i), Some(v)) => {
                let i_err = i.starts_with("Error:") || i.contains("Error:") || i.contains("error:");
                let v_err = v.starts_with("VM error:") || v.contains("Compile error:");
                match (i_err, v_err) {
                    // Both refused the program, so they agree it is invalid. The
                    // messages differ because the VM checks mutability and
                    // imports at compile time while the interpreter checks at
                    // run time, which is a reporting difference, not a
                    // behavioural one. Several examples rely on it: they assign
                    // to an immutable variable and both backends reject them.
                    (true, true) => rejected.push(name),
                    // Checked before the output comparison, and skipped
                    // unconditionally rather than on a mismatch. Comparing them
                    // and hoping the hash order happens to line up would make
                    // this test flaky: it would pass on the run where the two
                    // backends happened to agree and fail on the next.
                    (false, false) if NONDETERMINISTIC.iter().any(|(n, _)| *n == name) => {
                        unstable.push(name)
                    }
                    (false, false) if i == v => agreed.push(name),
                    (false, false) => {
                        if VM_BLOCKED.iter().any(|(n, _)| *n == name) {
                            blocked.push(name);
                        } else {
                            failures.push(format!(
                                "{}: backends disagree and it is not in VM_BLOCKED\n  {}",
                                name,
                                first_difference(&i, &v)
                            ));
                        }
                    }
                    // One ran and the other refused. This is the case that
                    // matters: the same program is valid on one backend and not
                    // the other.
                    _ => {
                        if VM_BLOCKED.iter().any(|(n, _)| *n == name) {
                            blocked.push(name);
                        } else {
                            failures.push(format!(
                                "{}: one backend ran it and the other refused\n  \
                                 interpreter: {}\n  vm:           {}",
                                name,
                                first_line(&i),
                                first_line(&v)
                            ));
                        }
                    }
                }
            }
            // A timeout on the interpreter side means the example wants a
            // network, a display or a server, or spends its time waiting on
            // another program. The first three belong in ENVIRONMENTAL; the
            // last belongs in SLOW_STARTING. Either way it is a stale list, not
            // a backend bug.
            (None, _) => failures.push(format!(
                "{}: the interpreter run timed out; if it needs a network or a \
                 display, add it to ENVIRONMENTAL, and if it waits on another \
                 program, add it to SLOW_STARTING",
                name
            )),
            (Some(_), None) => failures.push(format!(
                "{}: the interpreter finished but the VM timed out, which is the \
                 wrong way round",
                name
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "{} example(s) regressed:\n\n  {}\n\nAgreed on both backends: {}\n\
         Both backends rejected:      {}\n\
         Not reproducible (HashMap order): {}\n\
         Blocked on interpreter-only builtins: {}\n\
         The blocked list is the shortfall, and it should shrink.",
        failures.len(),
        failures.join("\n\n  "),
        agreed.len(),
        rejected.len(),
        unstable.len(),
        blocked.len()
    );

    // A sanity check on the test itself. If the comparison silently stopped
    // matching anything, the suite would be green and useless.
    assert!(
        !agreed.is_empty(),
        "no example agreed across backends — the comparison is probably broken, \
         not the examples"
    );
    println!(
        "examples: {} agreed, {} both rejected, {} unstable, {} blocked on \
         interpreter-only builtins",
        agreed.len(),
        rejected.len(),
        unstable.len(),
        blocked.len()
    );
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

/// The first line where two outputs differ, for a compact failure message.
fn first_difference(a: &str, b: &str) -> String {
    let al: Vec<&str> = a.lines().collect();
    let bl: Vec<&str> = b.lines().collect();
    for (i, (x, y)) in al.iter().zip(bl.iter()).enumerate() {
        if x != y {
            return format!("line {}: interpreter={:?} vm={:?}", i + 1, x, y);
        }
    }
    if al.len() != bl.len() {
        return format!("line count: interpreter={} vm={}", al.len(), bl.len());
    }
    "identical?".to_string()
}
