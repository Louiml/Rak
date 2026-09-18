//! Platform-specific path resolution (Linux vs Windows).

#![allow(dead_code)]

use anyhow::Result;
use std::path::PathBuf;

/// The user's home directory.
pub fn home() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))
}

/// `~/.rak` (Linux) / `%USERPROFILE%\.rak` (Windows).
pub fn rak_root() -> Result<PathBuf> {
    Ok(home()?.join(".rak"))
}

/// The default bin dir for the given scope.
pub fn default_bin_dir(scope: super::Scope) -> Result<PathBuf> {
    match scope {
        super::Scope::User => Ok(rak_root()?.join("bin")),
        super::Scope::System => {
            if cfg!(target_os = "windows") {
                Ok(PathBuf::from(r"C:\Program Files\Rak\bin"))
            } else {
                Ok(PathBuf::from("/usr/local/bin"))
            }
        }
    }
}

/// The default IDE dir for the given scope + mode.
pub fn default_ide_dir(scope: super::Scope, mode: super::IdeMode) -> Result<PathBuf> {
    match (scope, mode) {
        (super::Scope::User, _) => Ok(rak_root()?.join("ide")),
        (super::Scope::System, _) => {
            if cfg!(target_os = "windows") {
                Ok(PathBuf::from(r"C:\Program Files\Rak\ide"))
            } else {
                Ok(PathBuf::from("/opt/rak-ide"))
            }
        }
    }
}

/// The default packages dir (for RAK_PATH).
pub fn default_packages_dir(scope: super::Scope) -> Result<PathBuf> {
    match scope {
        super::Scope::User => Ok(rak_root()?.join("packages")),
        super::Scope::System => {
            if cfg!(target_os = "windows") {
                Ok(PathBuf::from(r"C:\Program Files\Rak\packages"))
            } else {
                Ok(PathBuf::from("/etc/rak/packages"))
            }
        }
    }
}

/// The manifest path for the given scope.
pub fn manifest_path(scope: super::Scope) -> Result<PathBuf> {
    match scope {
        super::Scope::User => Ok(rak_root()?.join("manifest.json")),
        super::Scope::System => {
            if cfg!(target_os = "windows") {
                Ok(PathBuf::from(r"C:\Program Files\Rak\manifest.json"))
            } else {
                Ok(PathBuf::from("/etc/rak/manifest.json"))
            }
        }
    }
}

/// Shell rc files to edit for PATH on Linux (user scope). Only files that
/// already exist are returned.
pub fn user_shell_rc_files() -> Result<Vec<PathBuf>> {
    let h = home()?;
    let candidates = [h.join(".bashrc"), h.join(".zshrc"), h.join(".bash_profile"), h.join(".profile")];
    Ok(candidates.into_iter().filter(|p| p.exists()).collect())
}

/// Whether the host is Windows.
pub fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

/// Whether the host is Linux.
pub fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

/// Windows registry/env helpers, implemented via `reg.exe` and PowerShell so
/// no extra crates are needed. The PATH helpers are append/remove-only: they
/// always preserve the user's existing PATH entries and only ever touch the
/// registry hive matching the install scope.
#[cfg(target_os = "windows")]
pub mod win {
    use anyhow::{anyhow, Result};
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// The user (HKCU) environment key.
    pub const USER_ENV_KEY: &str = "HKCU\\Environment";
    /// The system (HKLM) environment key.
    pub const SYSTEM_ENV_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

    /// Parse one `reg query` output line for value `name`. `reg query` prints
    /// `    <name>    <TYPE>    <data...>`; the data is everything after the
    /// type token (spaces inside the data must survive). Returns `None` when
    /// the line doesn't match.
    fn parse_query_line(line: &str, name: &str) -> Option<String> {
        let t = line.trim_start();
        if t.len() <= name.len() {
            return None;
        }
        let (head, rest) = t.split_at(name.len());
        if !head.eq_ignore_ascii_case(name) {
            return None;
        }
        let rest = rest.trim_start();
        // Skip the type token (REG_SZ / REG_EXPAND_SZ / ...).
        let i = rest.find(char::is_whitespace)?;
        Some(rest[i..].trim_start().trim_end().to_string())
    }

    /// Read a registry value's raw data via `reg query`. Returns `None` when
    /// the value does not exist. Data is returned unexpanded for
    /// REG_EXPAND_SZ values so %VAR% references are preserved on write-back.
    pub fn query_value(key: &str, name: &str) -> Option<String> {
        let out = Command::new("reg")
            .args(["query", key, "/v", name])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout.lines().find_map(|l| parse_query_line(l, name))
    }

