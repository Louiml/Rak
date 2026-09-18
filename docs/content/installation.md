# Installation

## The rak-setup installer

The custom installer is an interactive TUI wizard that installs `rakc`, `rakpkg`,
and the Rak IDE. It edits PATH (append-only — your existing PATH entries are
never touched), sets `RAK_PATH`, creates shortcuts + `.rak` associations, and
installs man pages + shell completions. Every action is recorded to
`~/.rak/manifest.json` for clean uninstall/upgrade.

**Component selection is a multi-select** — the first screen of the wizard lists
every component; toggle each with `<space>` and confirm with `<enter>`:

- `rakc compiler -> bin + PATH`
- `rakpkg package manager -> bin + PATH`
- `Rak IDE -> portable dir`
- `Set RAK_PATH env (package lookup)`
- `Shortcuts + .rak file association`
- `Man pages + shell completions`

Install scope: `user` (default, no privileges) or `system` (needs sudo/UAC).
The installer runs in net-install mode (downloads the latest GitHub release) or
offline-bundle mode (`--offline ./rak-bundle-<os>-x86_64.tar.gz`).

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash
# non-interactive:
curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash -s -- --yes --install rakc,rakpkg,ide --scope user
```

### Windows (PowerShell)

```powershell
iwr -useb https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.ps1 | iex
```

Or download the setup binary directly from the
[latest release](https://github.com/Louiml/Rak/releases/latest)
(`rak-setup-linux-x86_64` or `rak-setup-windows-x86_64.exe`) and run it.

### CLI flags

```text
rak-setup                      # interactive wizard (multi-select components)
rak-setup --yes --install rakc,rakpkg,ide --scope user
rak-setup --uninstall --yes
rak-setup --list               # show what's installed
rak-setup --offline ./rak-bundle-<os>-x86_64.tar.gz --yes
```

`--install` accepts a comma-separated list of components:
`rakc,rakpkg,ide,rakpath,shortcuts,man`. Unknown names are rejected.

### How PATH is edited (safety)

On Windows, `rak-setup` reads the registry value via `reg query`, appends the
bin dir only if it is not already present (case-insensitive, trailing
separator-tolerant), and writes the value back with `reg add` — to the registry
hive matching the scope (HKCU for user, HKLM for system). It never uses
`setx` (1024-char truncation, wrong-hive writes) and never overwrites the whole
PATH. A `WM_SETTINGCHANGE` broadcast tells running programs about the change.
Uninstalling removes only the rak PATH entry.

## IDE-native installers

The Tauri installers (NSIS/MSI on Windows, `.deb`/AppImage on Linux) remain on
the release page for IDE-only users who want the OS-native installer.

## Build from source

```bash
git clone https://github.com/Louiml/Rak.git
cd Rak
cargo build --release                    # rakc + rakpkg
cargo build --release --features gui     # rakc with GUI support
cd ide && npm install && npx tauri build # IDE
```

Linux requires `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev`, and `librsvg2-dev`.

## Platform support

Windows and Linux. On Windows, the GUI uses WebView2 (ships with Edge); on
Linux it uses WebKitGTK. `rakc build` produces `.exe` on Windows and an
executable with `chmod 755` on Linux.
