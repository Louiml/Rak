# v8.0.0 roadmap

Current version everywhere is `0.7.2` (`Cargo.toml`, `rakc/src/main.rs:6`,
`vscode-rak/package.json:5`, `ide/src-tauri/Cargo.toml`). Ten commits have
already landed parts of what the spec calls "v0.8" without touching that
number, so the tree is functionally mid-0.8 while still advertising 0.7.2.

`docs/rak-features-spec.md` Part 7 is the only forward plan in the repo. This
document sorts it into "specified but missing" and "worth adding", and both
are checked against the source rather than trusted from the status markers.
Several markers are stale in both directions: things marked `[SPEC]` are
already shipped, and shipped items are missing from the docs.

## Where the code stands

Two backends behind one frontend. `lexer.rs` and `parser.rs` feed `ast.rs`,
then `typecheck.rs` reads the AST and the two backends diverge:
`interpreter.rs` is a tree walker (7605 lines, its own `Value` enum),
`compiler.rs` lowers to `bytecode.rs` and `vm.rs` runs it (3664 lines, a second
`Value` enum in `value.rs`). Every language feature needs implementing twice.
That dual-backend tax is the root cause of nearly everything still listed as
`[SPEC]` below.

Roughly 279 unit tests in `rakc/src`, 52 in `stdlib`, 8 proptest suites, 13
`cargo-fuzz` targets in a separate nightly-only crate.

## Missing: already in the spec, not in the code

### 7A.4, 7A.5, 7A.6 backend parity (the big three)

| Item | Current state |
|---|---|
| VM streams | `value.rs` has no `Stream` variant. `vm.md:41` says "VM no". |
| VM `tunnel` / `udp_*` | `compiler.rs:915` returns "VM does not support 'tunnel' statement". |
| VM `import pkg.sub` | `inline_module` has no dotted resolution. `vm.md:37` confirms. |

These three are the reason the two backends still disagree, and they are the
reason half the examples only run on one of them. `ext_batteries.rs` and
`ext_osint.rs` already show the pattern that makes this tractable: one
`try_interp` for the tree walker, one `vm_natives()` table for the VM. Streams,
`UdpTransport` and nested modules should all follow it rather than growing
new opcodes.

Also still interpreter-only, per the coverage matrix: WebSocket and the
`argv()` / `parse_args` CLI entry point (`vm.md:42`, `vm.md:44`).

### 7A.9 checked generics

`typecheck.rs:225` and `typecheck.rs:226` blanket-accept any generic:
`(Generic(_), _) => true` and `(_, Generic(_)) => true`. Generic params parse
and struct/enum `type_params` are carried in the AST, but nothing unifies a
call site's argument type against a parameter type, so
`fn identity<T>(v: T) -> T` called as `identity("x")` type-checks fine. The
checker treats generics as an escape hatch, not as types.

### 7A.10 lazy `Iterator` trait

`zip`, `enumerate`, `fold`, `reduce`, `any`, `all`, `flat_map`, `take_while`,
`skip` and `for (k, v) in map` all shipped on both backends. The `Iterator`
protocol itself is still not a thing: `for` desugars to eager iteration over
arrays and strings, so a user type with a `next(self) -> Option` method gets
no lazy behavior. Either land the trait or amend the marker; right now the
marker hedges with "lazy Iterator trait pending" and the docs repeat that.

### 7A.11 sets

Raw strings (`lexer.rs:142`), triple-quoted strings (`lexer.rs:137`),
`if let` / `while let`, default params, named args, varargs and labeled loops
all shipped. Sets did not. There is no `Value::Set` in either backend, no
`set_of` / `set_add` / `set_has` / `set_union` / `set_intersect` / `set_diff`,
and no set literal syntax. This is the one real hole in the syntax pack and
the cheapest to close, since `Map` already exists in both backends with
insertion-order iteration.

### 7A.8 binding patterns, found while auditing the markers

The spec marks 7A.8 `[SHIPPED]` and shows `x @ [0x89, 'P', ..]`. That syntax
does not parse. `Pattern::Bind` appears nowhere outside the spec text, there
is no `At` token in the lexer, and the parser has no `@` handling. Struct and
enum patterns *are* real (they lower to `Op::MatchPat` descriptors), and match
guards are real and predate this pass. Only the binding pattern is missing.

Implementing it means a new token, a new `Pattern` variant, a binding slot in
`compiler.rs::pattern_descriptor`, and the corresponding case in the VM
matcher. The related open question is whether `Pattern::Struct`'s declared
shape, `Vec<(String, Pattern)>`, permits the `Point { x, y }` field-shorthand
form the spec advertises, or only the explicit `Point { x: x, y: y }`.

