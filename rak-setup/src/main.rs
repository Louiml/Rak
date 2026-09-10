//! Custom interactive installer for the Rak programming language.
//!
//! `rak-setup` is a self-contained TUI wizard that installs `rakc`, `rakpkg`,
//! and the Rak IDE on Linux and Windows, in either net-install (downloads
//! from the latest GitHub release) or offline-bundle mode. It edits PATH,
//! sets `RAK_PATH`, creates shortcuts + `.rak` associations, and installs man
//! pages + shell completions. A `~/.rak/manifest.json` records every action
//! for clean uninstall/upgrade.
//!
//! Run with no flags for the interactive wizard, or use flags for
//! non-interactive use (CI/scripts):
//!   rak-setup --yes --install rakc,rakpkg,ide --scope user
//!   rak-setup --uninstall --yes
//!   rak-setup --offline ./rak-bundle-linux-x86_64.tar.gz

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

mod manifest;
mod platform;
mod install;
mod uninstall;
mod wizard;

const REPO: &str = "Louiml/Rak";
const SETUP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The set of installable components.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Component {
    Rakc,
    Rakpkg,
    Ide,
    RakPath,
    Shortcuts,
    Man,
}

impl Component {
    fn all() -> &'static [Component] {
        &[
            Component::Rakc,
            Component::Rakpkg,
            Component::Ide,
            Component::RakPath,
            Component::Shortcuts,
            Component::Man,
        ]
    }
    fn label(&self) -> &'static str {
        match self {
            Component::Rakc => "rakc compiler -> bin + PATH",
            Component::Rakpkg => "rakpkg package manager -> bin + PATH",
            Component::Ide => "Rak IDE -> portable dir",
            Component::RakPath => "Set RAK_PATH env (package lookup)",
            Component::Shortcuts => "Shortcuts + .rak file association",
            Component::Man => "Man pages + shell completions",
        }
    }
    fn key(&self) -> &'static str {
        match self {
            Component::Rakc => "rakc",
            Component::Rakpkg => "rakpkg",
            Component::Ide => "ide",
            Component::RakPath => "rakpath",
            Component::Shortcuts => "shortcuts",
            Component::Man => "man",
        }
    }
    fn parse(k: &str) -> Option<Component> {
        Component::all().iter().copied().find(|c| c.key() == k)
    }
}

/// Install scope (privilege model).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Scope {
    User,
    System,
}

impl Scope {
    fn label(&self) -> &'static str {
        match self {
            Scope::User => "user (no privileges needed)",
            Scope::System => "system-wide (needs sudo / UAC)",
        }
    }
}

/// IDE install mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum IdeMode {
    Portable,
    System,
}

/// Resolved install configuration.
#[derive(Clone, Debug)]
pub struct Config {
    pub scope: Scope,
    pub components: Vec<Component>,
    pub bin_dir: PathBuf,
    pub ide_dir: PathBuf,
    pub ide_mode: IdeMode,
    pub packages_dir: PathBuf,
    pub net: bool,
    pub offline_bundle: Option<PathBuf>,
}

/// Detect the host OS+arch and return the asset-name suffix used in the
/// release (e.g. `linux-x86_64`, `windows-x86_64`).
fn asset_suffix() -> Result<String> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        return Err(anyhow!("unsupported OS (rak-setup supports linux/windows)"));
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        return Err(anyhow!("unsupported arch (only x86_64 for now)"));
    };
    Ok(format!("{}-{}", os, arch))
}

/// The per-asset name on the GitHub release.
fn asset_name(base: &str) -> Result<String> {
    let suffix = asset_suffix()?;
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    Ok(format!("{}-{}{}", base, suffix, ext))
}

/// The IDE portable archive name (tar.gz on Linux, zip on Windows).
fn ide_archive_name() -> Result<String> {
    let suffix = asset_suffix()?;
    let ext = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };
    Ok(format!("rak-ide-{}.{}", suffix, ext))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("rak-setup: {:#}", e);
            std::process::exit(1);
        }
    }
}

