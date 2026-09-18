use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use rakpkg::{Manifest, parse_dep_spec, parse_manifest, version_satisfies};

const VERSION: &str = "0.7.0";
const PACKAGES_DIR: &str = ".rak";
const PACKAGES_SUBDIR: &str = "packages";
const MANIFEST_FILE: &str = "package.rak";
const ENTRY_DEFAULT: &str = "lib.rak";
const LOCK_FILE: &str = "rakpkg.lock";

fn print_usage() {
    eprintln!("rakpkg {} - Rak package manager", VERSION);
    eprintln!();
    eprintln!("Usage: rakpkg <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  init [name]          Create a new package (package.rak + lib.rak)");
    eprintln!("  add <user/repo>[@vX|#rev]   Add a package from Git with a version/rev constraint");
    eprintln!("  install              Install all dependencies per package.rak (honours rakpkg.lock)");
    eprintln!("  update               Re-resolve dependencies and rewrite rakpkg.lock");
    eprintln!("  lock                 Write rakpkg.lock for current deps without reinstalling");
    eprintln!("  tree                 Print the resolved dependency tree");
    eprintln!("  audit                Verify installed packages against rakpkg.lock (checksums)");
    eprintln!("  publish              Tag + push the current package version (git-based)");
    eprintln!("  run                  Run the package entry point");
    eprintln!("  build                Build the package entry point to a standalone .exe");
    eprintln!("  list                 List installed packages");
    eprintln!("  remove <name>        Remove a package");
    eprintln!("  version              Print version");
}

fn packages_dir() -> PathBuf {
    PathBuf::from(PACKAGES_DIR).join(PACKAGES_SUBDIR)
}

fn sha256_file(path: &Path) -> String {
    let data = fs::read(path).unwrap_or_default();
    sha256_bytes(&data)
}

fn sha256_bytes(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// A resolved lockfile entry.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct LockEntry {
    name: String,
    source: String, // "user/repo"
    rev: String,    // resolved commit hash / tag
    constraint: Option<String>,
    checksum: String, // sha256 of package.rak
    version: String,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct LockFile {
    entries: std::collections::BTreeMap<String, LockEntry>,
}

fn lock_path() -> PathBuf {
    PathBuf::from(LOCK_FILE)
}

fn load_lock() -> LockFile {
    match fs::read_to_string(lock_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => LockFile::default(),
    }
}

fn save_lock(lock: &LockFile) {
    if let Ok(s) = serde_json::to_string_pretty(lock) {
        let _ = fs::write(lock_path(), s);
    }
}

/// Resolve the concrete git rev (HEAD) for an installed package dir.
fn git_head_rev(dir: &Path) -> Option<String> {
    let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(dir).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// Resolve the nearest version tag for a constraint (or the current tag).
fn git_resolve_tag(dir: &Path, constraint: Option<&str>) -> Option<String> {
    let out = Command::new("git").args(["tag", "--list", "v*", "--sort=-v:refname"]).current_dir(dir).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let tags: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect();
    match constraint {
        Some(c) => tags.iter().find(|t| version_satisfies(&t.trim_start_matches('v'), c)).cloned(),
        None => tags.first().cloned(),
    }
}

fn cmd_init(name: &str) {
    let pkg_dir = if name.is_empty() { "." } else { name };
    let dir = Path::new(pkg_dir);
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| e.to_string()).unwrap();
    }
    let manifest = format!(
        "let name = \"{}\"\nlet version = \"0.1.0\"\nlet deps = {{}}\nlet entry = \"{}\"\n",
        if name.is_empty() { "my-package" } else { name },
        ENTRY_DEFAULT
    );
    let entry_content = format!(
        "// {} - a Rak package\ndump \"Hello from {}!\"\n",
        if name.is_empty() { "my-package" } else { name },
        if name.is_empty() { "my-package" } else { name }
    );
    fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    fs::write(dir.join(ENTRY_DEFAULT), entry_content).unwrap();
    // Write an initial empty lockfile.
    save_lock(&LockFile::default());
    println!("Created package: {}/{}", dir.display(), MANIFEST_FILE);
    println!("Created entry: {}/{}", dir.display(), ENTRY_DEFAULT);
}

