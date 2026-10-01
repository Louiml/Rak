# Rak v8.0.0

## What's new in 8.0.0

### Security primitives

- **`unsafe "reason" { ... }` blocks.** Anything touching raw memory, FFI, or a
  wire format can be wrapped in a block carrying a written justification. The
  reason is enforced by the parser, so an unjustified exemption is a parse
  error rather than a warning, and `grep unsafe` returns every exemption with the
  author's own words attached. `rakc lint --audit` prints the same list as a
  review artifact, and the interpreter records which blocks were actually
  entered during a run. This is a review boundary, not a permission: the block
  executes exactly as written.
- **`requires` / `ensures` contracts on `fn`.** Preconditions are checked with
  the parameters bound and before a single statement of the body, so a violated
  precondition blames the caller and a function that would corrupt state before
  validating never gets the chance. Postconditions run after deferred cleanup,
  with `result` and the parameters in scope. Contracts survive into `async fn`,
  on both the sequential and the concurrent drive path.
- **`rakc verify`.** Runs a script under finite step, loop-iteration and
  recursion-depth budgets. It reports three outcomes and they are deliberately
  distinct, because conflating them would be misleading: `PASS` (completed, all
  contracts held), `FAIL` (a contract broke, an assert tripped, or a runtime
  error), and `SKIP` (a budget was hit, so nothing was proved). Exit code 2 means
  inconclusive, never "looks fine".
- **Constant-time comparison.** `ct_eq`, `ct_eq_hex`, and `ct_select`. Rak's `==`
  is a data-dependent branch that leaks the length of a shared prefix through its
  exit timing, which is enough to recover a MAC or a session token one byte at a
  time.
- **Zeroization.** `zeroize(bytes)` overwrites a buffer through a volatile path
  the optimizer cannot elide. `secret_delete` and the new `secret_delete_all`
  wipe values from memory and overwrite the store file with zeroes before
  rewriting it. The serialized buffer in the secrets store is wiped too, and
  file permissions are now set *before* the data lands rather than after.

### Crypto

