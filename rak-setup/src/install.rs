//! The install pipeline: download/extract/copy components, edit PATH, set
//! env, create shortcuts + associations, install man pages + completions,
//! and record everything to the manifest.

use anyhow::{anyhow, Context, Result};
use std::fs;
#[cfg(not(target_os = "windows"))]
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::manifest::{Action, Manifest};
use crate::platform;
use crate::wizard::{download_url, download_url_from};
use crate::{asset_name, ide_archive_name, status, Component, Config, Scope};

/// Run the install for the given `Config`.
pub fn run(cfg: &Config, _yes: bool) -> Result<i32> {
    let mut manifest = Manifest {
        version: crate::SETUP_VERSION.to_string(),
        scope: if cfg.scope == Scope::System {
            "system"
        } else {
            "user"
        }
        .to_string(),
        bin_dir: cfg.bin_dir.clone(),
        ide_dir: cfg.ide_dir.clone(),
        packages_dir: cfg.packages_dir.clone(),
        actions: vec![],
    };

    fs::create_dir_all(&cfg.bin_dir)
        .with_context(|| format!("creating bin dir {}", cfg.bin_dir.display()))?;
    manifest.record(Action::Dir {
        path: cfg.bin_dir.clone(),
    });

    // Retire the package manager that oyvey replaced. An install made before
    // this migration leaves a `rakpkg` binary in the bin dir that nothing else
    // would ever remove, and it would shadow nothing but still confuse anyone
    // who finds it. Recorded as an action so `rak-setup --uninstall` still
    // reverses it on a fresh install of this version.
    remove_obsolete_rakpkg(cfg, &mut manifest)?;

    // rakc + oyvey -> bin + PATH
    if cfg.components.contains(&Component::Rakc) {
        install_binary(cfg, "rakc", &mut manifest)?;
    }
    if cfg.components.contains(&Component::Oyvey) {
        install_binary(cfg, "oyvey", &mut manifest)?;
    }
    // PATH edit
    if cfg.components.contains(&Component::Rakc) || cfg.components.contains(&Component::Oyvey) {
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
    status(&format!(
        "install complete. {} actions recorded.",
        manifest.actions.len()
    ));
    if cfg.components.contains(&Component::Rakc) {
        println!("\nNext: open a new shell and run `rakc --version`.");
    } else if cfg.components.contains(&Component::Oyvey) {
        println!("\nNext: open a new shell and run `oyvey --version`.");
    }
    if cfg.scope == Scope::System {
        println!("(system install: you may need to log out/in for PATH changes to take effect)");
    }
    Ok(0)
}

/// Remove a leftover `rakpkg` binary from the bin dir, if one is there.
fn remove_obsolete_rakpkg(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    let name = if platform::is_windows() {
        "rakpkg.exe"
    } else {
        "rakpkg"
    };
    let path = cfg.bin_dir.join(name);
    if !path.is_file() {
        return Ok(());
    }
    status(&format!(
        "removing obsolete {} (replaced by oyvey)",
        path.display()
    ));
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    manifest.record(Action::File { path });
    Ok(())
}

/// Install a single binary (rakc or oyvey) into the bin dir.
fn install_binary(cfg: &Config, name: &str, manifest: &mut Manifest) -> Result<()> {
    let dst = cfg.bin_dir.join(if platform::is_windows() {
        format!("{}.exe", name)
    } else {
        name.to_string()
    });
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
    // Per-component, so `oyvey` is fetched from its own repository.
    let url = download_url_from(crate::repo_for(name), &asset);
    status(&format!("downloading {}", url));
    download(&url, &asset)
}

use std::io::Read;

/// Download `url`, reporting progress as it goes.
///
/// Reads in chunks rather than with one `read_to_end` so the GUI can show a
/// real progress bar: these are multi-megabyte downloads, and a single call
/// reports nothing until the last byte lands. `total` is `None` when the
/// response is chunked and sends no `Content-Length`, which the GUI renders
/// as an indeterminate bar.
fn download(url: &str, what: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| anyhow!("download failed for {}: {}", what, e))?;
    // ureq 2.x has no `content_length()`; read the header.
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0);

    let mut buf = Vec::with_capacity(total.unwrap_or(1 << 20) as usize);
    let mut reader = resp.into_reader();
    let mut chunk = [0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        done += n as u64;
        crate::progress(done, total);
    }
    // A final tick so the bar lands on 100% instead of stalling at the
    // last chunk boundary.
    crate::progress(done, total);
    Ok(buf)
}

/// Install the IDE from the portable archive into the configured IDE dir.
fn install_ide(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    status(&format!("installing IDE -> {}", cfg.ide_dir.display()));
    fs::create_dir_all(&cfg.ide_dir)?;
    manifest.record(Action::Dir {
        path: cfg.ide_dir.clone(),
    });

    if let Some(bundle) = &cfg.offline_bundle {
        extract_ide_from_bundle(bundle, &cfg.ide_dir)?;
    } else {
        let asset = ide_archive_name()?;
        let url = download_url(&asset);
        status(&format!("downloading {}", url));
        let buf = download(&url, "Rak IDE")?;
        extract_ide_archive(&buf, &cfg.ide_dir)?;
    }
    Ok(())
}

