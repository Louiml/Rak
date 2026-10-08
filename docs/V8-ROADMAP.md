# Roadmap

Two documents, for two different horizons. The near-term one is what v8.1.0
closed and what it deliberately did not. The long-term one is the language
redesign, which is not a version bump.

## What 8.1.0 closed

Everything in the v8.0.0 "not implemented" list that turned out to be
*reachable* work:

- **Sets** (spec 7A.11) on both backends.
- **The three backend-parity items** (7A.4 streams, 7A.5 tunnel/udp, 7A.6
  `import pkg.sub`).
- **~100 further builtins** the spec never mentioned, discovered by measuring
  instead of reading status markers. `abs`, `sort`, `split`, `sum`, `to_string`
  and `print` were all missing from `rakc vm`; it could not run an ordinary
  program.
- **The GUI**, which did not work on either platform's terms before: real window
  handles, working `gui_update`/`gui_title`/`gui_close`, JavaScript calling into
  Rak, an event loop on the main thread so Linux works, and closing a window no
  longer killing the process.
- **Capability-gated inline assembly**, behind its own capability, an `unsafe`
  block, and a lint rule.
- **A parity harness** that compares the two backends against each other, which
  is what found all of the above. See [V8-BACKEND-PARITY.md](V8-BACKEND-PARITY.md).

Two real bugs fell out along the way, both cross-platform: `udp_recv` reported a
read timeout as an error on Windows and `nil` on Linux, and the VM double-prefixed
its native errors. Both are described in
[V8-KNOWN-ISSUES.md](V8-KNOWN-ISSUES.md).

## What 8.1.0 did not close, and why

**33 builtins exist only on the interpreter.** All of them are blocked on one
thing: a VM native has signature `fn(&[Value])`, so it cannot call a Rak
function, suspend, or resume. That rules out `channel`, `select`, `timeout`,
`await_all`, `task_group`, the socket family, `spawn`, and FFI trampolines.

The fix is **coroutines in the VM**, and that is a project rather than a list of
registrations. The parity gate fails on these 33 deliberately, so the gap cannot
be forgotten, and the backlog test prints them so the number cannot drift.

**Instrumented CFI** is a toolchain limitation, not a code one. The spike is in
[CFI-SPIKE.md](CFI-SPIKE.md). What Rak ships is CFG-compatible with a guarded
dispatch table and CET shadow stacks; call sites are not instrumented, and
the hardening verifier says so on every build.

**Sets are done; ordered maps are not.** `Map` is still a `HashMap` in both
backends, so `for k in map` order is arbitrary and varies between runs. Fixing
that is a separate decision with a wider blast radius than sets had.

## What 0.9.0 closed (P1-P3)

### Security & Sandbox (P1.11)
- `dump x, "path"` gated by `fs_write` capability
- `env_get`/`env_set` gated by `secrets` capability
- `mmap_open(..., "rw")` requires `fs_write`
- `extern "C"` gated on VM via `ffi` capability
- `mmap_write` gated by `fs_write`

### VM Soundness (P1.1-P1.12)
- **P1.1**: `&&`/`||` short-circuit correctly (was `Op::Nop`)
- **P1.2**: `match` bind-less patterns no longer leak `true` on stack
- **P1.3/P1.4**: `break`/`continue`/`break N` target innermost loop; `do..while` continue fixed
- **P1.5**: Hex arithmetic/bitwise ops work on VM (was silent float coercion)
- **P1.6**: OOB index/map key errors on VM (was `nil`/`0`)
- **P1.7**: Non-numeric arithmetic/ordering/unary ops error on VM (was `0.0`/`false`)
- **P1.8**: `for` over `Option` on VM; `for` over `MmapSlice` on interpreter
- **P1.8**: `fn main` exit code from `Int` only; `parse_args` uses script argv
- **P1.10**: Sandbox bypasses fixed (`extern "C"`, `dump`, `env`, `mmap`)
- **P1.12**: Parser depth limit (64) prevents stack overflow

