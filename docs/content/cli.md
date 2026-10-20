# CLI reference

Two programs: `rakc`, the compiler and toolchain, and `oyvey`, the package
manager. Both are Rust, both are built from this repository (oyvey from its own
— see [Project](project.html)).

## rakc

```text
rakc run <file>      Run a Rak script on the interpreter
rakc vm <file>       Run on the bytecode VM (also accepts a .rkbc container)
rakc bench <file>    Benchmark interpreter vs VM
rakc build <file>    Compile to any target (--emit, --target, -C, --release, -o)
rakc check <file>    Lex + parse + type check; print diagnostics
rakc test [file]     Run test blocks (--filter NAME, --verbose)
rakc fmt <file>      Format source (--write, --check)
rakc lint <file>     Advisory style and security checks (--deny, --audit)
rakc verify <file>   Run under resource limits, check contracts
rakc lex <file>      Print tokens
rakc parse <file>    Print AST
rakc repl            Start an interactive REPL
rakc debug <file>    Line-oriented debugger on the bytecode VM
rakc dap <file>      Debug Adapter Protocol server on stdio
rakc fuzz <target>   Property-based fuzz a parser (--runs, --seed, --list)
rakc bindgen <h>     Generate Rak bindings from a C header  (-o out.rak)
rakc lsp             Start the language server on stdio
rakc version         Print the version
```

`rakc --version` and `rakc -V` print the same thing as `rakc version`.
`rakc --help` and `rakc -h` print the usage block.

**Reading from stdin:** `-` as the file argument reads the program from
standard input, so `echo 'dump 1' | rakc run -` works.

**Passing arguments to your program:** everything after `--` goes to the
program, verbatim. `rakc run tool.rak -- --port 8080` gives the program
`["--port", "8080"]`.

### Flags that apply to several commands

| Flag | Where | Meaning |
|---|---|---|
| `--sandbox` | run, vm, debug, test | Turn on the capability sandbox. |
| `--allow <list>` | run, vm, debug, test | Grant capabilities. |
| `--fs-root <dir>` | run, vm, debug, test | Confine filesystem access to a directory. |
| `--max-depth N` | run, vm, bench | Cap function-call recursion. Default 256. |
| `--repeat N` | bench | Samples per phase. Default 5. |

Flags may come before or after the file name. `--` ends rakc's own flags.

### The capability sandbox

Eight capabilities plus `all`. With `--sandbox` and no `--allow`, everything is
denied and only pure computation works.

| Capability | Grants |
|---|---|
| `net` | `net_listen`, `net_accept`, `net_connect`, `net_local_addr`, the `tcp_`/`udp_`/`http_`/`ws_` families, the `dns_*` family, `fetch`, `scan_ports`, `scan_subdomains`, `ct_subdomains`, `whois_lookup` |
| `fs_read` | `read`, `file_read`, `read_lines`, `stream_csv`, `stream_jsonl`, `mmap_open` (read mode), `file_list`, `file_exists`, `file_size`, `zip_list`, `zip_read` |
| `fs_write` | `write`, `file_write`, `file_append`, the `_bytes` variants, `zip_write`, `mmap_write`, `file_delete`, `file_mkdir`, `file_copy`, `file_rename`, `log_init`, `dump x, "path"` |
| `process` | the `process_` family |
| `ffi` | the `ffi_` family, `extern_call`, and `extern "C" { … }` declarations |
| `raw` | `net_raw_send`, `net_raw_recv`, the `pcap_` family |
| `gui` | the `gui_` family |
| `secrets` | the `secret_` family, `env_get`, `env_set` |
| `asm` | inline assembly |
| `all` | every capability above |

`fs` is accepted for `fs_write`, `proc` for `process`, `raw_sockets` for `raw`.
An unknown name is a warning, not an error: `sandbox: unknown capability 'x'
(ignored)`.

A refusal names the capability and the fix:

```text
sandbox: builtin 'net_connect' blocked — capability 'net' not granted (rerun with --allow net)
```

`--fs-root` is a separate, orthogonal limit: it confines paths, so a script
with `fs_read` granted still cannot read outside the root, and a symlink
pointing out of it is refused too.

```text
sandbox: path '/etc/passwd' is outside the filesystem scope '/tmp/work'
```

See [Safety](safety.html) for what each capability covers in detail.

### rakc test

Discovers and runs `test "name" { … }` blocks. Pass a file, or run it bare to
scan `tests/*.rak`, falling back to `test.rak`. `--filter NAME` runs only
blocks whose name contains `NAME`; `--verbose` prints failure messages. A
failing test exits non-zero.

```text
PASS  math/addition works
FAIL  math/subtraction works
      expected 3, got 4

2 passed
1 failed
```

