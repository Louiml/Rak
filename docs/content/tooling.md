# Tooling

## REPL

`rakc repl` starts an interactive session. State persists between lines, so
variables and functions stay defined. Unclosed blocks continue on the next
line. Commands start with `:`.

```text
rak> let x = 10
rak> dump x
[DUMP] 10
rak> fn add(a, b) { return a + b }
rak> dump add(3, 4)
[DUMP] 7
rak> :ast 1 + 2
rak> :help
```

`:vars` shows defined variables, `:ast <expr>` prints the AST,
`:bytecode <expr>` prints the compiled chunk, `:clear` resets state, `:quit`
exits.

## Language server

`rakc lsp` is a stdio language server. It reports parser and lexer diagnostics,
offers keyword, type, and builtin completion, shows hover text for
identifiers, and jumps to `fn`, `let`, `struct`, and `enum` definitions.

Build it with `cargo build --release --features lsp`. For Neovim, point
`nvim-lspconfig` at `rakc lsp` for `.rak` files.

## VS Code extension

The `vscode-rak/` directory has a VS Code extension with a TextMate grammar
(`source.rak`) covering the full v0.7 vocabulary: keywords (`let`, `fn`,
`match`, `try`/`catch`, `async`/`await`, `import`/`from`, `macro`, `const`,
`extern`, `binstruct`, `evidence`, `tunnel`), types (i8..u64, f32/f64,
hex8..hex64, Option/Result), regex literals, `f"..."` interpolation, `b"..."`
byte strings, char literals, macro placeholders (`$name`) and invocations
(`name!`), hex numbers with type suffixes, and the entire builtin set
(errors, async, streams, CLI, compression, crypto, VPN, forensics). It pairs
with `rakc lsp` for diagnostics.

## The Rak IDE

The IDE is Tauri v2, Next.js, TypeScript, and Tailwind CSS. It runs Rak
scripts as child processes, streams output, and has an integrated terminal.
The editor ships the same v0.7 syntax vocabulary with autocomplete (buffer
symbols + keywords + types + builtins + snippets), find/replace, and
multi-line editing commands (toggle comment `Ctrl+/`, duplicate `Ctrl+D`,
delete line `Ctrl+Shift+K`, move line `Alt+↑/↓`, go-to-line `Ctrl+G`).

## C header bindgen

`rakc bindgen header.h -o bindings.rak` reads a C header and generates a Rak
file with a function per C function (calling `extern_call`) and a struct per C
struct. Pointers map to `u64`, `char*` maps to `string`, numeric types map to
`i8` through `u64` and `f32`/`f64`.

```bash
cargo build --release --features bindgen
rakc bindgen sdl.h -o sdl.rak
```

The generated functions call `extern_call`, which returns an error until a
Rust wrapper is linked into the standard library. The wrapper is where the
actual FFI linking happens.

## Fuzzing

- `rakc/tests/proptest_harness.rs` — 8 proptest suites on the stable CI
  toolchain (no-panic / bounded-termination / no-OOB) for the lexer, parser,
  DNS parser, raw packet builders, WebSocket frame parser, TLS parser, JSON
  parser, and tunnel framing.
- `fuzz/` — a standalone libFuzzer workspace (13 targets: lexer, parser, eval,
  manifest, dns, tls, json, websocket, tunnel, netraw, csv, gzip, zip) for
  coverage-guided campaigns on nightly:
  `cargo +nightly fuzz run <target>`.
