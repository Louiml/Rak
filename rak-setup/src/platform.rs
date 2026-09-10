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
