//! Rak package-manager library: manifest parsing + dependency-spec parsing.
//! Exposed as a library so the compiler, IDE, and fuzz targets can share the
//! same logic without shelling out to the `rakpkg` binary.

use std::collections::HashMap;
use std::path::Path;

pub const MANIFEST_FILE: &str = "package.rak";
pub const ENTRY_DEFAULT: &str = "lib.rak";

/// A parsed `package.rak` manifest.
#[derive(Clone)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub deps: HashMap<String, String>, // name -> "user/repo[@v|#rev]"
    pub entry: String,
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest {
            name: String::new(),
            version: String::new(),
            deps: HashMap::new(),
            entry: ENTRY_DEFAULT.to_string(),
        }
    }
}

/// Parse a `package.rak` manifest. Supports single-line and multi-line
/// `let deps = { ... }` blocks and `let name` / `let version` / `let entry`.
pub fn parse_manifest(path: &Path) -> Result<Manifest, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read package.rak: {}", e))?;
    parse_manifest_str(&content)
}

/// Parse a manifest from an in-memory string (used by tests and fuzzing).
pub fn parse_manifest_str(content: &str) -> Result<Manifest, String> {
    let mut m = Manifest::default();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("//") || line.is_empty() {
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix("let name = ") {
            m.name = unquote(rest);
        } else if let Some(rest) = line.strip_prefix("let version = ") {
            m.version = unquote(rest);
        } else if let Some(rest) = line.strip_prefix("let entry = ") {
            m.entry = unquote(rest);
        } else if line.starts_with("let deps = {") || line.starts_with("let deps =") {
            // Consume the whole (possibly multi-line) object literal.
            let mut acc = line
                .trim_start_matches("let deps =")
                .to_string();
            if !acc.contains('}') {
                let mut closed = false;
                i += 1;
                while i < lines.len() && !closed {
                    let t = lines[i].trim();
                    acc.push_str(t);
                    if t.ends_with('}') {
                        closed = true;
                    }
                    i += 1;
                }
            }
            let inner = acc.trim().trim_start_matches('{').trim_end_matches('}').trim_end_matches(';');
            for pair in inner.split(',') {
                let pair = pair.trim();
                if pair.is_empty() {
                    continue;
                }
                if let Some(colon) = pair.find(':') {
                    let key = unquote(pair[..colon].trim());
                    let val = unquote(pair[colon + 1..].trim());
                    if !key.is_empty() {
                        m.deps.insert(key, val);
                    }
                }
            }
        }
        i += 1;
    }

    if m.name.is_empty() {
        return Err("package.rak missing 'let name'".to_string());
    }
    Ok(m)
}

fn unquote(s: &str) -> String {
    s.trim_end_matches(';').trim().trim_matches('"').to_string()
}

/// Split a `user/repo[@vX.Y.Z | ^X | #rev]` spec into
/// `(repo, version_constraint, pinned_rev)`.
pub fn parse_dep_spec(spec: &str) -> (String, Option<String>, Option<String>) {
    let mut repo = spec.to_string();
    let mut version: Option<String> = None;
    let mut rev: Option<String> = None;
    if let Some(pos) = repo.find('@') {
        let v = repo[pos + 1..].to_string();
        repo.truncate(pos);
        // The version portion may itself carry a `#rev` (e.g. `@^1.2#deadbeef`).
        if let Some(hash) = v.find('#') {
            let r = v[hash + 1..].to_string();
            version = Some(v[..hash].trim().to_string());
            rev = Some(r.trim().to_string());
        } else {
            version = Some(v.trim().to_string());
        }
    }
    if rev.is_none() {
        if let Some(pos) = repo.find('#') {
            let r = repo[pos + 1..].to_string();
            repo.truncate(pos);
            rev = Some(r.trim().to_string());
        }
    }
    (repo, version, rev)
}

/// Rough semver satisfaction for `^x.y`, `~x.y`, and exact `x.y.z`.
pub fn version_satisfies(actual: &str, constraint: &str) -> bool {
    let c = constraint.trim();
    if c.is_empty() {
        return true;
    }
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    };
    let a = parse(actual);
    let care = c.starts_with('^') || c.starts_with('~');
    let b = parse(c.trim_start_matches('^').trim_start_matches('~'));
    if a.is_empty() || b.is_empty() {
        return true;
    }
    if care {
        return a[..(b.len().min(a.len()))] == b[..(b.len().min(a.len()))];
    }
    a[..] == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_deps() {
        let m = parse_manifest_str("let name = \"p\"\nlet version = \"1.0.0\"\nlet deps = { a: \"u/a\", b: \"u/b\" }\n").unwrap();
        assert_eq!(m.name, "p");
        assert_eq!(m.deps.len(), 2);
        assert_eq!(m.deps.get("a").unwrap(), "u/a");
    }

    #[test]
    fn multi_line_deps() {
        let s = "let name = \"p\"\nlet deps = {\n  a: \"u/a\",\n  b: \"u/b\"\n}\n";
        let m = parse_manifest_str(s).unwrap();
        assert_eq!(m.deps.len(), 2);
    }

    #[test]
    fn dep_spec_split() {
        let (r, v, rev) = parse_dep_spec("u/r@^1.2#deadbeef");
        assert_eq!(r, "u/r");
        assert_eq!(v.as_deref(), Some("^1.2"));
        assert_eq!(rev.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn version_satisfies_caret() {
        assert!(version_satisfies("1.2.3", "^1.2"));
        assert!(!version_satisfies("2.0.0", "^1.2"));
        assert!(version_satisfies("1.2.3", "1.2.3"));
    }
}