//! Oyvey — the Rak package manager and build system.
//!
//! A Cargo-inspired CLI: a `package.rak` manifest, an `oyvey.lock` lockfile,
//! a global git cache, and tight integration with the `rakc` compiler. See
//! the [crate documentation](oyvey) for the module layout.

use anyhow::Context;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use oyvey::cache::Cache;
use oyvey::lock::{LockEntry, LockFile};
use oyvey::manifest::{Manifest, MANIFEST_FILE};
use oyvey::project::{
    find_project_root, load_lock, load_manifest, packages_dir, scaffold_init, scaffold_new,
    vendor_packages,
};
use oyvey::resolve::Resolver;
use oyvey::spec::parse_dep_spec;
use oyvey::{LOCK_FILE, RAK_PATH_ENV};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_usage() {
    println!("oyvey {} - Rak package manager and build system", VERSION);
    println!();
    println!("USAGE:");
    println!("  oyvey <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("  new <project>          Generate a new Rak project");
    println!("  init                   Initialize an existing directory as a project");
    println!("  add <github-repo>      Add a GitHub package dependency");
    println!("  remove <package>       Remove a dependency");
    println!("  install                Resolve and install dependencies (writes oyvey.lock)");
    println!("  update                 Re-resolve dependencies within constraints");
    println!("  build                  Compile the project through rakc");
    println!("  run                    Run the project entry point");
    println!("  test                   Run the project's tests");
    println!("  clean                  Remove build artifacts");
    println!("  list                   List installed packages");
    println!("  tree                   Print the dependency tree");
    println!("  audit                  Verify installed packages against the lockfile");
    println!("  lock                   Write oyvey.lock without installing");
    println!("  version                Print version");
    println!("  help                   Show this help");
    println!();
    println!("DEPENDENCY SPECS:");
    println!("  user/repo              track the default branch");
    println!("  user/repo@^1.2         caret constraint (compatible with 1.2)");
    println!("  user/repo@~1.2         tilde constraint (compatible with 1.2.x)");
    println!("  user/repo@1.2.3        exact version");
    println!("  user/repo#<rev>        pin to a git revision (tag, branch, or commit)");
    println!();
    println!("ENVIRONMENT:");
    println!("  OYVEY_HOME             global cache location (default: ~/.oyvey)");
    println!("  RAK_PATH               extra module search path (oyvey prepends <project>/packages)");
    println!();
    println!("Run `oyvey help <command>` for details on a specific command.");
}

fn print_cmd_help(cmd: &str) {
    match cmd {
        "new" => {
            println!("oyvey new <project> — generate a new Rak project");
            println!();
            println!("Creates <project>/ with a package.rak manifest, a src/main.rak entry");
            println!("point, a tests/ directory, a .gitignore, and a README. The project is");
            println!("immediately buildable with `oyvey build` and runnable with `oyvey run`.");
            println!();
            println!("FLAGS:");
            println!("  --lib      generate a library project (src/lib.rak, no main)");
        }
        "init" => {
            println!("oyvey init — initialize an existing directory as a project");
            println!();
            println!("Writes a package.rak manifest (if missing) and a src/main.rak entry");
            println!("point (if missing) in the current directory. Never overwrites.");
            println!();
            println!("FLAGS:");
            println!("  --lib      use a library entry point (src/lib.rak)");
        }
        "add" => {
            println!("oyvey add <github-repo> — add a GitHub package dependency");
            println!();
            println!("Clones the repository, reads its package name, and records it in the");
            println!("manifest's deps. The dependency is then installed and locked.");
            println!();
            println!("EXAMPLES:");
            println!("  oyvey add user/rak-net");
            println!("  oyvey add user/rak-crypto@^1.0");
            println!("  oyvey add user/rak-utils#deadbeef");
        }
        "remove" => {
            println!("oyvey remove <package> — remove a dependency");
            println!();
            println!("Drops the package from the manifest, removes its vendored copy, and");
            println!("prunes the lockfile.");
        }
        "install" => {
            println!("oyvey install — resolve and install dependencies");
            println!();
            println!("Resolves every dependency in package.rak (transitively), records the");
            println!("exact revision and checksum in oyvey.lock, and vendors each package");
            println!("into packages/. Honours the lockfile when it is up to date.");
        }
        "update" => {
            println!("oyvey update — re-resolve dependencies within constraints");
            println!();
            println!("Ignores the locked revisions and picks the newest version satisfying");
            println!("each constraint, then rewrites oyvey.lock and re-vendors.");
        }
        "build" => {
            println!("oyvey build — compile the project through rakc");
            println!();
            println!("Ensures dependencies are installed, then invokes `rakc build` on the");
            println!("entry point to produce a standalone executable.");
        }
        "run" => {
            println!("oyvey run — run the project entry point");
            println!();
            println!("Ensures dependencies are installed, then invokes `rakc run` on the");
            println!("entry point with RAK_PATH pointing at the vendored packages.");
        }
        "test" => {
            println!("oyvey test — run the project's tests");
            println!();
            println!("Ensures dependencies are installed, then invokes `rakc test` from the");
            println!("project root with RAK_PATH pointing at the vendored packages.");
        }
        "clean" => {
            println!("oyvey clean — remove build artifacts");
            println!();
            println!("Removes the executable produced by `oyvey build`.");
        }
        "list" => {
            println!("oyvey list — list installed packages");
            println!();
            println!("Shows every vendored package with its version and locked revision.");
        }
        "tree" => {
            println!("oyvey tree — print the dependency tree");
            println!();
            println!("Prints the resolved dependency graph from the lockfile, cycle-safe.");
        }
        "audit" => {
            println!("oyvey audit — verify installed packages against the lockfile");
            println!();
            println!("Checks that every locked package is vendored and that its manifest");
            println!("checksum matches. Exits non-zero on any mismatch.");
        }
        "lock" => {
            println!("oyvey lock — write oyvey.lock without installing");
            println!();
            println!("Resolves dependencies and writes the lockfile, but does not vendor");
            println!("packages into packages/.");
        }
        _ => print_usage(),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let cmd = args[1].as_str();
    let rest = &args[2..];

    let code = match dispatch(cmd, rest) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("oyvey error: {:#}", e);
            1
        }
    };
    std::process::exit(code);
}

