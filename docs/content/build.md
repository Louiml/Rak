# Building and targets

`rakc build` is a rustc-shaped multi-target front end. The front end — lex,
parse, type-check — is shared by every target; a target turns a checked module
into artifacts for a triple.

```bash
rakc build hello.rak                              # default: self-extracting exe
rakc build hello.rak --emit=c                     # emit C source
rakc build hello.rak --emit=c -o hello.c          # …to a named file
rakc build hello.rak --emit=link --release        # native binary via the C toolchain
rakc build hello.rak --emit=bytecode              # .rkbc container
rakc build hello.rak --emit=bytecode,c -o out/    # several artifacts
rakc build --print target-list                    # the accepted triples
rakc build --print host
```

## The emit kinds

| `--emit` | Artifact | Runs with |
|---|---|---|
| `interp` | none | the interpreter, now |
| `bytecode` | `.rkbc` container | `rakc vm program.rkbc` |
| `bytecode-text` | a disassembly listing | a text file |
| `c` | `.c` source | any C compiler |
| `wasm` | `.wasm` module | a browser, or a wasm runtime |
| `llvm-ir` | `.ll` text | LLVM (needs the `llvm` feature) |
| `obj` | a native object file | a linker |
| `exe` | a self-extracting executable | directly |
| `link` | a linked native executable | directly |

Aliases are accepted: `rkbc` for `bytecode`, `disasm` for `bytecode-text`,
`c-source` for `c`, `llir` for `llvm-ir`, `object` for `obj`, `interpret` and
`run` for `interp`.

Without flags, `rakc build` produces a self-extracting executable with the
source embedded.

## The `exe` target is packaging, not compilation

The default artifact is a byte-for-byte copy of the `rakc` binary with the
program's source appended behind a small footer. At startup such a binary
recognises its own footer, extracts the source, and runs it on the
interpreter.

That is worth knowing plainly: the source is carried verbatim rather than
translated, and the artifact is as large as the compiler. For a compiled
artifact, use `--emit=bytecode`, `--emit=c` or `--emit=link`.

## The `.rkbc` container

`--emit=bytecode` writes the chunk the VM runs as a file: compile once, ship
it, run it anywhere.

```bash
rakc build program.rak --emit=bytecode
rakc vm program.rkbc
```

The container is the same chunk `rakc vm` produces in-process, including baked
function values and nested chunks, so it is not a lossy artifact. Natives are
stored by name and re-bound against the running VM's registry on load, so a
chunk compiled by a build with the `gui` feature runs only where that feature
exists, with a clear error naming the missing native rather than a wrong
answer.

## The C target is a real translation

`--emit=c` writes C11 source. Every Rak value becomes a tagged `rak_val` in
the emitted runtime, and the `Display` formatting, the numeric tower, the
`fmt` spec grammar and `raise`/`try` unwinding are carried across, so a
compiled binary agrees with the interpreter byte for byte.

That claim is checked rather than asserted. A conformance corpus runs the
same programs on the interpreter, the VM and the C toolchain and diffs their
output; both suites pass with the interpreter's exact output. The corpus
caught eleven bugs during development, including a dangling-pointer array that
overflowed the stack on `dump [1,2]` and a union-aliasing bug that made every
builtin call return nil.

`--emit=c` needs no compiler — it only writes source. `--emit=obj` and
`--emit=link` need one.

## The honest subset

The C target compiles a single module, without the runtime features whose
implementation lives in Rust: no `spawn`/`await`, no sockets, no FFI, no
GUI, no `import`. Those are compile errors naming the construct and the
target, rather than silent behaviour changes — the same written-down-limits
policy the VM follows. The bytecode and `exe` targets keep the whole
language.

## The C toolchain

`--emit=obj` and `--emit=link` need a C compiler, found through `RAKC_CC`,
then `CC`, then `PATH`. On Windows the candidates are `clang`, `clang-cl`,
`gcc`, `cc` and `cl`; on unix, `clang`, `cc` and `gcc`. It is invoked as
`-std=c11 {opt_flag} [-g] {source.c} -o {dest} -lm`.

If no compiler is found, the error says where the generated source is and how
to compile it by hand.

The `wasm` target needs clang as well, and only for wasm triples.

## Target triples

```text
x86_64-pc-windows-msvc
x86_64-pc-windows-gnu
x86_64-unknown-linux-gnu
x86_64-unknown-linux-musl
x86_64-apple-darwin
aarch64-apple-darwin
aarch64-unknown-linux-gnu
wasm32-unknown-unknown
wasm32-wasi
```

The default is the host triple, which `rakc build --print host` prints.

## Codegen options

| Option | Values | Default |
|---|---|---|
| `-C opt-level=` | `0`/`none`, `1`/`less`, `2`/`default`, `3`/`aggressive`, `s`/`size`, `z` | `2` |
| `-C debuginfo=` | `0`, `1`, `2` | `0` |
| `-C panic=` | `abort`, `unwind` | `unwind` |

`--release` is shorthand for `-C opt-level=3 -C debuginfo=0`. Both `-C` and
`--codegen` are accepted, and the value may be on the same token:
`-Copt-level=3`.

## Output

Without `-o`, an artifact lands next to the source with the kind's extension:
`main.rak` becomes `main.rkbc`, `main.c`, `main.wasm`, `main.ll`, `main.o`
(or `main.obj`), or `main` (`main.exe` on Windows). With `-o <dir>`, artifacts
land in that directory.

A **text** artifact with no `-o` goes to stdout instead of a file, which is
what makes this work:

```bash
rakc build --emit=c hello.rak | clang -x c - -o hello
```
