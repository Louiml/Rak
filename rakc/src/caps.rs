//! Sandbox capability gating (`rakc run --sandbox [--allow csv]`).
//!
//! When the sandbox is enabled, dangerous builtin families are denied unless
//! explicitly re-granted with `--allow`. This exists so investigators can run
//! scripts fetched with `oyvey add <user/repo>` (or received from a third
//! party) without giving the script network, process, FFI, raw-socket, GUI,
//! secrets, or filesystem access by default.
//!
//! Enforcement is a single point of control for *capabilities*: every
//! interpreter builtin flows through `Interpreter::eval_builtin` and every VM
//! native flows through `Vm::call_value`, so name-based gating at those heads
//! covers all of them. `extern "C"` declarations are gated at the
//! foreign-call site (`ffi:extern`).
//!
//! ## Two kinds of question
//!
//! A capability answers *"is this operation allowed at all?"*, which is a
//! question about the **name** of a builtin, and is answered here.
//!
//! `--fs-root` answers a different question: *"of the paths this operation is
//! allowed to touch, which ones?"*, which is a question about a **path**. A
//! path cannot be checked from a name, so the scope check lives one layer
//! down, in `rak_stdlib::fs_scope`, at the functions that actually open
//! files. See that module for the details.
//!
//! ## Why the table is explicit rather than a family prefix
//!
//! A prefix table over free-form names has one failure mode, and it is the
//! one that bit this file repeatedly: **every new builtin is privileged until
//! somebody remembers to add it to a list.** `log_init` wrote a file, and
//! `reverse_dns` and `scan_subdomains` opened DNS queries, and none of the
//! three was in the table. A prefix cannot fix that, because a prefix says
//! what a name *starts* with and these names start with nothing in
//! particular.
//!
//! So the families below are spelled out where the family is not uniform, and
//! a prefix is used only where every member of the family really does share
//! the capability. The pure builders — packet construction, TLS parsing,
//! hashing, DNS *building* — are deliberately not gated, because they cannot
//! touch the outside world.

use std::path::PathBuf;
use std::sync::RwLock;

/// Capability flags. When the sandbox is active, `true` = allowed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sandbox {
    pub net: bool,
    /// Reading a file's contents. Separate from `fs_write` because the two
    /// are granted for different reasons: a script that may write a report
    /// has no business reading the private keys on the same machine.
    pub fs_read: bool,
    pub fs_write: bool,
    pub process: bool,
    pub ffi: bool,
    pub raw_sockets: bool,
    pub gui: bool,
    pub secrets: bool,
    /// Inline assembly. Separate from `ffi` and `raw_sockets` on purpose — see
    /// the note at the `asm` entry in `required_cap`.
    pub asm: bool,
}

static ACTIVE: RwLock<Option<Sandbox>> = RwLock::new(None);

/// Is the sandbox currently enabled for this process?
pub fn is_active() -> bool {
    ACTIVE.read().unwrap().is_some()
}

/// Enable the sandbox. `allow` is a comma-separated list of capability names
/// (`net,fs_read,fs_write,process,ffi,raw,gui,secrets`) or `all`.
pub fn enable(allow: &str) {
    let mut sb = Sandbox::default();
    for cap in allow.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match cap {
            "all" => {
                sb = Sandbox {
                    net: true,
                    fs_read: true,
                    fs_write: true,
                    process: true,
                    ffi: true,
                    raw_sockets: true,
                    gui: true,
                    secrets: true,
                    asm: true,
                }
            }
            "net" => sb.net = true,
            "fs_read" => sb.fs_read = true,
            "fs" | "fs_write" => sb.fs_write = true,
            "process" | "proc" => sb.process = true,
            "ffi" => sb.ffi = true,
            "raw" | "raw_sockets" => sb.raw_sockets = true,
            "gui" => sb.gui = true,
            "secrets" => sb.secrets = true,
            "asm" => sb.asm = true,
            other => eprintln!("sandbox: unknown capability '{}' (ignored)", other),
        }
    }
    *ACTIVE.write().unwrap() = Some(sb);
}

/// Disable the sandbox (mainly for tests).
pub fn disable() {
    *ACTIVE.write().unwrap() = None;
}

/// Set the filesystem scope: the only directory a sandboxed script may
/// touch.
///
/// `None` clears the scope, which is the default — an unsandboxed run has
/// none. The root is canonicalised on the way in, so a relative path is
/// resolved once and a symlinked root is compared by its real location.
pub fn set_fs_root(root: Option<PathBuf>) {
    rak_stdlib::fs_scope::set_root(root);
}