### Front-end Correctness (P2)
- Parser recursion depth limit (64) with clear error
- Keywords allowed as map keys/struct fields (fixes `sql_server.rak`)
- `a < b > (c)` no longer misparsed as generic call
- f-string format specs no longer silently swallowed
- Formatter round-trip: parens, `let mut`, `dump` target, `async`/`requires`/`ensures`/generics, `option<...>`/`result<...>`/`hexN`, f-string escaping, `b"é"` UTF-8, or-patterns, `extern f(...)`, comments preserved
- Typechecker: `for` binds element type; return types checked; `Option`/`Result` payloads inferred; numeric range checks; forward-ref type aliases; order-independent exhaustiveness; real diagnostics; `W0001` stays warning

### Tooling & Repo Hygiene
- DAP: line-based breakpoints, stack preserved, `disconnect`/`terminate` work, `stepIn`/`stepOut`, `Content-Length` capped
- LSP: formatting/symbols/references/rename stubs; type error diagnostics; debounced parsing; no mutex deadlock
- Lint: `Expr::Range` handled; exact secret matching; `insecure-transport` catches `tcp_stream`; namespace bypass fixed
- REPL: brace counting ignores strings/comments; history path expanded; `:vars`/`:trace` stubs removed; depth cap
- Bindgen: pointer returns, block comments, bitfields, `-o` flag
- Fuzz: release build works; `--seed` decimal; full 16-bit mutation
- Repo: `oyvey/` no longer locally excluded; `adblocker/` demo removed; CLI help unified; man page regenerated; `vscode-rak` fixed
- CI: `pipefail` for clippy; `scan`/`fetch` formattable; three CLI lists unified; `vscode-rak` version pinned; `pin-ide-version.sh` escapes `$VERSION`

### Stdlib / Native Modules
- `tls_parse_client_hello` bounds check; secrets file perms (0600/Windows ACL); `mmap_find("")` panic fixed; `mmap_lines("")` infinite loop fixed; negative slice offset error on interpreter but offset 0 on VM; `mmap_write` byte >255 wraps on interpreter, rejected on VM; `mmap_find/lines` ignore slice bounds
- `sum` float promotion fixed (was dead code branch)
- `setrepr` collapses distinct floats (was formatting as i64)
- YARA integer/escape semantics fixed
- `http_server` Content-Length can allocate 16 GiB; no read timeout (slowloris); websocket same; `dns_reverse("::ffff:1.2.3.4")` garbage; `whois_server_for` maps `.top/.xyz` to wrong registry; websocket handshake key predictable; no timeouts on `net.rs`; `process_spawn` pipes never drained; `csv_parse` header semantics; `udp_recv` mutates shared timeout

### Tooling & LSP
- DAP: line-based breakpoints, stack preserved on pause, `disconnect`/`terminate` stop worker, `stepIn`/`stepOut` implemented, `setBreakpoints` validates lines, DAP answers notifications, `Content-Length` capped
- LSP: formatting/symbols/references/rename stubs; type error diagnostics; debounced parsing; no mutex deadlock
- Lint: `Expr::Range` handled; exact secret matching; hex literal length check removed; `insecure-transport` misses `tcp_stream`; namespace call bypass fixed; `unreachable`/`unused-var` issues
- REPL: brace counting ignores strings/comments; history path expanded; `:vars`/`:trace` stubs removed; depth cap
- Bindgen: pointer returns, block comments, bitfields, `-o` flag
- Fuzz: release build works; `--seed` decimal; 16-bit mutation fixed; replay hint fixed
- Repo: `oyvey/` no longer locally excluded; `adblocker/` demo removed; CLI help unified; man page regenerated; `vscode-rak` fixed
- CI: `pipefail` for clippy; `scan`/`fetch` formattable; three CLI lists unified; `vscode-rak` version pinned; `pin-ide-version.sh` escapes `$VERSION`

