//! Transitive dependency resolution.
//!
//! Given the root manifest, the resolver walks the dependency graph
//! depth-first, resolving each `user/repo[@constraint][#rev]` spec to an
//! exact git revision:
//!
//! * A pinned `#rev` always wins.
//! * Otherwise the newest `v*` tag satisfying the constraint is used.
//! * With no constraint (and no tags), the default-branch HEAD is used.
//!
//! A lockfile entry is reused when its source matches and its version still
//! satisfies the manifest constraint — that is what makes builds
//! reproducible. Cycles are tolerated (a package already on the current path
//! is skipped), and two sources providing the same package name is a conflict.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

use crate::cache::Cache;
use crate::git;
use crate::lock::LockFile;
use crate::manifest::{parse_manifest, Manifest, MANIFEST_FILE};
use crate::spec::{parse_dep_spec, version_satisfies, DepSpec};

/// A fully resolved package.
#[derive(Clone, Debug)]
pub struct Resolved {
    /// The package name (from its own manifest).
    pub name: String,
    /// The version declared in its manifest.
    pub version: String,
    /// The source repository, as written in the manifest (`user/repo` or a URL).
    pub repo: String,
    /// The resolved git URL that was cloned.
    pub url: String,
    /// The exact git revision.
    pub rev: String,
    /// SHA-256 hex of the package's `package.rak`.
    pub checksum: String,
    /// The package's own manifest (for transitive deps).
    pub manifest: Manifest,
    /// The checkout path in the global cache.
    pub checkout: std::path::PathBuf,
}

/// The resolver state.
pub struct Resolver<'a> {
    cache: &'a Cache,
    lock: &'a LockFile,
    entries: BTreeMap<String, Resolved>,
    visiting: Vec<String>,
}

impl<'a> Resolver<'a> {
    pub fn new(cache: &'a Cache, lock: &'a LockFile) -> Self {
        Resolver {
            cache,
            lock,
            entries: BTreeMap::new(),
            visiting: Vec::new(),
        }
    }

    /// Resolve every dependency of `root` (transitively), returning the
    /// entries sorted by name.
    pub fn resolve_root(&mut self, root: &Manifest) -> Result<Vec<Resolved>> {
        for (name, spec) in &root.deps {
            let spec = parse_dep_spec(spec);
            self.visit(name, &spec)
                .with_context(|| format!("resolving dependency '{}'", name))?;
        }
        Ok(self.entries.values().cloned().collect())
    }

    /// Resolve a single dependency and, recursively, its own dependencies.
    fn visit(&mut self, name: &str, spec: &DepSpec) -> Result<()> {
        if let Some(existing) = self.entries.get(name) {
            if existing.repo != spec.repo {
                bail!(
                    "dependency conflict: package '{}' is provided by both '{}' and '{}'",
                    name,
                    existing.repo,
                    spec.repo
                );
            }
            return Ok(());
        }
        if self.visiting.iter().any(|n| n == name) {
            // Cycle: the package is already being resolved further up the
            // stack. Its entry will be filled in by that visit.
            return Ok(());
        }
        self.visiting.push(name.to_string());

        let url = crate::spec::repo_url(&spec.repo);
        let rev = self.resolve_rev(name, spec, &url)?;
        let checkout = self.cache.ensure_checkout(&url, &rev)?;
        let manifest_path = checkout.join(MANIFEST_FILE);
        let manifest = parse_manifest(&manifest_path).map_err(|e| {
            anyhow::anyhow!(
                "dependency '{}' ({}) has an invalid {}: {}",
                name,
                spec.repo,
                MANIFEST_FILE,
                e
            )
        })?;
        let checksum = sha256_file(&manifest_path);

        self.entries.insert(
            name.to_string(),
            Resolved {
                name: name.to_string(),
                version: manifest.version.clone(),
                repo: spec.repo.clone(),
                url,
                rev,
                checksum,
                manifest: manifest.clone(),
                checkout,
            },
        );

        for (sub_name, sub_spec) in &manifest.deps {
            let sub_spec = parse_dep_spec(sub_spec);
            self.visit(sub_name, &sub_spec)
                .with_context(|| format!("resolving transitive dependency '{}'", sub_name))?;
        }

        self.visiting.pop();
        Ok(())
    }

    /// Determine the exact revision for a spec, preferring the lockfile.
    ///
    /// The lockfile is what makes a build reproducible: if it already records
    /// this source at a version that still satisfies the manifest constraint,
    /// that exact revision is reused rather than re-resolved to whatever the
    /// default branch points at today.
    fn resolve_rev(&self, name: &str, spec: &DepSpec, url: &str) -> Result<String> {
        // A pinned rev always wins, lockfile or not.
        if let Some(rev) = &spec.rev {
            return Ok(rev.clone());
        }
        if let Some(entry) = self.lock.get(name) {
            let locked_url = entry
                .source
                .strip_prefix("git+")
                .unwrap_or(&entry.source);
            let (locked_base, locked_rev) = match locked_url.rsplit_once('#') {
                Some((base, rev)) => (base, rev),
                None => (locked_url, ""),
            };
            if locked_base == url
                && !locked_rev.is_empty()
                && version_satisfies(&entry.version, spec.constraint.as_deref().unwrap_or(""))
            {
                return Ok(locked_rev.to_string());
            }
        }
        self.fresh_rev(spec, url)
    }

    /// Resolve a spec to a revision straight from the remote.
    fn fresh_rev(&self, spec: &DepSpec, url: &str) -> Result<String> {
        let db = self.cache.ensure_db(url)?;
        let constraint = spec.constraint.as_deref().unwrap_or("");
        let tags = git::list_tags(&db).with_context(|| format!("listing tags for {}", spec.repo))?;
        if let Some(tag) = tags
            .iter()
            .find(|t| version_satisfies(t.trim_start_matches('v'), constraint))
        {
            return Ok(tag.clone());
        }
        if constraint.is_empty() || constraint == "*" {
            return git::head_rev(&db).with_context(|| format!("resolving HEAD for {}", spec.repo));
        }
        bail!(
            "no version of {} satisfies constraint '{}' (found tags: {})",
            spec.repo,
            constraint,
            if tags.is_empty() {
                "none".to_string()
            } else {
                tags.join(", ")
            }
        )
    }
}

/// SHA-256 hex of a file's contents.
pub fn sha256_file(path: &Path) -> String {
    let data = std::fs::read(path).unwrap_or_default();
    sha256_bytes(&data)
}

/// SHA-256 hex of a byte slice.
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        // SHA-256 of the empty string.
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
