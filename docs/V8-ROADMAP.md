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

**34 builtins exist only on the interpreter.** All of them are blocked on one
thing: a VM native has signature `fn(&[Value])`, so it cannot call a Rak
function, suspend, or resume. That rules out `channel`, `select`, `timeout`,
`await_all`, `task_group`, the socket family, `spawn`, and FFI trampolines.

The fix is **coroutines in the VM**, and that is a project rather than a list of
registrations. The parity gate fails on these 34 deliberately, so the gap cannot
be forgotten, and the backlog test prints them so the number cannot drift.

**Instrumented CFI** is a toolchain limitation, not a code one. The spike is in
[CFI-SPIKE.md](CFI-SPIKE.md). What Rak ships is CFG-compatible with a guarded
dispatch table and CET shadow stacks; call sites are not instrumented, and the
hardening verifier says so on every build.

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

1. **VM coroutines.** Unblocks the 34 builtins above, and is the precondition for
   `select`, `timeout` and structured concurrency on the VM.
2. **Insertion-ordered maps.** Small, and removes a real source of
   run-to-run nondeterminism.
3. **A return-value lint.** A warning for a function body whose last statement is
   a bare expression would catch the most-reported Rak bug in
   [V8-KNOWN-ISSUES.md](V8-KNOWN-ISSUES.md). Cheaper than fixing the semantics,
   and it can ship on 8.x.
4. **Shadow-call-stack.** Instrumented control-flow protection that does not need
   LTO, unlike CFI. Untested; the obvious next spike after CFI.
5. **A real `for`-over-stream type check**, so laziness does not depend on what a
   variable is named.

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
