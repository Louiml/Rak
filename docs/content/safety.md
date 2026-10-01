# Safety

This page covers what Rak does about security properties, and, just as
importantly, what it does not do. If you are auditing someone else's script,
read the second section before trusting the first.

Rak is a dynamic language with reference-counted values. It is not a
memory-safe systems language and does not claim to be. What it does provide
is a set of primitives, checks, and an audit trail that make unsafe code
visible and make the common crypto mistakes hard to write.

## What is enforced

### Unsafe blocks

Anything that touches raw memory, FFI, or a wire format should be wrapped in an
`unsafe` block with a written justification. The reason is mandatory and
enforced by the parser, not by the linter, so an unjustified exemption is a
parse error:

```rak
fn parse_frame(b) {
    unsafe "the binstruct decoder bounds-checked this length prefix on line 4" {
        let n = b[0] * 256 + b[1]
        return slice(b, 2, 2 + n)
    }
}
```

Be clear about what this is and is not. It is **not** a permission. The block
executes exactly as written: normal scoping, normal mutability enforcement,
normal error handling. There is no borrow checker in Rak to suspend, so `unsafe`
cannot grant access to something the language would otherwise forbid. What it
does is give you two things that matter for review:

- `grep unsafe` returns every exemption in a codebase, each with the author's
  own words attached. An exemption nobody had to explain is one nobody reads.
- `rakc lint --audit` prints the same list as a review artifact.

```
rakc lint tool.rak --audit
```

```
safety audit: 2 unsafe block(s) in tool.rak
   1. the binstruct decoder bounds-checked this length prefix on line 4
   2. TODO
  each of these bypasses the safety lint rules. check the justification holds,
  then check the block does not do what the justification does not cover.
```

`unsafe-thin-reason` flags a justification that is a placeholder: under 15
characters, or containing `todo`, `because`, `trust me`, `hack`, or `n/a`. A
fake justification is worse than none, because it reads as if someone had
thought about it.

The interpreter also records which `unsafe` blocks were actually *entered*
during a run, which is a different and sometimes more useful question than
which ones exist: a `ffi_call` on a branch that never runs on this input is not
a risk on this input.

### Capability sandbox

Every builtin that can reach outside the process is gated by name at two
choke points, one per backend. Nothing runs unless you ask for it.

```
rakc run untrusted.rak --sandbox --allow net,fs_write
```

Seven capabilities exist: `net`, `fs_write`, `process`, `ffi`, `raw`,
`gui`, `secrets`. With `--sandbox` and no `--allow`, all seven are denied
and only pure computation works.

Two details worth knowing. Packet *builders* are not gated, because they only
construct a `bytes` value and open no socket; `net_raw_send` and
`net_raw_recv` are what need `raw`. And file *reads* are not gated, because
imports would stop working. The reasoning is that anything able to exfiltrate
data is denied, so a read alone cannot do damage.

The limitation is real: the sandbox is a process-wide filter set once from the
command line. A module cannot narrow its own privileges, and an imported
package runs with whatever the invoker allowed. Per-module capability
declarations are on the roadmap and are not implemented.

### Static analysis

`rakc lint` has style rules and security rules. The security ones exist
because string literals used to be invisible to the linter, so nothing could
check them.

| Rule | What it catches |
|---|---|
| `hardcoded-secret` | A literal assigned to a name containing `secret`, `password`, `token`, `api_key`, `private_key` and similar. Also known vendor token formats (`sk-`, `ghp_`, `AKIA`, `xoxb-`) and key-length hex. |
| `plaintext-url` | An `http://` URL that is not localhost. |
| `weak-crypto` | `md5`, `sha1`, `rot13`, `xor`, or `file_hash(..., "md5")`. |
| `secret-compare` | `==` between secret-looking values. See constant-time below. |
| `ffi-raw-pointer` | `ffi_ptr`, `ffi_read`, `ffi_write`, `ffi_alloc`, `ffi_free`, `ffi_call`. |
| `insecure-transport` | `ws_connect` and `net_connect`, which carry no TLS. |
| `unsafe-thin-reason` | An `unsafe` block whose justification is a placeholder. |

All are advisory. `--deny` makes them exit non-zero for CI:

```
rakc lint tool.rak --deny
```