/// The filesystem scope currently in force, if any.
pub fn fs_root() -> Option<PathBuf> {
    rak_stdlib::fs_scope::root()
}

/// Map a builtin name to the capability it requires, if any.
///
/// Pure builders (packet/header construction, TLS parsing, hashing) are
/// intentionally not gated — they cannot touch the outside world.
///
/// The `raw_sockets` list is the only one that names individual builtins
/// rather than using a prefix. That is deliberate: `net_raw_` also covers the
/// pure builders, which must stay available in a sandbox, so the gate names
/// exactly the two that open a socket. If you add another socket-opening
/// builtin, add it here or it will be reachable from a sandboxed script.
fn required_cap(name: &str) -> Option<&'static str> {
    if name == "ffi:extern" {
        return Some("ffi");
    }
    let table: &[(&str, &[&str])] = &[
        (
            "net",
            &[
                "net_listen",
                "net_accept",
                "net_connect",
                "net_local_addr",
                // Every TCP/UDP/HTTP/WebSocket I/O builtin.
                "tcp_",
                "udp_",
                "http_",
                "ws_",
                // The DNS family is split rather than prefixed: `dns_build`
                // and `dns_parse` are pure wire-format helpers and must stay
                // reachable inside a sandbox, while the query family talks
                // to a resolver.
                "dns_lookup",
                "dns_query",
                "dns_resolve",
                "dns_reverse",
                "dns_records",
                "dns_walk",
                // `reverse_dns` is a PTR query, so it is networked — but its
                // name starts with "reverse", not "dns", so a `dns_` prefix
                // never caught it. Same for `scan_subdomains`, which sits
                // next to the gated `scan_ports` and did real DNS lookups
                // while ungated.
                "reverse_dns",
                // Network-capable investigation builtins.
                "scan_ports",
                "scan_subdomains",
                "fetch",
                "ct_subdomains",
                // `whois_lookup` is networked; `whois_parse` is a pure text
                // parser over a string the script already has.
                "whois_lookup",
                // `tunnel_` is deliberately *not* listed. Every `tunnel_*`
                // builtin is pure cryptography — framing, nonce derivation,
                // key derivation — and none of them opens a socket. The
                // `tunnel` statement itself goes through `udp_bind` and
                // `udp_send`, which are gated above.
            ],
        ),
        ("process", &["process_"]),
        // `extern_call` is the third FFI surface, after `ffi_call` and
        // `extern "C"`. It is a stub on both backends today, but
        // `rakc bindgen` *generates* calls to it, so it becomes an ungated
        // FFI escape the moment the stub is implemented. Gate it now.
        ("ffi", &["ffi_", "extern_call"]),
        // Socket-opening only. `net_raw_ipv4`, `net_raw_tcp`, `net_raw_icmp`,
        // `net_raw_arp_*` and `net_raw_csum` are pure byte construction and
        // are deliberately reachable from a sandbox.
        //
        // `pcap_` is a prefix rather than a single name because the gate
        // used to name `pcap_listen` — a builtin that does not exist — while
        // the ones that do exist, `pcap_open` and `pcap_next`, were
        // ungated. The test that "proved" the gate worked was asserting a
        // property of a name no code could reach.
        ("raw_sockets", &["net_raw_send", "net_raw_recv", "pcap_"]),
        ("gui", &["gui_", "window_"]),
        ("secrets", &["secret_", "env_get", "env_set"]),
        // Inline assembly. A separate capability rather than a reuse of `ffi`
        // or `raw` on purpose: `ffi` is about calling named functions through
        // a library Rak chose, and `raw` is about packet bytes. Assembly is
        // arbitrary machine code, which can do either of those and a great
        // deal more, including anything the sandbox has not thought to name.
        // A capability that is granted in the same breath as `raw` would be
        // granted by habit.
        ("asm", &["asm"]),
        (
            "fs_read",
            &[
                // `read` is a prefix, so it also covers `read_lines`. Those
                // two are the only names that start with it.
                "read",
                "file_read",
                "read_lines",
                // The streaming readers open a file under the hood.
                "stream_csv",
                "stream_jsonl",
                // A memory map is a file the script can read the whole of.
                "mmap_open",
                // Directory enumeration and metadata. `file_exists` and
                // `file_size` are only a bit and a number, but they are
                // reconnaissance of the filesystem, and a script that may
                // not read a file has no business asking whether one is
                // there.
                "file_list",
                "file_exists",
                "file_size",
                // The zip *readers* take a path, so they read a file. The
                // writer does not, which is why `zip_write` is under
                // `fs_write` and these two are here.
                "zip_list",
                "zip_read",
            ],
        ),
        (
            "fs_write",
            &[
                "file_write",
                "file_append",
                "zip_write",
                // Byte-exact writers, and writing through a mapping. Each of these
                // modifies a file, so gating `file_write` while leaving these
                // ungated would have been a hole in the sandbox rather than a
                // narrower surface.
                "file_write_bytes",
                "file_append_bytes",
                "mmap_write",
                // The mutators that were missing. Deleting, renaming, copying and
                // creating directories all change the filesystem exactly as much
                // as writing a file does, so a sandbox that gated `file_write` but
                // not these four was not a narrower surface -- it was a different
                // one. Verified: under an active sandbox `file_delete` and
                // `file_mkdir` both completed with exit 0.
                "file_delete",
                "file_mkdir",
                "file_rename",
                "file_copy",
                // `log_init` opens a file append-only for writing. It is not a
                // `file_`-prefixed name, so it was missed by the list above
                // and a sandboxed script could create and append to arbitrary
                // files. Gating `file_write` while leaving this open is a hole
                // in the sandbox, not a narrower surface.
                "log_init",
                // `write` is the shortest mutator name there is and is the one
                // most likely to be reached for first. It is the only registered
                // builtin beginning with "write", so using it as a prefix does not
                // over-block.
                "write",
                // `dump x, "path.txt"` writes a file; gate it like one.
                "dump:file_write",
            ],
        ),
    ];
    for (cap, prefixes) in table {
        for p in *prefixes {
            if name.starts_with(p) {
                return Some(cap);
            }
        }
    }
    None
}