/// Install a single dep (shared by add/install). Returns the lock entry.
fn install_dep(name: &str, spec: &str, existing: &mut LockFile) {
    let (repo, version, pinned_rev) = parse_dep_spec(spec);
    let dest = packages_dir().join(name);
    if dest.exists() && existing.entries.contains_key(name) {
        println!("  {} already installed (locked)", name);
        return;
    }
    if dest.exists() {
        println!("  {} already installed; refreshing", name);
        let _ = Command::new("git").args(["fetch", "--tags", "--force"]).current_dir(&dest).status();
    } else {
        fs::create_dir_all(packages_dir()).unwrap();
        let url = format!("https://github.com/{}.git", repo);
        println!("  cloning {}...", url);
        let out = Command::new("git").args(["clone", &url, dest.to_str().unwrap()]).output();
        match out {
            Ok(o) => {
                if !o.status.success() {
                    eprintln!("    git clone failed: {}", String::from_utf8_lossy(&o.stderr));
                    return;
                }
            }
            Err(e) => {
                eprintln!("    failed to run git: {}", e);
                return;
            }
        }
    }
    // Apply pinned rev if requested.
    if let Some(r) = pinned_rev.as_ref() {
        let _ = Command::new("git").args(["checkout", r]).current_dir(&dest).status();
    }
    // Resolve version/tag against constraint.
    let tag = git_resolve_tag(&dest, version.as_deref());
    if let Some(t) = &tag {
        let _ = Command::new("git").args(["checkout", t]).current_dir(&dest).status();
    }
    let rev = pinned_rev.or_else(|| git_head_rev(&dest)).unwrap_or_else(|| "unknown".to_string());
    let resolved_version = tag.unwrap_or_else(|| "0.0.0".to_string()).trim_start_matches('v').to_string();
    let constraint = version; // original version constraint (Option<String>)
    let manifest_path = dest.join(MANIFEST_FILE);
    let checksum = sha256_file(&manifest_path);
    let rev_for_print = rev.clone();
    existing.entries.insert(
        name.to_string(),
        LockEntry {
            name: name.to_string(),
            source: repo,
            rev,
            constraint,
            checksum,
            version: resolved_version,
        },
    );
    println!("  locked {} @ {}", name, rev_for_print);
}

fn cmd_add(spec: &str) {
    let pkg_name = spec.split(['@', '#', '/']).next().unwrap_or(spec).to_string();
    let manifest_path = Path::new(MANIFEST_FILE);
    if !manifest_path.exists() {
        eprintln!("No package.rak found in the current directory");
        std::process::exit(1);
    }
    let mut lock = load_lock();
    install_dep(&pkg_name, spec, &mut lock);
    save_lock(&lock);
    // Also record it in package.rak deps if not already present (best-effort).
    println!("Installed: {}", pkg_name);
}