fn dispatch(cmd: &str, args: &[String]) -> Result<i32, anyhow::Error> {
    match cmd {
        "--version" | "-V" | "version" => {
            println!("oyvey {}", VERSION);
            Ok(0)
        }
        "--help" | "-h" | "help" => {
            if let Some(sub) = args.first() {
                print_cmd_help(sub);
            } else {
                print_usage();
            }
            Ok(0)
        }
        "new" => cmd_new(args),
        "init" => cmd_init(args),
        "add" => cmd_add(args),
        "remove" => cmd_remove(args),
        "install" => cmd_install(args),
        "update" => cmd_update(args),
        "build" => cmd_build(args),
        "run" => cmd_run(args),
        "test" => cmd_test(args),
        "clean" => cmd_clean(args),
        "list" => cmd_list(args),
        "tree" => cmd_tree(args),
        "audit" => cmd_audit(args),
        "lock" => cmd_lock(args),
        other => {
            eprintln!("unknown command: {}", other);
            print_usage();
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// new / init
// ---------------------------------------------------------------------------

fn cmd_new(args: &[String]) -> Result<i32, anyhow::Error> {
    let (name, lib) = parse_new_init_args(args, "oyvey new <project>")?;
    let path = PathBuf::from(&name);
    let proj_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.clone());
    scaffold_new(&path, &proj_name, lib)?;
    println!("Created project: {}", path.display());
    println!("  {} (manifest)", MANIFEST_FILE);
    println!("  {} (entry point)", if lib { "src/lib.rak" } else { "src/main.rak" });
    println!("  tests/ (tests)");
    println!();
    println!("Next steps:");
    println!("  cd {}", path.display());
    println!("  oyvey run");
    Ok(0)
}

fn cmd_init(args: &[String]) -> Result<i32, anyhow::Error> {
    let (_name, lib) = parse_new_init_args(args, "oyvey init")?;
    let root = std::env::current_dir()?;
    let proj_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-package".to_string());
    scaffold_init(&root, &proj_name, lib)?;
    println!("Initialized project in {}", root.display());
    Ok(0)
}

/// Parse `[--lib] [name]` for new/init. Returns (name, lib).
fn parse_new_init_args(args: &[String], usage: &str) -> Result<(String, bool), anyhow::Error> {
    let mut lib = false;
    let mut name: Option<String> = None;
    for a in args {
        match a.as_str() {
            "--lib" => lib = true,
            "--help" | "-h" => {
                print_cmd_help(if usage.contains("new") { "new" } else { "init" });
                std::process::exit(0);
            }
            other => {
                if other.starts_with('-') {
                    anyhow::bail!("unknown flag '{}' (try `oyvey help`)", other);
                }
                if name.is_none() {
                    name = Some(other.to_string());
                } else {
                    anyhow::bail!("unexpected argument '{}' (usage: {})", other, usage);
                }
            }
        }
    }
    let name = name.unwrap_or_else(|| ".".to_string());
    Ok((name, lib))
}

// ---------------------------------------------------------------------------
// add / remove
// ---------------------------------------------------------------------------

fn cmd_add(args: &[String]) -> Result<i32, anyhow::Error> {
    let spec = require_arg(args, "oyvey add <github-repo>")?;
    let root = find_project_root(Path::new("."))?;
    let mut manifest = load_manifest(&root)?;

    // Resolve the spec to learn the package's declared name.
    let dep = parse_dep_spec(&spec);
    if dep.repo.is_empty() {
        anyhow::bail!("invalid dependency spec '{}' (expected user/repo)", spec);
    }
    let cache = Cache::open()?;
    let resolved = resolve_one(&cache, &dep)?;
    let pkg_name = resolved.name.clone();

    if manifest.deps.contains_key(&pkg_name) {
        println!("{} is already a dependency", pkg_name);
        return Ok(0);
    }

    manifest.deps.insert(pkg_name.clone(), spec.trim().to_string());
    write_manifest(&root, &manifest)?;
    println!("Added {} -> {}", pkg_name, spec.trim());

    // Install (resolves the whole graph, vendors, writes the lockfile).
    install(&root, false)?;
    Ok(0)
}

fn cmd_remove(args: &[String]) -> Result<i32, anyhow::Error> {
    let name = require_arg(args, "oyvey remove <package>")?;
    let root = find_project_root(Path::new("."))?;
    let mut manifest = load_manifest(&root)?;
    if manifest.deps.remove(&name).is_none() {
        anyhow::bail!("'{}' is not a dependency of this project", name);
    }
    write_manifest(&root, &manifest)?;
    println!("Removed {} from {}", name, MANIFEST_FILE);

    // Re-resolve to prune the lockfile and stale vendored copies.
    install(&root, false)?;
    Ok(0)
}

/// Resolve a single dependency spec to a `Resolved` (used by `add` to learn
/// the package name before recording it).
fn resolve_one(cache: &Cache, dep: &oyvey::spec::DepSpec) -> Result<oyvey::resolve::Resolved, anyhow::Error> {
    // A one-off manifest with a single dep keyed by the repo name, so the
    // resolver walks it and reports the package's declared name.
    let mut deps = std::collections::BTreeMap::new();
    deps.insert(dep.repo.clone(), dep_repo_string(dep));
    let synthetic = Manifest {
        deps,
        ..Default::default()
    };
    let lock = LockFile::default();
    let mut resolver = Resolver::new(cache, &lock);
    let resolved = resolver.resolve_root(&synthetic)?;
    resolved
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("could not resolve {}", dep.repo))
}

fn dep_repo_string(dep: &oyvey::spec::DepSpec) -> String {
    let mut s = dep.repo.clone();
    if let Some(c) = &dep.constraint {
        s.push('@');
        s.push_str(c);
    }
    if let Some(r) = &dep.rev {
        s.push('#');
        s.push_str(r);
    }
    s
}

// ---------------------------------------------------------------------------
// install / update / lock
// ---------------------------------------------------------------------------

fn cmd_install(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("install");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    install(&root, false)?;
    Ok(0)
}

fn cmd_update(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("update");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    install(&root, true)?;
    Ok(0)
}

fn cmd_lock(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("lock");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    let manifest = load_manifest(&root)?;
    let cache = Cache::open()?;
    let lock = LockFile::default();
    let mut resolver = Resolver::new(&cache, &lock);
    let resolved = resolver.resolve_root(&manifest)?;
    let lock = lockfile_from_resolved(&resolved);
    oyvey::lock::save_lock(&root.join(LOCK_FILE), &lock)?;
    println!("Wrote {} ({} packages)", root.join(LOCK_FILE).display(), lock.package.len());
    Ok(0)
}

/// Resolve + vendor + write the lockfile. `update` ignores locked revisions.
fn install(root: &Path, update: bool) -> Result<(), anyhow::Error> {
    let manifest = load_manifest(root)?;
    let existing = load_lock(root).unwrap_or_default();
    let cache = Cache::open()?;

    // For a plain install, reuse the lockfile's revisions (reproducible).
    // For update, resolve fresh within constraints.
    let base_lock = if update { LockFile::default() } else { existing };
    let mut resolver = Resolver::new(&cache, &base_lock);
    let resolved = resolver.resolve_root(&manifest)?;

    vendor_packages(root, &resolved)?;
    let lock = lockfile_from_resolved(&resolved);
    oyvey::lock::save_lock(&root.join(LOCK_FILE), &lock)?;

    if resolved.is_empty() {
        println!("No dependencies to install");
    } else {
        let verb = if update { "Updated" } else { "Installed" };
        println!("{} {} package(s) into {}", verb, resolved.len(), packages_dir(root).display());
        println!("Wrote {}", root.join(LOCK_FILE).display());
    }
    Ok(())
}

fn lockfile_from_resolved(resolved: &[oyvey::resolve::Resolved]) -> LockFile {
    let mut lock = LockFile::default();
    for r in resolved {
        lock.upsert(LockEntry {
            name: r.name.clone(),
            version: r.version.clone(),
            source: format!("git+{}#{}", r.url, r.rev),
            checksum: r.checksum.clone(),
        });
    }
    lock
}

// ---------------------------------------------------------------------------
// build / run / test
// ---------------------------------------------------------------------------

fn cmd_build(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("build");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    ensure_deps(&root)?;
    let manifest = load_manifest(&root)?;
    let entry = manifest.resolved_entry(&root);
    if !root.join(&entry).is_file() {
        anyhow::bail!(
            "entry point '{}' not found (declared in {})",
            manifest.entry,
            MANIFEST_FILE
        );
    }
    let rakc = rakc_path()?;
    // rakc already prints `Built: <path>` and the embedded-source size, so
    // oyvey does not repeat it here.
    let status = invoke_rakc(&root, &rakc, &["build".to_string(), entry.clone()])?;
    Ok(status.code().unwrap_or(0))
}

fn cmd_run(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("run");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    ensure_deps(&root)?;
    let manifest = load_manifest(&root)?;
    let entry = manifest.resolved_entry(&root);
    if !root.join(&entry).is_file() {
        anyhow::bail!(
            "entry point '{}' not found (declared in {})",
            manifest.entry,
            MANIFEST_FILE
        );
    }
    let rakc = rakc_path()?;
    let status = invoke_rakc(&root, &rakc, &["run".to_string(), entry.clone()])?;
    Ok(status.code().unwrap_or(0))
}

fn cmd_test(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("test");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    // When no flags were passed, discover the suite the same way `rakc test`
    // does and bail out with a clear message if the project has none. Left to
    // rakc it falls back to `test.rak`, which for a project without one is a
    // read error reported as a failing test.
    let mut files: Vec<String> = Vec::new();
    let mut forwarded_flags: Vec<String> = args.to_vec();
    if args.iter().all(|a| a == "--help" || a == "-h") {
        forwarded_flags.clear();
    }
    let explicit_file = args
        .iter()
        .find(|a| !a.starts_with('-') && a.ends_with(".rak"))
        .cloned();
    if explicit_file.is_none() {
        if let Ok(entries) = std::fs::read_dir(root.join("tests")) {
            let mut raks: Vec<String> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "rak").unwrap_or(false))
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            raks.sort();
            files = raks;
        }
        if files.is_empty() && root.join("test.rak").is_file() {
            files.push(root.join("test.rak").to_string_lossy().to_string());
        }
        if files.is_empty() {
            println!("No tests found (looked in tests/ and test.rak)");
            return Ok(0);
        }
    }
    ensure_deps(&root)?;
    let rakc = rakc_path()?;
    // Forward any extra flags (e.g. --filter) plus the discovered files.
    let mut forward = vec!["test".to_string()];
    forward.extend(forwarded_flags);
    if let Some(f) = explicit_file {
        forward.push(f);
    } else {
        forward.extend(files);
    }
    let status = invoke_rakc(&root, &rakc, &forward)?;
    Ok(status.code().unwrap_or(0))
}