/// VM-side check (VM errors are plain strings).
pub fn check_str(name: &str) -> Result<(), String> {
    let sb = match &*ACTIVE.read().unwrap() {
        Some(s) => *s,
        None => return Ok(()), // sandbox off: everything allowed
    };
    let cap = match required_cap(name) {
        Some(c) => c,
        None => return Ok(()),
    };
    let allowed = match cap {
        "net" => sb.net,
        "fs_read" => sb.fs_read,
        "fs_write" => sb.fs_write,
        "process" => sb.process,
        "ffi" => sb.ffi,
        "raw_sockets" => sb.raw_sockets,
        "gui" => sb.gui,
        "secrets" => sb.secrets,
        "asm" => sb.asm,
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "sandbox: builtin '{}' blocked — capability '{}' not granted (rerun with --allow {})",
            name, cap, cap
        ))
    }
}

/// Check a builtin that takes a path: the capability, and then the scope.
///
/// The capability half is the same name-based check as [`check_str`]. The
/// scope half is a path check against `--fs-root`, and it is the only part
/// that cannot live in the name table.
pub fn check_path(name: &str, path: &str) -> Result<(), String> {
    check_str(name)?;
    rak_stdlib::fs_scope::check(path)
}

/// Check only the filesystem scope, for a call site whose capability has
/// already been checked by the name-based head.
pub fn check_fs_root(path: &str) -> Result<(), String> {
    rak_stdlib::fs_scope::check(path)
}

/// Interpreter-side check (uses `RakError`).
pub fn check_builtin(name: &str) -> crate::Result<()> {
    check_str(name).map_err(crate::RakError::Runtime)
}

/// Interpreter-side path check (uses `RakError`).
pub fn check_builtin_path(name: &str, path: &str) -> crate::Result<()> {
    check_path(name, path).map_err(crate::RakError::Runtime)
}

/// The sandbox options a command line can carry.
#[derive(Debug, Clone, Default)]
pub struct SandboxOptions {
    /// Was `--sandbox` given?
    pub enabled: bool,
    /// The `--allow` list, verbatim.
    pub allow: String,
    /// `--max-depth N`, if given.
    pub max_depth: Option<u32>,
    /// `--fs-root DIR`, if given.
    pub fs_root: Option<String>,
}

