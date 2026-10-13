## 1.0.0 - 2026-10-13

Rak 1.0.0 is about the build. `rakc build` is now a rustc-shaped multi-target front
end, and the package manager `oyvey` ships with a Cargo-shaped manifest, path
dependencies, and a versioned lockfile. Both are verified against the interpreter
they must agree with.

### The compiler compiles to anything

`rakc build` keeps its old meaning (a standalone executable) and grows the rustc
one. The front end — lex, parse, type-check — is shared by every target; a target
is anything that turns a checked module into artifacts for a triple:

    $ rakc build hello.rak --emit=bytecode
    Emitted: hello.rkbc (1632 bytes)
    $ rakc vm hello.rkbc
    [DUMP] Hello, World

| `--emit` | Artifact |
|---|---|
| `bytecode` | a `.rkbc` container, runnable with `rakc vm` |
| `c` | a self-contained C11 file |
| `link` | a native binary, built with the host C toolchain |
| `exe` | the historical self-extracting executable (the default) |

The `.rkbc` container is the chunk the bytecode VM runs as a file: compile once,
ship it, run it anywhere. It is lossless — baked function values, nested chunks
and captured environments all survive the round trip, and natives are re-bound
by name at load time, so a chunk that needs a feature this build lacks says so
rather than answering with a wrong value.

The C target is a real translation. Every Rak value becomes a tagged `rak_val`
in the emitted runtime, and the Display formatting, the numeric tower, the `fmt`
spec grammar and `raise`/`try` unwinding are carried across so a compiled binary
agrees with the interpreter byte for byte. That claim is checked, not asserted:
a conformance corpus runs the same programs on the interpreter, the VM and the C
toolchain and diffs their output, and the C and bytecode suites pass with the
interpreter's exact output. The corpus caught eleven bugs during development,
including a dangling-pointer array that overflowed the stack on `dump [1,2]` and
a union-aliasing bug that made every builtin call return nil.

