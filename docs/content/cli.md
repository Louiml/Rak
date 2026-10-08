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
rakc dap <file>     Debug Adapter Protocol server (0.8)
rakc verify <file>  Run under resource limits, check contracts (--steps, --depth, --iters)
rakc --version
```

### rakc fmt and rakc lint (8.0.0)

`rakc fmt` is an AST-to-source printer: `rakc fmt f.rak` prints the canonical
form, `--write` rewrites in place, and `--check` exits 1 if the file is not
already formatted, which is what you want in CI.

`rakc lint` has five style rules (`unused-var`, `shadowed`, `unreachable`,
`missing-ret-type`, `duplicate-import`) and nine security rules
(`hardcoded-secret`, `plaintext-url`, `weak-crypto`, `secret-compare`,
`ffi-raw-pointer`, `insecure-transport`, `env-get`, `env-set`, `unsupported-asm`).
Both are advisory; `--deny` exits 1 when anything fires. See [Safety](safety.html) for what the security rules
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

### rakc check (0.8.0)

`rakc check <file>` runs the static type checker over the AST in addition to
lexing and parsing. It infers literal types, checks explicit annotations and
function-call argument types, and flags non-exhaustive or repeated enum-variant
match patterns. Type mismatches are reported with the expected and found types
and the offending source line. See [Language reference](language.html#static-type-checking)
for the diagnostic format.

Every command that parses source (`run`, `check`, `fmt`, `lint`, `parse`,
`vm`, `bench`, `test`, `verify`, `debug`, `dap`, `repl`, `fuzz`) now runs on a
dedicated 64 MiB stack, so deeply nested programs report the parser's depth
error instead of crashing with a stack overflow. `lex` is iterative and `build`
only embeds the source, so neither needs it.

### rakc test (0.7.1)

`rakc test` discovers and runs `test "name" { ... }` blocks. Pass a file, or
run it bare to scan `tests/*.rak` (falling back to `test.rak`). Flags:
`--filter NAME` runs only blocks whose name contains `NAME`, and `--verbose`
prints failure messages. A failing test exits non-zero. See
[Tooling](tooling.html#test-runner) for the assertion set and sample output.

### rakc verify (0.9.0)

`rakc verify <file>` runs the program under resource limits and reports
whether all `requires` and `ensures` contracts hold, up to the given
`--steps`, `--depth`, and `--iters` limits. A violation exits non-zero.

### rakc dap (0.8.0)

`rakc dap program.rak` serves the [Debug Adapter Protocol](vm.html#debug-adapter-protocol-08)
over stdio (VS Code and similar editors): line breakpoints, continue/step,
stack/locals/globals inspection, and `evaluate`. Program output streams as DAP
`output` events. See the VM docs for the session walkthrough.

## oyvey

```text
oyvey new <project>       Generate a new Rak project (manifest, entry, tests)
oyvey init                Initialize an existing directory as a project
oyvey add <github-repo>   Add a GitHub package dependency (supports @version, #rev)
oyvey remove <package>    Remove a dependency
oyvey install             Resolve + install dependencies (writes oyvey.lock)
oyvey update              Re-resolve dependencies within constraints
oyvey build               Compile the project through rakc
oyvey run                 Run the entry point via rakc
oyvey test                Run the project's tests via rakc
oyvey clean               Remove the built executable
oyvey list                List installed packages
oyvey tree                Print the resolved dependency tree (cycle-safe)
oyvey audit               Verify installed checksums against the lockfile
oyvey lock                Write oyvey.lock without installing
```

`oyvey` is the official Rak package manager and build system. It is
Cargo-inspired: a `package.rak` manifest declares the project and its
dependencies, an `oyvey.lock` records the exact resolved revision and checksum
of every dependency, and a global git cache (`~/.oyvey`, override with
`OYVEY_HOME`) keeps clones so builds are reproducible.

### The manifest

The manifest is a Rak file:

```rak
let name = "mylib"
let version = "0.1.0"
let description = "A Rak package"
let license = "MIT"
let entry = "src/main.rak"
let deps = {
    net: "user/rak-net",
    crypto: "user/rak-crypto@^1.0"
}
```

### Dependency specs

| Spec | Meaning |
|------|---------|
| `user/repo` | track the default branch (HEAD) |
| `user/repo@^1.2` | caret - compatible with `1.2` |
| `user/repo@~1.2` | tilde - compatible with `1.2.x` |
| `user/repo@1.2.3` | exact version |
| `user/repo@*` | any version |
| `user/repo#<rev>` | pin to an exact git revision (tag, branch, or commit) |
| `https://host/user/repo` | an explicit URL (self-hosted Git, mirrors) |

A `#rev` may be combined with a constraint (`user/repo@^1.2#deadbeef`); the
revision wins. Transitive dependencies are resolved automatically. Two
different sources claiming the same package name is reported as a conflict
rather than resolved silently one way.

### The lockfile

`oyvey.lock` is Cargo-style TOML. Every package records the exact revision it
was built from and a SHA-256 checksum of its manifest, which `oyvey audit`
verifies:

```toml
version = 1

[[package]]
name = "rak-net"
version = "0.2.1"
source = "git+https://github.com/user/rak-net#0123456789abcdef0123456789abcdef01234567"
checksum = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
```

`oyvey install` reuses the revisions in the lockfile when they still satisfy the
manifest constraints, so an install is reproducible; `oyvey update` re-resolves
to the newest matching versions.

### How packages reach the compiler

Installed packages are vendored into `<project>/packages/`, which is the
directory the compiler's `import` already searches. When `oyvey run`, `build`,
or `test` invoke `rakc`, they prepend that directory to `RAK_PATH`, so
`import <pkg>` resolves regardless of where the entry point lives:

```rak
import mylib                // -> packages/mylib (or packages/mylib/init.rak)
```

`oyvey build` and `oyvey run` drive the existing `rakc` compiler - Oyvey does
not duplicate any compiler functionality.
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

CLI builtins (0.8): `argv()`, `stdin_read_line()`, `stdin_read_all()`,
`eprint(...)`, and `parse_args(spec, argv) -> map`, which handles
`--flag value`, `--flag=value`, boolean `--flag`, and positionals under `""`.
Also `env_get(name)`, `env_set(name, value)` for environment variable access.

### Sandbox CLI

```text
rakc run untrusted.rak --sandbox --allow net,fs_write,secrets
```

Seven capabilities: `net`, `fs_write`, `process`, `ffi`, `raw`, `gui`, `secrets`.
With `--sandbox` and no `--allow`, all seven are denied and only pure
computation works. Use `--allow` to grant specific capabilities.

`env_get(name)` and `env_set(name, value)` are capability-gated by the
`secrets` capability.