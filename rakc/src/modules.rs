//! Module resolution — turn an import target (a file path or a dotted name)
//! into a `.rak` file on disk, searching the importer's directory, then
//! `./packages/`, then the `RAK_PATH` env-var directories. Shared by the
//! interpreter and the compiler.

use std::path::{Path, PathBuf};

/// The result of resolving a dotted import path. For `import m` the leaf is
/// `m.rak` (or `m/init.rak`) with no init; for `import pkg.sub` the leaf is
/// `pkg/sub.rak` and `init` is the package's `pkg/init.rak` (loaded first).
#[derive(Debug, Clone)]
pub struct DottedResolve {
    pub init: Option<PathBuf>,
    pub leaf: PathBuf,
}

/// The ordered list of directories searched for a module name: the importing
/// file's own directory, `./packages/` (rakpkg deps), then any `RAK_PATH`
/// entries (`;` on Windows, `:` on Unix).
pub fn search_dirs(importer_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![importer_dir.to_path_buf()];
    dirs.push(importer_dir.join("packages"));
    if let Ok(p) = std::env::var("RAK_PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for d in p.split(sep) {
            let d = d.trim();
            if !d.is_empty() {
                dirs.push(PathBuf::from(d));
            }
        }
    }
    dirs
}

/// Resolve a dotted import path (`["m"]` or `["pkg", "sub"]`) to a file.
pub fn resolve_dotted(importer_dir: &Path, parts: &[String]) -> Option<DottedResolve> {
    if parts.is_empty() {
        return None;
    }
    for d in search_dirs(importer_dir) {
        if parts.len() == 1 {
            let f = d.join(format!("{}.rak", parts[0]));
            if f.is_file() {
                return Some(DottedResolve { init: None, leaf: f });
            }
            let init = d.join(parts[0].as_str()).join("init.rak");
            if init.is_file() {
                return Some(DottedResolve { init: None, leaf: init });
            }
        } else {
            let pkg_dir = d.join(parts[0].as_str());
            let init = pkg_dir.join("init.rak");
            // Descend through intermediate segments, then `<last>.rak`.
            let mut leaf = pkg_dir.clone();
            for seg in &parts[1..parts.len() - 1] {
                leaf = leaf.join(seg.as_str());
            }
            leaf = leaf.join(format!("{}.rak", parts.last().unwrap()));
            if leaf.is_file() {
                let init = if init.is_file() { Some(init) } else { None };
                return Some(DottedResolve { init, leaf });
            }
        }
    }
    None
}

/// Canonicalise a path (resolving `..`/`.`); falls back to the input on
/// failure (e.g. a non-existent file during a cycle).
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rak_mod_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn resolve_single_file() {
        let d = tmp("single");
        fs::write(d.join("m.rak"), "dump 1").unwrap();
        let r = resolve_dotted(&d, &["m".to_string()]).unwrap();
        assert_eq!(r.leaf, d.join("m.rak"));
        assert!(r.init.is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn resolve_package_init() {
        let d = tmp("pkg");
        fs::create_dir_all(d.join("pkg")).unwrap();
        fs::write(d.join("pkg").join("init.rak"), "dump 1").unwrap();
        let r = resolve_dotted(&d, &["pkg".to_string()]).unwrap();
        assert_eq!(r.leaf, d.join("pkg").join("init.rak"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn resolve_dotted_submodule() {
        let d = tmp("dotted");
        fs::create_dir_all(d.join("pkg")).unwrap();
        fs::write(d.join("pkg").join("init.rak"), "dump 1").unwrap();
        fs::write(d.join("pkg").join("sub.rak"), "dump 2").unwrap();
        let r = resolve_dotted(&d, &["pkg".to_string(), "sub".to_string()]).unwrap();
        assert_eq!(r.leaf, d.join("pkg").join("sub.rak"));
        assert_eq!(r.init.as_deref().unwrap(), d.join("pkg").join("init.rak").as_path());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn resolve_missing_returns_none() {
        let d = tmp("missing");
        assert!(resolve_dotted(&d, &["nope".to_string()]).is_none());
        let _ = fs::remove_dir_all(&d);
    }
}
