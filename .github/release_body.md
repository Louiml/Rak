# Rak v0.7.2

## What's new in 0.7.2

### Debugger (DAP server)
- New **`rakc dap`** — a Debug Adapter Protocol server so any DAP-capable editor/IDE can debug `.rak` scripts: set/remove breakpoints, step, step-over, finish, continue, inspect stack frames, scopes, and variables, and evaluate expressions live.
- The **VS Code extension** ships a debug configuration (adapter type `rak`, launching `rakc dap`) — open a script, set a breakpoint, and press F5. Includes a matching `examples/dap_demo.rak`.
- Fixed the DAP line-number mapping so client-side (1-based) breakpoints hit the correct VM (0-based) line.

### stdlib batteries
- New built-in modules covering common tasks:
  - **Time** — `time_now`, `time_parse`, `time_format`, `time_utc`, `time_elapsed`, sleep helpers.
  - **Random** — `rand_int`, `rand_float`, `rand_uuid`, `rand_seed`, `rand_bytes`.
  - **Data formats** — `csv_*` read/write (RFC-4180), `yaml_parse`, JSON round-tripping helpers (new `stdlib/src/datafmt.rs`).
  - **Archives** — `gzip`/`gunzip`, `zip_archive`/`zip_list`/`zip_extract` (new `stdlib/src/archive.rs`).
- New gallery examples: `batteries_demo.rak`.

### OSINT pack
- Practical open-source-intelligence toolkit (new `rakc/src/ext_osint.rs`, `stdlib/src/{whois,ctlogs,yara,report}.rs`):
  - **WHOIS** — `whois_lookup` + `whois_parse` for domain records.
  - **CT logs** — `ct_subdomains` certificate-transparency subdomain enumeration.
  - **YARA-lite** — `yara_scan` over raw bytes (hex/string patterns, `at`/`and`/`all of them`/`none of them`).
  - **Reports** — `report_markdown` structured Markdown evidence output.
- New gallery examples: `osint_demo.rak`, `osint_pack_demo.rak`, plus docs page `osint.md`.

### Language core
- **`in` operator** — `x in collection` membership tests for arrays, maps, strings, and streams.
- **Destructuring `let`** — bind multiple variables from an array/map in one statement.
- **Slice & negative indexing** — `a[1..3]`, `a[-1]`, array/byte/string slicing with safe bounds.
- New docs page `batteries.md`; README "What's new in 0.7.2".

### IDE upgrades
- Autocomplete + syntax highlighting for every new batteries & OSINT builtin.
- Three new example scripts in the gallery (`batteries`, `osint`, `core_v072`), each verified to run byte-identically on both the interpreter and the bytecode VM.
- IDE + VS Code extension bumped to 0.7.2.

### Installers & bundles
- Linux: portable `rak-ide` tar.gz, offline `rak-bundle` tar.gz, `.deb`, and `.AppImage`.
- Windows: portable `rak-ide` zip, offline `rak-bundle` zip, NSIS `-setup.exe`, and WiX `.msi`.
- Standalone `rakc`, `rakpkg`, and `rak-setup` binaries for both platforms.

## Bug fixes
- DAP: 1-based ↔ 0-based line translation on `setBreakpoints` / `stackTrace`.
- `examples/osint_pack_demo.rak`: fixture keys now match the normalized WHOIS map; YARA demo uses a verified rule set; live network lookups commented so the demo stays hermetic.
- Removed the redundant `core_v08.rak` example (duplicate of `core_demo.rak`).