# Known issues

Bugs that are known, reproduced, and **not fixed** in this release. Each entry
says what happens, what causes it, and what the workaround is.

Nothing here is a "not yet implemented" feature — those live in
[docs/V8-BACKEND-PARITY.md](V8-BACKEND-PARITY.md) and
[docs/rak-features-spec.md](rak-features-spec.md). These are places where Rak
behaves in a way you would not expect, and where a reasonable program gets the
wrong answer.

## A function body without `return` evaluates to `nil`

**Severity: high.** This is the most likely thing to bite you, because it looks
like it works.

```rak
fn dbl(x) { x * 2 }
dump dbl(3)          // [DUMP] nil
```

The fix is to write `return`:

```rak
fn dbl(x) { return x * 2 }
dump dbl(3)          // [DUMP] 6
```

**Cause.** A Rak function's value is its `return` expression. There is no
implicit tail return, so a body whose last statement is an expression rather
than a `return` produces no value.

**Scope.** Both backends, and it reproduces on the v8.0.0 tag, so it is
long-standing rather than a regression. It is most visible through
`stream_map` and `filter`, which take a function:

```rak
// yields [nil, nil, nil]
collect(stream_map(stream_from_array([1, 2, 3]), fn(x) { x * 10 }))

// yields [10, 20, 30]
collect(stream_map(stream_from_array([1, 2, 3]), fn(x) { return x * 10 }))
```

**Not fixed here** because changing it is a language semantics change, not a
bug fix, and it would alter the meaning of every existing program. The right
change is a decision to make, with a migration story — see the v9 roadmap entry
on ownership and borrowing, which has to settle function semantics anyway.

**Workaround.** Use `return` in every function body, and turn the
`unsafe-thin-reason`-style lint on if you have one for it. There is not one yet;
that is the cheap mitigation and it has not been written.

## A stream `for` loop is only lazy for recognised names

**Severity: medium.** A `for` loop over a stream bound to an unrecognised
variable name materialises the whole stream instead of pulling from it.

```rak
// lazy: the compiler recognises `stream_from_array(..)`
for x in stream_from_array([1, 2, 3]) { dump x }

// NOT lazy: `s` does not look like a stream constructor
let s = stream_from_array([1, 2, 3])
for x in s { dump x }
```

Both produce the same output for a finite stream. The difference shows up with
an unbounded one, where the second form never terminates because
`Op::IterItems` builds the entire array before the loop starts.

**Cause.** `compiler.rs::iterable_is_stream` decides by syntax — a call to one
of the eight known stream constructors, or an identifier whose name starts with
`stream`, is `lines`, or ends in `_stream`. This avoids emitting a runtime type
probe at the top of every loop.

**Workaround.** Name stream variables so the heuristic matches, or call the
constructor directly in the `for` header.

**Fix.** A runtime type check, which needs either a new opcode or inspecting the
value before the loop body. Both are small; neither is done.

## Windows and Linux report socket timeouts differently — fixed, but the
## pattern may recur

**Severity: low, fixed in this release.** Worth recording because the class of
bug is easy to reintroduce.

`udp_recv(t, n, timeout)` returned an **error** on Windows where it returned
`nil` on Linux, for identical code. A read timeout is not a failure, and a Rak
program polling a transport in a loop should not need a `try` on one platform
and not the other.

**Cause.** The implementation matched only `io::ErrorKind::WouldBlock`, which is
what Unix reports for a timed-out `recv_from` (`EAGAIN`). Windows reports the
same condition as `WSAETIMEDOUT` (10060), which `std` surfaces as
`ErrorKind::TimedOut` — a different variant.

**Fix.** `TimedOut` and `Interrupted` are now both treated as "nothing
arrived", with a regression test in `stdlib/src/tunnel.rs`.

**The general lesson:** platform error *kinds* do not line up one-to-one, so any
code matching on `ErrorKind` across a platform boundary needs both variants, and
a test that exercises it. The same trap applies to `TimedOut` vs `WouldBlock` on
file locks, and to `ConnectionRefused` vs `ConnectionReset` on shutdown.

## `err` prefixes differ between backends

**Severity: low, cosmetic.** The same error reports differently under the two
backends:

```
rakc run:  Error: Runtime error: expected a stream, got array
rakc vm:   VM error: filter: expected a stream, got array
```

**Cause.** The interpreter prefixes with `Runtime error:` and the VM with
`VM error: <builtin>:`, where the builtin name comes from `Vm::call_value`'s
wrapper. Both are transport-level, not part of the message.

**Status.** The VM no longer double-prefixes — that was a real bug, where the
natives included the name *and* the wrapper added it, producing
`filter: filter: ...`. `run_on_both` now normalises the transport prefixes before
comparing errors, so this does not produce false parity failures. The visible
difference remains, and normalising it everywhere would mean deciding on one
error format for the whole project.

## The VM has no `exit`

**Severity: low.** `exit(n)` works under `rakc run` and is `Undefined` under
`rakc vm`.

**Cause.** A native *could* call `std::process::exit`, but that would skip the
VM's frame teardown, its deferred calls, and the interpreter's buffered output.
There is no correct place to put it without a real unwind story.

**Workaround.** Run with `rakc run`, or return a value from `fn main` — the
CLI's exit code comes from `fn main`'s return value, which works on both
backends.

## `asm` is interpreter-only, on purpose

**Severity: none, by design.** `asm` reports `Undefined` under `rakc vm`.

A bytecode VM has no instructions to escape into, so a VM implementation could
only be a lookup table pretending to be one. Registering the name on both sides
would have made the parity gate quiet while the feature silently stopped
touching the hardware. This is listed in the gate's `INTERP_ONLY` allowance with
its reason.

## Inline assembly bypasses the capability sandbox

**Severity: informational, by design.** The `asm` escape is not mediated by the
sandbox: the sandbox gates *builtins*, and assembly is the machine executing an
instruction directly. `extern "C"` is the safer escape, because Rak can see the
call.

Three independent gates apply and all three are required:

1. the `asm` capability (`--allow asm`), checked before anything is assembled
2. an `unsafe` block with a written justification, so every use is greppable
3. the `inline-asm` lint rule

The operand is restricted to alphanumerics, underscores and spaces, so it cannot
encode an arbitrary byte string, and the instruction list is a short set of
read-only CPU queries rather than an assembler. An unknown instruction is an
error naming what *is* available.

`asm("tsc_invariant_hz")` returns **0** on some CPUs and in most VMs, because
CPUID leaves `0x15` and `0x16` are commonly zeroed by a hypervisor. Zero is the
documented "unknown" case; a program that divides by it must check.

## Instrumented CFI is not available

**Severity: informational.** The release binaries are CFG-compatible with a
guarded dispatch table and CET shadow stacks, but call sites are **not**
instrumented. The hardening verifier says so:

```
[ok] Control Flow Guard (GUARD_CF)  image is CFG-compatible; call sites are NOT instrumented (rustc limitation)
```

The full spike, including why `-Zsanitizer=cfi` cannot be used here, is in
[docs/CFI-SPIKE.md](CFI-SPIKE.md). Short version: it needs nightly, full LTO,
single codegen unit, a rebuilt `std`, and a CFI-clean dependency graph — and it
is not supported for the Windows target at all.

## `import pkg.sub` requires the files on disk

**Severity: none.** Both backends resolve an import against the importing
file's directory, so a module cannot be supplied in memory. The parity test for
this writes `pkg/init.rak` and `pkg/sub.rak` to a temp directory.