### 7B type system and tooling

- `rakc check` v2. `typecheck.rs` has no function-signature table, no
  inference through function bodies, no `?`-propagation check (verify the
  return type admits `Err`), and no cross-module cache. It is a local,
  single-pass checker today.
- `rakc doc` and `///` doc comments. No doc token in the lexer, no doc
  collection on AST items, no command in `main.rs`. LSP hover cannot show
  docs because there are none.
- Bytecode cache. `rakc vm file.rak --cache` does not exist. `Chunk` is
  already a clean serializable shape (`code`, `constants`, `lines`), so a
  hash-validated `.rakc` file is mostly serde glue.
- Tail-call optimization. Nothing in `interpreter.rs` or `vm.rs`. Deep Rak
  recursion overflows the Rust stack. Given the language already ships
  `spawn` and `async`, a stack overflow killing a whole process is the worst
  failure mode available.

### 7C OSINT pack

- `binstruct` v2 remainder. Bitfields and `repeat` (length-prefixed arrays,
  `ast.rs:304`) are done. Conditional fields (`field: type if <expr>`) and
  enum-discriminant tables are not.
- `#[track_evidence]`. `cite` and `evidence<T>` work as explicit calls; the
  opt-in attribute does not exist.
- `tls_inspect`. Named in the capability table at `caps.rs:85` but no
  implementation anywhere in `rakc` or `stdlib`. It parses a ClientHello; it
  does not complete a handshake or fetch a chain.
- `pcap_listen`. Named at `caps.rs:92`. `stdlib/src/pcap.rs` is a
  feature-gated stub that errors unless built with `--features pcap`.
- `net_raw_icmp_ping`, `net_raw_arp_request`, `arp_scan`. Absent.
  `caps.rs:92` lists an older `net_raw_icmp` name, and
  `stdlib/src/net_raw.rs:159` documents that I/O is unix-only.
- Windows raw sockets via Npcap's `PacketSendPackets` / `PacketReceivePacket`.
  Not implemented.
- macOS CI. `.github/workflows/release.yml` builds ubuntu and windows only.

## New features to add

Ordered by what I think v8.0.0 should actually spend its budget on.

### Make the VM the default backend

The interpreter and the VM are not peers. The VM is faster, has the debugger
and the DAP server, and is where a bytecode cache and TCO would land. The
interpreter is the reference implementation people read.

Ship the remaining parity gaps, promote `rakc run` to the VM, and keep the
tree walker behind `rakc run --interp` as a conformance oracle. Every later
feature then only needs implementing once in production, with the interpreter
retained for tests. Doing this after adding more features means paying the
dual tax on each of them instead.

### Real IDE-grade language server work

`lsp.rs` is 272 lines with completion, hover, goto-definition and diagnostics,
plus a hard-coded keyword list, type list and roughly 200-entry builtin
table that has not been updated for the batteries, OSINT or iterator builtins
added in the last ten commits. The VS Code extension is TextMate-only and
tells users to wire an LSP client by hand.

Add rename, references, document symbols, semantic tokens, inlay hints and
code actions. Add a workspace index so a single server handles a package
rather than one file at a time. Ship an LSP client inside the extension so
`vscode-rak` works on install. This is the highest-leverage tooling work
available and it is unblocked by everything else.

### Async that does not leak

`async` exists with deferred bodies, `await`, `spawn` and channels, but
`async fn` runs synchronously on the VM (`vm.md:33`) and there is no
structured concurrency. Add `task { }` scopes that join on exit, `?` inside
`async fn`, `join` and `select` combinators, and `await for` over async
iterators. A spawned task that outlives the value it borrowed is currently a
silent use-after-free, which is a much worse bug than a compile error.

### `let ... else`, trait default bodies, `dyn`/`impl Trait`

Small, high-frequency ergonomics. `let Some(v) = opt else { return }` and
slice patterns with ranges in `match` cover common shapes people currently
write by hand. Trait methods with default bodies need a real trait protocol
on the VM (currently `vm.md:22` says traits are interpreter-only).
`dyn Trait` and `impl Trait` in argument position make the type checker
useful on real code instead of only catching `let x: int = "s"`.

### `unsafe` with an audit trail

The language is for security work and already has `--sandbox` with
capability gating in `caps.rs`. Add an `unsafe { }` block marker that requires
a reason string, greps in CI, and gets recorded into `evidence` provenance
chains. It fits the existing security posture, and it gives the `report`
builtin something honest to print.

### Deterministic evidence