/// Parse `--sandbox` / `--allow <csv>` / `--max-depth N` / `--fs-root <dir>`
/// out of trailing CLI args, returning the options and the remaining args.
///
/// `--max-depth` lives here rather than in `main` for one reason: this function
/// is what removes flags from the script's own argv, so a depth flag parsed
/// anywhere else would be handed to the program.
pub fn parse_cli(rest: &[String]) -> (SandboxOptions, Vec<String>) {
    let mut opts = SandboxOptions::default();
    let mut clean = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--sandbox" => opts.enabled = true,
            "--allow" => {
                i += 1;
                if let Some(v) = rest.get(i) {
                    opts.allow = v.clone();
                }
            }
            "--max-depth" => {
                i += 1;
                match rest.get(i).and_then(|v| v.parse::<u32>().ok()) {
                    Some(d) if d > 0 => opts.max_depth = Some(d),
                    _ => eprintln!("--max-depth needs a positive number, e.g. --max-depth 512"),
                }
            }
            "--fs-root" => {
                i += 1;
                match rest.get(i) {
                    Some(v) if !v.is_empty() => opts.fs_root = Some(v.clone()),
                    _ => eprintln!("--fs-root needs a directory, e.g. --fs-root ./work"),
                }
            }
            other => clean.push(other.to_string()),
        }
        i += 1;
    }
    (opts, clean)
}

