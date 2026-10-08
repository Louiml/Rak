# Known issues

Bugs that are known and reproduced. Most are **not fixed** in this release; a
few are fixed and kept here for the record, because the shape of the bug
matters more than the fix.

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

## Every VM `for` loop over a stream errored

**Severity: high, fixed in this release.** Recorded because the docs
described the intended behavior as if it shipped, and because the cause was a
lowering that had never once executed.

```rak
for x in stream_from_array([1, 2, 3]) { dump x }
```

```
rakc run:  [DUMP] 1  [DUMP] 2  [DUMP] 3
rakc vm:   VM error: cannot iterate over this value (stream)
```

Both forms errored on the VM — the recognised constructor call in the `for`
header and the stream bound to a variable alike.

**Cause.** `compile_stream_for` was added with the lazy-streams feature, but
`compile_for` never called it: every stream iterable fell into the
`Op::IterItems` path, which does not know how to iterate a `Value::Stream`.
The function was dead from the commit that introduced it, and it was also
broken — its emission patched the opening forward jump to a target *after*
the body, so even wired in it would have jumped straight over the loop. The
docs around it (a syntax-limited lazy lowering that worked for recognised
names, materialising otherwise) described intent, not behavior.

**Fix.** `compile_for` now dispatches on the runtime type: `__is_stream(v)`
is called once before the loop (registered on both backends), each iteration
branches on that cached result to either pull one element (`stream_next`) or
index the materialised items, and both steps join at a single copy of the
body, so `break`, `continue` and loop labels behave identically whichever
kind of iterable showed up. The syntactic name heuristic is gone with the
dead code: a stream under any variable name iterates lazily, and an array
named `lines` iterates as an array instead of being mistaken for a stream.

## VM call semantics diverged from the interpreter on every non-trivial signature

**Severity: high, fixed in this release.** `Value::Closure` carried only a
parameter *count*, so the VM bound arguments by position and Nil-padded to
that count. Every signature feature that is not "positional, all required"
was therefore wrong on `rakc vm`:

| Signature | `rakc run` | `rakc vm` (before) |
| --- | --- | --- |
| `fn f(a, b = 10)` called as `f(1)` | `b = 10` | `b = nil` |
| `fn f(a, b = a + 1)` called as `f(7)` | `Undefined variable: a` | `b = nil` |
| `fn g(a, ...rest)` called as `g(1, 2, 3)` | `rest = [2, 3]` | `rest = 2` |
| `fn h(a)` called as `h(1, 2)` | `too many positional arguments (1 extra)` | silently dropped |
| `fn m(a, b)` called as `m(1)` | `missing required argument 'b'` | `b = nil` |
| `fn main()` (no parameters) | `too many positional arguments (1 extra)` | ran fine |

Three of those are silent wrong answers rather than crashes, which is the
worst shape of divergence: the same program prints different values on the
two backends, and only one of them is the interpreter's answer.

**Cause.** Binding lived entirely at the call site (`call_value` had the
argument list, the AST `Param` list stayed with the interpreter), and the
compiled closure threw the signature away after reading `params.len()`. The
interpreter's `bind_params` handles rest, defaults, optionals and arity by
name; the VM's pad-to-N handled none of them. `fn main()` was a special case
of the arity bug: both backends handed the argv array to an entry point that
had declared no parameter to receive it, so the interpreter's own arity check
rejected the call while the VM's pad absorbed it.

