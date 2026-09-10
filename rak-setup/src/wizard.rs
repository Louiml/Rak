//! The interactive TUI wizard (dialoguer). Builds a `Config` from user input,
//! or from CLI flags in `--yes` mode.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

use crate::platform;
use crate::{confirm, Component, Config, IdeMode, Scope};

/// Build a `Config` from CLI flags (non-interactive `--yes` mode).
pub fn config_from_flags(
    install_components: Option<Vec<String>>,
    scope: Option<String>,
    bin_dir: Option<PathBuf>,
    ide_dir: Option<PathBuf>,
    ide_mode: Option<String>,
    offline: Option<PathBuf>,
) -> Result<Config> {
    let comps: Vec<Component> = match install_components {
        Some(keys) => keys
            .iter()
            .filter_map(|k| Component::parse(k))
            .collect(),
        None => Component::all().to_vec(),
    };
    if comps.is_empty() {
        return Err(anyhow!("--install: no valid components (use rakc,rakpkg,ide,rakpath,shortcuts,man)"));
    }
    let scope = match scope.as_deref() {
        Some("system") => Scope::System,
        _ => Scope::User,
    };
    let ide_mode = match ide_mode.as_deref() {
        Some("system") => IdeMode::System,
        _ => IdeMode::Portable,
    };
    let bin_dir = bin_dir.unwrap_or(platform::default_bin_dir(scope)?);
    let ide_dir = ide_dir.unwrap_or(platform::default_ide_dir(scope, ide_mode)?);
    let packages_dir = platform::default_packages_dir(scope)?;
    Ok(Config {
        scope,
        components: comps,
        bin_dir,
        ide_dir,
        ide_mode,
        packages_dir,
        net: offline.is_none(),
        offline_bundle: offline,
    })
}

/// Run the interactive wizard and return a `Config`.
pub fn interactive(offline: Option<PathBuf>) -> Result<Config> {
    use dialoguer::{Input, MultiSelect, Select};

    println!("rak-setup — custom installer for the Rak language\n");

    // Detect an existing install and offer upgrade/uninstall/reinstall.
    if let Some(_m) = crate::manifest::Manifest::load(Scope::User)? {
        let choice = Select::new()
            .with_prompt("An existing Rak install was found")
            .items(&["Reinstall / upgrade (replace)", "Uninstall", "Cancel"])
            .default(0)
            .interact()?;
        match choice {
            0 => { /* proceed with fresh install over top */ }
            1 => {
                crate::uninstall::run(false)?;
                return Err(anyhow!("uninstall complete; re-run rak-setup to install fresh"));
            }
            _ => return Err(anyhow!("cancelled")),
        }
    }

    let scope_items = [Scope::User.label(), Scope::System.label()];
    let scope_idx = Select::new()
        .with_prompt("Install scope")
        .items(&scope_items)
        .default(0)
        .interact()?;
    let scope = if scope_idx == 1 { Scope::System } else { Scope::User };
    if scope == Scope::System && !confirm("System scope needs admin/sudo. Continue?", true) {
        return Err(anyhow!("cancelled"));
    }

    let comp_items: Vec<String> = Component::all().iter().map(|c| c.label().to_string()).collect();
    let selection = MultiSelect::new()
        .with_prompt("Components to install (space to toggle)")
        .items(&comp_items)
        .defaults(&(0..comp_items.len()).map(|_| true).collect::<Vec<_>>())
        .interact()?;
    let components: Vec<Component> = selection.into_iter().filter_map(|i| Component::all().get(i).copied()).collect();
    if components.is_empty() {
        return Err(anyhow!("no components selected"));
    }

    let ide_mode = if components.contains(&Component::Ide) {
        let items = ["portable dir", "system location"];
        let idx = Select::new().with_prompt("IDE install mode").items(&items).default(0).interact()?;
        if idx == 1 { IdeMode::System } else { IdeMode::Portable }
    } else {
        IdeMode::Portable
    };

    let default_bin = platform::default_bin_dir(scope)?;
    let bin_dir: PathBuf = Input::new()
        .with_prompt("Bin directory (rakc, rakpkg)")
        .default(default_bin.to_string_lossy().to_string())
        .interact_text()?
        .into();

    let ide_dir = if components.contains(&Component::Ide) {
        let default_ide = platform::default_ide_dir(scope, ide_mode)?;
        let s: String = Input::new()
            .with_prompt("IDE directory")
            .default(default_ide.to_string_lossy().to_string())
            .interact_text()?;
        s.into()
    } else {
        platform::default_ide_dir(scope, ide_mode)?
    };

    let default_pkgs = platform::default_packages_dir(scope)?;
    let packages_dir: PathBuf = Input::new()
        .with_prompt("Packages directory (RAK_PATH)")
        .default(default_pkgs.to_string_lossy().to_string())
        .interact_text()?
        .into();

    let net = offline.is_none();

    // Summary.
    println!("\n--- Summary ---");
    println!("scope:      {}", scope.label());
    println!("components: {}", components.iter().map(|c| c.key()).collect::<Vec<_>>().join(","));
    println!("bin dir:     {}", bin_dir.display());
    if components.contains(&Component::Ide) {
        println!("ide dir:     {} ({})", ide_dir.display(), if ide_mode == IdeMode::System { "system" } else { "portable" });
    }
    println!("packages:    {}", packages_dir.display());
    println!("mode:        {}", if net { "net-install (download latest release)" } else { "offline bundle" });
    println!();

    if !confirm("Proceed with install?", true) {
        return Err(anyhow!("cancelled"));
    }

    Ok(Config {
        scope,
        components,
        bin_dir,
        ide_dir,
        ide_mode,
        packages_dir,
        net,
        offline_bundle: offline,
    })
}

/// Resolve the GitHub release download URL for a given (already-suffixed) asset
/// name on the latest release.
pub fn download_url(asset: &str) -> String {
    format!("https://github.com/{}/releases/latest/download/{}", crate::REPO, asset)
}