/// Make sure dependencies are resolved and vendored. Skips network when the
/// lockfile is fresh and every package is already vendored.
fn ensure_deps(root: &Path) -> Result<(), anyhow::Error> {
    let manifest = load_manifest(root)?;
    let lock = load_lock(root).unwrap_or_default();
    let fresh = !manifest.deps.is_empty()
        && manifest.deps.keys().all(|d| lock.get(d).is_some())
        && manifest
            .deps
            .keys()
            .all(|d| packages_dir(root).join(d).exists());
    if fresh {
        return Ok(());
    }
    install(root, false)
}

/// Invoke rakc with CWD = project root and RAK_PATH pointing at the vendored
/// packages, so `import <pkg>` resolves.
fn invoke_rakc(root: &Path, rakc: &str, args: &[String]) -> Result<ExitStatus, anyhow::Error> {
    let pkgs = packages_dir(root);
    let mut rak_path = pkgs.to_string_lossy().to_string();
    if let Ok(existing) = env::var(RAK_PATH_ENV) {
        if !existing.is_empty() {
            let sep = if cfg!(windows) { ';' } else { ':' };
            rak_path.push(sep);
            rak_path.push_str(&existing);
        }
    }
    let status = Command::new(rakc)
        .args(args)
        .current_dir(root)
        .env(RAK_PATH_ENV, &rak_path)
        .status()
        .with_context(|| format!("running {} (is rakc installed?)", rakc))?;
    Ok(status)
}