**Fix.** The closure now carries a `ParamShape` per parameter (name, rest,
optional, has-default) — the call-time facts, without the `Expr`. `call_value`
binds rest into an array, raises `too many positional arguments (n extra)` and
`missing required argument 'x'` with the interpreter's exact wording, and
passes the number of positionals actually supplied in a hidden `__argc` local.
Defaults cannot travel as expressions (a default may reference an earlier
parameter, so evaluating one needs the callee's bindings), so
`compile_function` emits each default into the callee body's prologue, keyed
off `__argc`: omitted fills the default, an explicit `nil` stays `nil`. The
interpreter's `bind_params` was fixed to define already-bound parameters
before evaluating a default, which turns `fn f(a, b = a + 1)` from an error
into the working thing both backends now do. `run_main` (both backends) passes
argv only when `main` declares a parameter.

**Still divergent, deliberately.** Named arguments (`f(b: 2, a: 1)`) work on
the interpreter and are a compile error on the VM (`VM does not support named
arguments`). That is a loud stop, not a silent wrong answer; supporting them
means either call-site reordering against a known signature or a binding map
at the call, and neither was needed to close the silent cases above.

## Windows and Linux report socket timeouts differently — fixed, but the pattern may recur

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

## Inline assembly is gated, but the sandbox cannot mediate the instruction

**Severity: informational, by design.** `asm(...)` is gated like any builtin —
`--sandbox` without `--allow asm` stops it with `sandbox: builtin 'asm'
blocked` — but the gate is on the call, not on what the instruction does once
the CPU executes it. `extern "C"` is the safer escape, because Rak can see the
call.

Three independent gates apply and all three are required:

1. the `asm` capability (`--allow asm`), checked before anything is assembled
2. an `unsafe` block with a written justification — enforced by the parser, so
   `asm(...)` outside `unsafe` does not parse and every use is greppable
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

## A `for` loop stopped at the first falsy element on the VM

**Severity: high, fixed in this release.** Recorded because it was silent, it
affected every container rather than only bytes, and no diagnostic pointed at it.

```
let buf = bytes([0, 15, 16, 255])
let mut n = 0
for b in buf { n = n + 1 }
dump n            // interp: 4    vm (before): 0
```

**Cause.** The VM's `for` loop decided whether to continue by testing the *element's*
truthiness rather than the loop bound: `IndexGet`, then `JumpIfFalse`. `0`, `""` and
`false` are all falsy, so iteration ended at the first of them. `0x00` is the most
common byte in a binary file, so `for b in buffer` walked a buffer and then stopped
dead at the first NUL, with no error at all. A buffer of `[0]` iterated zero times.

**Fix.** Added `Op::Len` and made the loop test `idx < len`. A loop bound has to be a
comparison against a length, not a test of the value being carried.

**Why it went unnoticed.** The only test covering byte iteration used `bytes([1, 2])`,
which has no falsy byte in it.

## `Hex` values rendered zero-padded on the VM

**Severity: low, fixed in this release.** Kept because the padding shipped, and
because a value that prints as `0x0000000000001234` looks like it carries
information it does not.

```
rakc vm:   [DUMP] 0x0000000000001234
rakc run:  [DUMP] 0x1234
```

**Cause.** The VM's `Hex` value carried a digit-width field, and `Display` printed
`width / 4` digits. Nothing ever populated it from the source -- the compiler
hardcoded `Value::Hex(h, 64)` for every hex literal -- so all 16 slots were used.
The interpreter has no width field and prints the minimum via `0x{:X}`.

**Fix.** Removed the field. It had exactly one reader, the `Display` arm.

**Not a comparison bug.** `Hex(0) == 0` was true on both backends throughout:
`Value`'s `PartialEq` has a cross-representation numeric fallback for
`Hex`/`Int`/`Float`, and `Op::Eq` routes straight through it. An earlier draft of
this entry claimed otherwise and was wrong.

## The VM's module globals were not namespaced

**Severity: medium. Mostly fixed in this release; one symptom remains.** Kept as a
single entry because six of the seven symptoms had one cause, and because the one
that is left is the interesting half.

The interpreter gives each module its own `Env`, so a module's top-level bindings
live in a cell of its own and a name in a module means what that module's file says
it means. The VM compiles a module's body into the *same chunk*, so those bindings
used to be ordinary chunk globals in one flat namespace with the importer's.

**The fix.** Each inlined module's top-level names are mangled per module
(`__rak_modcell_3$COUNT`), and the mangled name is recorded as the *backing* global
in `ModuleCell.exports`, which already existed to map an exported name to the global
behind it. `pub X` stays spelled `X` for the namespace and for `m.X`; the mangled
name is only ever a chunk-global detail. No bytecode format change and no VM
instruction was needed for that part.

Two things had to be threaded through the compiler for it to hold, and both were
misses rather than design:

* `compile_function` builds a fresh sub-compiler for a closure body. It copies
  `func_names`, `func_closures`, `macros` and `base_dir`; a module's functions read
  their module's own top-level names, so it has to copy the rename map too.
* `compile_expr` had a **second, independent implementation** of identifier loading,
  alongside `compile_ident_load`. `return scale` went through that one, so it
  compiled to a load of the unmangled global while every *declaration* went to the
  mangled one -- `Undefined: __rak_modcell_1$scale`, or, in the other direction, a
  value read from the importer's globals. Two implementations of the same thing was
  the bug.

### Fixed

**2. A module's private top-level is no longer visible to the importer.** The
importer's `scale` no longer resolves, because the module's `scale` is in a global of
its own.

**3. Reading a private name through the handle reports it.** `p.privv` is an error
naming `privv` on both backends; it used to read as `nil` on the VM, which made a
misspelling, a private name and a genuine nil indistinguishable at the call site. A
misspelled export is now named too. This deliberately breaks with the VM's
nil-for-a-missing-key convention for maps, because a silent `nil` out of `m.COUNT` is
a bad way to learn that `COUTN` was misspelled.

**4. Two modules may export the same name.** This was the worst of the six, because
nothing reported it: both modules' `shared` were one chunk global, so `a1.sa()`
reported `b1`'s state with no diagnostic anywhere.

**5. `from m import x` is a copy with or without an alias**, and so is
`from m import *`. The unaliased form used to bind *nothing*, on the reasoning that
the global was "already present (inlined)" -- true while every module shared one flat
namespace, and false as soon as they did not. That made the unaliased `from` *live*
on this backend while copying on the interpreter, which is the opposite of what the
same line does everywhere else.

**6. `mod { }` blocks are namespaced.** Two blocks declaring the same private name
no longer share it, and a block's private name is not in the enclosing file's scope.

### Still open: a module body sees names it never declared

```rak
// peek.rak
pub fn peek() { return TOKEN }
```
```rak
let TOKEN = "from-main"
import peek
dump peek.peek()      // interp: Undefined variable: TOKEN    vm: "from-main"
```

Mangling gives a module its own globals for the names it **declares**. `TOKEN` is not
one of them, so it is not in the rename map and it still resolves against the
importer's globals.

The same root cause shows from the other side inside a `mod { }` block, where the
interpreter is the stricter one -- it cannot see the enclosing file's `let`, its
functions, or its imports, while the VM resolves all three:

```rak
let TOP = 7
mod w { pub fn g() { return TOP } }    // interp: Undefined variable: TOP   vm: 7
```

**Why it is not fixed.** Closing it needs a module *scope* in the compiler: a free
name inside a module body that is neither local, nor one of its own top-level names,
nor a name it imported, nor a builtin, has to be an error. The compiler cannot make
that call, because it has no list of builtin globals -- the VM registers those at
startup -- so it cannot tell `len` (fine) from `TOKEN` (leaked). Guessing would break
every module that calls a builtin.

What it needs is a compile-time table of the names the runtime provides, which is a
real piece of work rather than a tweak. Until then, a module that references a name
it does not declare is a latent leak: the interpreter will refuse it and the VM will
find the importer's binding if there happens to be one by that name.

`rakc/tests/module_state.rs` pins this: the tests that assert a *fixed* symptom now
assert agreement, and `a_module_cannot_read_the_importers_globals` fails loudly with
"convert this to agree_on" if the divergence ever closes, so it cannot be forgotten.

### Not covered

Namespacing a module's declarations does **not** namespace the compiler-generated
globals keyed by name rather than by module: `__method_<Type>_<name>` from `impl`
blocks, `__bin_decode_<Name>` / `__bin_encode_<Name>`, and
`__enum_new_<Enum>_<Variant>`. Two modules that add a method to the same type, or
define a binstruct of the same name, still collide, and nothing reports it. That is
pre-existing and unchanged by this work; it is the same class of bug and would need
the same treatment applied to those tables.

## `import pkg.sub` requires the files on disk

**Severity: none.** Both backends resolve an import against the importing
file's directory, so a module cannot be supplied in memory. The parity test for
this writes `pkg/init.rak` and `pkg/sub.rak` to a temp directory.
