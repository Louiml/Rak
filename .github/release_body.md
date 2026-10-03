## 0.8.4 — 2026-10-03

Security and performance release. This one should be read before upgrading, not
after.

### Security

**FFI pointer provenance — an arbitrary write anywhere in the process is closed.**

`ffi_write` and `ffi_read` did this:

```rust
unsafe { *((ptr as usize + off) as *mut u8) = byte }
```

with no check that `ptr` was ever allocated, and none that `off` was inside it.
Because `ffi_ptr(n)` builds a pointer from *any* integer,
`ffi_write(ffi_ptr(ADDRESS), 0, 0x41)` could write a byte anywhere the process
could reach. `ffi_read` was the matching read. This is the most severe item in the
security review, and unlike most of that list it was completely real.

A pointer is now *provenanced* if Rak allocated it (`ffi_alloc`,
`ffi_string_to_cstr`) or was told it owns a region (`ffi_trust`). Provenanced
pointers are range-checked on every access against the size recorded at allocation.
A pointer that is neither is refused by name, and the error says what to do about
it.

`ffi_trust` is the deliberate escape hatch. A language with FFI that refused every
foreign address would be useless, and resolving a symbol's address and then calling
it is legitimate work. What is not acceptable is an address that is *silently*
accepted, because that is indistinguishable from a safety check that always passes.
`ffi_trust` turns unchecked pointer arithmetic into an assertion written down in the
source — the same bargain `unsafe { reason }` makes everywhere else. It also
narrows: trusting 4 bytes of a 16-byte allocation makes an access at offset 4 fail
again, which is tested.

`ffi_cstr_to_string` was a denial of service by another route — it scanned for a NUL
byte with no bound, so a pointer to a NUL-free buffer walked off the end into
unmapped memory, reachable from a pointer that came out of a network response. It
is now bounded at 1 MiB.

`ffi_read_i32` checks all four bytes, not merely that `off` is inside: `off` being
valid while the read runs three bytes past the end is still a read past the end.

The bounds arithmetic is done in `u128`, not `u64`, on purpose. `ptr + off` in `u64`
wraps, and a bounds check a large offset can defeat is not a check. There is a test
that specifically tries to defeat it.

Both backends share `rak_stdlib::ffi::Allocations` so the logic lives in one tested
place. `ffi_trust` is covered by the existing `ffi_` capability prefix, so it needs
no new sandbox entry.

Existing `ffi_alloc`/`ffi_free` and legitimate access are unchanged and tested.

### Performance

**Constant folding.** `compile_expr` had none. Every `2 * 3` became two `LoadConst`s,
an `AddI` and a push, on every evaluation — inside the innermost loop of every
numeric program. `fold_int_binary` now folds the literal-on-literal case for the
arithmetic and bitwise operators, and is deliberately narrow: both operands must be
literals; division or remainder by zero, out-of-range shift counts, and overflow all
fold to *nothing* so the runtime behaviour is unchanged.

**`rakc bench` now measures something.** The old version timed
`rakc::eval(&source)` against `vm.run()` and nothing else. That is not a backend
comparison: the interpreter figure included lexing, parsing and setup, the VM figure
was bytecode execution on an already-compiled chunk, and compile time was attributed
to nobody. It also used `as_millis()`, so anything under a millisecond printed
`0 ms`.

It now reports five phases — lex, parse, compile, interp, vm — each over `--repeat`
samples (default 5) after an unmeasured warm-up, as a median. The two execution
numbers are finally like for like, and the output labels them "execution only" so
they cannot be misread. On `examples/bench.rak` in a debug build it reports 6.61x,
which is a measured number where the README previously had a hand-written estimate.

**A benchmark corpus.** `examples/bench/` has six workloads covering recursion,
arithmetic, arrays, map fields, strings and bytes.

### Fixes

Three backend message divergences, all the same defect — the VM and the interpreter
described the same failure differently, so an error message told you which backend
you were on:

| expression | interpreter | VM (before) |
|---|---|---|
| `5 % 0` | `Division by zero` | `rem by zero` |
| `5 / 0` | `Division by zero` | `div by zero` |
| `n * x` | `Undefined variable: x` | `Undefined: x` |

The first two were Rust's internal panic wording copied into hand-written `Err`
strings. The interpreter was itself inconsistent on the third (`Undefined variable:`
in two places, `Undefined:` in a third); both backends are now normalised to the
clearer form.

### Upgrade notes

If you use FFI, you may need to add `ffi_trust` calls. Any `ffi_write`/`ffi_read`
through a pointer that did not come from `ffi_alloc` or `ffi_string_to_cstr` will now
fail at the point of use rather than corrupting memory silently — which is the
intended behaviour, but it is a behavioural change.

Release notes are generated from `.github/release_body.md` at tag time.