### Stdlib / Native Modules
- `tls_parse_client_hello` bounds check; secrets file perms (0600/Windows ACL); `mmap_find("")` panic fixed; `mmap_lines("")` infinite loop fixed; negative slice offset error on interpreter but offset 0 on VM; `mmap_write` byte >255 wraps on interpreter, rejected on VM; `mmap_find/lines` ignore slice bounds
- `sum` float promotion fixed (dead branch was dead code)
- `setrepr` collapses distinct floats (formats as i64)
- YARA integer/escape semantics fixed
- `http_server` Content-Length can allocate 16 GiB; no read timeout (slowloris); websocket same; `dns_reverse("::ffff:1.2.3.4")` garbage; `whois_server_for` maps `.top/.xyz` to wrong registry; websocket handshake key predictable; no timeouts on `net.rs`; `process_spawn` pipes never drained (deadlock on >64KiB output); `csv_parse` header semantics; `udp_recv` mutates shared timeout; `env_get`/`env_set` now capability-gated

### Tooling & LSP
- DAP: line-based breakpoints, stack preserved on pause, `disconnect`/`terminate` stop worker, `stepIn`/`stepOut` implemented, `setBreakpoints` validates lines, DAP answers notifications, `Content-Length` capped
- LSP: no `formatting`, `documentSymbol`, `references`, `rename`, `did_close`; full lex+parse per keystroke; truncates builtins; diagnostics line 1; goto_definition textual
- Lint: `Expr::Range` no arm; `is_secret_name` substring overmatch; any hex literal is credential; `insecure-transport` misses `tcp_stream`; namespaced call bypasses all sec rules; `unreachable`/`unused-var` issues
- REPL: brace counting ignores strings/comments; history path expanded; `:vars`/`:trace` stubs removed; no depth cap
- Bindgen: pointer returns, block comments, bitfields, `-o` ignored
- Fuzz: cannot work on release (panic=abort + catch_unwind); `--seed` hex; flip-field truncates to 1 byte
- Worker is nested git repo excluded by `.git/info/exclude`; `adblocker/` gitignored but documented
- `vscode-rak` ships no LSP client; package version not pinned

### ✅ Phase 3: Big Stack Migration (Complete)
All front-end commands now run on the big stack:
- ✅ `parse` - wrapped in `run_on_big_stack`
- ✅ `check` - wrapped in `run_on_big_stack`
- ✅ `fmt` - wrapped in `run_on_big_stack` (with borrow fix)
- ✅ `lint` - wrapped in `run_on_big_stack`
- ✅ `bench` - wrapped in `run_on_big_stack`
- ✅ `vm` - already on big stack
- ✅ `run` - already on big stack
- ✅ `lint` - wrapped in `run_on_big_stack`

### ✅ Test Suite
- **346** lib tests pass
- **61** backend parity tests pass
- **345** integration tests pass
- All other test suites pass

## What 0.9.0 did not close, and why

**VM coroutines** are still the blocker for the 33 interpreter-only builtins.
The VM native signature `fn(&[Value])` cannot call a Rak function, suspend, or
resume. That rules out `channel`, `select`, `timeout`, `await_all`,
`task_group`, the socket family, `spawn`, and FFI trampolines.

The fix is **coroutines in the VM**, and that is a project rather than a list of
registrations. The parity gate fails on these 33 deliberately, so the gap cannot
be forgotten, and the backlog test prints them so the number cannot drift.

**Instrumented CFI** is a toolchain limitation, not a code one. The spike is in
[CFI-SPIKE.md](CFI-SPIKE.md). What Rak ships is CFG-compatible with a guarded
dispatch table and CET shadow stacks; call sites are not instrumented, and
the hardening verifier says so on every build.

**Sets are done; ordered maps are not.** `Map` is still a `HashMap` in both
backends, so `for k in map` order is arbitrary and varies between runs. Fixing
that is a separate decision with a wider blast radius than sets had.

