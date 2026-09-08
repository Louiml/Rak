use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "0.3.0";
const PACKAGES_DIR: &str = ".rak";
const PACKAGES_SUBDIR: &str = "packages";
const MANIFEST_FILE: &str = "package.rak";
const ENTRY_DEFAULT: &str = "lib.rak";

fn print_usage() {
    eprintln!("rakpkg {} - Rak package manager", VERSION);
    eprintln!();
    eprintln!("Usage: rakpkg <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  init [name]       Create a new package (package.rak + lib.rak)");
    eprintln!("  add <user/repo>   Add a package from Git");
    eprintln!("  install           Install all dependencies from package.rak");
    eprintln!("  run               Run the package entry point");
    eprintln!("  build             Build the package entry point to a standalone .exe");
    eprintln!("  list              List installed packages");
    eprintln!("  remove <name>    Remove a package");
    eprintln!("  version           Print version");
}

fn packages_dir() -> PathBuf {
    PathBuf::from(PACKAGES_DIR).join(PACKAGES_SUBDIR)
}

struct Manifest {
    name: String,
    version: String,
    deps: std::collections::HashMap<String, String>,
    entry: String,
}

fn parse_manifest(path: &Path) -> Result<Manifest, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Cannot read package.rak: {}", e))?;
    let mut name = String::new();
    let mut version = String::new();
    let mut deps = std::collections::HashMap::new();
    let mut entry = ENTRY_DEFAULT.to_string();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("//") || line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("let name = ") {
            name = rest.trim_end_matches(';').trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("let version = ") {
            version = rest.trim_end_matches(';').trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("let entry = ") {
            entry = rest.trim_end_matches(';').trim_matches('"').to_string();
        } else if line.starts_with("let deps = {") {
            let inner = line.trim_start_matches("let deps = {").trim_end_matches("};");
            for pair in inner.split(',') {
                let pair = pair.trim();
                if pair.is_empty() { continue; }
                if let Some(colon_pos) = pair.find(':') {
                    let key = pair[..colon_pos].trim().trim_matches('"').to_string();
                    let val = pair[colon_pos + 1..].trim().trim_matches('"').to_string();
                    deps.insert(key, val);
                }
            }
        }
    }

    if name.is_empty() {
        return Err("package.rak missing 'let name'".to_string());
    }

    Ok(Manifest { name, version, deps, entry })
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
    println!("Created package: {}/{}", dir.display(), MANIFEST_FILE);
    println!("Created entry: {}/{}", dir.display(), ENTRY_DEFAULT);
}

fn cmd_add(repo: &str) {
    let pkg_name = repo.split('/').last().unwrap_or(repo).to_string();
    let dest = packages_dir().join(&pkg_name);
    if dest.exists() {
        eprintln!("Package '{}' already installed", pkg_name);
        std::process::exit(1);
    }
    fs::create_dir_all(packages_dir()).unwrap();
    let url = format!("https://github.com/{}.git", repo);
    println!("Cloning {}...", url);
    let output = Command::new("git")
        .args(&["clone", &url, dest.to_str().unwrap()])
        .output();
    match output {
        Ok(o) => {
            if o.status.success() {
                println!("Installed: {} -> {}", pkg_name, dest.display());
            } else {
                eprintln!("git clone failed: {}", String::from_utf8_lossy(&o.stderr));
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to run git: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_install() {
    let manifest_path = Path::new(MANIFEST_FILE);
    if !manifest_path.exists() {
        eprintln!("No package.rak found");
        std::process::exit(1);
    }
    let manifest = match parse_manifest(&manifest_path) {
        Ok(m) => m,
        Err(e) => { eprintln!("{}", e); std::process::exit(1); }
    };
    if manifest.deps.is_empty() {
        println!("No dependencies to install");
        return;
    }
    for (name, repo) in &manifest.deps {
        let dest = packages_dir().join(name);
        if dest.exists() {
            println!("{} already installed", name);
            continue;
        }
        println!("Installing {} from {}...", name, repo);
        let url = format!("https://github.com/{}.git", repo);
        let output = Command::new("git")
            .args(&["clone", &url, dest.to_str().unwrap()])
            .output();
        match output {
            Ok(o) => {
                if o.status.success() {
                    println!("  Installed: {}", name);
                } else {
                    eprintln!("  Failed: {}", String::from_utf8_lossy(&o.stderr));
                }
            }
            Err(e) => eprintln!("  Failed: {}", e),
        }
    }
}

fn cmd_run() {
    let manifest_path = Path::new(MANIFEST_FILE);
    if !manifest_path.exists() {
        eprintln!("No package.rak found");
        std::process::exit(1);
    }
    let manifest = match parse_manifest(&manifest_path) {
        Ok(m) => m,
        Err(e) => { eprintln!("{}", e); std::process::exit(1); }
    };
    let entry = &manifest.entry;
    if !Path::new(entry).exists() {
        eprintln!("Entry point '{}' not found", entry);
        std::process::exit(1);
    }
    let rakc = find_rakc();
    let status = Command::new(&rakc).args(&["run", entry]).status();
    match status {
        Ok(s) => {
            if !s.success() { std::process::exit(1); }
        }
        Err(e) => { eprintln!("Failed to run rakc: {}", e); std::process::exit(1); }
    }
}

fn cmd_build() {
    let manifest_path = Path::new(MANIFEST_FILE);
    if !manifest_path.exists() {
        eprintln!("No package.rak found");
        std::process::exit(1);
    }
    let manifest = match parse_manifest(&manifest_path) {
        Ok(m) => m,
        Err(e) => { eprintln!("{}", e); std::process::exit(1); }
    };
    let entry = &manifest.entry;
    if !Path::new(entry).exists() {
        eprintln!("Entry point '{}' not found", entry);
        std::process::exit(1);
    }
    let rakc = find_rakc();
    let status = Command::new(&rakc).args(&["build", entry]).status();
    match status {
        Ok(s) => {
            if !s.success() { std::process::exit(1); }
        }
        Err(e) => { eprintln!("Failed to run rakc: {}", e); std::process::exit(1); }
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
    println!("Removed: {}", name);
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
                eprintln!("Usage: rakpkg add <user/repo>");
                std::process::exit(1);
            }
            cmd_add(&args[2]);
        }
        "install" => cmd_install(),
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
