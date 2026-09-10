//! The install pipeline: download/extract/copy components, edit PATH, set
//! env, create shortcuts + associations, install man pages + completions,
//! and record everything to the manifest.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::{Action, Manifest};
use crate::platform;
use crate::wizard::download_url;
use crate::{asset_name, ide_archive_name, status, Component, Config, Scope};

/// Run the install for the given `Config`.
pub fn run(cfg: &Config, _yes: bool) -> Result<i32> {
    let mut manifest = Manifest {
        version: crate::SETUP_VERSION.to_string(),
        scope: if cfg.scope == Scope::System { "system" } else { "user" }.to_string(),
        bin_dir: cfg.bin_dir.clone(),
        ide_dir: cfg.ide_dir.clone(),
        packages_dir: cfg.packages_dir.clone(),
        actions: vec![],
    };

    fs::create_dir_all(&cfg.bin_dir).with_context(|| format!("creating bin dir {}", cfg.bin_dir.display()))?;
    manifest.record(Action::Dir { path: cfg.bin_dir.clone() });

    // rakc + rakpkg -> bin + PATH
    if cfg.components.contains(&Component::Rakc) {
        install_binary(cfg, "rakc", &mut manifest)?;
    }
    if cfg.components.contains(&Component::Rakpkg) {
        install_binary(cfg, "rakpkg", &mut manifest)?;
    }
    // PATH edit
    if cfg.components.contains(&Component::Rakc) || cfg.components.contains(&Component::Rakpkg) {
        add_to_path(cfg, &mut manifest)?;
    }
    // IDE -> dir
    if cfg.components.contains(&Component::Ide) {
        install_ide(cfg, &mut manifest)?;
    }
    // RAK_PATH env
    if cfg.components.contains(&Component::RakPath) {
        set_rak_path(cfg, &mut manifest)?;
    }
    // Shortcuts + .rak association
    if cfg.components.contains(&Component::Shortcuts) {
        create_shortcuts(cfg, &mut manifest)?;
    }
    // Man pages + completions
    if cfg.components.contains(&Component::Man) {
        install_man_and_completions(cfg, &mut manifest)?;
    }

    manifest.save(cfg.scope)?;
    status(&format!("install complete. {} actions recorded.", manifest.actions.len()));
    println!("\nNext: open a new shell and run `rakc --version`.");
    if cfg.scope == Scope::System {
        println!("(system install: you may need to log out/in for PATH changes to take effect)");
    }
    Ok(0)
}

/// Install a single binary (rakc or rakpkg) into the bin dir.
fn install_binary(cfg: &Config, name: &str, manifest: &mut Manifest) -> Result<()> {
    let dst = cfg.bin_dir.join(if platform::is_windows() { format!("{}.exe", name) } else { name.to_string() });
    status(&format!("installing {} -> {}", name, dst.display()));
    let bytes = fetch_or_bundle(cfg, name)?;
    fs::write(&dst, &bytes)?;
    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dst)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dst, perms)?;
    }
    manifest.record(Action::File { path: dst.clone() });
    Ok(())
}

/// Fetch an asset's bytes — either from the GitHub release (net mode) or from
/// the offline bundle.
fn fetch_or_bundle(cfg: &Config, name: &str) -> Result<Vec<u8>> {
    if let Some(bundle) = &cfg.offline_bundle {
        return extract_from_bundle(bundle, name);
    }
    let asset = asset_name(name)?;
    let url = download_url(&asset);
    status(&format!("downloading {}", url));
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| anyhow!("download failed for {}: {}", asset, e))?;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf)?;
    Ok(buf)
}

use std::io::Read;

/// Install the IDE from the portable archive into the configured IDE dir.
fn install_ide(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    status(&format!("installing IDE -> {}", cfg.ide_dir.display()));
    fs::create_dir_all(&cfg.ide_dir)?;
    manifest.record(Action::Dir { path: cfg.ide_dir.clone() });

    if let Some(bundle) = &cfg.offline_bundle {
        extract_ide_from_bundle(bundle, &cfg.ide_dir)?;
    } else {
        let asset = ide_archive_name()?;
        let url = download_url(&asset);
        status(&format!("downloading {}", url));
        let resp = ureq::get(&url)
            .call()
            .map_err(|e| anyhow!("IDE download failed: {}", e))?;
        let mut buf = Vec::new();
        resp.into_reader().read_to_end(&mut buf)?;
        extract_ide_archive(&buf, &cfg.ide_dir)?;
    }
    Ok(())
}