`evidence<T>` and `Provenance` already track tool, target, timestamp, byte
offsets and parent. Two additions make that useful for real work. Hash the
provenance chain so a report is reproducible and tamper-evident, and
content-address raw artifacts so a report can be re-verified against the
bytes it was derived from. `report` currently emits Markdown and stops there.

### Library gaps an OSINT user hits fast

- PostgreSQL and Redis wire protocol clients. A SQL engine exists as an
  example, not as a library.
- Full YAML. `stdlib/src/datafmt.rs` is a documented subset, which is a trap
  for anyone who tries to parse a Kubernetes manifest with it.
- PCAP writing. Reading works; there is no way to produce a capture.
- ASN.1 / DER beyond the existing X.509 path.
- EXIF and image header parsing.
- CBOR and MessagePack. `serde_json` covers JSON; most binary protocol work
  needs something else.
- A non-backtracking regex option. The engine is native, and a script that
  runs against attacker-controlled input can be handed a ReDoS pattern.
- SOCKS5 and SSH clients.
- MaxMind DB lookups for ASN and geo enrichment, which is the first thing
  anyone writes after `whois` and `ct_subdomains`.

### Build and release

- A `CHANGELOG.md`. There is none, which makes the 0.7.x line hard to follow.
- A published MSRV and a documented semver policy for `rakc` and `rak-stdlib`.
- `cargo test` and `cargo audit` in CI as blocking jobs. Fuzzing exists but
  runs nowhere automatically; `fuzz/README.md` calls any crash a
  release-blocker, which is only true if something runs it.
- `rakc watch` and `rakc profile`. `Chunk.lines` already maps source lines to
  bytecode offsets, so a sampling profiler is mostly bookkeeping.
- `rakc coverage` from the test runner.
- A real `-O` pass. There is no optimization anywhere, so `bench` measures
  the same work twice.

## Housekeeping

Small, but each one misleads someone.

- `rakc fmt` and `rakc lint` are implemented and wired into `main.rs:562`
  and `main.rs:593` but missing from `print_usage()`.
- The REPL banner says v0.3.0 (`repl.rs:4`).
- `rakpkg/src/main.rs:7` hardcodes `0.7.0`.
- `docs/content/vm.md` rows for binary pattern matching, trait protocols,
  method-call dispatch and enum patterns are all marked "no" and are all
  shipped. Same table, opposite drift, further down.
- `chumsky` is a declared dependency of `rakc` and the parser is hand
  written. Drop it or use it.
- `raklib/` was an empty untracked directory. Removed.
- `.gitignore` already covers `*.exe`, `ide/.next/`, `ide/out/` and
  `*.tsbuildinfo`; nothing large is committed. The largest tracked file is
  `rakc/src/interpreter.rs` at 360 KB. No action needed.
- `adblocker/` is a nested git repository with its own `.git`, so it shows up
  as untracked in the parent. Its own README says it is deliberately local and
  unpublished. Nothing references it from the main docs, so a fresh clone has
  no `adblocker/`. Worth a decision at some point: either commit it as an
  example or leave the folder out of the repo entirely.

## Suggested cut line

If 8.0.0 has to ship on a schedule, this is the split I would use.

Must ship, because they are promised in Part 7 and the version says 0.8.0:
sets, checked generics, the three backend parity gaps, `rakc check` v2,
`rakc doc`, TCO, the bytecode cache, and the macOS CI matrix. Update the
stale markers in `vm.md` and the spec.

Should ship, because they are the actual 8.0.0 story: promote the VM to
default behind the three parity gaps, then the language server work, `let ...
else`, structured concurrency, and the `CHANGELOG`.

Defer to 8.1: `unsafe` blocks, deterministic evidence hashing, and the
library expansion. None of them are urgent, and all three are large.

## Security features Rak does not have, and why

A request asked for six categories of security capability. Four are now
substantially covered (see `docs/content/safety.md`). Five items in that
request are not things Rak can add without a redesign, and pretending
otherwise would be worse than saying so. Each is recorded here with what the
realistic substitute is and what the work would actually be.

### Ownership and borrowing

**Not a missing feature, a language redesign.** There is no `&` reference type:
`lexer.rs:174` tokenises `&` and `parser.rs:1861` lowers it to
`BinOp::BitAnd`. The only pointer type is `ast.rs:403`'s `Type::Ptr`, which
exists solely for C FFI and carries no lifetime. `typecheck.rs:622` returns
`false` from `is_immutable_name` and explicitly leaves mutability to the
backends; there is no use/def analysis, no alias tracking, no `move` keyword.

Values are clone-on-use rather than move-on-use throughout a 7,605-line
interpreter, and containers are copy-on-write. Adding borrow checking means
rewriting the type checker and auditing every clone site, then breaking every
program that relies on the current sharing semantics. That is a language with a
different name, not a version bump.