/// The file `rakc build` produces for an entry point (`src/main.rak` ->
/// `src/main`, plus `.exe` on Windows).
fn build_output_name(entry: &str) -> String {
    let base = entry.strip_suffix(".rak").unwrap_or(entry);
    if cfg!(windows) {
        format!("{}.exe", base)
    } else {
        base.to_string()
    }
}

// ---------------------------------------------------------------------------
// clean / list / tree / audit
// ---------------------------------------------------------------------------

fn cmd_clean(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("clean");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    let manifest = load_manifest(&root)?;
    let exe = build_output_name(&manifest.resolved_entry(&root));
    let mut removed = 0;
    for cand in [&exe, &format!("{}.exe", exe)] {
        let p = root.join(cand);
        if p.is_file() {
            std::fs::remove_file(&p)
                .with_context(|| format!("removing {}", p.display()))?;
            println!("Removed {}", p.display());
            removed += 1;
        }
    }
    if removed == 0 {
        println!("Nothing to clean");
    }
    Ok(0)
}

fn cmd_list(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("list");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    let lock = load_lock(&root)?;
    if lock.package.is_empty() {
        println!("No packages installed");
        return Ok(0);
    }
    println!("Installed packages ({}):", lock.package.len());
    for entry in &lock.package {
        let short: String = entry.source.rsplit('#').next().unwrap_or("").chars().take(12).collect();
        println!("  {} v{} ({})", entry.name, entry.version, short);
    }
    Ok(0)
}