### rakc verify

Runs a program under resource limits and reports whether every `requires` and
`ensures` clause held. Three outcomes, three exit codes:

| Exit | Meaning |
|---|---|
| 0 | The program completed and every contract held. |
| 1 | The program failed: a contract was violated, an assertion tripped, or a runtime error escaped. |
| 2 | A limit was hit, so nothing was proved. |

`--steps N` (default 1,000,000), `--depth N` (default 256) and `--iters N`
(default 1,000,000) set the budget; `--quiet` drops the banner. Exit 2 says so
and tells you what to raise.

### rakc fmt and rakc lint

`rakc fmt` is an AST-to-source printer: it prints the canonical form to
stdout, `--write` rewrites in place, `--check` exits 1 if the file is not
already formatted, which is what you want in CI. The output is a fixed point —
formatting it again changes nothing.

`rakc lint` has five style rules (`unused-var`, `shadowed`, `unreachable`,
`missing-ret-type`, `duplicate-import`) and nine security rules
(`hardcoded-secret`, `plaintext-url`, `weak-crypto`, `secret-compare`,
`ffi-raw-pointer`, `insecure-transport`, `unescaped-interpolation`,
`inline-asm`, `unsafe-thin-reason`). Findings print as
`warning[rule]: message`. The rules are advisory; `--deny` exits 1 when
anything fires. `--audit` prints every `unsafe` block's justification for
review. See [Safety](safety.html) for what the security rules detect and where
they have false negatives.

### rakc fuzz

A deterministic mutation loop, meant to run in ordinary CI on any platform.
Run it from a debug build: the harness uses `catch_unwind` and the release
profile sets `panic = "abort"`.

```text
rakc fuzz all                        # 20000 runs per target
rakc fuzz parse --runs 100000
rakc fuzz dns --seed 0xC0FFEE        # reproducible
rakc fuzz --list
```

Targets: `lex`, `parse`, `eval`, `dns`, `tls`, `json`, `websocket`, `tunnel`,
`netraw`, `csv`, `gzip`, `zip`, `all`. Also `--max-len N` (default 4096) and
`--out FILE` (default `fuzz-crash-<target>.bin`). A crashing input is written
out and the printed seed replays the run. Exit is 1 if any target crashed.

The coverage-guided `cargo-fuzz` targets in `fuzz/` are still there for long
Linux or WSL runs.

### rakc check