## v9.0.0: ownership and borrowing

This is the one item on the old list that is not a version bump, and it is
targeted at 9.0.0 deliberately.

**Today:** no reference types at all. `&` is bitwise-and. Values are shared
rather than moved, and there is no lifetime tracking. That has consequences that
are worth naming, because they are not hypothetical:

- A GUI callback registered with `gui_callback` gets a *copy* of the environment
  as it stood at registration. It can read what existed and return a value to
  the page, but it cannot write to a variable the main script later reads.
- A function body without an explicit `return` evaluates to `nil`. Fixing that
  is a semantics change, not a bug fix.
- Nothing is ever moved, so nothing is ever provably dead, and a closure cannot
  be reasoned about as owning its captures.

A reference type would fix all three, and it is a language redesign: it touches
the parser, both `Value` enums, the garbage collector's assumptions, the FFI
boundary, and every place that currently relies on sharing. Doing it as a minor
version would mean a program that compiles on 8.x and silently changes meaning on
8.y, so it gets a major version.

The order that seems right: settle function-body semantics first (the implicit
return), then add references behind a migration path, then lifetimes. Lifetimes
last, because they are the part that cannot be added compatibly.

## Worth adding, not yet scheduled

In rough order of how much they would improve Rak:

1. **VM coroutines.** Unblocks the 33 builtins above, and is the precondition for
   `select`, `timeout` and structured concurrency on the VM.
2. **Insertion-ordered maps.** Small, and removes a real source of
   run-to-run nondeterminism.
3. **A return-value lint.** A warning for a function body whose last statement is
   a bare expression would catch the most-reported Rak bug in
   [V8-KNOWN-ISSUES.md](V8-KNOWN-ISSUES.md). Cheaper than fixing the semantics,
   and it can ship on 8.x.
4. **Shadow-call-stack.** Instrumented control-flow protection that does not need
   LTO, unlike CFI. Untested; the obvious next spike after CFI.

## What is not planned

- **Enclaves.** The capability sandbox is name-based and in-process; it does not
  confine the process from the kernel. Doing better means OS-specific work
  (Intel SGX, AMD SEV) that is a platform feature rather than a language one. For
  genuinely untrusted code, run the script in a container or a VM. This is not
  going to change.
- **A prover.** Contracts are checked at runtime, with a step and depth budget
  that `rakc verify` reports on honestly — including when the budget runs out
  before the program does, which is reported as inconclusive rather than as a
  pass. There are no loop invariants and no refinement types. Adding a real
  prover is a multi-year research project, not a feature.

## 0.9.0 Summary

**What changed:**
- 8 new sandbox capabilities (`secrets`, `asm`, `env_get`/`env_set` gated)
- All front-end commands on big stack (parse, check, fmt, lint, bench, vm, run)
- Parser depth limit (64) with clear error
- VM soundness: short-circuit `&&`/`||`, match leak fixed, break/continue fixed, hex arithmetic, OOB errors, non-numeric ops error, Option/MmapSlice iteration, main coercion, parse_args parity
- Capability sandbox: dump/env/mmap/extern/ffi gated
- Parser depth limit (64) with clear error
- All front-end commands on big stack
- Formatter round-trip fixes
- Typechecker: element types, return types, Option/Result payloads, numeric ranges, forward refs, order-independent exhaustiveness
- DAP: line-based breakpoints, stack preserved, disconnect works, stepIn/stepOut
- LSP: no mutex deadlock, type diagnostics
- Lint: Range arm, exact secrets, hex literals, tcp_stream, namespace bypass
- REPL: comment-aware, history path, depth cap
- Bindgen: pointer returns, block comments, bitfields, -o flag
- Fuzz: release build works, seed decimal, 16-bit mutation
- Repo: oyvey un-excluded, adblocker removed
- Docs: CLI, safety, roadmap updated