What exists instead: `unsafe "reason" { ... }` blocks, shipped in 8.0.0. The
parser requires the reason, so an unjustified exemption is a parse error rather
than a warning, and `rakc lint --audit` prints every exemption with its
justification as a review artifact. `grep unsafe` therefore returns a list a
human can actually read. Also runtime `let mut` enforcement at
`interpreter.rs:617` and five compile-time sites in `compiler.rs`, a capability
sandbox that keeps FFI behind a switch, and the `ffi-raw-pointer` lint rule.
The realistic path to real memory safety is a Rust or Zig FFI target with a
narrow API, not a borrow checker bolted onto a dynamic language.

### Control-flow integrity

Real CFI means instrumenting every indirect call site, which requires an LLVM
backend. The `inkwell` dependency in `Cargo.toml:30` is commented out, and
`rakc` emits bytecode for its own VM rather than machine code, so there is
nowhere to insert a check.

What landed in 8.0.0 instead, in two parts.

Compiler and profile: a release profile with `lto = "fat"`,
`codegen-units = 1`, `panic = "abort"`, and `strip = "symbols"`. The one that
matters most is `overflow-checks = true`, because Cargo's default in release is
`false`, which silently wraps and turns a length calculation into a heap
overflow.

Linker: `.cargo/config.toml` adds `/guard:cf`, `/CETCOMPAT`, `/DYNAMICBASE`,
`/HIGHENTROPY_VA`, `/NXCOMPAT` on MSVC, and full RELRO, a non-executable stack,
a stack canary, and forced frame pointers on Linux.

State the limit honestly, because the flags are easy to oversell. `/guard:cf`
marks the PE CFG-compatible and initialises the guard dispatch table with
load-time validation, which blocks the attack that overwrites that table. It
does not instrument Rust's own indirect calls, so an attacker who gains
execution can still redirect one. Verified empirically: the built `rakc.exe`
has `DllCharacteristics = 0xC160` (GUARD_CF, DYNAMICBASE, HIGH_ENTROPY_VA,
NX_COMPAT all set) and a 0x140-byte load config, but contains no
`__guard_check_icall`, because there is nothing instrumented to check.

Because a linker flag is only a claim, `dist/verify_hardening.py` reads the
produced PE or ELF back and asserts the bits are really set. It has a
`--self-test` that exercises both parsers on synthetic headers, positive and
negative, since a verifier that silently always passes is worse than none. Both
release jobs run it. This already earned its place: the first version of the
flags passed both `/NXCOMPAT` and `/NXCOMPAT:NO`, which silently disabled DEP,
and the PE read-back caught it.

### Inline assembly

No stable-Rust path exists. `asm!` is nightly-gated and rejected in a crate
that must build on `stable-x86_64-pc-windows-msvc` in CI.

What exists: `extern "C"` FFI (`ast.rs:255`, `stdlib/src/ffi.rs`) with up to
eight integer or pointer arguments, which reaches the same hardware. It is
gated behind the `ffi` capability and flagged by the `ffi-raw-pointer` lint
rule, so it is at least findable during review.

### Formal verification

No prover, no contracts, no loop invariants, no pre/postconditions, no
refinement types. Grepping `rakc` for `coq`, `lean`, `isabelle`, `proof`,
`verify`, `smt` returns only the English word "requires" in error strings.
`ast.rs:377`'s `Type` enum has no index or level kinds, so dependent typing is
not a parser change away.

What exists: match-exhaustiveness checking in `typecheck.rs:486` (emits `W0001`
for a missing enum variant, `E0223` for an unreachable arm), and the
`evidence<T>` provenance chain, which records where a value came from without
proving anything about it. If someone wants machine-checked properties, the
honest route is exporting to a language with a prover, not growing one here.

### Enclaves

SGX and TrustZone need an OS, a driver story, and a threat model that a hobby
compiler cannot meaningfully own. Even the realistic substitute, installing
seccomp-bpf or Landlock filters at process start, is not present: grepping both
crates for `sgx`, `trustzone`, `wasm`, `seccomp`, `landlock`, `isolate`
returns nothing.

What exists: the capability sandbox in `caps.rs`, which is name-based,
in-process, and set once from CLI flags. It stops a script from calling
network builtins, but it does not confine the process from the kernel, and a
native library loaded through `ffi` is outside its view entirely. The honest
framing for `docs/content/safety.md` is that this is defence in depth for
Rak-level code, not a malware-analysis sandbox. If that use case matters, run
the script under a container or a VM.