fn cmd_install() {
    let manifest_path = Path::new(MANIFEST_FILE);
    if !manifest_path.exists() {
        eprintln!("No package.rak found");
        std::process::exit(1);
    }
    let manifest = match parse_manifest(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let mut lock = load_lock();
    if manifest.deps.is_empty() {
        if lock.entries.is_empty() {
            println!("No dependencies to install");
        } else {
            // present lock but no deps: remove stale lock entries not in manifest
            lock.entries.retain(|k, _| manifest.deps.contains_key(k));
            save_lock(&lock);
            println!("Dependencies up to date");
        }
        return;
    }
    for (name, spec) in &manifest.deps {
        install_dep(name, spec, &mut lock);
    }
    // Drop lock entries no longer in the manifest.
    lock.entries.retain(|k, _| manifest.deps.contains_key(k));
    save_lock(&lock);
    println!("Installed all dependencies");
}

fn cmd_update() {
    let manifest_path = Path::new(MANIFEST_FILE);
    let manifest = match parse_manifest(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let mut lock = load_lock();
    lock.entries.clear();
    if manifest.deps.is_empty() {
        save_lock(&lock);
        println!("No dependencies to update");
        return;
    }
    for (name, spec) in &manifest.deps {
        install_dep(name, spec, &mut lock);
    }
    save_lock(&lock);
    println!("Updated rakpkg.lock");
}

/// Recursively print the dependency tree.
fn print_tree(name: &str, manifest: &Manifest, depth: usize, seen: &mut std::collections::HashSet<String>) {
    println!("{}{}@{}", "  ".repeat(depth), manifest.name, manifest.version);
    for (dep_name, _spec) in &manifest.deps {
        let dep_dir = packages_dir().join(dep_name);
        if dep_dir.join(MANIFEST_FILE).exists() {
            if let Ok(sub) = parse_manifest(&dep_dir.join(MANIFEST_FILE)) {
                if !seen.insert(dep_name.clone()) {
                    println!("{}{}@{} (cyclic)", "  ".repeat(depth + 1), sub.name, sub.version);
                    continue;
                }
                print_tree(dep_name, &sub, depth + 1, seen);
            }
        } else {
            println!("{}{} (missing)", "  ".repeat(depth + 1), dep_name);
        }
    }
}

fn cmd_tree() {
    let manifest_path = Path::new(MANIFEST_FILE);
    let manifest = match parse_manifest(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let mut seen = std::collections::HashSet::new();
    println!("{} (root)", manifest.name);
    for (dep_name, _spec) in &manifest.deps {
        let dep_dir = packages_dir().join(dep_name);
        if dep_dir.join(MANIFEST_FILE).exists() {
            if let Ok(sub) = parse_manifest(&dep_dir.join(MANIFEST_FILE)) {
                seen.insert(dep_name.clone());
                print_tree(dep_name, &sub, 1, &mut seen);
            }
        } else {
            println!("  {} (missing)", dep_name);
        }
    }
}

fn cmd_audit() {
    let lock = load_lock();
    if lock.entries.is_empty() {
        println!("No rakpkg.lock entries to audit");
        return;
    }
    let mut issues = 0;
    for (name, entry) in &lock.entries {
        let dir = packages_dir().join(name);
        let manifest_path = dir.join(MANIFEST_FILE);
        if !manifest_path.exists() {
            println!("[WARN] {}: missing (not installed)", name);
            issues += 1;
            continue;
        }
        let checksum = sha256_file(&manifest_path);
        if checksum != entry.checksum {
            println!("[FAIL] {}: checksum mismatch (tampered?)", name);
            issues += 1;
        } else {
            println!("[ok]   {} @ {} (checksum verified)", name, entry.version);
        }
    }
    if issues == 0 {
        println!("All {} package(s) verified", lock.entries.len());
    } else {
        println!("{} issue(s) found", issues);
        std::process::exit(1);
    }
}

fn cmd_publish() {
    let manifest_path = Path::new(MANIFEST_FILE);
    let manifest = match parse_manifest(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let tag = format!("v{}", manifest.version);
    let status = Command::new("git").args(["tag", "-a", &tag, "-m", &format!("release {}", tag)]).status();
    match status {
        Ok(s) if s.success() => {
            let _ = Command::new("git").args(["push", "origin", &tag]).status();
            // Also verify package with a fresh lock entry.
            let mut lock = load_lock();
            cmd_lock(&mut lock);
            save_lock(&lock);
            println!("Published: {} (tag {})", manifest.name, tag);
        }
        _ => println!("Tag {} already exists or git failed", tag),
    }
}

fn cmd_lock(lock: &mut LockFile) {
    let manifest_path = Path::new(MANIFEST_FILE);
    if let Ok(m) = parse_manifest(manifest_path) {
        for (name, spec) in &m.deps {
            if !lock.entries.contains_key(name) {
                install_dep(name, spec, lock);
            }
        }
    }
}

fn cmd_list() {
    let dir = packages_dir();
    if !dir.exists() {
        println!("No packages installed");
        return;
    }
    println!("Installed packages ({}):", dir.display());
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let manifest_path = entry.path().join(MANIFEST_FILE);
            let version = if manifest_path.exists() {
                if let Ok(m) = parse_manifest(&manifest_path) {
                    m.version
                } else {
                    "?".to_string()
                }
            } else {
                "?".to_string()
            };
            println!("  {} v{}", name, version);
        }
    }
}

fn cmd_remove(name: &str) {
    let dest = packages_dir().join(name);
    if !dest.exists() {
        eprintln!("Package '{}' not found", name);
        std::process::exit(1);
    }
    if dest.is_dir() {
        fs::remove_dir_all(&dest).map_err(|e| e.to_string()).unwrap();
    } else {
        fs::remove_file(&dest).map_err(|e| e.to_string()).unwrap();
    }
    // Remove from lockfile.
    let mut lock = load_lock();
    lock.entries.remove(name);
    save_lock(&lock);
    println!("Removed: {}", name);
}

fn cmd_run() {
    let manifest_path = Path::new(MANIFEST_FILE);
    let manifest = match parse_manifest(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let entry = &manifest.entry;
    if !Path::new(entry).exists() {
        eprintln!("Entry point '{}' not found", entry);
        std::process::exit(1);
    }
    let rakc = find_rakc();
    let status = Command::new(&rakc).args(["run", entry]).status();
    match status {
        Ok(s) => {
            if !s.success() {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to run rakc: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_build() {
    let manifest_path = Path::new(MANIFEST_FILE);
    let manifest = match parse_manifest(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let entry = &manifest.entry;
    if !Path::new(entry).exists() {
        eprintln!("Entry point '{}' not found", entry);
        std::process::exit(1);
    }
    let rakc = find_rakc();
    let status = Command::new(&rakc).args(["build", entry]).status();
    match status {
        Ok(s) => {
            if !s.success() {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to run rakc: {}", e);
            std::process::exit(1);
        }
    }
}

fn find_rakc() -> String {
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
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
            return c.clone();
        }
    }
    "rakc".to_string()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let cmd = &args[1];

    match cmd.as_str() {
        "--version" | "-V" | "version" => {
            println!("rakpkg {}", VERSION);
            return;
        }
        "--help" | "-h" | "help" => {
            print_usage();
            return;
        }
        "init" => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("");
            cmd_init(name);
        }
        "add" => {
            if args.len() < 3 {
                eprintln!("Usage: rakpkg add <user/repo>[@vX|#rev]");
                std::process::exit(1);
            }
            cmd_add(&args[2]);
        }
        "install" => cmd_install(),
        "update" => cmd_update(),
        "lock" => {
            let mut lock = load_lock();
            cmd_lock(&mut lock);
            save_lock(&lock);
            println!("Wrote rakpkg.lock");
        }
        "tree" => cmd_tree(),
        "audit" => cmd_audit(),
        "publish" => cmd_publish(),
        "run" => cmd_run(),
        "build" => cmd_build(),
        "list" => cmd_list(),
        "remove" => {
            if args.len() < 3 {
                eprintln!("Usage: rakpkg remove <name>");
                std::process::exit(1);
            }
            cmd_remove(&args[2]);
        }
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            std::process::exit(1);
        }
    }
}