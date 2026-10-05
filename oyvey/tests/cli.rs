//! End-to-end tests for the `oyvey` CLI binary.
//!
//! These drive the actual executable through a temp directory and a temp
//! `OYVEY_HOME`, so they cover the pieces the library tests do not: manifest
//! discovery from the current directory, the scaffolding a new project gets,
//! and the exact exit codes each command returns.
//!
//! The tests that would need the network or a real GitHub repository are not
//! here; dependency resolution against local repositories is covered by
//! `tests/resolve.rs`, which exercises the same code path with `file://` URLs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn oyvey_bin() -> PathBuf {
    // The integration-test binary lives in target/<profile>/deps/.
    let mut p = std::env::current_exe().expect("current exe");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join(if cfg!(windows) { "oyvey.exe" } else { "oyvey" })
}

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "oyvey_cli_{}_{}_{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create temp dir");
        Tmp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `oyvey` in `dir` with an isolated cache.
fn run(dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(oyvey_bin())
        .args(args)
        .current_dir(dir)
        .env("OYVEY_HOME", home)
        .output()
        .expect("run oyvey")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn version_and_help_succeed() {
    let t = Tmp::new("help");
    let o = run(t.path(), t.path(), &["--version"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).starts_with("oyvey "), "{}", stdout(&o));

    let o = run(t.path(), t.path(), &["help"]);
    assert!(o.status.success());
    let s = stdout(&o);
    for expected in ["new", "init", "add", "remove", "install", "update", "build", "run", "test", "clean", "list"] {
        assert!(s.contains(expected), "help missing `{}`:\n{}", expected, s);
    }

    // Per-command help.
    let o = run(t.path(), t.path(), &["help", "add"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("github-repo"));
}

#[test]
fn unknown_command_exits_nonzero() {
    let t = Tmp::new("unknown");
    let o = run(t.path(), t.path(), &["frobnicate"]);
    assert!(!o.status.success(), "unknown command should fail");
}

#[test]
fn commands_without_a_project_report_a_clear_error() {
    let t = Tmp::new("noproject");
    for cmd in ["install", "run", "build", "test", "list", "tree", "audit", "update"] {
        let o = run(t.path(), t.path(), &[cmd]);
        assert!(!o.status.success(), "{} should fail without a project", cmd);
        let e = stderr(&o);
        assert!(
            e.contains("package.rak"),
            "{} error should mention package.rak, got: {}",
            cmd,
            e
        );
    }
}

#[test]
fn new_scaffolds_a_runnable_project() {
    let t = Tmp::new("new");
    let home = t.path().join("cache");
    let proj = t.path().join("demo");

    let o = run(t.path(), &home, &["new", "demo"]);
    assert!(o.status.success(), "{}", stderr(&o));

    for f in [
        "package.rak",
        "src/main.rak",
        "tests/main.rak",
        ".gitignore",
        "README.md",
    ] {
        assert!(
            proj.join(f).is_file(),
            "oyvey new should create {}",
            f
        );
    }

    // The manifest names the project and points at a real entry point.
    let manifest = std::fs::read_to_string(proj.join("package.rak")).unwrap();
    assert!(manifest.contains("let name = \"demo\""));
    assert!(manifest.contains("src/main.rak"));

    // `list` and `tree` work on the fresh project (no deps yet).
    let o = run(&proj, &home, &["list"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("No packages installed"));
}

#[test]
fn new_refuses_to_clobber_a_non_empty_directory() {
    let t = Tmp::new("new_ne");
    let home = t.path().join("cache");
    let proj = t.path().join("demo");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("keep.txt"), "x").unwrap();

    let o = run(t.path(), &home, &["new", "demo"]);
    assert!(!o.status.success(), "should refuse a non-empty directory");
    assert!(proj.join("keep.txt").is_file(), "must not delete existing files");
}

#[test]
fn new_lib_makes_a_library_project() {
    let t = Tmp::new("newlib");
    let home = t.path().join("cache");
    let proj = t.path().join("mylib");

    let o = run(t.path(), &home, &["new", "mylib", "--lib"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(proj.join("src/lib.rak").is_file(), "library entry point");
    let manifest = std::fs::read_to_string(proj.join("package.rak")).unwrap();
    assert!(manifest.contains("src/lib.rak"));
}

#[test]
fn init_creates_a_manifest_in_place() {
    let t = Tmp::new("init");
    let home = t.path().join("cache");
    let proj = t.path().join("myproj");
    std::fs::create_dir_all(&proj).unwrap();

    let o = run(&proj, &home, &["init"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(proj.join("package.rak").is_file());
    assert!(proj.join("src/main.rak").is_file());
    // The project name defaults to the directory name.
    let manifest = std::fs::read_to_string(proj.join("package.rak")).unwrap();
    assert!(manifest.contains("let name = \"myproj\""));
}

#[test]
fn init_does_not_overwrite_an_existing_manifest() {
    let t = Tmp::new("init_keep");
    let home = t.path().join("cache");
    let proj = t.path().join("p");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("package.rak"), "let name = \"original\"\n").unwrap();

    let o = run(&proj, &home, &["init"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let manifest = std::fs::read_to_string(proj.join("package.rak")).unwrap();
    assert!(manifest.contains("original"));
}

#[test]
fn lock_writes_an_empty_lockfile_for_a_dep_free_project() {
    let t = Tmp::new("lock");
    let home = t.path().join("cache");
    let proj = t.path().join("p");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("package.rak"),
        "let name = \"p\"\nlet entry = \"main.rak\"\nlet deps = {}\n",
    )
    .unwrap();
    std::fs::write(proj.join("main.rak"), "dump 1\n").unwrap();

    let o = run(&proj, &home, &["lock"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let lock = std::fs::read_to_string(proj.join("oyvey.lock")).unwrap();
    assert!(lock.contains("version = 1"), "{}", lock);
    assert!(lock.contains("@generated"), "lockfile should be marked generated");
}

#[test]
fn remove_reports_an_unknown_dependency() {
    let t = Tmp::new("remove_missing");
    let home = t.path().join("cache");
    let proj = t.path().join("p");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("package.rak"),
        "let name = \"p\"\nlet entry = \"main.rak\"\nlet deps = {}\n",
    )
    .unwrap();

    let o = run(&proj, &home, &["remove", "nope"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("not a dependency"));
}
