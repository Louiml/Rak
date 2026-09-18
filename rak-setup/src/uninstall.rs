//! Uninstall: read the manifest and reverse every recorded action.

use anyhow::Result;

use crate::manifest::Action;
use crate::platform;
use crate::{confirm, status, Scope};

/// List what's installed (from the user-scope manifest).
pub fn list() -> Result<i32> {
    match crate::manifest::Manifest::load(Scope::User)? {
        Some(m) => {
            println!("Rak install (scope={}, v{}):", m.scope, m.version);
            println!("  bin dir:     {}", m.bin_dir.display());
            println!("  ide dir:     {}", m.ide_dir.display());
            println!("  packages:    {}", m.packages_dir.display());
            println!("  actions:     {}", m.actions.len());
            for a in &m.actions {
                let s = match a {
                    Action::File { path } => format!("file {}", path.display()),
                    Action::Dir { path } => format!("dir  {}", path.display()),
                    Action::PathLine { rc_file, .. } => format!("PATH line in {}", rc_file.display()),
                    Action::EnvUser { key, value } | Action::EnvSystem { key, value } => {
                        format!("env {} = {}", key, value)
                    }
                    Action::DesktopEntry { path } => format!("desktop {}", path.display()),
                    Action::MimeAssoc => "mime assoc".to_string(),
                    Action::RegKey { path } => format!("reg {}", path),
                    Action::StartMenuShortcut { path } => format!("shortcut {}", path.display()),
                };
                println!("    - {}", s);
            }
            Ok(0)
        }
        None => {
            println!("No Rak install found (no manifest at {}).", platform::manifest_path(Scope::User)?.display());
            Ok(0)
        }
    }
}

/// Run uninstall: reverse the manifest's actions.
pub fn run(yes: bool) -> Result<i32> {
    let manifest = match crate::manifest::Manifest::load(Scope::User)? {
        Some(m) => m,
        None => {
            println!("No user-scope Rak install found to uninstall.");
            // also try system
            if let Some(m) = crate::manifest::Manifest::load(Scope::System)? {
                if yes || confirm(&format!("Uninstall system-scope install (v{})?", m.version), false) {
                    return uninstall_manifest(&m, Scope::System);
                }
            }
            return Ok(0);
        }
    };
    if !yes {
        if !confirm(&format!("Uninstall Rak v{} ({} actions)?", manifest.version, manifest.actions.len()), false) {
            return Err(anyhow::anyhow!("cancelled"));
        }
    }
    uninstall_manifest(&manifest, Scope::User)
}

fn uninstall_manifest(m: &crate::manifest::Manifest, scope: Scope) -> Result<i32> {
    // Reverse actions.
    for a in m.actions.iter().rev() {
        match a {
            Action::File { path } => {
                let _ = std::fs::remove_file(path);
                status(&format!("removed file {}", path.display()));
            }
            Action::Dir { path } => {
                // Only recurse-delete directories that belong to Rak (inside
                // ~/.rak or the recorded IDE dir); for anything else (e.g.
                // /usr/local/bin) just try to remove it if empty.
                let owned = platform::rak_root().map(|r| path.starts_with(&r)).unwrap_or(false)
                    || m.ide_dir == *path;
                let removed = if owned {
                    std::fs::remove_dir_all(path).is_ok()
                } else {
                    std::fs::remove_dir(path).is_ok()
                };
                if removed {
                    status(&format!("removed dir {}", path.display()));
                } else {
                    status(&format!("left dir {} (not empty or not owned)", path.display()));
                }
            }
            Action::PathLine { rc_file, line } => {
                let contents = std::fs::read_to_string(rc_file).unwrap_or_default();
                let cleaned: String = contents
                    .lines()
                    .filter(|l| l.trim() != line.trim() && l.trim() != "# added by rak-setup")
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = std::fs::write(rc_file, cleaned);
                status(&format!("removed PATH line from {}", rc_file.display()));
            }
            Action::EnvUser { key, value } => {
                #[cfg(target_os = "windows")]
                {
                    if key.eq_ignore_ascii_case("PATH") {
                        // Remove only the rak bin entry — never the whole
                        // PATH value.
                        platform::win::path_remove(platform::win::USER_ENV_KEY, value);
                        platform::win::broadcast_env_change();
                    } else {
                        platform::win::delete_value(platform::win::USER_ENV_KEY, key);
                    }
                }
                let _ = value;
                status(&format!("removed env {}", key));
            }
            Action::EnvSystem { key, value } => {
                #[cfg(target_os = "windows")]
                {
                    if key.eq_ignore_ascii_case("PATH") {
                        platform::win::path_remove(platform::win::SYSTEM_ENV_KEY, value);
                        platform::win::broadcast_env_change();
                    } else {
                        platform::win::delete_value(platform::win::SYSTEM_ENV_KEY, key);
                    }
                }
                let _ = value;
                status(&format!("removed env {} (system)", key));
            }
            Action::DesktopEntry { path } => {
                let _ = std::fs::remove_file(path);
            }
            Action::MimeAssoc => {
                status("(mime association left; remove manually if needed)");
            }
            Action::RegKey { path } => {
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("reg")
                        .args(["delete", path, "/f"])
                        .status();
                }
                let _ = path;
            }
            Action::StartMenuShortcut { path } => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    crate::manifest::Manifest::delete(scope)?;
    status("uninstall complete.");
    Ok(0)
}