Flags mirror rustc's: `--emit` (comma-separated), `--target <triple>`
(`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
`wasm32-wasi`, `wasm32-unknown-unknown`, ...), `-C opt-level=0..3|s|z`,
`-C debuginfo=0..2`, `-C panic=abort|unwind`, `--release`, `-o <path>`, and
`--print target-list|host`. See the README's "Compile to anything" section.

### The package manager is Cargo-shaped

`oyvey` reads `Rak.toml` — the package/dependencies/features/profile tables
Cargo uses — and a `package.rak` project keeps resolving exactly as it did. The
manifest is edited with `toml_edit`, so `oyvey add` preserves every comment in
the file instead of regenerating it from the six keys it models.

    [package]
    name = "mylib"
    version = "0.1.0"

    [dependencies]
    net = "1.2"                          # registry requirement
    local = { path = "../local" }       # sibling directory
    gh = { git = "https://github.com/user/repo", branch = "main" }

A path dependency resolves in place, transposes transitively, and is vendored
where `rakc` finds a bare `import local`. The lockfile is v4: entries carry a
source kind (`git+`/`path+`) and their own dependency lists, and a path
dependency is recorded without a checksum — a mutable directory is not a
verifiable artifact, and `oyvey audit` does not pretend otherwise.

`oyvey check` validates a project through `rakc check` without building it, and
`oyvey lock`/`tree` route through the same resolver whatever manifest the
project declares itself with.

### Binaries

`rakc` and `oyvey` are released for Linux x86_64 and Windows x86_64, each
smoke-tested before upload (`oyvey new` + `list`, `rakc run`). The Rak release
additionally carries `rak-setup` and the IDE bundles it always has. Both
binaries ship behind the hardening verifier (ASLR, high-entropy VA, DEP,
CFG-compatible image, `LOAD_CONFIG`).

### Known limitations

**The C target is an honest subset.** It compiles a single module without the
runtime features whose implementation lives in Rust: no `spawn`/`await`, no
sockets, no FFI, no GUI, no `import`. Those are compile errors naming the
construct and the target, rather than silent behaviour changes. `--emit=bytecode`
and `--emit=exe` keep the whole language.

**WASM routes through clang.** The `wasm32-*` targets compile the C translation
with a wasm-targeted clang; they are covered by the same corpus when that
toolchain is present.

**No registry yet.** `oyvey` resolves git and path dependencies; the registry
client (`search`, `publish`, `login`) is the next piece.

## 0.9.1 - 2026-10-07

A patch release that fixes one user-facing bug found by exercising the 0.9.0 artifacts:
`rakc run`, `check` and `verify` on a path that does not exist panicked with
`thread 'main' panicked ... Failed to read file` and, on Windows, exited with
`0xC0000409`. All three now report the file and the OS reason and exit 1:

    $ rakc run nope.rak
    Error reading nope.rak: The system cannot find the file specified. (os error 2)

A directory passed as the script is reported for what it is (`Access is denied` /
`Is a directory`) instead of being indistinguishable from a missing file. The release
smoke test also derives its expected version from the release tag, so the next cut
cannot drift from the tag it ships.

The 0.9.0 feature set and fixes are unchanged from the section below.

## 0.9.0 — 2026-10-06

The release where Rak stops being a language that runs programs and starts being one that
tells you when it does not. Most of what is below is a correctness fix to something that
previously succeeded with the wrong answer, which is why so many of them were silent.

The installer and the package manager are finished and verified. The compiler executes
considerably more than it checks — that gap is real, listed under **Known limitations**, and
narrowed but not closed here.

### Packaging

**A native installer.** `rak-setup` opens a window with no arguments: `eframe`/`egui` on
`glow`, so there is no WebView2 and no `wgpu` dependency to go missing. It shows progress,
keeps a log pane, checks GitHub for a newer release, installs from an offline bundle,
uninstalls, hides its console window, and reports a startup failure in a native dialog
instead of a console nobody can see. The CLI and CI entry points are unchanged.

### Package manager

**Real semver.** Caret, tilde, comparators, wildcards, comma ranges, prereleases and build
metadata. An unparseable constraint is now an error; it used to match everything, so a typo
silently widened the requirement and installed the newest tag.

**The lockfile is trustworthy.** Each entry records a SHA-256 over the whole vendored
directory rather than over `package.rak`. An edited source file — including the entry point
that actually runs — and a renamed one are both detected. Previously the checksum covered
metadata only, so an edited dependency still audited as intact. An unreadable file is now
reported separately from a mismatch, because those are different problems.

**Offline is a rule, not an accident.** An existing checkout is reused without invoking git.
`--offline` makes that a guarantee: anything not already cached is an error instead of a
fetch. `--locked` refuses to change the lockfile and names the package that would have
changed; `--frozen` is both. An unrecognised flag is an error — every command used to accept
anything, so `oyvey build --release` looked honoured right up until it failed inside `rakc`.

**Manifests are not damaged.** `oyvey add` records the package's declared name rather than
the argument you typed, and rewrites only the `deps` block, so comments and unknown keys
survive. A corrupt lockfile is an error instead of being silently discarded and overwritten.
A comparator range in a manifest — `>=1.0, <2.0` — used to be split on its comma and
resolved against a weaker one-sided constraint than the file asked for.

### Correctness

These are the ones that mattered. Each was silent: the program ran and produced a wrong
answer, or the same program behaved differently depending only on which backend ran it.

**Distinct values compared equal.** `PartialEq` ended by comparing type discriminants, so
every struct equalled every other struct of the same type and every enum value equalled every
other value of the same enum:

```rak
struct P { x: int }
dump P { x: 1 } == P { x: 2 }   // was true
```

A set deduplicated such values, a `!=` guard never fired, and a map keyed on one returned the
wrong entry. Structs, enums, results and sets now compare by content.

**Type annotations were checked on one backend only.** `let c: int = "x"` failed under
`rakc run` and succeeded under `rakc vm`, because the compiler never looked at the
annotation. That is worse than having no annotations: nothing reported the gap, and the
backend you happened to test on decided whether they meant anything. Both backends now
enforce them and word the failure identically.

**Integer arithmetic wrapped.** `9223372036854775807 + 1` was `-9223372036854775808`. A
wrapped result is worse than a wrong one, because it is a plausible number, so the bug
surfaces somewhere else or nowhere. Overflow is now reported on both backends, including
`i64::MIN / -1`, which panicked in debug builds and wrapped in release ones — so correctness
no longer depends on the build profile. Floating-point overflow is unchanged and still IEEE.

**Comparison lost precision above 2^53.** Every operand was converted to `f64` first, and
2^53 + 1 is the first integer an `f64` cannot hold, so both sides of
`9007199254740993 == 9007199254740993.0` rounded to the same value and compared equal.
Anything crossing the int/float boundary above 2^53 — a nanosecond timestamp, a database id,
a hash — compared equal to its neighbour.

That last one led to a second: the VM's constant pool deduplicated on numeric equality, so
`F64(2.0)` reused the slot holding `I64(2)` and loaded an integer. `-2.0` is a unary
negation, so it became a negated integer, and `1.0 / 3.0` beside a `1` anywhere in the file
became integer division.

**Casts reported nothing.** `"abc" as int` was `0`; `1 as bool` was the integer `1`; `-1 as
u8` was `0xFFFFFFFFFFFFFFFF`. Every cast either did nothing or substituted a plausible value.
Narrowing is range-checked, and `char` and `bool` are now real conversions rather than
integers that pretended.

**Declarations were parsed and discarded.** `type X = T` did nothing, so an alias was
actively broken as an annotation. `trait` did nothing, so a misspelled method left the real
one unimplemented and the failure appeared much later as `No method ...` at a call site
nowhere near the typo. `struct` field types were dropped, so no struct or enum literal was
ever checked. All three are now recorded and checked where they are written.

**A malformed escape deleted characters.** `"\xZZb"` was `"b"` — two characters consumed and
discarded, so a typo in an escape silently shortened a string and the program read text its
source never said.

**`ord` and `chr` disagreed on what a character is.** `ord('A')` was 39, the code point of the
quote the renderer included, while `ord("A")` was correctly 65. `chr(65)` built a
one-character `String`, so `chr(65) == 'A'` was false and the pair never composed.

### Runtime

`fn main` runs with `argv` and its exit code is honoured. Output and errors from inside
`main` are retained. `--sandbox` actually enables. A negative `substr` bound is a catchable
error rather than a process abort. An exception restores the environment, call depth and
defer stack. `--` passes flag-shaped arguments through. `break` and `continue` can no longer
escape into a caller's loop. Map and struct rendering is key-sorted and array sorting is
numeric, so output is reproducible run to run and identical across backends.

### Known limitations

Deliberately unchanged, and listed because each one is a decision rather than an oversight:

- **A function's value is its `return` expression.** `fn f() { 7 }` yields nil; there is no
  implicit tail return. Changing it would alter the meaning of every existing program, so it
  is a v9 decision with a migration story attached.
- **The VM still rejects several constructs outright** — `async {}`, `test {}`, `assert`,
  `trace`, `scan`, `fetch`, `Lambda`, `Spawn`, `Raise`, `Comprehension`, `as`, `TypedInt`,
  `Range`, `ensures`. It refuses rather than misbehaving, which is the honest failure, but it
  is not support.
- **Not implemented:** generics, trait bounds and `where` clauses, binding patterns, tail-call
  optimisation, doc comments, `while`/`for` `else`, and closure capture of mutated locals.
  There is no general-purpose Rak-level standard library. The LSP cannot format.
- `substr` returns `""` out of range while `slice` clamps. Two slicing builtins, two
  policies, neither documented.

### Verification

346 unit tests, 61 backend-parity tests, 55 CLI tests and every other integration suite pass
on both repositories, with CI green. Release binaries for `rakc`, `rak-setup` and `oyvey`
pass the hardening verifier (ASLR, high-entropy VA, DEP, CFG-compatible image, `LOAD_CONFIG`).
`oyvey` is versioned and released from its own repository.

Release notes are generated from `.github/release_body.md` at tag time.