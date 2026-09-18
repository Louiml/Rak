# Rak v0.7.0

Upgraded compiler, new key features, IDE + installer fixes.

## What's new in 0.7.0

### Compiler & language upgrades
- **Structured errors** — `catch e` binds a first-class `Error` value with a `kind`, source span (`file:line:col`), `cause`, context map, and backtrace. New builtins: `error`, `err_message`, `err_kind`, `err_line`, `err_col`, `err_file`, `err_cause`, `err_context`, `err_with_context` (interpreter + VM).
- **True async concurrency** — `await_all`, `select` (race), `timeout`, `task_group`, `async_sleep`, `async_yield`, and deferred `async fn` bodies running on a bounded worker pool (thousands of lightweight ops). Added the shared Tokio runtime (`async_rt.rs`) with a non-Tokio counting semaphore.
- **Streaming** — pull-based `Value::Stream` with `stream_from_array`, `stream_map`, `filter`, `take`, `stream_next`, `collect`, `read_lines`, and `tcp_stream`; lazily consumed by `for`.
- **CLI** — optional `fn main(argv) -> int` entry with real process exit codes, plus `argv()`, `stdin_read_line`, `stdin_read_all`, `eprint`, and the `parse_args(spec, argv)` flag parser.
- **Data processing** — lazy `stream_csv` (RFC-4180) / `stream_jsonl` parsers and `parse_csv_line`, plus `gzip`/`gunzip`/`deflate`/`inflate` and `zip_archive`/`zip_list`/`zip_extract` (new `stdlib/src/stream_io.rs`).
- **VM line mapping** — the compiler emits a source line-marker per top-level statement into `Chunk.lines` for source-to-bytecode mapping.

### `rakc debug`
- New bytecode-VM source debugger: `break`, `continue`/`c`, `step`/`s`, `next`/`n`, `finish`, `locals`, `stack`/`backtrace`, `frame`, `print <name>`, and full `disassemble` with line markers + operands.

### rakpkg
- Now a lib + bin so `parse_manifest_str` is reusable/fuzzable.
- Version/rev constraints (`user/repo@^1.2`, `#rev`), `rakpkg.lock` (resolved rev + manifest SHA-256 checksum), and new commands: `update`, `lock`, `tree` (cycle-safe dependency graph), `audit`, `publish`.

### Fuzzing
- 8 proptest harnesses (stable CI) for the lexer, parser, DNS, packet builders, WebSocket, TLS, JSON, and tunnel framing (`rakc/tests/proptest_harness.rs`).
- A standalone libFuzzer / cargo-fuzz pack (`fuzz/`, 13 targets: lexer, parser, eval, manifest, dns, tls, json, websocket, tunnel, netraw, csv, gzip, zip).

### IDE + VS Code
- Fixed bugs in the IDE.
- Added the full v0.7.0 syntax vocabulary to the IDE editor (CodeEditor) and the VS Code extension (vscode-rak grammar, version 0.5.1 → 0.7.0): structured errors, async orchestration, streaming, CLI, and compression builtins, plus 9 new editor snippets (`fn main`, `await_all`, `task_group`, `timeout`, `stream`, `read_lines`, `stream_csv`, `parse_args`, structured errors).

### Documentation
- Restructured the docs: content is now split into markdown page files under `docs/content/*.md` (based on `README.md` and `rak-features-spec.md`), and `docs.html` is now a lightweight markdown-driven viewer (dependency-free renderer + hash router) served by GitHub Pages.

## Bug fixes

### Rak installer (rak-setup)
- **Fix PATH clobbering on Windows** — the installer no longer overwrites a user's entire PATH. Previously a broken `reg query` parse could capture the registry type token and `setx /M` could write the *system* PATH even for user installs, breaking unrelated CLIs (e.g. rustc/cargo). Now PATH edits are **append-only** on the correct registry hive (HKCU for user, HKLM for system), `setx` is removed, and a `WM_SETTINGCHANGE` broadcast refreshes running programs.
- **Uninstall** now removes only the rak PATH entry (instead of deleting the whole PATH value) and only recurses into rak-owned directories.
- Component selection is now an explicit **multi-select** on the first screen (space to toggle, enter to confirm), with `rakc` + `rakpkg` preselected.
- `--install` validates component names and rejects unknown ones.