//! Secrets API: store/retrieve named secrets without scattering env-var
//! handling everywhere.
//!
//! Resolution order (first hit wins):
//!   1. in-memory session store (`secret_set`)
//!   2. env var (name as given)
//!   3. `~/.rak/secrets.json` (0600, user-owned) — a durable store
//! Returns `None` when unset. Values are never logged.
//!
//! The file store is deliberately simple JSON with user-only permissions. For
//! production-grade storage an OS keyring (feature-gated `keyring` crate) is a
//! planned follow-up.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static SESSION: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn secrets_path() -> Option<PathBuf> {
    let base = std::env::var("RAK_PATH")
        .ok()
        .and_then(|p| {
            #[cfg(windows)]
            let parts = p.split(';');
            #[cfg(not(windows))]
            let parts = p.split(':');
            parts.filter(|s| !s.is_empty()).next().map(|s| s.to_string())
        })
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().map(|h| PathBuf::from(h).join(".rak")))?;
    Some(base.join("secrets.json"))
}

fn read_file() -> HashMap<String, String> {
    let Some(p) = secrets_path() else { return HashMap::new() };
    let Ok(text) = fs::read_to_string(&p) else { return HashMap::new() };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_file(map: &HashMap<String, String>) -> Result<(), String> {
    let Some(p) = secrets_path() else { return Ok(()) };
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
    }
    let json = serde_json::to_string(map).map_err(|e| format!("secrets: serialize: {}", e))?;
    fs::write(&p, json).map_err(|e| format!("secrets: write: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Get a secret. Resolution: session -> env var -> file store.
pub fn get(name: &str) -> Option<String> {
    if let Ok(g) = SESSION.lock() {
        if let Some(m) = g.as_ref() {
            if let Some(v) = m.get(name) {
                return Some(v.clone());
            }
        }
    }
    if let Ok(v) = std::env::var(name) {
        return Some(v);
    }
    read_file().get(name).cloned()
}

/// Set a secret in the session store (not persisted to disk).
pub fn set(name: &str, value: &str) {
    if let Ok(mut g) = SESSION.lock() {
        g.get_or_insert_with(HashMap::new).insert(name.to_string(), value.to_string());
    }
}

/// Persist a secret to the durable file store (0600).
pub fn persist(name: &str, value: &str) -> Result<(), String> {
    let mut map = read_file();
    map.insert(name.to_string(), value.to_string());
    write_file(&map)
}

/// Delete a secret from both the session and the file store.
pub fn delete(name: &str) -> Result<(), String> {
    if let Ok(mut g) = SESSION.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(name);
        }
    }
    let mut map = read_file();
    map.remove(name);
    write_file(&map)
}

/// List all secret names (session + file store).
pub fn list() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if let Ok(g) = SESSION.lock() {
        if let Some(m) = g.as_ref() {
            names.extend(m.keys().cloned());
        }
    }
    names.extend(read_file().keys().cloned());
    names.sort();
    names.dedup();
    names
}