/// Extract a single named binary from a tar.gz bundle (Linux) or zip (Windows).
fn extract_from_bundle(bundle: &Path, name: &str) -> Result<Vec<u8>> {
    let target = if platform::is_windows() { format!("{}.exe", name) } else { name.to_string() };
    #[cfg(target_os = "windows")]
    {
        let f = fs::File::open(bundle)?;
        let mut za = zip::ZipArchive::new(f)?;
        for i in 0..za.len() {
            let mut zf = za.by_index(i)?;
            if zf.name().ends_with(&target) {
                let mut buf = Vec::new();
                zf.read_to_end(&mut buf)?;
                return Ok(buf);
            }
        }
        Err(anyhow!("{} not found in bundle", target))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let f = fs::File::open(bundle)?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut ar = tar::Archive::new(gz);
        for entry in ar.entries()? {
            let mut e = entry?;
            if e.path()?.to_string_lossy().ends_with(&target) {
                let mut buf = Vec::new();
                e.read_to_end(&mut buf)?;
                return Ok(buf);
            }
        }
        Err(anyhow!("{} not found in bundle", target))
    }
}

/// Extract the IDE archive bytes into `dst` (tar.gz on Linux, zip on Windows).
fn extract_ide_archive(buf: &[u8], dst: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let cursor = std::io::Cursor::new(buf);
        let mut za = zip::ZipArchive::new(cursor)?;
        za.extract(dst)?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(buf));
        let mut ar = tar::Archive::new(gz);
        ar.unpack(dst)?;
        Ok(())
    }
}

/// Extract the IDE dir from a bundle into `dst`.
fn extract_ide_from_bundle(bundle: &Path, dst: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let f = fs::File::open(bundle)?;
        let mut za = zip::ZipArchive::new(f)?;
        // Extract entries under an "ide/" prefix into dst.
        for i in 0..za.len() {
            let mut zf = za.by_index(i)?;
            let name = zf.name().to_string();
            if name.starts_with("ide/") {
                let rel = &name["ide/".len()..];
                let out = dst.join(rel);
                if name.ends_with('/') {
                    fs::create_dir_all(&out)?;
                } else {
                    if let Some(p) = out.parent() { fs::create_dir_all(p)?; }
                    let mut buf = Vec::new();
                    zf.read_to_end(&mut buf)?;
                    fs::write(&out, &buf)?;
                }
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let f = fs::File::open(bundle)?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut ar = tar::Archive::new(gz);
        for entry in ar.entries()? {
            let mut e = entry?;
            let path = e.path()?.to_string_lossy().to_string();
            if path.starts_with("ide/") {
                let rel = &path["ide/".len()..];
                let out = dst.join(rel);
                e.unpack(&out)?;
            }
        }
        Ok(())
    }
}

/// Add the bin dir to PATH (user or system) and record it.
fn add_to_path(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    let bin = cfg.bin_dir.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    {
        // Minimal Win reg edit via the `reg`/`setx` commands (no extra crate).
        let (hive, subkey) = match cfg.scope {
            Scope::User => ("HKCU", "Environment"),
            Scope::System => ("HKLM", r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
        };
        status(&format!("adding {} to PATH ({})", bin, hive));
        // Read current PATH
        let read = std::process::Command::new("reg")
            .args(["query", &format!("{}\\{}", hive, subkey), "/v", "PATH"])
            .output();
        let current = match read {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).to_string();
                s.lines()
                    .find_map(|l| l.split("    PATH    ").nth(1).map(|s| s.trim().to_string()))
                    .unwrap_or_default()
            }
            Err(_) => String::new(),
        };
        if !current.split(';').any(|p| p == bin) {
            let new = if current.is_empty() { bin.clone() } else { format!("{};{}", current, bin) };
            let _ = std::process::Command::new("setx")
                .arg("/M")
                .arg(&new)
                .status();
            // setx /M writes system PATH; for user PATH we use reg add.
            let _ = std::process::Command::new("reg")
                .args(["add", &format!("{}\\{}", hive, subkey), "/v", "PATH", "/t", "REG_EXPAND_SZ", "/f", "/d", &new])
                .status();
            manifest.record(Action::EnvUser { key: "PATH".to_string(), value: bin.clone() });
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        match cfg.scope {
            Scope::User => {
                let line = format!("export PATH=\"{}:$PATH\"", bin);
                for rc in platform::user_shell_rc_files()? {
                    let contents = fs::read_to_string(&rc).unwrap_or_default();
                    if !contents.contains(&line) {
                        status(&format!("appending PATH export to {}", rc.display()));
                        let mut f = fs::OpenOptions::new().append(true).create(true).open(&rc)?;
                        writeln!(f, "\n# added by rak-setup\n{}\n", line)?;
                        manifest.record(Action::PathLine { rc_file: rc, line: line.clone() });
                    }
                }
                Ok(())
            }
            Scope::System => {
                let profile = PathBuf::from("/etc/profile.d/rak.sh");
                status(&format!("writing {}", profile.display()));
                fs::create_dir_all(profile.parent().unwrap())?;
                let content = format!("# added by rak-setup\nexport PATH=\"{}:$PATH\"\n", bin);
                fs::write(&profile, content)?;
                manifest.record(Action::File { path: profile });
                Ok(())
            }
        }
    }
}

/// Set the RAK_PATH env var to the packages dir.
fn set_rak_path(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    let pkgs = cfg.packages_dir.to_string_lossy().to_string();
    fs::create_dir_all(&cfg.packages_dir)?;
    #[cfg(target_os = "windows")]
    {
        let (hive, subkey) = ("HKCU", "Environment");
        status(&format!("setting RAK_PATH={}", pkgs));
        let _ = std::process::Command::new("reg")
            .args(["add", &format!("{}\\{}", hive, subkey), "/v", "RAK_PATH", "/t", "REG_SZ", "/f", "/d", &pkgs])
            .status();
        manifest.record(Action::EnvUser { key: "RAK_PATH".to_string(), value: pkgs });
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let line = format!("export RAK_PATH=\"{}\"", pkgs);
        for rc in platform::user_shell_rc_files()? {
            let contents = fs::read_to_string(&rc).unwrap_or_default();
            if !contents.contains(&line) {
                let mut f = fs::OpenOptions::new().append(true).create(true).open(&rc)?;
                writeln!(f, "\n# added by rak-setup\n{}\n", line)?;
                manifest.record(Action::PathLine { rc_file: rc, line: line.clone() });
            }
        }
        Ok(())
    }
}

/// Create desktop/start-menu shortcuts and .rak file association.
fn create_shortcuts(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    let ide_exe = cfg.ide_dir.join(if platform::is_windows() { "rak-ide.exe" } else { "rak-ide" });
    #[cfg(not(target_os = "windows"))]
    {
        let desktop_dir = match cfg.scope {
            Scope::User => platform::home()?.join(".local/share/applications"),
            Scope::System => PathBuf::from("/usr/share/applications"),
        };
        fs::create_dir_all(&desktop_dir)?;
        let entry = desktop_dir.join("rak-ide.desktop");
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=Rak IDE\nExec={}\nIcon=rak-ide\nCategories=Development;\nTerminal=false\n",
            ide_exe.display()
        );
        fs::write(&entry, content)?;
        manifest.record(Action::DesktopEntry { path: entry });
        status(&format!("created {}", manifest.actions.last().unwrap()));
    }
    #[cfg(target_os = "windows")]
    {
        let startmenu = platform::home()?.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs");
        if startmenu.exists() {
            let lnk = startmenu.join("Rak IDE.lnk");
            // Best-effort: create a simple .url-style stub. A real .lnk needs
            // COM; we write a launcher .bat alongside as a reliable fallback.
            let bat = startmenu.join("Rak IDE.bat");
            fs::write(&bat, format!("@echo off\nstart \"\" \"{}\"\n", ide_exe.display()))?;
            manifest.record(Action::StartMenuShortcut { path: bat });
            let _ = lnk;
        }
    }
    let _ = ide_exe;
    Ok(())
}