    /// Write a registry value via `reg add`, erroring on failure.
    pub fn set_value(key: &str, name: &str, reg_type: &str, data: &str) -> Result<()> {
        let out = Command::new("reg")
            .args(["add", key, "/v", name, "/t", reg_type, "/d", data, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| anyhow!("failed to run reg add: {e}"))?;
        if !out.status.success() {
            return Err(anyhow!(
                "reg add {} /v {} failed: {}",
                key,
                name,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }

    /// Delete a registry value (best effort).
    pub fn delete_value(key: &str, name: &str) {
        let _ = Command::new("reg")
            .args(["delete", key, "/v", name, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }

    /// Compare two PATH entries, ignoring case and trailing separators.
    fn same_path_entry(a: &str, b: &str) -> bool {
        fn norm(s: &str) -> &str {
            s.trim().trim_end_matches(&['\\', '/'][..])
        }
        norm(a).eq_ignore_ascii_case(norm(b))
    }

    /// Append `entry` to the PATH value under `key` if it isn't there yet.
    /// The existing PATH content is always preserved. Returns `Ok(true)` if
    /// the PATH was modified.
    pub fn path_add(key: &str, entry: &str) -> Result<bool> {
        let current = query_value(key, "PATH").unwrap_or_default();
        if current.split(';').any(|p| same_path_entry(p, entry)) {
            return Ok(false);
        }
        let base = current.trim().trim_end_matches(';');
        let new = if base.is_empty() {
            entry.to_string()
        } else {
            format!("{base};{entry}")
        };
        set_value(key, "PATH", "REG_EXPAND_SZ", &new)?;
        Ok(true)
    }

    /// Remove `entry` from the PATH value under `key`, preserving everything
    /// else. If no entries remain the value itself is deleted.
    pub fn path_remove(key: &str, entry: &str) {
        let Some(current) = query_value(key, "PATH") else {
            return;
        };
        let kept: Vec<&str> = current.split(';').filter(|p| !same_path_entry(p, entry)).collect();
        if kept.is_empty() {
            delete_value(key, "PATH");
        } else if kept.join(";") != current {
            let _ = set_value(key, "PATH", "REG_EXPAND_SZ", &kept.join(";"));
        }
    }

    /// Base64-encode a UTF-16LE string (the format PowerShell's
    /// `-EncodedCommand` expects). Avoids every shell-quoting hazard.
    fn utf16le_base64(s: &str) -> String {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
        }
        out
    }

    /// Broadcast WM_SETTINGCHANGE so already-running programs (Explorer, etc.)
    /// pick up env changes — the useful side effect `setx` had. Best effort.
    pub fn broadcast_env_change() {
        let script = concat!(
            "Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition ",
            "'[DllImport(\"user32.dll\", SetLastError = true, CharSet = CharSet.Auto)] ",
            "public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'; ",
            "$r = [UIntPtr]::Zero; ",
            "[Win32.NativeMethods]::SendMessageTimeout([IntPtr]0xffff, 0x1a, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r) | Out-Null"
        );
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &utf16le_base64(script)])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parse_keeps_data_but_drops_type_token() {
            let line = "    PATH    REG_EXPAND_SZ    C:\\Program Files;C:\\Users\\x\\.rak\\bin";
            assert_eq!(
                parse_query_line(line, "PATH").as_deref(),
                Some("C:\\Program Files;C:\\Users\\x\\.rak\\bin")
            );
        }

        #[test]
        fn parse_handles_reg_sz_and_case() {
            assert_eq!(parse_query_line("    Path    REG_SZ    a;b", "PATH").as_deref(), Some("a;b"));
            assert_eq!(parse_query_line("    RAK_PATH    REG_SZ    C:\\pkgs", "PATH"), None);
        }

        #[test]
        fn parse_ignores_key_and_header_lines() {
            assert_eq!(parse_query_line("HKEY_CURRENT_USER\\Environment", "PATH"), None);
            assert_eq!(parse_query_line("", "PATH"), None);
        }

        #[test]
        fn same_entry_ignores_case_and_trailing_sep() {
            assert!(same_path_entry(r"C:\Users\x\.rak\bin\", r"c:\users\X\.rak\bin"));
            assert!(!same_path_entry(r"C:\a", r"C:\b"));
        }

        #[test]
        fn base64_matches_reference() {
            // UTF-16LE bytes of "hi" = 68 00 69 00 -> aABp / AA==
            assert_eq!(utf16le_base64("hi"), "aABpAA==");
            assert_eq!(utf16le_base64(""), "");
            // UTF-16LE bytes of "A" = 41 00 -> QQ
            assert_eq!(utf16le_base64("A"), "QQA=");
        }

        #[test]
        fn path_add_and_remove_roundtrip() {
            let key = "HKCU\\Software\\rak_setup_test";
            delete_value(key, "PATH");
            // No PATH value yet -> create it with just the new entry.
            assert!(path_add(key, "C:\\a").unwrap());
            assert_eq!(query_value(key, "PATH").as_deref(), Some("C:\\a"));
            // Append preserves existing entries.
            assert!(path_add(key, "C:\\B\\").unwrap());
            assert_eq!(query_value(key, "PATH").as_deref(), Some("C:\\a;C:\\B\\"));
            // Idempotent (case-insensitive, trailing separator ignored).
            assert!(!path_add(key, "c:\\b").unwrap());
            assert_eq!(query_value(key, "PATH").as_deref(), Some("C:\\a;C:\\B\\"));
            // Remove only the target entry.
            path_remove(key, "c:\\b");
            assert_eq!(query_value(key, "PATH").as_deref(), Some("C:\\a"));
            // Last entry removed -> value deleted entirely.
            path_remove(key, "c:\\a");
            assert!(query_value(key, "PATH").is_none());
            let _ = Command::new("reg").args(["delete", key, "/f"]).status();
        }
    }
}
