//! Dependency specs and version constraints.
//!
//! A dependency spec is `user/repo[@constraint][#rev]`:
//!
//! * `user/repo` — track the default branch (HEAD).
//! * `user/repo@^1.2` — caret: compatible with `1.2` (same major).
//! * `user/repo@~1.2` — tilde: compatible with `1.2` (same major.minor).
//! * `user/repo@1.2.3` — exact version.
//! * `user/repo@*` — any version.
//! * `user/repo#deadbeef` — pin to an exact git revision (tag, branch, or
//!   commit hash). A `#rev` may be combined with a constraint
//!   (`user/repo@^1.2#deadbeef`); the rev wins.

/// A parsed dependency spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepSpec {
    /// The GitHub repository (`user/repo`).
    pub repo: String,
    /// The version constraint (`^1.2`, `~1.2`, `1.2.3`, `*`), if any.
    pub constraint: Option<String>,
    /// A pinned git revision, if any.
    pub rev: Option<String>,
}

/// Split a `user/repo[@vX.Y.Z | ^X | #rev]` spec into its parts.
pub fn parse_dep_spec(spec: &str) -> DepSpec {
    let mut repo = spec.trim().to_string();
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
    repo = repo.trim().to_string();
    DepSpec {
        repo,
        constraint: version,
        rev,
    }
}

/// Rough semver satisfaction for `^x.y`, `~x.y`, exact `x.y.z`, and `*`.
/// A missing/empty constraint matches anything.
pub fn version_satisfies(actual: &str, constraint: &str) -> bool {
    let c = constraint.trim();
    if c.is_empty() || c == "*" {
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
        let n = b.len().min(a.len());
        return a[..n] == b[..n];
    }
    a[..] == b
}

/// The git URL for a `user/repo` spec.
///
/// A bare `user/repo` is a GitHub repository. A spec that already looks like a
/// URL (`https://`, `ssh://`, `git@`, `file://`) is passed through unchanged,
/// which is what lets a project depend on a self-hosted Git server — and what
/// lets the test suite resolve real repositories without the network.
pub fn repo_url(repo: &str) -> String {
    let r = repo.trim();
    if r.contains("://") || r.starts_with("git@") {
        return r.to_string();
    }
    format!("https://github.com/{}.git", r)
}

/// The `user/repo` form of a URL, for display in the manifest.
pub fn repo_display(url: &str) -> String {
    url.strip_prefix("https://github.com/")
        .unwrap_or(url)
        .trim_end_matches(".git")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_repo() {
        let s = parse_dep_spec("u/r");
        assert_eq!(s.repo, "u/r");
        assert_eq!(s.constraint, None);
        assert_eq!(s.rev, None);
    }

    #[test]
    fn caret_and_rev() {
        let s = parse_dep_spec("u/r@^1.2#deadbeef");
        assert_eq!(s.repo, "u/r");
        assert_eq!(s.constraint.as_deref(), Some("^1.2"));
        assert_eq!(s.rev.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn rev_only() {
        let s = parse_dep_spec("u/r#abc123");
        assert_eq!(s.repo, "u/r");
        assert_eq!(s.rev.as_deref(), Some("abc123"));
    }

    #[test]
    fn version_satisfies_caret() {
        assert!(version_satisfies("1.2.3", "^1.2"));
        assert!(!version_satisfies("2.0.0", "^1.2"));
        assert!(version_satisfies("1.2.3", "1.2.3"));
        assert!(version_satisfies("9.9.9", "*"));
        assert!(version_satisfies("9.9.9", ""));
    }
}
