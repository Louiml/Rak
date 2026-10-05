//! The `package.rak` manifest.
//!
//! The manifest is a Rak file with `let` bindings — the same convention
//! `rakpkg` used, kept so existing packages keep working. Oyvey extends it
//! with optional `description` / `license` fields and a `deps` map whose
//! values are `user/repo[@constraint][#rev]` specs.
//!
//! ```rak
//! let name = "mylib"
//! let version = "0.1.0"
//! let description = "A Rak package"
//! let license = "MIT"
//! let entry = "src/main.rak"
//! let deps = {
//!     net: "user/rak-net",
//!     crypto: "user/rak-crypto@^1.0"
//! }
//! ```

use std::collections::BTreeMap;
use std::path::Path;

/// The manifest file name.
pub const MANIFEST_FILE: &str = "package.rak";

/// The default entry point for a binary project (Cargo's `src/main.rs`
/// convention, translated to Rak).
pub const ENTRY_DEFAULT: &str = "src/main.rak";

/// The default entry point for a library project.
pub const ENTRY_LIB: &str = "src/lib.rak";

/// A parsed `package.rak` manifest.
#[derive(Clone, Debug)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub entry: String,
    /// Dependency name -> `user/repo[@constraint][#rev]`.
    pub deps: BTreeMap<String, String>,
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest {
            name: String::new(),
            version: "0.1.0".to_string(),
            description: String::new(),
            license: String::new(),
            entry: ENTRY_DEFAULT.to_string(),
            deps: BTreeMap::new(),
        }
    }
}

/// Parse a `package.rak` manifest from disk.
pub fn parse_manifest(path: &Path) -> Result<Manifest, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    parse_manifest_str(&content)
}

/// Parse a manifest from an in-memory string (used by tests and fuzzing).
pub fn parse_manifest_str(content: &str) -> Result<Manifest, String> {
    let mut m = Manifest::default();
    // A UTF-8 BOM is common in files written by Windows editors (and by
    // PowerShell's `Out-File -Encoding utf8`). Left in place it glues itself to
    // the front of the first token, so `let name` stops being a prefix match
    // and every manifest looks like it is missing its name.
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
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
        } else if let Some(rest) = line.strip_prefix("let description = ") {
            m.description = unquote(rest);
        } else if let Some(rest) = line.strip_prefix("let license = ") {
            m.license = unquote(rest);
        } else if let Some(rest) = line.strip_prefix("let entry = ") {
            m.entry = unquote(rest);
        } else if line.starts_with("let deps = {") || line.starts_with("let deps =") {
            // Consume the whole (possibly multi-line) object literal.
            let mut acc = line.trim_start_matches("let deps =").to_string();
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
            let inner = acc
                .trim()
                .trim_start_matches('{')
                .trim_end_matches('}')
                .trim_end_matches(';');
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

/// Candidate entry points, most specific first, used when a manifest does not
/// declare one or declares one that is not present.
const ENTRY_CANDIDATES: [&str; 5] = ["init.rak", "lib.rak", "main.rak", "src/lib.rak", "src/main.rak"];

impl Manifest {
    /// The entry point to actually use, given the directory the manifest is in.
    ///
    /// A manifest that declares `entry` and has that file wins. Otherwise the
    /// conventional names are tried in order, because the entry is optional and
    /// a package that is only ever imported should not have to spell it out. The
    /// declared entry is returned unchanged when nothing exists, so a wrong
    /// `entry` still produces the "entry point not found" error rather than
    /// silently building some other file.
    pub fn resolved_entry(&self, root: &Path) -> String {
        let declared = self.entry.trim();
        if !declared.is_empty() && root.join(declared).is_file() {
            return declared.to_string();
        }
        for cand in ENTRY_CANDIDATES {
            if root.join(cand).is_file() {
                return cand.to_string();
            }
        }
        if declared.is_empty() {
            ENTRY_DEFAULT.to_string()
        } else {
            declared.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_deps() {
        let m = parse_manifest_str(
            "let name = \"p\"\nlet version = \"1.0.0\"\nlet deps = { a: \"u/a\", b: \"u/b\" }\n",
        )
        .unwrap();
        assert_eq!(m.name, "p");
        assert_eq!(m.version, "1.0.0");
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
    fn optional_fields() {
        let s = "let name = \"p\"\nlet description = \"d\"\nlet license = \"MIT\"\nlet entry = \"main.rak\"\n";
        let m = parse_manifest_str(s).unwrap();
        assert_eq!(m.description, "d");
        assert_eq!(m.license, "MIT");
        assert_eq!(m.entry, "main.rak");
    }

    #[test]
    fn missing_name_is_error() {
        assert!(parse_manifest_str("let version = \"1.0.0\"\n").is_err());
    }

    #[test]
    fn utf8_bom_is_tolerated() {
        // Windows editors and `Out-File -Encoding utf8` prepend a BOM; without
        // stripping it the first `let` no longer matches a prefix.
        let m = parse_manifest_str("\u{feff}let name = \"p\"\nlet version = \"2.0.0\"\n").unwrap();
        assert_eq!(m.name, "p");
        assert_eq!(m.version, "2.0.0");
    }
}