/// Extract a single named binary from a tar.gz bundle (Linux) or zip (Windows).
fn extract_from_bundle(bundle: &Path, name: &str) -> Result<Vec<u8>> {
    let target = if platform::is_windows() {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
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
            if let Some(rel) = name.strip_prefix("ide/") {
                let out = dst.join(rel);
                if name.ends_with('/') {
                    fs::create_dir_all(&out)?;
                } else {
                    if let Some(p) = out.parent() {
                        fs::create_dir_all(p)?;
                    }
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

/// Add the bin dir to PATH (user or system) and record it. The edit is
/// append-only: the user's existing PATH entries are always preserved, and
/// only the registry hive matching the scope is touched (HKCU for user,
/// HKLM for system). `setx` is never used (it truncates PATH at 1024 chars
/// and `setx /M` writes the *system* PATH even for user installs).
fn add_to_path(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    let bin = cfg.bin_dir.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    {
        let (key, hive) = match cfg.scope {
            Scope::User => (platform::win::USER_ENV_KEY, "HKCU"),
            Scope::System => (platform::win::SYSTEM_ENV_KEY, "HKLM"),
        };
        status(&format!("adding {} to PATH ({}, append-only)", bin, hive));
        if platform::win::path_add(key, &bin)? {
            match cfg.scope {
                Scope::User => manifest.record(Action::EnvUser {
                    key: "PATH".to_string(),
                    value: bin,
                }),
                Scope::System => manifest.record(Action::EnvSystem {
                    key: "PATH".to_string(),
                    value: bin,
                }),
            }
            platform::win::broadcast_env_change();
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
                        manifest.record(Action::PathLine {
                            rc_file: rc,
                            line: line.clone(),
                        });
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
        match cfg.scope {
            Scope::User => {
                status(&format!("setting RAK_PATH={}", pkgs));
                platform::win::set_value(platform::win::USER_ENV_KEY, "RAK_PATH", "REG_SZ", &pkgs)?;
                manifest.record(Action::EnvUser {
                    key: "RAK_PATH".to_string(),
                    value: pkgs,
                });
            }
            Scope::System => {
                status(&format!("setting RAK_PATH={} (system)", pkgs));
                platform::win::set_value(
                    platform::win::SYSTEM_ENV_KEY,
                    "RAK_PATH",
                    "REG_SZ",
                    &pkgs,
                )?;
                manifest.record(Action::EnvSystem {
                    key: "RAK_PATH".to_string(),
                    value: pkgs,
                });
            }
        }
        platform::win::broadcast_env_change();
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
                manifest.record(Action::PathLine {
                    rc_file: rc,
                    line: line.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Create desktop/start-menu shortcuts and .rak file association.
fn create_shortcuts(cfg: &Config, manifest: &mut Manifest) -> Result<()> {
    let ide_exe = cfg.ide_dir.join(if platform::is_windows() {
        "rak-ide.exe"
    } else {
        "rak-ide"
    });
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
        let entry_path = entry.clone();
        manifest.record(Action::DesktopEntry { path: entry });
        status(&format!("created desktop entry {}", entry_path.display()));
    }
    #[cfg(target_os = "windows")]
    {
        let startmenu =
            platform::home()?.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs");
        if startmenu.exists() {
            let lnk = startmenu.join("Rak IDE.lnk");
            // Best-effort: create a simple .url-style stub. A real .lnk needs
            // COM; we write a launcher .bat alongside as a reliable fallback.
            let bat = startmenu.join("Rak IDE.bat");
            fs::write(
                &bat,
                format!("@echo off\nstart \"\" \"{}\"\n", ide_exe.display()),
            )?;
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
    let man2 = include_str!("../../dist/man/oyvey.1");
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
    manifest.record(Action::File {
        path: man_dir.join("rakc.1"),
    });
    fs::write(man_dir.join("oyvey.1"), man2)?;
    manifest.record(Action::File {
        path: man_dir.join("oyvey.1"),
    });

    #[cfg(not(target_os = "windows"))]
    {
        fs::create_dir_all(&comp_dir)?;
        let bash = include_str!("../../dist/completions/rakc.bash");
        fs::write(comp_dir.join("rakc"), bash)?;
        manifest.record(Action::File {
            path: comp_dir.join("rakc"),
        });
        let oyvey_bash = include_str!("../../dist/completions/oyvey.bash");
        fs::write(comp_dir.join("oyvey"), oyvey_bash)?;
        manifest.record(Action::File {
            path: comp_dir.join("oyvey"),
        });
        let zsh = include_str!("../../dist/completions/rakc.zsh");
        let zsh_dir = match cfg.scope {
            Scope::User => platform::home()?.join(".local/share/zsh/site-functions"),
            Scope::System => PathBuf::from("/usr/share/zsh/site-functions"),
        };
        fs::create_dir_all(&zsh_dir)?;
        fs::write(zsh_dir.join("_rakc"), zsh)?;
        manifest.record(Action::File {
            path: zsh_dir.join("_rakc"),
        });
        let oyvey_zsh = include_str!("../../dist/completions/oyvey.zsh");
        fs::write(zsh_dir.join("_oyvey"), oyvey_zsh)?;
        manifest.record(Action::File {
            path: zsh_dir.join("_oyvey"),
        });
    }
    #[cfg(target_os = "windows")]
    {
        let _ = comp_dir;
    }
    status("installed man pages + completions");
    Ok(())
}
