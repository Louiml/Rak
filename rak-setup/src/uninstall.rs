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
                    Action::EnvUser { key, .. } | Action::EnvSystem { key, .. } => format!("env {}", key),
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
                // Only remove if empty-ish (best effort); don't nuke user data.
                let _ = std::fs::remove_dir_all(path);
                status(&format!("removed dir {}", path.display()));
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
            Action::EnvUser { key, .. } => {
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("reg")
                        .args(["delete", "HKCU\\Environment", "/v", key, "/f"])
                        .status();
                }
                let _ = key;
                status(&format!("removed env {}", key));
            }
            Action::EnvSystem { key, .. } => {
                let _ = key;
                status(&format!("(system env {} left; remove manually if needed)", key));
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
