# CLI reference

## rakc

```text
rakc run <file>     Run a Rak script on the interpreter
rakc vm <file>      Run on the bytecode VM
rakc build <file>   Build a standalone executable
rakc bench <file>   Benchmark interpreter vs VM
rakc check <file>   Static type check; print diagnostics with line/column
rakc fmt <file>     Format source (--write, --check)
rakc lint <file>    Advisory style and security checks (--deny)
rakc fuzz <target>  Property-based fuzz a parser (--runs, --seed, --list)
rakc test [file]    Run test blocks (--filter NAME, --verbose)
rakc lex <file>     Print tokens
rakc parse <file>   Print AST
rakc repl           Start an interactive REPL
rakc lsp            Start the language server (stdio)
rakc bindgen <h>    Generate Rak bindings from a C header
rakc debug <file>   Bytecode-VM source debugger (0.7)
rakc --version
```

### rakc fmt and rakc lint (8.0.0)

`rakc fmt` is an AST-to-source printer: `rakc fmt f.rak` prints the canonical
form, `--write` rewrites in place, and `--check` exits 1 if the file is not
already formatted, which is what you want in CI.

`rakc lint` has five style rules (`unused-var`, `shadowed`, `unreachable`,
`missing-ret-type`, `duplicate-import`) and six security rules
(`hardcoded-secret`, `plaintext-url`, `weak-crypto`, `secret-compare`,
`ffi-raw-pointer`, `insecure-transport`). Both are advisory; `--deny` exits 1
when anything fires. See [Safety](safety.html) for what the security rules
detect and where they have false negatives.

### rakc fuzz (8.0.0)

```text
rakc fuzz all                        # 20000 runs per target
rakc fuzz parse --runs 100000
rakc fuzz dns --seed 0xC0FFEE        # reproducible
rakc fuzz --list
```

Targets: `lex`, `parse`, `eval`, `dns`, `tls`, `json`, `websocket`, `tunnel`,
`netraw`, `csv`, `gzip`, `zip`, `all`. A crashing input is written to
`fuzz-crash-<target>.bin` and the printed seed replays the run. Exit is 1 if
any target crashed.

This is a deterministic mutation loop, not coverage-guided fuzzing, and it is
meant to run in ordinary CI on any platform. Run it from a debug build: the
harness uses `catch_unwind` and the release profile sets `panic = "abort"`.
The coverage-guided `cargo-fuzz` targets in `fuzz/` are still there for long
Linux or WSL runs.

### rakc check (0.7.1)

`rakc check <file>` runs the static type checker over the AST in addition to
lexing and parsing. It infers literal types, checks explicit annotations and
function-call argument types, and flags non-exhaustive or repeated enum-variant
match patterns. Type mismatches are reported with the expected and found types
and the offending source line. See [Language reference](language.html#static-type-checking)
for the diagnostic format.

### rakc test (0.7.1)

`rakc test` discovers and runs `test "name" { ... }` blocks. Pass a file, or
run it bare to scan `tests/*.rak` (falling back to `test.rak`). Flags:
`--filter NAME` runs only blocks whose name contains `NAME`, and `--verbose`
prints failure messages. A failing test exits non-zero. See
[Tooling](tooling.html#test-runner) for the assertion set and sample output.

## rakc debug (0.7)

`rakc debug program.rak` launches a bytecode-VM source debugger. The compiler
emits a source line-marker per top-level statement into `Chunk.lines`, so
breakpoints map to bytecode offsets (source ↔ bytecode mapping). The VM pauses
at line boundaries and drives an interactive REPL; program output streams
after each pause.

Commands:

```text
break <line|file:line>   set a breakpoint (1-based statement index)
continue / c             run to the next breakpoint
step / s                 step one statement
next / n                 step over calls
finish                   run out of the current function
locals                   print local[N] slots of the current frame
stack / bt / backtrace   function call chain with line numbers
frame                    current frame info
print <name>             print a local
disassemble / dis        full bytecode listing with line markers + operands
quit / q, help
```

`disassemble` shows the line→bytecode mapping so you can place breakpoints
precisely.

## rakc dap (0.7.2)

`rakc dap program.rak` serves the [Debug Adapter Protocol](vm.html#debug-adapter-protocol-08)
over stdio (VS Code and similar editors): line breakpoints, continue/step,
stack/locals/globals inspection, and `evaluate`. Program output streams as DAP
`output` events. See the VM docs for the session walkthrough.

## rakpkg

```text
rakpkg init [name]       Create a new package (package.rak + lib.rak)
rakpkg add <user/repo>   Add a package from GitHub (supports @version, #rev)
rakpkg install           Install all dependencies (writes rakpkg.lock)
rakpkg update            Update deps within constraints
rakpkg lock              Resolve deps -> rakpkg.lock (rev + SHA-256 checksum)
rakpkg tree              Print the recursive dependency graph (cycle-safe)
rakpkg audit             Verify installed checksums against the lockfile
rakpkg publish           Publish package metadata
rakpkg run               Run the entry point via rakc run
rakpkg build             Build to a standalone executable via rakc build
rakpkg list              List installed packages
rakpkg remove <name>     Remove a package
```

Dependency constraints (0.7): `user/repo@^1.2`, `user/repo@1.2.3`,
`user/repo#<rev>`. `rakpkg.lock` records the resolved rev and a SHA-256
checksum of each manifest, which `audit` verifies.

The manifest is a Rak file:

```rak
let name = "mylib"
let version = "0.1.0"
let deps = { net: "user/rak-net", crypto: "user/rak-crypto" }
let entry = "lib.rak"
```

## Entry point (writing CLIs in Rak, 0.7)

`fn main(argv) -> int` is an optional entry point: top-level code runs first,
then `main` is called with the args array, and its `int` return becomes the
process exit code (works for `rakc run` and built `.exe`).

```rak
dump "starting..."        // top-level code runs first

fn main(argv) -> int {
    let args = parse_args({ verbose: "bool", out: "string" }, argv)
    if args.verbose { dump "verbose on" }
    print "hello"
    return 0
}
```

CLI builtins (0.7): `argv()`, `stdin_read_line()`, `stdin_read_all()`,
`eprint(...)`, and `parse_args(spec, argv) -> map`, which handles
`--flag value`, `--flag=value`, boolean `--flag`, and positionals under `""`.
