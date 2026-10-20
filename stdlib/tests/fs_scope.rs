//! Tests for the filesystem scope confinement.
//!
//! These live in their own integration-test binary rather than in the
//! module's `#[cfg(test)]` block, and that is not a style choice.
//!
//! The confinement root is process-global state. The library's own unit
//! tests live in the same binary as the module, and several of them open
//! files: `mmap::open`, `file::read_bytes`, `archive::zip_read` and
//! `log::init` all call `fs_scope::check` first. A scope test that set the
//! root while one of those was running would make the *other* test fail —
//! and the failure reads as "the sandbox let a path through", which is a
//! much more alarming thing to see than "two tests raced".
//!
//! An integration test is a separate binary, and therefore a separate
//! process, so the scope cannot leak into anything else. The lock below is
//! still needed: these tests share one process with each other.

use rak_stdlib::fs_scope::{check, is_scoped, root, set_root};
use std::path::Path;

/// The scope root is process-global state, so every test that changes it has
/// to be serialised against the others in this binary.
///
/// `into_inner` rather than `unwrap`: a test that panicked while holding the
/// lock poisons it, and the next test should still run.
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_root<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let previous = root();
    set_root(Some(dir.to_path_buf()));
    let out = f();
    set_root(previous);
    out
}

/// A path under the root is allowed, and one outside it is not.
#[test]
fn paths_are_confined_to_the_root() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = std::env::temp_dir().join("rak_fs_scope_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inside")).unwrap();
    let root = dir.canonicalize().unwrap();

    with_root(&root, || {
        assert!(check(root.join("inside").to_str().unwrap()).is_ok());
        assert!(check(root.join("inside").join("f.txt").to_str().unwrap()).is_ok());
        assert!(check(root.parent().unwrap().to_str().unwrap()).is_err());
        assert!(check("/etc/passwd").is_err());
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// `..` must not walk out of the root. This is the whole reason the check
/// normalises rather than comparing the raw string.
#[test]
fn dot_dot_cannot_escape_the_root() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = std::env::temp_dir().join("rak_fs_scope_dotdot");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.canonicalize().unwrap();

    with_root(&root, || {
        let escape = root.join("..").join("etc").join("passwd");
        assert!(check(escape.to_str().unwrap()).is_err());
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// A relative path is resolved against the current directory, so a relative
/// path that leaves the root is refused too.
#[test]
fn relative_paths_are_resolved_against_the_cwd() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = std::env::temp_dir().join("rak_fs_scope_rel");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.canonicalize().unwrap();

    let previous_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();
    with_root(&root, || {
        assert!(check("inside.txt").is_ok());
        assert!(check("../outside.txt").is_err());
    });
    std::env::set_current_dir(previous_cwd).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// With no root set, every path is allowed. This is the unsandboxed default
/// and the module must not get in the way.
#[test]
fn no_root_means_no_restriction() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // The root is restored afterwards: leaving it cleared would silently
    // widen the scope for whatever ran next.
    let previous = root();
    set_root(None);
    assert!(check("/etc/passwd").is_ok());
    assert!(check("").is_ok());
    assert!(!is_scoped());
    set_root(previous);
}

/// A symlink inside the root that points outside it must not be a way out.
/// A lexical check would allow it, because the path *looks* like it is under
/// the root.
#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_root_is_refused() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = std::env::temp_dir().join("rak_fs_scope_symlink");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("root")).unwrap();
    let root = dir.join("root").canonicalize().unwrap();
    std::os::unix::fs::symlink("/etc", dir.join("root").join("escape")).unwrap();

    with_root(&root, || {
        let link = root.join("escape").join("passwd");
        assert!(
            check(link.to_str().unwrap()).is_err(),
            "a symlink pointing out of the root must not be a way out"
        );
    });
    let _ = std::fs::remove_dir_all(&dir);
}