Runs the static type checker over the AST in addition to lexing and parsing.
It infers literal types, checks explicit annotations and function-call
argument types, and flags non-exhaustive or repeated enum-variant match
patterns, unknown struct fields and impossible casts. See
[Language reference](language.html#static-type-checking) for the diagnostic
format.

The checker is **opt-in** — only `rakc check` runs it — and it is **not sound**:
it does not model generics, it does not enforce mutability, and it does not
check across modules. Runtime annotations are still enforced by both backends
regardless.

### rakc build

`rakc build` is a rustc-shaped multi-target front end. See
[Building and targets](build.html) for the full story.

```text
rakc build hello.rak                              # default: self-extracting exe
rakc build hello.rak --emit=c                     # emit C source
rakc build hello.rak --emit=link --release        # native binary via the C toolchain
rakc build hello.rak --emit=bytecode              # .rkbc container
rakc build --print target-list                    # the accepted triples
rakc build --print host
```

## oyvey

```text
oyvey new <project>      Generate a new Rak project (Rak.toml, src/main.rak, tests)
oyvey init               Initialize an existing directory as a project
oyvey add <github-repo>  Add a dependency
oyvey remove <package>   Remove a dependency
oyvey install            Resolve and install dependencies (writes oyvey.lock)
oyvey update             Re-resolve dependencies within constraints
oyvey build              Compile the project through rakc
oyvey check              Validate the project without building it
oyvey run                Run the entry point
oyvey test               Run the project's tests
oyvey clean              Remove build artifacts
oyvey list               List installed packages
oyvey tree               Print the resolved dependency tree
oyvey audit              Verify installed packages against the lockfile
oyvey lock               Write oyvey.lock without installing
oyvey version            Print the version
oyvey help [command]     Show help
```

### Flags

| Flag | Commands | Meaning |
|---|---|---|
| `--offline` | install, update, lock, build, check, run, test | Do not touch the network; use the cache and the lockfile only. |
| `--locked` | install, update | Refuse to change the lockfile. |
| `--frozen` | install, update | Both of the above, and no network. |
| `--lib` | new, init | Scaffold a library rather than an application. |
| `--sandbox` / `--no-sandbox` | run, test | Force the sandbox on or off. It is on by default. |
| `--allow <list>` | run, test | Replace the default capability grant. |
| `--fs-root <dir>` | run, test | Confine filesystem access to a directory. |
| `--filter <name>` | test | Forwarded to `rakc test`. |

### oyvey run and oyvey test are sandboxed by default

A dependency is code you did not write; that is the point of the default.
`oyvey run` and `oyvey test` execute the program with `--sandbox --allow
fs_read` — it can read files and nothing else: no network, no process, no
foreign library, no writes.

`--allow` **replaces** that grant rather than adding to it, so
`--allow net,fs_read` grants exactly those two, and `--allow=` grants none.
`--no-sandbox` turns it off entirely.

### The manifest

`Rak.toml` is the manifest. It uses Cargo's table model:

```toml
[package]
name = "mylib"
version = "0.1.0"
edition = "2026"
description = "A Rak package"
license = "MIT"

[dependencies]
net = "1.2"                                       # registry, semver requirement
local = { path = "../local" }                     # sibling directory
gh = { git = "https://github.com/user/repo", branch = "main" }
renamed = { git = "https://github.com/user/repo", package = "real-name" }
```

The legacy `package.rak` manifest — a Rak file with `let name = "…"` and a
`deps` map — is still read, and is translated into the same model.

A dependency can be a **registry** requirement (`"1.2"`, `"^1.2"`, `"~1.2"`,
`"1.2.3"`, `"*"`), a **git** source (`{ git = …, branch = …, tag = …, rev = … }`),
or a **path** (`{ path = … }`). A git dependency may be renamed with
`package = "…"`, and may request `features = [...]` or turn off the default set
with `default-features = false`.

`oyvey add` and `oyvey remove` edit the manifest in place with `toml_edit`, so
every comment in the file survives. `oyvey add` accepts a bare spec
(`user/rak-net`, `user/rak-net@^1.0`, `user/rak-net#deadbeef`).

### The lockfile

`oyvey.lock` is Cargo-style TOML. Every package records the exact revision it
was built from and a SHA-256 checksum **over every file in the package**, not
just its manifest — the manifest is metadata, and a hash of metadata would
have left every other file unchecked. `oyvey audit` verifies it.

```toml
version = 4

[[package]]
name = "rak-net"
version = "0.2.1"
source = "git+https://github.com/user/rak-net#0123456789abcdef0123456789abcdef01234567"
checksum = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
```

A path dependency is recorded with a `path+` source and **no checksum**: a
mutable directory is not a verifiable artifact, and `oyvey audit` does not
pretend otherwise.

### How packages reach the compiler

Installed packages are vendored into `<project>/packages/`, the directory the
compiler's `import` already searches. When `oyvey run`, `build` or `test`
invoke `rakc`, they prepend that directory to `RAK_PATH`, so
`import <pkg>` resolves regardless of where the entry point lives.

```rak
import mylib                // -> packages/mylib (or packages/mylib/init.rak)
```

`oyvey build` and `oyvey run` drive the existing `rakc` compiler. Oyvey
duplicates no compiler functionality.

## Writing a CLI in Rak

`fn main(argv) -> int` is an optional entry point. Top-level code runs first,
then `main` is called with the arguments, and its `int` return becomes the
process exit code — for `rakc run` and for a built executable.

```rak
dump "starting..."        // top-level code runs first

fn main(argv) -> int {
    let args = parse_args({ verbose: "bool", out: "string" }, argv)
    if args.verbose { dump "verbose on" }
    print "hello"
    return 0
}
```

The CLI builtins:

| Builtin | Does |
|---|---|
| `argv()` | The script's own arguments. |
| `args()` | The process's arguments (`std::env::args()` without the program name). |
| `stdin_read_line()` | One line from stdin, or nil at EOF. |
| `stdin_read_all()` | The rest of stdin. |
| `print(v)` | To stdout. |
| `eprint(v)` | To stderr. |
| `parse_args(spec, argv?)` | A flag parser; see below. |
| `env_get(name)` | An environment variable, or nil. Gated by `secrets`. |
| `env_set(name, value)` | Sets an environment variable. Gated by `secrets`. |

`parse_args` takes a map of names to kinds — `"bool"`, `"string"` or
`"int"` — and returns a map. It handles `--flag value`, `--flag=value`,
boolean `--flag`, and positionals under the empty-string key `""`. A flag that
is not in the spec is a boolean.

```rak
let args = parse_args({ verbose: "bool", out: "string", port: "int" }, argv)
if args.verbose { dump "verbose" }
dump args.out
dump args[""]          // the positional arguments
```

`argv()` and `args()` are different things and are easy to confuse:
`argv()` is the script's arguments, which is what a program should use;
`args()` is the process's, which includes whatever invoked the interpreter.
