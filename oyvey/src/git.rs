//! Git operations against the global cache.
//!
//! Oyvey shells out to the `git` CLI (the same approach `rakpkg` took) so it
//! works on any platform git runs on, with no libgit2 dependency. Every
//! function returns a clear error when git is missing or a command fails.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The extra `-c` flags every git invocation carries.
///
/// `core.longpaths` is the important one on Windows. A cache checkout nests
/// several directories deep and git then writes `.git/objects/pack/pack-<sha>.idx`
/// underneath it, which overflows the classic 260-character `MAX_PATH` limit
/// for anything but a very short home directory — the clone dies with
/// "Filename too long". Setting it per-invocation puts git on the
/// extended-length path API without touching the user's global git config.
/// It is a no-op on other platforms.
const GIT_FLAGS: [&str; 2] = ["-c", "core.longpaths=true"];

/// Run a git command, returning stdout. Fails with context on non-zero exit.
fn git(dir: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(GIT_FLAGS);
    cmd.args(args);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd
        .output()
        .with_context(|| format!("failed to run `git {}` (is git installed?)", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Clone a repository URL into `dest` (bare). Errors if `dest` already exists.
pub fn clone_bare(url: &str, dest: &Path) -> Result<()> {
    if dest.exists() {
        bail!("cache db already exists: {}", dest.display());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    git(None, &["clone", "--bare", url, &dest.to_string_lossy()])
        .map(|_| ())
        .with_context(|| format!("cloning {}", url))
}

/// Fetch all tags (and the default branch) into an existing bare db.
pub fn fetch(db: &Path) -> Result<()> {
    git(
        Some(db),
        &["fetch", "--tags", "--force", "origin"],
    )
    .map(|_| ())
    .with_context(|| format!("fetching {}", db.display()))
}

/// List `v*` tags sorted newest-first by version.
pub fn list_tags(db: &Path) -> Result<Vec<String>> {
    let out = git(
        Some(db),
        &["tag", "--list", "v*", "--sort=-v:refname"],
    )?;
    Ok(out.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

/// Resolve the default-branch HEAD commit of a bare db.
pub fn head_rev(db: &Path) -> Result<String> {
    let out = git(Some(db), &["rev-parse", "HEAD"])?;
    Ok(out.trim().to_string())
}

/// Verify that a revision exists in the db.
pub fn rev_exists(db: &Path, rev: &str) -> Result<bool> {
    let mut cmd = Command::new("git");
    cmd.current_dir(db);
    cmd.args(["cat-file", "-e", &format!("{}^{{commit}}", rev)]);
    let out = cmd
        .output()
        .with_context(|| format!("failed to run `git cat-file` in {}", db.display()))?;
    Ok(out.status.success())
}

/// Materialize the tree at `rev` into `dest`.
///
/// This clones from the local bare `db` (cheap: a local clone hardlinks the
/// object store) and then detaches HEAD at `rev`. The result is a normal git
/// working tree; `.git` is stripped when the package is vendored.
///
/// An earlier version used `git archive --format=tar | tar -x`, which needed an
/// external `tar` on PATH. That is not a reasonable thing to require of a
/// package manager on a minimal Windows box, so the extraction is done with git
/// itself instead.
pub fn export_tree(db: &Path, rev: &str, dest: &Path) -> Result<()> {
    if dest.exists() {
        std::fs::remove_dir_all(dest)
            .with_context(|| format!("clearing {}", dest.display()))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    if !rev_exists(db, rev)? {
        bail!("revision '{}' not found in {}", rev, db.display());
    }
    git(
        None,
        &[
            "clone",
            "--quiet",
            "--no-checkout",
            &db.to_string_lossy(),
            &dest.to_string_lossy(),
        ],
    )
    .with_context(|| format!("cloning {} into {}", db.display(), dest.display()))?;
    git(
        Some(dest),
        &["checkout", "--quiet", "--detach", rev],
    )
    .with_context(|| format!("checking out {} in {}", rev, dest.display()))?;
    Ok(())
}

/// The path of the git executable, or an error naming what to install.
pub fn git_executable() -> Result<PathBuf> {
    if let Ok(path) = which("git") {
        return Ok(path);
    }
    bail!("git is required for Oyvey but was not found on PATH. Install git from https://git-scm.com/")
}

/// Locate an executable on PATH (cross-platform).
fn which(name: &str) -> Result<PathBuf> {
    let path_var = std::env::var("PATH").context("reading PATH")?;
    let exts: Vec<&str> = if cfg!(windows) {
        vec![".exe", ".cmd", ".bat", ""]
    } else {
        vec![""]
    };
    for dir in path_var.split(if cfg!(windows) { ';' } else { ':' }) {
        if dir.is_empty() {
            continue;
        }
        for ext in &exts {
            let candidate = Path::new(dir).join(format!("{}{}", name, ext));
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("`{}` not found on PATH", name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_git() {
        // git is required for Oyvey to work at all, so it must be on PATH in
        // any environment the tests run in.
        assert!(which("git").is_ok());
    }
}
