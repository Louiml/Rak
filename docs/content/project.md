# Project

## Layout

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
│       ├── gui.rs         Native egui window + widget management
│       ├── lsp.rs         Language server
│       ├── typecheck.rs   Static type checker
│       ├── fmt.rs         Canonical formatter
│       ├── lint.rs        Style and security lints
│       ├── fuzz.rs        Property-based fuzzer
│       ├── repl.rs        Interactive REPL
│       ├── dap.rs         Debug Adapter Protocol server
│       ├── caps.rs        The capability sandbox
│       └── targets/       Multi-target compilation
│           ├── rkbc.rs      The .rkbc container (writer/reader)
│           ├── rkbc_ast.rs  AST codec for baked function values
│           ├── c.rs         The C target: translation + native compile
│           ├── c/           The emitted C runtime and code generator
│           ├── wasm.rs      The WebAssembly target
│           ├── llvm.rs      The LLVM backend (feature-gated)
│           ├── exe.rs       The self-extracting executable target
│           └── toolchain.rs Host C compiler discovery
├── stdlib/            Rust native stdlib (net, crypto, encoding, recon, web, file, js, log, process, secrets, http_server, websocket, tunnel, dns, tls, whois, yara, archive, mmap)
├── playground/        The interpreter, compiled to WebAssembly for the docs site
├── rak-setup/         Custom interactive installer (net-install + offline)
├── ide/               Tauri + Next.js IDE
├── vscode-rak/        VS Code extension (TextMate grammar + LSP pairing)
├── fuzz/              libFuzzer pack (12 targets, nightly)
├── dist/              One-liner bootstraps (install.sh / install.ps1), man pages, completions
├── docs/              The documentation site, served from this repository
├── examples/          See the Examples page
└── .github/workflows/ CI (tests, a browser-build job, clippy and fmt) and the release pipeline
```

**`oyvey` is not in this tree.** The package manager lives in its own
repository, [Louiml/oyvey](https://github.com/Louiml/oyvey), and has its own
release cycle. It is installed by `rak-setup` alongside `rakc`.

## Tech stack

The compiler is Rust. Logos handles lexing. The parser is a hand-written
recursive descent parser. There are four execution targets: a tree-walking
interpreter, a stack-based bytecode VM, a translation to C11 compiled by the
host toolchain, and a self-extracting executable. The front end is shared, and
a conformance corpus runs the same programs on all of them and diffs their
output.

The standard library uses `ureq` for HTTP, `scraper` for HTML parsing,
`md-5`/`sha1`/`sha2` for hashing, `rsa` and `p256` for RSA and ECDSA,
`x25519-dalek` and `chacha20poly1305` for the tunnel crypto, `hkdf` and
`pbkdf2` for key derivation, `subtle` for the constant-time helpers,
`zeroize` for wiping, `x509-parser` for TLS certificates, `flate2` and `zip`
for archives, `memmap2` for memory-mapped files, `libloading` for FFI,
`socket2` for raw sockets, `std::net` for TCP, `std::thread` and `mpsc` for
concurrency, and `tokio` for the async event loop. JSON is `serde_json`.

The IDE is Tauri v2, Next.js, TypeScript and Tailwind CSS. It runs Rak
scripts as child processes, streams their output, and has an integrated
terminal.

## The design spec

The full design and implementation plan for every systems-level feature is in
[`rak-features-spec.md`](https://github.com/Louiml/Rak/blob/main/docs/rak-features-spec.md). The backend-parity story is
in [`V8-BACKEND-PARITY.md`](https://github.com/Louiml/Rak/blob/main/docs/V8-BACKEND-PARITY.md), the known divergences
in [`V8-KNOWN-ISSUES.md`](https://github.com/Louiml/Rak/blob/main/docs/V8-KNOWN-ISSUES.md), and the security review in
[`SECURITY-AUDIT-2026-10.md`](https://github.com/Louiml/Rak/blob/main/docs/SECURITY-AUDIT-2026-10.md).

## License

`MIT OR Apache-2.0` — the workspace metadata declares both, and the `LICENSE`
file is the Apache-2.0 text.

## Author

Louiml (Ryuzaki)