/// List the recognized sandbox capabilities (for `--help` text).
pub fn capability_names() -> &'static [&'static str] {
    &[
        "net", "fs_read", "fs_write", "process", "ffi", "raw", "gui", "secrets", "asm", "all",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // The sandbox is process-global state; serialize these tests.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn sandbox_blocks_net_but_not_math() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("net_connect").is_err());
        assert!(check_str("tcp_read").is_err());
        assert!(check_str("process_spawn").is_err());
        assert!(check_str("ffi_load").is_err());
        assert!(check_str("file_write").is_err());
        assert!(check_str("secret_get").is_err());
        assert!(check_str("len").is_ok());
        assert!(check_str("sha256").is_ok());
        disable();
        assert!(check_str("net_connect").is_ok());
    }

    #[test]
    fn allow_regrants() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("net,ffi");
        assert!(check_str("net_connect").is_ok());
        assert!(check_str("ffi_load").is_ok());
        assert!(check_str("process_spawn").is_err());
        disable();
    }

    #[test]
    fn parse_cli_strips_flags() {
        let args = vec![
            "hello".to_string(),
            "--sandbox".to_string(),
            "--allow".to_string(),
            "net,ffi".to_string(),
            "world".to_string(),
        ];
        let (opts, clean) = parse_cli(&args);
        assert!(opts.enabled);
        assert_eq!(opts.allow, "net,ffi");
        assert_eq!(clean, vec!["hello".to_string(), "world".to_string()]);
    }

    /// `--fs-root` takes a value, and the value must not be handed to the
    /// program as an argument.
    #[test]
    fn parse_cli_reads_the_fs_root() {
        let args = vec![
            "--fs-root".to_string(),
            "/tmp/work".to_string(),
            "script.rak".to_string(),
        ];
        let (opts, clean) = parse_cli(&args);
        assert_eq!(opts.fs_root.as_deref(), Some("/tmp/work"));
        assert_eq!(clean, vec!["script.rak".to_string()]);
    }

    /// Pure packet builders run no code and open no socket, so they must stay
    /// available inside a sandbox. Only send/recv need `raw`.
    #[test]
    fn packet_builders_are_allowed_but_send_recv_are_not() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        // Pure construction: fine.
        assert!(check_str("net_raw_ipv4").is_ok());
        assert!(check_str("net_raw_tcp").is_ok());
        assert!(check_str("net_raw_udp").is_ok());
        assert!(check_str("net_raw_tcp_syn").is_ok());
        assert!(check_str("net_raw_csum").is_ok());
        assert!(check_str("net_raw_icmp").is_ok());
        assert!(check_str("net_raw_icmp_ping").is_ok());
        assert!(check_str("net_raw_arp_request").is_ok());
        assert!(check_str("net_raw_arp_reply").is_ok());
        assert!(check_str("net_raw_arp_parse").is_ok());
        // Socket-opening: denied.
        assert!(check_str("net_raw_send").is_err());
        assert!(check_str("net_raw_recv").is_err());
        disable();
    }

    /// The PCAP gate has to name a builtin that exists.
    ///
    /// It used to name `pcap_listen`, which no code registers, while the two
    /// that *are* registered — `pcap_open` and `pcap_next` — were ungated.
    /// The test that "proved" the gate worked was asserting a property of a
    /// name nothing could reach.
    #[test]
    fn the_pcap_gate_names_builtins_that_exist() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("pcap_open").is_err());
        assert!(check_str("pcap_next").is_err());
        enable("raw");
        assert!(check_str("pcap_open").is_ok());
        assert!(check_str("pcap_next").is_ok());
        disable();
    }

    /// `log_init` opens a file append-only for writing. It was not in the
    /// `fs_write` list, so a sandboxed script with no capabilities could
    /// create and append to arbitrary files.
    #[test]
    fn log_init_needs_the_fs_write_capability() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("log_init").is_err());
        enable("fs_write");
        assert!(check_str("log_init").is_ok());
        disable();
    }

    /// `reverse_dns` and `scan_subdomains` do real DNS work. Neither name
    /// starts with a prefix the `net` list used to carry, so both were
    /// reachable from a sandbox with no network capability.
    #[test]
    fn the_dns_family_is_gated_by_name_not_by_prefix() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("reverse_dns").is_err());
        assert!(check_str("scan_subdomains").is_err());
        assert!(check_str("scan_ports").is_err());
        assert!(check_str("dns_lookup").is_err());
        assert!(check_str("dns_query").is_err());
        assert!(check_str("whois_lookup").is_err());
        assert!(check_str("ct_subdomains").is_err());
        // The pure half of those families stays reachable: they cannot
        // touch the outside world.
        assert!(check_str("dns_build").is_ok());
        assert!(check_str("dns_parse").is_ok());
        assert!(check_str("whois_parse").is_ok());
        assert!(check_str("subdomain_enum").is_ok());
        disable();
    }

    /// Every `tunnel_*` builtin is pure cryptography. Gating them under
    /// `net` meant a sandboxed script could not frame a packet or derive a
    /// key, which pushed users towards granting `net` for everything.
    #[test]
    fn the_tunnel_family_is_pure_crypto_and_stays_reachable() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("tunnel_frame").is_ok());
        assert!(check_str("tunnel_unframe").is_ok());
        assert!(check_str("tunnel_nonce").is_ok());
        assert!(check_str("tunnel_preshared_key").is_ok());
        disable();
    }

    /// `extern_call` is the third FFI surface, and `rakc bindgen` generates
    /// calls to it. It is a stub today, so the gate is preventive.
    #[test]
    fn extern_call_needs_the_ffi_capability() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("extern_call").is_err());
        assert!(check_str("ffi:extern").is_err());
        enable("ffi");
        assert!(check_str("extern_call").is_ok());
        assert!(check_str("ffi:extern").is_ok());
        disable();
    }

    /// Reading a file is its own capability, separate from writing one.
    #[test]
    fn reading_a_file_needs_the_fs_read_capability() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("read").is_err());
        assert!(check_str("file_read").is_err());
        assert!(check_str("file_read_bytes").is_err());
        assert!(check_str("read_lines").is_err());
        assert!(check_str("stream_csv").is_err());
        assert!(check_str("stream_jsonl").is_err());
        assert!(check_str("mmap_open").is_err());
        assert!(check_str("file_list").is_err());
        assert!(check_str("file_exists").is_err());
        assert!(check_str("file_size").is_err());
        assert!(check_str("zip_list").is_err());
        assert!(check_str("zip_read").is_err());
        // Writing is still a separate capability.
        assert!(check_str("file_write").is_err());
        enable("fs_read");
        assert!(check_str("read").is_ok());
        assert!(check_str("file_read").is_ok());
        assert!(check_str("zip_read").is_ok());
        // ...and granting read does not grant write.
        assert!(check_str("file_write").is_err());
        disable();
    }

    /// The crypto helpers must not be reachable as a way to bypass the
    /// secrets gate, and the constant-time ones must work with no capability
    /// at all because they touch nothing.
    #[test]
    fn crypto_helpers_need_no_capability_but_secret_wipe_does() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("");
        assert!(check_str("ct_eq").is_ok());
        assert!(check_str("ct_eq_hex").is_ok());
        assert!(check_str("ct_select").is_ok());
        assert!(check_str("zeroize").is_ok());
        assert!(check_str("rsa_keypair").is_ok());
        assert!(check_str("ecdsa_sign").is_ok());
        assert!(check_str("secret_delete_all").is_err());
        disable();
    }

    /// `all` has to grant the new capability too, or `--allow all` is a
    /// weaker sandbox than it looks.
    #[test]
    fn all_grants_fs_read() {
        let _guard = TEST_LOCK.lock().unwrap();
        enable("all");
        assert!(check_str("read").is_ok());
        assert!(check_str("file_read").is_ok());
        disable();
    }
}