Detection is heuristic. A credential assembled at runtime will not be found,
and a long path or SQL fragment will not be flagged as a secret.

### Release hardening

`Cargo.toml` sets `lto = "fat"`, `codegen-units = 1`,
`overflow-checks = true`, `panic = "abort"`, and `strip = "symbols"` for
release builds. The arithmetic one matters most: Cargo's default is
`overflow-checks = false` in release, which silently wraps and turns a length
calculation into a heap overflow. Keep it on.

`.cargo/config.toml` adds target-specific linker flags. On MSVC:
`/guard:cf`, `/CETCOMPAT`, `/DYNAMICBASE`, `/HIGHENTROPYVA`, `/NXCOMPAT`. On
Linux: full RELRO, non-executable stack, a stack canary, and forced frame
pointers.

Be precise about what Control Flow Guard does and does not do here. `/guard:cf`
marks the PE CFG-compatible and initialises the guard dispatch table with
load-time validation, which blocks the attack that overwrites that table. It
does **not** mean every indirect call is instrumented: rustc does not emit CFG
check calls for Rust code, the way Clang's `-fsanitize=cfi` instruments C and
Rust. An attacker who gains execution can still redirect an uninstrumented
indirect call. `/CETCOMPAT` has the same caveat. Neither is a substitute for
compiling the C parts of a program with an instrumenting compiler and linking
that object in, which is what `rakc bindgen` exists to help with.

Because a linker flag is only a claim, `dist/verify_hardening.py` reads the
produced binary back and asserts the bits are really there, and both release
jobs run it. A typo like passing `/NXCOMPAT` and `/NXCOMPAT:NO` together, which
silently disables DEP, is caught that way.

```
python dist/verify_hardening.py --self-test
python dist/verify_hardening.py target/release/rakc.exe
```

## Crypto

All crypto is pure Rust. Nothing calls OpenSSL or libcrypto. That is a real
property, not a claim: the `ffi` capability exists for your own
`extern "C"` declarations and is never used by the crypto path.

| | |
|---|---|
| Symmetric | AES-256-GCM, ChaCha20-Poly1305 |
| Hashing | SHA-256, SHA-1, MD5 (weak, lint-flagged) |
| MAC | HMAC-SHA256, HKDF-SHA256, PBKDF2-HMAC-SHA256 |
| Signatures | Ed25519, ECDSA P-256, RSA PKCS#1 v1.5 with SHA-256 |
| Key agreement | X25519 |
| Key transport | RSA-OAEP with SHA-256 |

RSA and ECDSA keys are DER, so they interoperate with OpenSSL for reading and
writing, but no OpenSSL code is linked into the binary.

### Constant-time comparison

`==` on a Rak value is a data-dependent branch. It returns as soon as it finds
a differing byte, so the wall-clock time reveals how much of a prefix two
values share. Against an attacker who can time your comparison, that is enough
to recover a MAC or a session token one byte at a time.

Use these instead:

```
dump ct_eq(got, expected)        // bytes
dump ct_eq_hex(got, expected)    // hex, case-insensitive
let masked = ct_select(choice, a, b)   // branchless select
```

`rakc lint` flags `==` between secret-looking names, but it cannot see that a
value came from a computation.

### Zeroization

`Vec::clear` and `drop` free memory without overwriting it, so a key in a heap
buffer survives in freed pages until something reuses them.

- `zeroize(bytes)` overwrites a buffer through a volatile path the optimizer
  cannot remove as a dead store, and returns it.
- `secret_delete(name)` wipes the value from memory and overwrites the store
  file with zeroes before rewriting it.
- `secret_delete_all()` does the same for the whole store.

What this does not cover: `secret_get` returns a `String` by value, so your
copy is yours to wipe. A process that is killed mid-write can still leave
plaintext in swap or a core dump.

## Packet forging

Pure builders, available inside a sandbox because they run no code and open
no socket. All are cross-platform; only `net_raw_send` / `net_raw_recv` are
unix-only, since they need `CAP_NET_RAW`.