- **RSA** — `rsa_keypair`, `rsa_sign`, `rsa_verify`, `rsa_encrypt`, `rsa_decrypt`
  (PKCS#1 v1.5 over SHA-256, OAEP with SHA-256). Keys are DER.
- **ECDSA over NIST P-256** — `ecdsa_keypair`, `ecdsa_sign`, `ecdsa_verify`, with
  64-byte `r||s` signatures. DER keys.

The suite previously had only Ed25519 and X25519, both non-standard curves, so
nothing interoperated with ordinary OpenSSL tooling. Everything remains pure Rust
with no OpenSSL linkage.

### Static analysis

- Six new security lint rules: `hardcoded-secret`, `plaintext-url`,
  `weak-crypto`, `secret-compare`, `ffi-raw-pointer`, `insecure-transport`, plus
  `unsafe-thin-reason`.
- This required fixing a real bug: the linter had no `Expr::String` arm, so
  string literals fell through to `_ => {}` and no rule could ever inspect a
  byte of user text. A hardcoded `sk-live-...` was completely invisible. There is
  a regression test for exactly that.
- `rakc fmt` and `rakc lint` are now listed in `rakc --help`.

### Packet crafting

- **ICMP** — `net_raw_icmp`, `net_raw_icmp_ping`, `net_raw_icmp_echo_reply`, with
  matching `id`/`seq` so a reply can be correlated to its request.
- **ARP** — `net_raw_arp_request`, `net_raw_arp_reply`, and `net_raw_arp_parse`,
  which accepts a frame with or without the Ethernet header and returns `nil` on
  anything that is not ARP, so raw capture can be fed straight in.

`caps.rs` previously gated a `net_raw_icmp` builtin that did not exist; the gate
now matches real builtins, and packet *builders* are documented as reachable
inside a sandbox because they only construct a `bytes` value.

### Fuzzing on the stable toolchain

- **`rakc fuzz <target>`** — a deterministic mutation loop in the compiler
  itself, covering the lexer, parser, interpreter and the stdlib parsers. 12
  targets, reproducible via `--seed`, with a crashing input written to disk and
  the seed printed for replay.
- The 13 `cargo-fuzz` targets in `fuzz/` are still there for long
  coverage-guided campaigns, but they need nightly and `-fsanitize=fuzzer`,
  which `stable-x86_64-pc-windows-msvc` cannot provide. Nothing was running them
  automatically before this.

### Build hardening

- Release profile gains `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`
  and `strip = "symbols"`. The one that matters most is `overflow-checks = true`:
  Cargo's default in release is `false`, which silently wraps and turns a length
  calculation into a heap overflow.
- `.cargo/config.toml` adds `/guard:cf`, `/CETCOMPAT`, `/DYNAMICBASE`,
  `/HIGHENTROPYVA`, `/NXCOMPAT` on MSVC, and full RELRO, a non-executable stack,
  a stack canary and forced frame pointers on Linux.
- **Stated precisely, because these are easy to oversell.** `/guard:cf` marks the
  image CFG-compatible and guards the dispatch table, but rustc does not
  instrument Rust's own indirect calls, so there is no `__guard_check_icall` in
  the binary. This is not equivalent to Clang's `-fsanitize=cfi`. It closes a
  real class of bugs; it does not close ROP or JOP.
- `dist/verify_hardening.py` reads the produced binary back and asserts the bits
  are really set, and both release jobs run it. It has a `--self-test` covering
  both the PE and ELF parsers on synthetic headers, positive and negative. This
  already caught a real mistake: an early flag set passed both `/NXCOMPAT` and
  `/NXCOMPAT:NO`, silently disabling DEP.

### Deep recursion no longer kills the process

The tree walker burned several native frames per Rak call, and a Rak call costs
several KB of native stack in a debug build. `fib(15)` needed roughly a megabyte,
which is more than the default thread stack allows, so a moderately recursive
program died with a bare "has overflowed its stack" and no Rak-level line number.
The interpreter now runs on a thread with an explicit 64 MB stack. `rakc verify`
additionally bounds recursion depth, loop iterations and total steps, so runaway
recursion is a reportable outcome rather than a crash.

### Tooling

- The VS Code TextMate grammar's builtin list was a single 2,900-character line,
  close to TextMate's practical limits and unmaintainable. It is now 17 grouped
  per-family patterns, the longest 371 characters.
- The builtin lists in the LSP, the IDE editor, and the grammar were all stale
  and disagreed with each other. All three are updated, and the LSP list had been
  missing the entire batteries, OSINT, iterator, FFI, mmap and `net_raw` families.
- New `docs/content/safety.md`, covering what is enforced and — just as
  importantly — what is not.

### Correctness

- `docs/content/vm.md` claimed binary pattern matching, method-call dispatch and
  user enum patterns were unsupported on the VM. All three shipped; the table was
  wrong.
- `Pattern::Bind` does not exist: the `@` binding-pattern syntax the spec
  advertised does not parse. Marked `[SPEC]` rather than `[SHIPPED]`, with the
  implementation steps recorded. Spec markers in both directions were wrong and
  are now corrected against the code.
- `expr_str` fell back to the AST debug form for calls, indexing, field access
  and ranges. Contract clauses and assertion failures are mostly calls, so the
  most important diagnostics were the least readable. Now rendered as source.

### Housekeeping

- All version strings moved to 8.0.0, including the REPL banner (still said
  v0.3.0) and `rakpkg` (still said 0.7.0).
- `raklib/` was an empty untracked directory. Removed.
- `adblocker/` added to `.gitignore`: it is a nested git repository kept
  deliberately local, and without this `git add -A` fails outright.
- `scripts/run-tests-safe.ps1` runs the test binary under a memory and time
  watchdog. A stack overflow becomes a Windows Error Reporting event, and with
  the system default of automatic memory dumps that writes a multi-gigabyte file.

## Known limitations

Stated plainly rather than discovered later.

- **The GUI is incomplete and Windows-only.** `gui_update`, `gui_title` and
  `gui_close` are declared but do nothing: `gui.rs` stores `HashMap<i64, ()>` and
  discards the `Window` and `WebView` handles. JavaScript cannot call into Rak —
  the IPC handler receives the message and drops it. Closing a window ends the
  process, because tao's `run` calls `process::exit`. Linux does not work at all,
  because `gui_open` builds the event loop off the main thread, which tao
  rejects. The VM has no GUI natives, so `rakc vm` reports
  `Undefined: gui_open`. The README and docs now say this instead of implying
  otherwise.
- **No ownership or borrowing.** There is no `&` reference type; `&` is
  bitwise-and. Values are shared rather than moved and there is no lifetime
  tracking. This is a language redesign, not a version bump.
- **No instrumented control-flow integrity.** See the hardening section above for
  exactly what is and is not provided.
- **No inline assembly.** `extern "C"` FFI is the escape hatch, and it is
  capability-gated and lint-flagged.
- **No formal verification.** Contracts are checked at runtime; there is no
  prover, no loop invariants, no pre/postconditions in the refinement-type sense.
- **No enclaves.** The capability sandbox is name-based and in-process. It does
  not confine the process from the kernel. For genuinely untrusted code, run the
  script in a container or a VM.
- **Sets are not implemented** (7A.11), and the backend parity items in
  `docs/rak-features-spec.md` Part 7A.4–7A.6 (VM streams, VM `tunnel`/`udp_*`, VM
  `import pkg.sub`) remain open.

`docs/V8-ROADMAP.md` records the reasoning and the cost for each gap.
