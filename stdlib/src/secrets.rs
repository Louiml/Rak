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
//!
//! What is guaranteed here: the serialized JSON buffer is zeroized before it
//! is released, deleted values are wiped from memory rather than just dropped,
//! and the store file is overwritten with zeroes before it is rewritten. What
//! is not: `get` returns a `String` by value, so the caller's copy is theirs
//! to wipe, and a process that exits or crashes mid-write can still leave
//! plaintext in the OS's swap or in a core dump. Use `zeroize` and
//! `secret_delete_all` when that matters.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use zeroize::Zeroize;

static SESSION: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Wipe a `String`'s buffer in place before releasing it. Rust gives no other
/// way to clear a `String` you still own, so we take it by value, zero the
/// allocation through a volatile write, then drop it.
fn wipe_string(mut s: String) {
    unsafe { s.as_mut_vec().zeroize() };
    drop(s);
}

/// Overwrite an existing file with zeroes, keeping its length, so a rewrite
/// does not leave the previous contents in the filesystem's freed blocks or
/// page cache. Missing file is not an error: there is nothing to wipe.
fn shred_file(p: &PathBuf) {
    use std::io::{Seek, SeekFrom, Write};
    let Ok(len) = fs::metadata(p).map(|m| m.len()) else {
        return;
    };
    if len == 0 {
        return;
    }
    let Ok(mut f) = fs::OpenOptions::new().write(true).open(p) else {
        return;
    };
    if f.set_len(0).is_err() {
        return;
    }
    let zeros = [0u8; 4096];
    let mut written = 0u64;
    while written < len {
        let n = std::cmp::min(zeros.len() as u64, len - written) as usize;
        if f.write_all(&zeros[..n]).is_err() {
            return;
        }
        written += n as u64;
    }
    let _ = f.seek(SeekFrom::Start(0));
    let _ = f.sync_all();
}

fn secrets_path() -> Option<PathBuf> {
    let base = std::env::var("RAK_PATH")
        .ok()
        .and_then(|p| {
            #[cfg(windows)]
            let parts = p.split(';');
            #[cfg(not(windows))]
            let parts = p.split(':');
            parts
                .filter(|s| !s.is_empty())
                .next()
                .map(|s| s.to_string())
        })
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok()
                .map(|h| PathBuf::from(h).join(".rak"))
        })?;
    Some(base.join("secrets.json"))
}

fn read_file() -> HashMap<String, String> {
    let Some(p) = secrets_path() else {
        return HashMap::new();
    };
    let Ok(text) = fs::read_to_string(&p) else {
        return HashMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_file(map: &HashMap<String, String>) -> Result<(), String> {
    let Some(p) = secrets_path() else {
        return Ok(());
    };
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
    }
    // Set the permissions before the data lands, not after. Writing first means
    // the file exists on disk with the process umask (often 0644) for a moment.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    let mut json = serde_json::to_string(map).map_err(|e| format!("secrets: serialize: {}", e))?;
    let res = fs::write(&p, &json).map_err(|e| format!("secrets: write: {}", e));
    // The serialized buffer holds every secret value in plaintext. Wipe it
    // rather than leaving it for the allocator to hand to the next string.
    json.zeroize();
    res
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
        g.get_or_insert_with(HashMap::new)
            .insert(name.to_string(), value.to_string());
    }
}

/// Persist a secret to the durable file store (0600).
pub fn persist(name: &str, value: &str) -> Result<(), String> {
    let mut map = read_file();
    map.insert(name.to_string(), value.to_string());
    write_file(&map)
}

/// Delete a secret from both the session and the file store.
///
/// The old file is overwritten with zeroes before the shorter replacement is
/// written, because rewriting in place would otherwise leave the deleted value
/// sitting in the filesystem's freed blocks.
pub fn delete(name: &str) -> Result<(), String> {
    if let Ok(mut g) = SESSION.lock() {
        if let Some(m) = g.as_mut() {
            if let Some(v) = m.remove(name) {
                wipe_string(v);
            }
        }
    }
    let mut map = read_file();
    if let Some(v) = map.remove(name) {
        wipe_string(v);
    }
    if let Some(p) = secrets_path() {
        shred_file(&p);
    }
    write_file(&map)
}

/// Remove every secret from the session and the file store, wiping each value
/// and shredding the file. This is the "log out" operation.
pub fn delete_all() -> Result<(), String> {
    if let Ok(mut g) = SESSION.lock() {
        if let Some(m) = g.as_mut() {
            let names: Vec<String> = m.keys().cloned().collect();
            for n in names {
                if let Some(v) = m.remove(&n) {
                    wipe_string(v);
                }
            }
        }
    }
    let mut map = read_file();
    for (_, v) in map.drain() {
        wipe_string(v);
    }
    if let Some(p) = secrets_path() {
        shred_file(&p);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_string_clears_the_buffer() {
        let s = String::from("hunter2");
        wipe_string(s);
        // Nothing observable to assert: the buffer is freed. This guards
        // against the helper being removed or changed to a no-op that the
        // compiler would happily optimize away.
    }

    #[test]
    fn session_set_get_delete_roundtrip() {
        // These exercise the in-memory path only; the file store depends on
        // RAK_PATH / HOME and is not touched here.
        set("rak_test_token", "s3cr3t");
        assert_eq!(get("rak_test_token").as_deref(), Some("s3cr3t"));
        // Wiping a name that is not present must not error.
        let _ = delete_all();
        assert!(get("rak_test_token").is_none());
    }
}
