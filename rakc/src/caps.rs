//! Sandbox capability gating (`rakc run --sandbox [--allow csv]`).
//!
//! When the sandbox is enabled, dangerous builtin families are denied unless
//! explicitly re-granted with `--allow`. This exists so investigators can run
//! scripts fetched with `rakpkg add <user/repo>` (or received from a third
//! party) without giving the script network, process, FFI, raw-socket, GUI,
//! secrets, or filesystem-write access by default.
//!
//! Filesystem *reads* remain allowed in the default sandbox (denying them
//! would break imports/modules); everything that can exfiltrate data
//! (network, process spawning, FFI) is denied, so reads cannot leak.
//!
//! Enforcement is a single point of control: every interpreter builtin flows
//! through `Interpreter::eval_builtin` and every VM native flows through
//! `Vm::call_value`, so name-based gating at those heads covers all of them.
//! `extern "C"` declarations are gated at the foreign-call site
//! (`ffi:extern`).

use std::sync::RwLock;

/// Capability flags. When the sandbox is active, `true` = allowed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sandbox {
    pub net: bool,
    pub fs_write: bool,
    pub process: bool,
    pub ffi: bool,
    pub raw_sockets: bool,
    pub gui: bool,
    pub secrets: bool,
}

static ACTIVE: RwLock<Option<Sandbox>> = RwLock::new(None);

/// Is the sandbox currently enabled for this process?
pub fn is_active() -> bool {
    ACTIVE.read().unwrap().is_some()
}

/// Enable the sandbox. `allow` is a comma-separated list of capability names
/// (`net,fs_write,process,ffi,raw,gui,secrets`) or `all`.
pub fn enable(allow: &str) {
    let mut sb = Sandbox::default();
    for cap in allow.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match cap {
            "all" => sb = Sandbox {
                net: true, fs_write: true, process: true, ffi: true,
                raw_sockets: true, gui: true, secrets: true,
            },
            "net" => sb.net = true,
            "fs" | "fs_write" => sb.fs_write = true,
            "process" | "proc" => sb.process = true,
            "ffi" => sb.ffi = true,
            "raw" | "raw_sockets" => sb.raw_sockets = true,
            "gui" => sb.gui = true,
            "secrets" => sb.secrets = true,
            other => eprintln!("sandbox: unknown capability '{}' (ignored)", other),
        }
    }
    *ACTIVE.write().unwrap() = Some(sb);
}

/// Disable the sandbox (mainly for tests).
pub fn disable() {
    *ACTIVE.write().unwrap() = None;
}


/// Map a builtin name to the capability it requires, if any. Pure builders
/// (packet/header construction, TLS parsing, hashing) are intentionally not
/// gated — they cannot touch the outside world.
fn required_cap(name: &str) -> Option<&'static str> {
    if name == "ffi:extern" {
        return Some("ffi");
    }
    let table: &[(&str, &[&str])] = &[
        (
            "net",
            &[
                "net_listen", "net_accept", "net_connect", "net_local_addr",
                // every TCP/UDP/HTTP/WebSocket/DNS I/O I/O builtin
                "tcp_", "udp_", "http_", "ws_", "dns_",
                // network-capable investigation builtins (ext packs)
                "scan_ports", "fetch", "tunnel_", "whois", "ct_subdomains",
                "tls_inspect",
            ],
        ),
        ("process", &["process_"]),
        ("ffi", &["ffi_"]),
        (
            "raw_sockets",
            &["net_raw_send", "net_raw_recv", "net_raw_icmp", "pcap_listen"],
        ),
        ("gui", &["gui_", "window_"]),
        ("secrets", &["secret_"]),
        (
            "fs_write",
            &["file_write", "file_append", "append_file", "mkdir", "remove_file",
              "zip_write", "report_write"],
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
        "fs_write" => sb.fs_write,
        "process" => sb.process,
        "ffi" => sb.ffi,
        "raw_sockets" => sb.raw_sockets,
        "gui" => sb.gui,
        "secrets" => sb.secrets,
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

/// Interpreter-side check (uses `RakError`).
pub fn check_builtin(name: &str) -> crate::Result<()> {
    check_str(name).map_err(crate::RakError::Runtime)
}

/// Parse `--sandbox` / `--allow <csv>` out of trailing CLI args, returning
/// `(sandbox_enabled, allow_csv, remaining_args)`.
pub fn parse_cli(rest: &[String]) -> (bool, String, Vec<String>) {
    let mut on = false;
    let mut allow = String::new();
    let mut clean = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--sandbox" => on = true,
            "--allow" => {
                i += 1;
                if let Some(v) = rest.get(i) {
                    allow = v.clone();
                }
            }
            other => clean.push(other.to_string()),
        }
        i += 1;
    }
    (on, allow, clean)
}

/// List the recognized sandbox capabilities (for `--help` text).
pub fn capability_names() -> &'static [&'static str] {
    &["net", "fs_write", "process", "ffi", "raw", "gui", "secrets", "all"]
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
        assert!(check_str("file_read").is_ok());
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
        let (on, allow, clean) = parse_cli(&args);
        assert!(on);
        assert_eq!(allow, "net,ffi");
        assert_eq!(clean, vec!["hello".to_string(), "world".to_string()]);
    }
}
