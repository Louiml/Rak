# Installation

## The rak-setup installer

The custom installer is an interactive wizard that installs `rakc`, `oyvey`
and the Rak IDE. It edits PATH (append-only — your existing PATH entries are
never touched), sets `RAK_PATH`, creates shortcuts and `.rak` associations,
and installs man pages and shell completions. Every action is recorded in
`~/.rak/manifest.json` for a clean uninstall.

**Component selection is a multi-select** — the first screen of the wizard
lists every component; toggle each with `<space>` and confirm with `<enter>`:

- `rakc compiler -> bin + PATH`
- `oyvey package manager -> bin + PATH`
- `Rak IDE -> portable dir`
- `Set RAK_PATH env (package lookup)`
- `Shortcuts + .rak file association`
- `Man pages + shell completions`

Install scope is `user` (default, no privileges) or `system` (needs
sudo/UAC). The installer runs in net-install mode, downloading the latest
GitHub release, or in offline-bundle mode with
`--offline ./rak-bundle-<os>-x86_64.tar.gz`.

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash
# non-interactive:
curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash -s -- --yes --install rakc,oyvey,ide --scope user
```

### Windows (PowerShell)

```powershell
iwr -useb https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.ps1 | iex
```

Or download `rak-setup-linux-x86_64` or `rak-setup-windows-x86_64.exe` from
the [latest release](https://github.com/Louiml/Rak/releases/latest) and run
it.

### Flags

```text
rak-setup                      # interactive wizard (multi-select components)
rak-setup --yes --install rakc,oyvey,ide --scope user
rak-setup --uninstall --yes
rak-setup --list               # show what is installed
rak-setup --offline ./rak-bundle-<os>-x86_64.tar.gz --yes
```

`--install` accepts a comma-separated list of components:
`rakc,oyvey,ide,rakpath,shortcuts,man`. Unknown names are rejected.

### How PATH is edited

On Windows, `rak-setup` reads the registry value with `reg query`, appends
the bin dir only if it is not already present (case-insensitively, and
tolerating a trailing separator), and writes the value back with `reg add` —
to the registry hive matching the scope (HKCU for user, HKLM for system). It
never uses `setx`, which truncates at 1024 characters and writes the wrong
hive, and it never overwrites the whole PATH. A `WM_SETTINGCHANGE` broadcast
tells running programs about the change. Uninstalling removes only rak's PATH
entry.

## IDE-native installers

The Tauri installers (NSIS/MSI on Windows, `.deb`/AppImage on Linux) remain
on the release page for IDE-only users who want the OS-native installer.

## Build from source

```bash
git clone https://github.com/Louiml/Rak.git
cd Rak
cargo build --release                        # rakc, the standard library, rak-setup
cargo build --release -p rakc --features gui # rakc with native GUI support
```

**`oyvey` is a separate repository** and is not a member of this workspace,
so the workspace build does not produce it:

```bash
git clone https://github.com/Louiml/oyvey.git
cd oyvey && cargo build --release
```

The `gui` feature belongs to the `rakc` crate, so it needs `-p rakc`; a
workspace-wide `--features gui` does not enable it.

Linux needs no extra system packages for the GUI: it is native `egui` on the
`glow` renderer, with no browser engine involved. Building the *IDE* on Linux
is a separate matter — that is a Tauri app and does need
`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev` and
`librsvg2-dev`.

The browser playground is built with:

```bash
rustup target add wasm32-unknown-unknown
cargo build -p rak-playground --profile playground --target wasm32-unknown-unknown
```

and the result is copied to `docs/rak.wasm`, which the documentation site
serves.

## Platform support

Windows and Linux. The GUI uses native `egui` on both, so it needs no
browser engine and no WebView2 or WebKitGTK. The event loop runs on the main
thread on both platforms, because the windowing backend binds to the display
connection from whichever thread creates the loop and rejects any other.

`rakc build` produces `.exe` on Windows and an executable with `chmod 755` on
Linux.

The release binaries for `rakc` and `rak-setup` ship behind the hardening
verifier: ASLR, high-entropy VA, DEP, and a CFG-compatible image.