fn run(args: &[String]) -> Result<i32> {
    let mut yes = false;
    let mut uninstall = false;
    let mut list = false;
    let mut install_components: Option<Vec<String>> = None;
    let mut scope: Option<String> = None;
    let mut bin_dir: Option<PathBuf> = None;
    let mut ide_dir: Option<PathBuf> = None;
    let mut ide_mode: Option<String> = None;
    let mut offline: Option<PathBuf> = None;
    let mut help = false;
    let mut version = false;

    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--yes" | "-y" => yes = true,
            "--uninstall" => uninstall = true,
            "--list" => list = true,
            "--help" | "-h" => help = true,
            "--version" | "-V" => version = true,
            "--install" => {
                i += 1;
                install_components = Some(args.get(i).map(|s| s.split(',').map(|x| x.trim().to_string()).collect()).unwrap_or_default());
            }
            "--scope" => {
                i += 1;
                scope = args.get(i).cloned();
            }
            "--bin-dir" => {
                i += 1;
                bin_dir = args.get(i).map(PathBuf::from);
            }
            "--ide-dir" => {
                i += 1;
                ide_dir = args.get(i).map(PathBuf::from);
            }
            "--ide-mode" => {
                i += 1;
                ide_mode = args.get(i).cloned();
            }
            "--offline" => {
                i += 1;
                offline = args.get(i).map(PathBuf::from);
            }
            _ => return Err(anyhow!("unknown argument '{}' (try --help)", a)),
        }
        i += 1;
    }

    if help {
        print_help();
        return Ok(0);
    }
    if version {
        println!("rak-setup {}", SETUP_VERSION);
        return Ok(0);
    }
    if list {
        return uninstall::list();
    }
    if uninstall {
        return uninstall::run(yes);
    }
    // install
    let cfg = if yes {
        wizard::config_from_flags(install_components, scope, bin_dir, ide_dir, ide_mode, offline)?
    } else {
        wizard::interactive(offline.clone())?
    };
    install::run(&cfg, yes)
}

fn print_help() {
    println!("rak-setup {} — custom installer for the Rak language", SETUP_VERSION);
    println!();
    println!("USAGE:");
    println!("  rak-setup                      # interactive wizard");
    println!("  rak-setup --yes --install rakc,rakpkg,ide --scope user");
    println!("  rak-setup --uninstall --yes");
    println!("  rak-setup --list               # show what's installed");
    println!("  rak-setup --offline ./rak-bundle-<os>-x86_64.tar.gz --yes");
    println!();
    println!("FLAGS:");
    println!("  --yes / -y             non-interactive (use flags below)");
    println!("  --uninstall            uninstall existing install (reads manifest)");
    println!("  --list                 list installed components from manifest");
    println!("  --install <a,b,..>     components: rakc,rakpkg,ide,rakpath,shortcuts,man");
    println!("  --scope <s>           user | system");
    println!("  --bin-dir <path>       bin install dir");
    println!("  --ide-dir <path>       IDE install dir");
    println!("  --ide-mode <m>         portable | system");
    println!("  --offline <bundle>     install from a local bundle (no download)");
    println!("  --version / -V         print version");
    println!("  --help / -h            this help");
}

/// Read a yes/no confirmation. Returns true for yes.
fn confirm(prompt: &str, default: bool) -> bool {
    use dialoguer::Confirm;
    Confirm::new().with_prompt(prompt).default(default).interact().unwrap_or(default)
}

/// Print a status line.
fn status(msg: &str) {
    println!("[rak-setup] {}", msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_suffix_is_x86_64_on_this_host() {
        if cfg!(target_arch = "x86_64") {
            let s = asset_suffix().unwrap();
            assert!(s == "linux-x86_64" || s == "windows-x86_64", "got {}", s);
        }
    }

    #[test]
    fn asset_names_match_os() {
        let n = asset_name("rakc").unwrap();
        if cfg!(target_os = "windows") {
            assert!(n.ends_with(".exe"), "{}", n);
        } else {
            assert!(!n.contains(".exe"), "{}", n);
        }
        assert!(n.contains("rakc"));
    }

    #[test]
    fn ide_archive_extension() {
        let n = ide_archive_name().unwrap();
        if cfg!(target_os = "windows") {
            assert!(n.ends_with(".zip"), "{}", n);
        } else {
            assert!(n.ends_with(".tar.gz"), "{}", n);
        }
    }

    #[test]
    fn component_parse_roundtrip() {
        for c in Component::all() {
            assert_eq!(Component::parse(c.key()), Some(*c));
        }
    }
}

// Suppress unused-import warnings for platform-specific helpers re-exported
// through submodules; the public surface is the wizard/install entry points.
#[allow(unused_imports)]
use platform::{home, rak_root};
