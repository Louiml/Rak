# Instrumented control-flow integrity: spike result

**Status: not achievable for Rak's release binaries with the available
toolchain. The blocker is the toolchain, not the code.**

This is the finding from attempting what `docs/V8-ROADMAP.md` and the v8.0.0
release notes describe as missing: *instrumented* CFI, where every indirect call
site is checked, as opposed to what Rak currently ships.

## What Rak ships today

`.cargo/config.toml` passes `/guard:cf` and `/CETCOMPAT` to the linker on
Windows, and full RELRO plus a non-executable stack on Linux.
`dist/verify_hardening.py` checks the result and reports:

```
[ok] Control Flow Guard (GUARD_CF)  image is CFG-compatible; call sites are NOT instrumented (rustc limitation)
```

The parenthetical is the whole point. The image is marked CFG-compatible and a
guard dispatch table is initialised with load-time checks, so an attacker cannot
overwrite that table to redirect a call — and `/CETCOMPAT` adds shadow-stack and
indirect-branch tracking. What is missing is per-call-site type checking. Rust's
codegen does not emit the instrumentation, and the linker flag cannot add it.

## What was tried

### 1. `-Zcfi`

```
$ rustc -Zcfi ...
error: unknown unstable option: `cfi`
```

This option does not exist on nightly 1.101.0. It appears never to have existed
under that name.

### 2. `-Zsanitizer=cfi`

This is the real option, and it exists. On the Windows target:

```
$ rustc +nightly -Zsanitizer=cfi ...
error: cfi sanitizer is not supported for this target
```

Then, walking the other errors it reports one at a time:

| Requirement | Error when missing |
| --- | --- |
| `-Clto` or `-Clinker-plugin-lto` | `` `-Zsanitizer=cfi` requires `-Clto` or `-Clinker-plugin-lto` `` |
| `-Ccodegen-units=1` (on top of LTO) | `` `-Zsanitizer=cfi` with `-Clto` requires `-Ccodegen-units=1` `` |
| `std` built with the same flag | `` mixing `-Zsanitizer` will cause an ABI mismatch in crate `...` `` |

So the full requirement set is:

```
nightly + -Zsanitizer=cfi -Clto -Ccodegen-units=1 -Zbuild-std (with rust-src)
```

and even then only for a target LLVM supports. `x86_64-pc-windows-msvc` is not
one of them.

### 3. Clang and lld

```
clang      -> NOT FOUND
clang-cl   -> NOT FOUND
lld-link   -> NOT FOUND
ld.lld     -> NOT FOUND
```

The documented alternative is to drive rustc's codegen from Clang so that
Clang's own `-fsanitize=cfi` can be used. No Clang is installed, and installing
one would not help on its own: the same ABI-mismatch rule applies, so `std` would
still need rebuilding with the sanitizer, which means `-Zbuild-std` and a working
C toolchain regardless.

## What it would cost if the toolchain allowed it

Worth recording, because it is the reason "just add a flag" is not the answer:

- **Nightly only.** `-Zsanitizer` is rejected outright on stable, so the release
  build would pin a nightly. Every Rust release can change `-Z` behaviour, so
  this is a build that can break without a commit.
- **Full LTO, single codegen unit.** Materially slower builds, and LTO changes
  inlining, which changes what is being instrumented in the first place.
- **`std` rebuilt from source.** `-Zbuild-std` plus the `rust-src` component,
  compiled on every CI run, for the whole dependency graph.
- **The dependency graph has to be CFI-clean.** CFI requires a type-identifier
  summary for every function pointer. Crates that are not instrumented get a
  fallback or an error, and Rak's graph includes `ring`, `wry`, `tao`, `regex`
  and `serde_json`. Each of those either needs instrumenting or an explicit
  allowlist entry.

That last point is the one that would have decided it even with a working
toolchain: CFI is only as good as the fraction of the binary that is
instrumented, and an allowlist for the uninstrumented third-party code would
have to be as carefully justified as the `unsafe` blocks Rak already audits.

## Decision

**Do not ship it, and do not pretend the current CFG is equivalent.** The
hardening verifier already says the right thing — "image is CFG-compatible; call
sites are NOT instrumented" — and the release notes carry the same caveat. That
is the honest position and it is not changing in this release.

What is worth doing instead, in rough order of value:

1. **Reduce indirect-call surface.** CFI protects indirect branches, so a
   language with fewer of them needs less of it. Rak's `Value::NativeFn` is
   `Arc<dyn Fn(&[Value]) -> ...>`, which is one indirect call per builtin
   dispatch; a `fn`-pointer table would be both faster and a smaller target.
2. **`-Zsanitizer=shadow-call-stack`** is a more realistic instrumented
   control-flow protection and does not need LTO. Untested here, but it is the
   obvious next spike.
3. Revisit when rustc ships a stable CFI path, or when the Windows target
   supports it. Until then the answer is a build-system limitation, and a build
   system limitation is not worth an unstable toolchain pin.

## Reproducing

```powershell
rustup toolchain install nightly
rustc +nightly -Zcfi --crate-type lib --emit=metadata - -      # unknown option
rustc +nightly -Zsanitizer=cfi --crate-type lib --emit=metadata - -  # not supported for target
Get-Command clang, lld-link, ld.lld                              # all absent
```

WSL has stable 1.98.1, which rejects `-Z` outright, so the nightly requirement
applies there too.