/// Install man pages + shell completions (embedded via include_str!).
fn install_man_and_completions(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    let man1 = include_str!("../../dist/man/rakc.1");
    let man2 = include_str!("../../dist/man/rakpkg.1");
    let (man_dir, comp_dir) = match cfg.scope {
        Scope::User => (
            platform::home()?.join(".local/share/man/man1"),
            platform::home()?.join(".local/share/bash-completion/completions"),
        ),
        Scope::System => (
            PathBuf::from("/usr/share/man/man1"),
            PathBuf::from("/usr/share/bash-completion/completions"),
        ),
    };
    fs::create_dir_all(&man_dir)?;
    fs::write(man_dir.join("rakc.1"), man1)?;
    manifest.record(Action::File { path: man_dir.join("rakc.1") });
    fs::write(man_dir.join("rakpkg.1"), man2)?;
    manifest.record(Action::File { path: man_dir.join("rakpkg.1") });

    #[cfg(not(target_os = "windows"))]
    {
        fs::create_dir_all(&comp_dir)?;
        let bash = include_str!("../../dist/completions/rakc.bash");
        fs::write(comp_dir.join("rakc"), bash)?;
        manifest.record(Action::File { path: comp_dir.join("rakc") });
        let zsh = include_str!("../../dist/completions/rakc.zsh");
        let zsh_dir = match cfg.scope {
            Scope::User => platform::home()?.join(".local/share/zsh/site-functions"),
            Scope::System => PathBuf::from("/usr/share/zsh/site-functions"),
        };
        fs::create_dir_all(&zsh_dir)?;
        fs::write(zsh_dir.join("_rakc"), zsh)?;
        manifest.record(Action::File { path: zsh_dir.join("_rakc") });
    }
    #[cfg(target_os = "windows")]
    {
        let _ = comp_dir;
    }
    status("installed man pages + completions");
    Ok(())
}