fn cmd_tree(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("tree");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    let manifest = load_manifest(&root)?;
    let lock = load_lock(&root)?;
    println!("{} v{} (root)", manifest.name, manifest.version);
    let mut seen = std::collections::HashSet::new();
    for name in manifest.deps.keys() {
        if let Some(entry) = lock.get(name) {
            print_tree_entry(entry, &lock, 1, &mut seen);
        } else {
            println!("  {} (not installed)", name);
        }
    }
    Ok(0)
}

fn print_tree_entry(
    entry: &LockEntry,
    lock: &LockFile,
    depth: usize,
    seen: &mut std::collections::HashSet<String>,
) {
    let indent = "  ".repeat(depth);
    if !seen.insert(entry.name.clone()) {
        println!("{}{} v{} (cyclic)", indent, entry.name, entry.version);
        return;
    }
    println!("{}{} v{}", indent, entry.name, entry.version);
    // Read this package's deps from its vendored manifest.
    let vendored = packages_dir(Path::new(".")).join(&entry.name).join(MANIFEST_FILE);
    if let Ok(m) = oyvey::manifest::parse_manifest(&vendored) {
        for sub in m.deps.keys() {
            if let Some(sub_entry) = lock.get(sub) {
                print_tree_entry(sub_entry, lock, depth + 1, seen);
            } else {
                println!("{}  {} (not installed)", indent, sub);
            }
        }
    }
    seen.remove(&entry.name);
}

