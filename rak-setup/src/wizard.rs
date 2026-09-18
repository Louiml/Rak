//! The interactive TUI wizard (dialoguer). Builds a `Config` from user input,
//! or from CLI flags in `--yes` mode.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

use crate::platform;
use crate::{confirm, status, Component, Config, IdeMode, Scope};

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
        Some(keys) => {
            let mut out = Vec::new();
            for k in &keys {
                match Component::parse(k) {
                    Some(c) => out.push(c),
                    None => {
                        return Err(anyhow!(
                            "--install: unknown component '{}' (use: rakc,rakpkg,ide,rakpath,shortcuts,man)",
                            k
                        ))
                    }
                }
            }
            out
        }
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

    // Component selection — a MULTI-select: toggle each entry with <space>,
    // confirm the whole set with <enter>. Any combination can be chosen.
    let comp_items: Vec<String> = Component::all().iter().map(|c| c.label().to_string()).collect();
    let preselected: Vec<bool> = Component::all()
        .iter()
        .map(|c| matches!(c, Component::Rakc | Component::Rakpkg))
        .collect();
    let selection = MultiSelect::new()
        .with_prompt("Components to install (↑/↓ move, <space> toggle, <enter> confirm)")
        .items(&comp_items)
        .defaults(&preselected)
        .interact()?;
    let components: Vec<Component> = selection.into_iter().filter_map(|i| Component::all().get(i).copied()).collect();
    if components.is_empty() {
        return Err(anyhow!("no components selected — toggle at least one with <space>, then press <enter>"));
    }
    status(&format!(
        "selected components: {}",
        components.iter().map(|c| c.key()).collect::<Vec<_>>().join(", ")
    ));

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

    let needs_bin = components.contains(&Component::Rakc) || components.contains(&Component::Rakpkg);
    let ide_mode = if components.contains(&Component::Ide) {
        let items = ["portable dir", "system location"];
        let idx = Select::new().with_prompt("IDE install mode").items(&items).default(0).interact()?;
        if idx == 1 { IdeMode::System } else { IdeMode::Portable }
    } else {
        IdeMode::Portable
    };

    // Only prompt for directories that the selected components actually use.
    let bin_dir: PathBuf = if needs_bin {
        let default_bin = platform::default_bin_dir(scope)?;
        Input::new()
            .with_prompt("Bin directory (rakc, rakpkg)")
            .default(default_bin.to_string_lossy().to_string())
            .interact_text()?
            .into()
    } else {
        platform::default_bin_dir(scope)?
    };

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

    let packages_dir: PathBuf = if components.contains(&Component::RakPath) {
        let default_pkgs = platform::default_packages_dir(scope)?;
        Input::new()
            .with_prompt("Packages directory (RAK_PATH)")
            .default(default_pkgs.to_string_lossy().to_string())
            .interact_text()?
            .into()
    } else {
        platform::default_packages_dir(scope)?
    };

    let net = offline.is_none();

    // Summary.
    println!("\n--- Summary ---");
    println!("scope:       {}", scope.label());
    println!("components:  {}", components.iter().map(|c| c.key()).collect::<Vec<_>>().join(","));
    if needs_bin {
        println!("bin dir:     {}", bin_dir.display());
    }
    if components.contains(&Component::Ide) {
        println!("ide dir:     {} ({})", ide_dir.display(), if ide_mode == IdeMode::System { "system" } else { "portable" });
    }
    if components.contains(&Component::RakPath) {
        println!("packages:    {}", packages_dir.display());
    }
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
