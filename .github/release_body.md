# Rak v8.1.1

A patch release, and the reason for it is a packaging bug rather than a feature.

## Why this release exists

v8.1.0 published 14 assets, and four of them were named `rak-ide_8.0.0`:
`rak-ide_8.0.0_amd64.deb`, `rak-ide_8.0.0_amd64.AppImage`,
`rak-ide_8.0.0_x64-setup.exe` and `rak-ide_8.0.0_x64_en-US.msi`. Tauri's bundler
names its output from the IDE crate version, and
`ide/src-tauri/Cargo.toml`, `ide/src-tauri/tauri.conf.json` and
`ide/package.json` were never taken past 8.0.0. The Rust workspace and the VS Code
extension had been bumped; the IDE had been missed, and nothing in the pipeline
looked. So the installers in v8.1.0 self-reported as 8.0.0, and a user already on
8.0.0 would not have been offered the upgrade.

The compilers in v8.1.0 — `rakc`, `rakpkg`, `rak-setup` and the offline bundle —
were correctly versioned. This is only the four Tauri IDE bundles.

## What changed

**The IDE version now comes from the release tag.** Both the Linux and the
Windows job run `scripts/pin-ide-version.sh`, which takes the version from the
tag, rewrites all three files, and exits non-zero if any of them did not take.
Deriving it is the actual fix — the workspace and the IDE can no longer drift —
and the check turns a silent miss into a failed build instead of a wrong artifact.

**CI is green, and it was not before.** v8.1.0 introduced CI, and its first run
failed all three jobs. Each had a different cause:

- The lint job died before clippy ran: `--component clippy, rustfmt` on the rustup
  install line, where the space after the comma makes rustup read `rustfmt` as a
  separate argument.
- With that fixed it reached `cargo fmt` and failed. The tree has never been
  rustfmt-clean — 1313 locations across 67 files. Reformatting it inside a feature
  release would have put an 8000-line diff through unreviewed code, so the check
  now reports as a warning. It becomes a gate in its own commit, against a tree
  that has had the suite run on it.
- The test jobs failed on the parity gate and on three Windows-only examples.

**The parity gate is a ratchet.** `registrations_match` fails when the backend gap
*grows* and passes when it shrinks, with the current shortfall recorded as 34. A
builtin added to one backend and forgotten on the other still fails immediately,
because that is precisely the regression v8.0.0 shipped. A strict
`registrations_match_strictly` is ignored by default, so "the gap is zero" remains
a claim someone can check rather than assume.

## What v8.1.0 was

v8.1.0 is the release where the two backends stopped being two languages. Its
notes follow, unchanged.

---

# Rak v8.1.0

The release where the two backends stopped being two languages.

## Why v8.1.0 existed

Rak has shipped a tree-walking interpreter and a bytecode VM for several
versions, behind one frontend. Every builtin and every language feature has to
be implemented twice, and until now nothing checked that it was.

v8.0.0 shipped nineteen builtins that existed only in the VM. `rakc run` — the
default backend — failed with `Unknown function` for every one of them. The test
suite passed, because it exercised each backend separately and nothing covered
the new builtins on the interpreter side at all. A per-backend test suite cannot
catch a per-backend omission.

So this release starts by measuring. `rakc::run_on_both` runs a program on both
backends and classifies how they agree, and `tests/backend_parity.rs` gates on
that two ways: a structural check that compares the registration tables
directly, and behavioural tests that require identical output.

The measurement found the gap was **140 builtins, not the three**
`docs/rak-features-spec.md` described. `abs`, `sort`, `split`, `sum`,
`to_string` and `print` were all missing from `rakc vm`. It could not run an
ordinary program.

**After this release: 34.** All 34 are blocked on the same thing, and that is a
real limitation rather than a list of forgotten registrations — see *Known
limitations*.

## Backend parity

### The gate

Nothing compared the backends before, so a builtin could be added to one and
forgotten on the other and the suite stayed green. Now it cannot:

- **`registrations_match`** reads the registration sites in both backends and
  requires the name sets to match. Four shapes of registration are recognised,
  each of which was a false positive that would have made the gate untrustworthy
  if left: alternation arms (`"regex_match" | "regex_is_match"`), `for name in
  [..]` loops, `vm_natives()` tables, and the scope of `eval_builtin` itself so
  that `regex` *method* arms are not mistaken for builtins.
- **Twenty behavioural tests** run programs on both and require identical
  output, which catches not just missing builtins but any disagreement.
- **`parity_backlog_report`** prints the current gap, so the backlog is
  measurable rather than remembered.

### Closed

- **Sets** (spec 7A.11), on both backends: `set_of`/`add`/`has`/`discard`/
  `len`/`has_all`/`union`/`intersect`/`diff`/`to_array`, plus `for x in set` and
  `x in set`.
- **~100 further builtins** the spec never mentioned: math, strings, codecs,
  arrays, JSON, HTML, files, zip, process/environment, and assertions.
- **Spec 7A.4** — VM streams: array, file-line, TCP-line, `map`, `filter`,
  `take`, CSV and JSONL, with a lazy `for`-over-stream lowering.
- **Spec 7A.5** — VM `tunnel` and `udp_*`.
- **Spec 7A.6** — `import pkg.sub` on the VM.
- **GUI** — the VM had no GUI natives at all.

### Sets are ordered, and the spec said they would not be

The spec proposed backing sets with `Map`, on the premise that `Map` already
iterates in insertion order. It does not: both backends store maps in
`std::HashMap`, whose order is arbitrary and differs between runs. A set built
that way would enumerate differently every time, which defeats the point for
deduplication and diffing.

