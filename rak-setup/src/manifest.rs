//! The install manifest — records every action so uninstall/upgrade can undo
//! it cleanly. Lives at `~/.rak/manifest.json` (user) or `/etc/rak/manifest.json`
//! (system).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::platform;

/// A single recorded action that the installer performed (and that uninstall
/// must reverse).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    File { path: PathBuf },
    Dir { path: PathBuf },
    PathLine { rc_file: PathBuf, line: String },
    EnvUser { key: String, value: String },
    EnvSystem { key: String, value: String },
    DesktopEntry { path: PathBuf },
    MimeAssoc,
    RegKey { path: String },
    StartMenuShortcut { path: PathBuf },
}

/// The manifest.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub scope: String,
    pub bin_dir: PathBuf,
    pub ide_dir: PathBuf,
    pub packages_dir: PathBuf,
    pub actions: Vec<Action>,
}

impl Manifest {
    pub fn load(scope: crate::Scope) -> Result<Option<Manifest>> {
        let p = platform::manifest_path(scope)?;
        if !p.exists() {
            return Ok(None);
        }
        let s = fs::read_to_string(&p)?;
        let m: Manifest = serde_json::from_str(&s)?;
        Ok(Some(m))
    }

    pub fn save(&self, scope: crate::Scope) -> Result<()> {
        let p = platform::manifest_path(scope)?;
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let s = serde_json::to_string_pretty(self)?;
        fs::write(&p, s)?;
        Ok(())
    }

    pub fn delete(scope: crate::Scope) -> Result<()> {
        let p = platform::manifest_path(scope)?;
        if p.exists() {
            fs::remove_file(&p)?;
        }
        Ok(())
    }

    pub fn record(&mut self, action: Action) {
        self.actions.push(action);
    }
}