fn cmd_audit(args: &[String]) -> Result<i32, anyhow::Error> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            print_cmd_help("audit");
            return Ok(0);
        }
    }
    let root = find_project_root(Path::new("."))?;
    let lock = load_lock(&root)?;
    if lock.package.is_empty() {
        println!("No {} entries to audit", LOCK_FILE);
        return Ok(0);
    }
    let mut issues = 0;
    for entry in &lock.package {
        let manifest_path = packages_dir(&root).join(&entry.name).join(MANIFEST_FILE);
        if !manifest_path.is_file() {
            println!("[WARN] {}: missing (not installed)", entry.name);
            issues += 1;
            continue;
        }
        let checksum = oyvey::resolve::sha256_file(&manifest_path);
        if checksum != entry.checksum {
            println!("[FAIL] {}: checksum mismatch (tampered?)", entry.name);
            issues += 1;
        } else {
            println!("[ok]   {} v{} (checksum verified)", entry.name, entry.version);
        }
    }
    if issues == 0 {
        println!("All {} package(s) verified", lock.package.len());
        Ok(0)
    } else {
        println!("{} issue(s) found", issues);
        Ok(1)
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn require_arg(args: &[String], usage: &str) -> Result<String, anyhow::Error> {
    match args.first() {
        Some(a) if !a.starts_with('-') => Ok(a.clone()),
        _ => anyhow::bail!("missing argument (usage: {})", usage),
    }
}

/// Locate the rakc binary. Searches PATH, the directory containing the oyvey
/// executable, and the workspace target dirs.
fn rakc_path() -> Result<String, anyhow::Error> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let mut candidates = vec!["rakc".to_string()];
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("rakc").to_string_lossy().to_string());
            candidates.push(parent.join(format!("rakc{}", ext)).to_string_lossy().to_string());
        }
    }
    candidates.push(format!("target/release/rakc{}", ext));
    candidates.push(format!("target/debug/rakc{}", ext));
    for c in &candidates {
        if Command::new(c).arg("--version").output().is_ok() {
            return Ok(c.clone());
        }
    }
    anyhow::bail!(
        "rakc not found. Oyvey drives the rakc compiler; install it from \
         https://github.com/Louiml/Rak or build it with `cargo build --release -p rakc`."
    )
}

/// Serialize a manifest back to Rak source (used by add/remove).
fn write_manifest(root: &Path, manifest: &Manifest) -> Result<(), anyhow::Error> {
    let mut out = String::new();
    out.push_str(&format!("let name = \"{}\"\n", manifest.name));
    out.push_str(&format!("let version = \"{}\"\n", manifest.version));
    if !manifest.description.is_empty() {
        out.push_str(&format!("let description = \"{}\"\n", manifest.description));
    }
    if !manifest.license.is_empty() {
        out.push_str(&format!("let license = \"{}\"\n", manifest.license));
    }
    out.push_str(&format!("let entry = \"{}\"\n", manifest.entry));
    if manifest.deps.is_empty() {
        out.push_str("let deps = {}\n");
    } else {
        out.push_str("let deps = {\n");
        for (name, spec) in &manifest.deps {
            out.push_str(&format!("    {}: \"{}\",\n", name, spec));
        }
        out.push_str("}\n");
    }
    let path = root.join(MANIFEST_FILE);
    std::fs::write(&path, out)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_name_strips_rak() {
        if cfg!(windows) {
            assert_eq!(build_output_name("src/main.rak"), "src/main.exe");
        } else {
            assert_eq!(build_output_name("src/main.rak"), "src/main");
        }
        if cfg!(windows) {
            assert_eq!(build_output_name("main.rak"), "main.exe");
        } else {
            assert_eq!(build_output_name("main.rak"), "main");
        }
    }

    #[test]
    fn require_arg_rejects_missing() {
        assert!(require_arg(&[], "usage").is_err());
        assert!(require_arg(&["--flag".to_string()], "usage").is_err());
        assert_eq!(require_arg(&["x".to_string()], "usage").unwrap(), "x");
    }
}
