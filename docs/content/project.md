# Project

## Project structure

```text
Rak/
├── rakc/              Compiler, interpreter, VM, GUI
│   └── src/
│       ├── lexer.rs       Tokenizer (logos)
│       ├── parser.rs      Recursive descent parser
│       ├── ast.rs         AST definitions
│       ├── interpreter.rs Tree-walking interpreter + stdlib builtins
│       ├── bytecode.rs    Opcode set and chunk
│       ├── compiler.rs    AST to bytecode
│       ├── vm.rs          Stack-based bytecode VM
│       ├── gui.rs         WebView2/WebKitGTK window management
│       ├── repl.rs        Interactive REPL
│       ├── lsp.rs         Language server
│       └── bindgen.rs     C header bindgen
├── stdlib/            Rust native stdlib (net, crypto, encoding, recon, web,
│                      file, json, log, process, secrets, http_server,
│                      websocket, tunnel, stream_io)
├── rakpkg/            Package manager CLI (lib + bin, lockfile, constraints)
├── rak-setup/         Custom interactive installer (TUI wizard: net-install +
│                      offline bundle, multi-select components, append-only PATH)
├── dist/              One-liner bootstraps (install.sh / install.ps1), man
│                      pages, completions
├── ide/               Tauri + Next.js IDE
├── vscode-rak/        VS Code extension (TextMate grammar + LSP pairing)
├── fuzz/              libFuzzer pack (13 targets, nightly)
├── examples/          See the Examples page
└── .github/workflows/ CI (Linux .deb + AppImage builds, proptest suites)
```

## Tech stack

The compiler is Rust. Logos handles lexing. The parser is hand-written
recursive descent. Two backends: a tree-walking interpreter and a stack-based
bytecode VM.

The standard library uses `ureq` for HTTP, `scraper` for HTML parsing,
`md-5`/`sha1`/`sha2` for hashing, `std::net` for TCP, `std::thread` and
`mpsc` for concurrency, `libloading` for FFI, `memmap2` for memory-mapped
files, `tokio` for the async event loop, `chacha20poly1305`/`x25519-dalek`/`hkdf`
for the VPN toolkit, and `serde_json` for real JSON.

The IDE is Tauri v2, Next.js, TypeScript, and Tailwind CSS. It runs Rak
scripts as child processes, streams output, and has an integrated terminal.

## Design spec

The full design and implementation plan — syntax, exact Rust changes, error
handling, edge cases, and errata for every systems-level feature — is in
[`rak-features-spec.md`](../rak-features-spec.md).

## License

Apache-2.0

## Author

Louiml (Ryuzaki)
