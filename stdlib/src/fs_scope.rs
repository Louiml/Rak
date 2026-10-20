//! Filesystem scope confinement for the capability sandbox.
//!
//! The capability table in `rakc` answers *"is this operation allowed at
//! all?"* ג€” that is a question about the **name** of a builtin, and it is
//! enforced in one place: every interpreter builtin flows through
//! `Interpreter::eval_builtin` and every VM native flows through
//! `Vm::call_value`, so a name-based gate at those two heads covers all of
//! them.
//!
//! `--fs-root` answers a different question: *"of the paths this operation is
//! allowed to touch, which ones?"* ג€” that is a question about a **path**, and
//! a path cannot be checked from a name. So the scope check lives here, at
//! the layer that actually opens files.
//!
//! ## Why this lives in the standard library
//!
//! Every script-facing filesystem operation in Rak goes through
//! [`crate::file`], [`crate::mmap`], [`crate::archive`], [`crate::log`] or
//! [`crate::crypto`]. Putting the check in those modules means a builtin added
//! tomorrow is scoped by construction, rather than by remembering to call
//! something. The handful of call sites that bypass the standard library
//! (the interpreter's own `read`/`write`, the two `LinesStream::open`
//! implementations, the VM's `read`/`write` natives and the two
//! `dump`-to-file paths) call [`check`] directly.
//!
//! The scope is *off* unless `--fs-root` was given, so this module costs
//! nothing for an ordinary program.

use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;

/// The confinement root, canonicalised when it is set.
///
/// `None` means no scope: every path is allowed. That is the default, and it
/// is what an unsandboxed run gets.
static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set the confinement root, or clear it with `None`.
///
/// The root is canonicalised immediately, so a relative `--fs-root` is
/// resolved against the current directory once rather than on every check, and
/// a symlinked root is compared by its real location.
pub fn set_root(root: Option<PathBuf>) {
    let canonical = root.and_then(|r| std::fs::canonicalize(&r).ok().or(Some(r)));
    *ROOT.write().unwrap() = canonical;
}

/// The confinement root, if one is set.
pub fn root() -> Option<PathBuf> {
    ROOT.read().unwrap().clone()
}

/// Is a scope currently in force?
pub fn is_scoped() -> bool {
    ROOT.read().unwrap().is_some()
}

/// Check that `path` is inside the scope, if one is set.
///
/// Returns `Ok(())` when no scope is in force, so a caller can call this
/// unconditionally at the top of a path-taking function.
pub fn check(path: &str) -> Result<(), String> {
    let Some(root) = root() else {
        return Ok(());
    };
    let resolved = resolve(path);
    if resolved.starts_with(&root) {
        return Ok(());
    }
    Err(format!(
        "sandbox: path '{}' is outside the filesystem scope '{}'",
        path,
        display(&root)
    ))
}

/// Render a path for a message, without the platform's verbatim prefix.
///
/// `std::fs::canonicalize` on Windows returns `\\?\C:\...`, which is correct
/// but unreadable in a diagnostic.
fn display(p: &Path) -> String {
    let s = p.to_string_lossy();
    #[cfg(windows)]
    {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    }
    #[cfg(not(windows))]
    {
        s.to_string()
    }
}

/// Resolve `path` as far as the filesystem allows, then normalise it.
///
/// The path is made absolute against the current directory, then the deepest
/// existing ancestor is canonicalised. Canonicalising the ancestor matters:
/// a symlink *inside* the scope that points outside it would otherwise walk
/// straight out, because a lexical check never looks at what a symlink
/// points at.
fn resolve(path: &str) -> PathBuf {
    let p = Path::new(path);
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    // The path does not exist (a file about to be created, say). Canonicalise
    // the directory that holds it and re-attach the final component.
    if let (Some(parent), Some(name)) = (p.parent(), p.file_name()) {
        if let Ok(c) = std::fs::canonicalize(parent) {
            return c.join(name);
        }
    }
    normalize(&absolute(p))
}

/// Make a path absolute against the current directory.
fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(p),
            Err(_) => p.to_path_buf(),
        }
    }
}

/// Remove `.` and resolve `..` textually.
///
/// Textual, not filesystem-based, because the path being checked may not
/// exist yet ג€” a file about to be written, or a directory about to be
/// created. `Path::pop` leaves a root alone when there is nothing above it,
/// so `..` at the root stays at the root rather than escaping it.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir => out.push(Path::new("/")),
            Component::Prefix(pre) => out.push(pre.as_os_str()),
            Component::Normal(part) => out.push(part),
        }
    }
    out
}
