# Rak for VS Code

Syntax highlighting and language features for the [Rak](https://github.com/Louiml/Rak) programming language.

## Setup

1. Install `rakc` (with the `lsp` feature built):
   ```bash
   cargo build --release --features lsp
   ```
2. Make sure `rakc` is on your PATH, or configure the path in VS Code settings.

## Language server

`rakc lsp` is a stdio language server that provides diagnostics, completion, hover, and go-to-definition. VS Code launches it automatically for `.rak` files when configured. Since this extension uses the TextMate grammar only, add the following to your user `settings.json` to enable the LSP manually (or use any LSP client like `vscode-lsp`):

```json
"rak.server.path": "/path/to/rakc"
```

For Neovim (with `nvim-lspconfig`), configure `rakc lsp` as the command for `.rak` files.

## Changelog

### 0.5.1

- **Custom installer** — `rak-setup` TUI wizard (net-install + offline bundle) now highlighted; `binstruct`/`evidence`/`rest` keywords carry over from 0.5.0.

### 0.5.0

Syntax highlighting for the new **Forensic Structs** and **evidence provenance** features:

- **`binstruct`** — the `binstruct` keyword and declarative wire-format layout blocks (`binstruct Name { field: type, … }`).
- **`evidence`** — the `evidence` keyword and `evidence<T> from expr` provenance-tag expressions.
- **Field types** — `u8`..`u64`/`i8`..`i64` width types with `be`/`le` endianness suffixes, `bytes(n)`, `rest`, and nested binstruct refs.
- **New builtins** — `report`, `cite`, `provenance`, `strip_evidence`, and the `Name.decode(...)` / `Name.encode(...)` binstruct methods.

### 0.4.0

Syntax highlighting for the new Rak language features:

- **Imports & exports** — `import`, `from`, `export`, `pub use`, `pub use {…} from`.
- **Async** — `async fn`, `await`, `async`/`await` keywords now color correctly.
- **Macros** — `macro`, `$placeholder` variables, and `name!(…)` macro invocations.
- **Constants & FFI** — `const` and `extern "C" { … }` blocks; `*u8`/`*i8`/`*void` pointer types.
- **New builtins** — `ffi_*`, `mmap_*`, `net_raw_*`, `dns_*`, `tls_*`, `pcap_*`, `http_get_async`, `tcp_probe` (highlighted as `support.function`).
- **Literals** — regex literals `/…/flags`, byte strings `b"…"`, char literals `'…'`, and `{expr}` interpolation inside `f"…"`.
- **Operators** — `|>` (pipeline), `..`, `!`, `$`.

### 0.3.0

Initial TextMate grammar + language configuration.