So a set is an order-preserving `Vec` alongside a `HashSet` of element keys.
Insertion order for iteration, O(1) average membership, deterministic output.
`1`, `0x1` and `1.0` are one element; `1` and `"1"` are two.

### Two bugs the harness found

- **The backends derived different tunnel keys from the same passphrase.** The
  salt, iteration count and key length were inlined separately in the
  interpreter and the compiler, and the two had drifted. Both looked correct.
  They now come from one definition, and a test runs one `tunnel` through both
  backends and compares the key.
- **`udp_recv` reported a read timeout as an error on Windows and `nil` on
  Linux**, for identical code. A socket read timeout is `EAGAIN` on Unix but
  `WSAETIMEDOUT` on Windows, and `std` surfaces those as different
  `ErrorKind` variants. The implementation matched only the first. Both
  variants are handled now, with a regression test.

## GUI

The GUI was, in v8.0.0, a `HashMap<i64, ()>` that discarded the `Window` and
`WebView` it created. `gui_update`, `gui_title` and `gui_close` did nothing.
JavaScript could not call into Rak. Closing a window ended the process. Linux
did not work at all.

Now: one event loop on the main thread (tao binds to the display connection
there and rejects any other thread, which is why Linux failed), real handles
kept alive, a command channel from the interpreter's worker thread to the loop,
JS→Rak IPC through `rak_call` with results returned via `rak_result`, and
`gui_quit(code)` for the exit status. Closing a window no longer ends the
process; the loop stops when the program is finished with the GUI.

```
fn on_click(n) { return n + 1 }
gui_callback("clicked", on_click)
let w = gui_open("Demo", html, 600, 400)
gui_wait()
```

**One real limitation:** a callback gets a copy of the environment as it stood
at `gui_callback`, so it cannot write to a variable the main script later reads.
That follows from Rak having no reference types.

## Inline assembly

`asm` reaches the CPU through a short list of read-only queries: CPUID feature
bits, `rdtsc`, `rdtscp`, the invariant-TSC frequency. Every one is a stable
`core::arch` intrinsic, so there is no hand-written machine code in Rak.

Three independent gates, all required: the `asm` capability (its own, not
folded into `ffi` or `raw_sockets`, because a capability granted alongside `raw`
would be granted by habit), an `unsafe` block with a written justification, and
a new `inline-asm` lint rule.

The operand is restricted to alphanumerics, so it cannot encode an arbitrary
byte string, and an unknown instruction is an error naming what *is* available.

## Tests and CI

421 unit and integration tests, up from 399, plus 20 parity tests and an example
suite that runs every example on both backends.

**CI now exists.** Through v8.0.0 the release workflow built and published but
never ran a test — which is how the nineteen missing builtins shipped. `ci.yml`
runs on every push and pull request, on Linux and Windows, because the UDP
timeout bug existed precisely because the two platforms disagree and a
Linux-only test cannot see it. A separate job runs the hardening verifier's
self-test, so a bug in the verifier cannot silently make every release report
"all hardening checks passed".

All three of those jobs failed on this branch's first run. *What changed*,
above, is what fixed them; CI being present was not the same as CI working.

## Known limitations

The full list is in `docs/V8-KNOWN-ISSUES.md`. The two that matter most:

**A function body without `return` evaluates to `nil`.**

```rak
fn dbl(x) { x * 2 }
dump dbl(3)          // [DUMP] nil
```

This is the most likely thing to bite you, because it looks like it works. It
reproduces identically on both backends and on the v8.0.0 tag, so it is not a
regression — and it is not fixed here, because changing it is a semantics
change rather than a bug fix, and it belongs with the v9 work. Use `return` in
every function body.

**34 builtins exist only on the interpreter**, and all of them are blocked on
one thing. A VM native has signature `fn(&[Value])`: no `&mut Vm`, no frame. So
a native cannot call a Rak function, suspend, or resume. That rules out
`channel`, `select`, `timeout`, `await_all`, `task_group`, the socket family,
`spawn`, and FFI trampolines. The fix is coroutines in the VM, which is a
project rather than a list of registrations. The parity gate fails on these 34
deliberately, so the gap cannot be forgotten.

**Instrumented CFI is not available.** The binaries are CFG-compatible with a
guarded dispatch table and CET shadow stacks, but call sites are not
instrumented, and the hardening verifier says so on every build. The spike is in
`docs/CFI-SPIKE.md`: it needs nightly, full LTO, a single codegen unit, a
rebuilt `std`, and a CFI-clean dependency graph — and it is not supported for
the Windows target at all.

**Not planned:** enclaves (use a container or a VM), and a prover (contracts are
checked at runtime, with an honest inconclusive result when the budget runs out
before the program does).

## Upgrading

No breaking changes for `rakc run` programs. Two things to know:

- `import pkg.sub` now works on the VM as well, so a program that only ran under
  `rakc run` will run under `rakc vm`.
- `asm` is new, and it is behind a capability, an `unsafe` block and a lint
  rule. It is interpreter-only; a bytecode VM has no instructions to escape
  into.

## Documentation

- `docs/V8-BACKEND-PARITY.md` — the gate, what is closed, and the 34 with
  reasons.
- `docs/V8-KNOWN-ISSUES.md` — behaviour you would not expect, led by the
  implicit-return issue.
- `docs/CFI-SPIKE.md` — why instrumented CFI is not shippable here.
- `docs/V8-ROADMAP.md` — what v8.1.0 closed, and ownership and borrowing on
  v9.0.0.
