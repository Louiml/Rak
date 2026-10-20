# Tooling

## The REPL

```bash
rakc repl
```

```text
Rak REPL v1.1.0
Type :help for a list of commands. Ctrl-D or :quit to exit.

rak> let x = 10
rak> dump x
[DUMP] 10
rak> fn add(a, b) { return a + b }
rak> dump add(3, 4)
[DUMP] 7
```

State persists between lines, and an unfinished block continues on the next
line with a `..` prompt. The history is kept in `~/.rak_history`.

| Command | Does |
|---|---|
| `:help` | The command list. |
| `:vars` | The variables in the innermost scope, name-sorted. |
| `:ast <expr>` | Print the AST of an expression. |
| `:bytecode <expr>` | Print the compiled bytecode. |
| `:history` | The input history. |
| `:clear` | Reset the interpreter state. |
| `:quit`, `:q` | Exit. |

Any other line is evaluated as Rak source. A failure prints `error: …` and the
session continues.

## The test runner

`rakc test` discovers and runs `test "name" { … }` blocks. Pass a file, or
run it bare to scan `tests/*.rak`, falling back to `test.rak`.

```bash
rakc test tests/math.rak
rakc test --filter "addition"
rakc test --verbose
```

```text
Running Rak tests...

PASS  math/addition works
FAIL  math/subtraction works
      expected 3, got 4

1 passed
1 failed
```

`--filter` is a substring match on the test name. A failing test exits
non-zero.

The assertion set is `assert`, `assert_eq`, `assert_ne`, `assert_true`,
`assert_false` and `expect_error`.

## The language server

`rakc lsp` speaks the Language Server Protocol on stdio. It reports parser
and lexer diagnostics, offers keyword, type and builtin completion, shows
hover text for identifiers, and jumps to `fn`, `let`, `struct` and `enum`
definitions.

```bash
cargo build --release --features lsp
```

The `vscode-rak/` directory has a VS Code extension with a TextMate grammar to
pair with it. For Neovim, point `nvim-lspconfig` at `rakc lsp` for `.rak`
files.

## The IDE

`ide/` is a Tauri + Next.js application. It runs Rak scripts as child
processes, streams their output into an integrated terminal, and includes a
GUI Designer that draws a window and emits native Rak code.

Building it needs Node and, on Linux, `libwebkit2gtk-4.1-dev`,
`libgtk-3-dev`, `libayatana-appindicator3-dev` and `librsvg2-dev`.

## The debugger

`rakc debug program.rak` is a source debugger on the bytecode VM, with
statement-to-bytecode line mapping.

```text
dbg> break 12
dbg> c
[stopped] line 12
dbg> locals
  count = 3
dbg> stack
  #0 <main> (line 12)
dbg> print count
  3
dbg> q
```

| Command | Does |
|---|---|
| `break <line>` or `break <file:line>` | Set a breakpoint. |
| `continue`, `c` | Run on. |
| `step`, `s`, `next`, `n` | One source line. |
| `finish` | Run to the next breakpoint or the end. |
| `locals` | The current frame's locals. |
| `stack`, `bt`, `backtrace` | The call stack. |
| `print <name>` | One local's value. |
| `disassemble`, `dis` | The line-annotated bytecode. |
| `frame` | The current line and the stack depth. |
| `quit`, `q` | Stop. |

An empty line prints the locals. There is no watchpoint, no conditional
breakpoint, and no distinction between stepping into and stepping over.

## The Debug Adapter Protocol server

`rakc dap program.rak` speaks DAP over stdio, so VS Code and similar editors
get breakpoints, stepping, and variable inspection.

See [Bytecode VM](vm.html#the-debug-adapter-protocol) for what it supports
and what it does not.

## rakc verify

`rakc verify` runs a program under resource limits and reports whether every
contract held.

```bash
rakc verify program.rak --steps 100000 --depth 256 --iters 1000
```

It has three outcomes and three exit codes: 0 when the program completed and
every contract held, 1 when the program failed, and 2 when a limit was hit, so
nothing was proved.

## C header bindgen

`rakc bindgen header.h -o bindings.rak` reads a C header and generates a Rak
file with a function per C function and a struct per C struct. Pointers map
to `u64`, `char*` maps to `string`, and numeric types map to `i8` through
`u64` and `f32`/`f64`.

```bash
cargo build --release --features bindgen
rakc bindgen sdl.h -o sdl.rak
```

The generated functions call `extern_call`, which errors until a Rust wrapper
is linked into the standard library. The wrapper is where the actual FFI
linking happens.

## Fuzzing

`rakc fuzz <target>` is a deterministic mutation fuzzer that runs in ordinary
CI. It needs a debug build, because the harness uses `catch_unwind` and the
release profile sets `panic = "abort"`.

```bash
rakc fuzz all
rakc fuzz dns --seed 0xC0FFEE
```

The `fuzz/` directory holds the coverage-guided `cargo-fuzz` targets for long
runs. See [Safety](safety.html#fuzzing) for the target list.

## The formatter and the linter

`rakc fmt` prints the canonical form of a file, `--write` rewrites it in
place, and `--check` exits 1 if it is not already formatted. The output is a
fixed point: formatting it again changes nothing.

`rakc lint` runs the style and security rules. See
[Safety](safety.html#the-linters-security-rules).