```
net_raw_ipv4(src, dst, proto, payload)
net_raw_tcp(src, dst, sport, dport, flags, seq, ack, payload)
net_raw_udp(src, dst, sport, dport, payload)
net_raw_tcp_syn(src, dst, sport, dport)
net_raw_icmp(src, dst, id, seq, payload)          // echo request
net_raw_icmp_ping(src, dst, id, seq)
net_raw_icmp_echo_reply(src, dst, id, seq, payload)
net_raw_arp_request(src_mac, src_ip, target_ip)
net_raw_arp_reply(src_mac, src_ip, target_mac, target_ip)
net_raw_arp_parse(frame) -> map | nil
net_raw_csum(bytes) -> int
```

`net_raw_arp_parse` accepts a frame with or without the Ethernet header and
returns `nil` on anything that is not ARP, so you can feed it raw capture bytes
without filtering first. It returns a map with `opcode`, `sender_mac`,
`sender_ip`, `target_mac`, and `target_ip`.

For declaration-heavy formats, `binstruct` gives you pack and unpack with
bitfields and length-prefixed arrays. See
[Forensic structs](forensics.html).

## Fuzzing

`rakc fuzz` runs a deterministic mutation loop against the lexer, parser,
interpreter, and the stdlib parsers. It is on the stable toolchain, so it runs
in ordinary CI and on a Windows dev box.

```
rakc fuzz all                        # 20000 runs per target
rakc fuzz parse --runs 100000
rakc fuzz dns --seed 0xC0FFEE        # reproducible
rakc fuzz --list
```

Targets: `lex`, `parse`, `eval`, `dns`, `tls`, `json`, `websocket`, `tunnel`,
`netraw`, `csv`, `gzip`, `zip`, `all`. A crashing input is written to
`fuzz-crash-<target>.bin`, and the printed `--seed` replays the run.

Run it from a debug build. The harness uses `catch_unwind`, and the release
profile sets `panic = "abort"`.

This is not coverage-guided. The `fuzz/` directory has 13 `cargo-fuzz` targets
that are, but they need nightly and `-fsanitize=fuzzer`, which MSVC cannot
provide. Use `rakc fuzz` as the always-on regression net and cargo-fuzz when
you have a Linux or WSL box for a long run.

## What Rak does not do

These are not oversights to be patched later. Each one is a different order of
magnitude of work, and pretending otherwise would be dishonest.

**No ownership or borrowing.** There is no `&` reference type; `&` is
bitwise-and. Values are shared and copied rather than moved, and there is no
lifetime tracking, so a use-after-free in Rak is a bug in the runtime or in an
FFI call, not in your source. Adding borrow checking means rewriting the type
checker and every value-cloning site across a 7,600-line interpreter.

What exists instead: `unsafe` blocks that mark and justify every exemption,
runtime `let mut` enforcement, a capability sandbox that keeps FFI behind a
switch, and a lint rule that names the unchecked-memory builtins. The realistic
path to real memory safety is a Rust or Zig FFI target with a narrow API, not a
borrow checker bolted onto a dynamic language.

**No instrumented control-flow integrity.** The binary is CFG-compatible and
carries a guarded dispatch table, full RELRO, ASLR, and DEP, all verified by
`dist/verify_hardening.py`. What is missing is instrumentation: rustc does not
emit CFG check calls for Rust code the way Clang's `-fsanitize=cfi` does for C.
So the dispatch table cannot be hijacked, but an uninstrumented indirect call
can still be redirected once you have execution. Closing that needs an LLVM
backend, and the `inkwell` dependency in `Cargo.toml` is commented out.

**No inline assembly.** `extern "C"` FFI is the escape hatch, which is
functionally comparable but not the same thing, and it is capability-gated.

**No formal verification.** No theorem prover, no contracts, no loop
invariants, no pre/postconditions. The closest thing is match-exhaustiveness
checking in `rakc check`.

**No enclaves.** No SGX, no TrustZone, no seccomp or landlock filtering. The
capability sandbox is name-based and in-process; it does not confine the
process from the kernel.

**No event loop.** Concurrency is thread-per-task with a counting semaphore
sized at `cpus * 16`. `task_group` is the well-behaved path. The HTTP server
handles connections serially, so one slow client blocks the rest. Thousands of
concurrent connections needs epoll/kqueue/io_uring, which is a runtime rework.

`docs/V8-ROADMAP.md` has the reasoning and cost for each of these.
