# Security policy

## What this document is

Rak is a cybersecurity language: it exists to analyse hostile input, forge packets
and drive network tools. The interesting security question is therefore not "is it
safe to run untrusted Rak code" — that is never true of a language with `unsafe` and
raw sockets — but **"when I point it at someone else's script, what will it do to
me?"**

This document answers that. `docs/content/safety.md` covers the same ground from the
programmer's side.

## Reporting a vulnerability

Open a private security advisory on the repository rather than a public issue. Please
include the script or input that reproduces it and what you expected instead.

There is no bounty and no SLA. Reported issues are fixed in the order they are
understood, not the order they arrive.

## What is enforced, and where

### At parse time — cannot be bypassed

| | |
| --- | --- |
| `unsafe { reason }` | The justification is mandatory. An `unsafe` block without one is a **parse error**, so this is the parser enforcing it, not a linter that can be skipped. |

### At runtime

| | |
| --- | --- |
| Capability sandbox | `--sandbox --allow <list>` restricts `net`, `fs_read`, `fs_write`, `process`, `ffi`, `raw`, `gui`, `secrets` and `asm`. Run untrusted code under `--sandbox` with the narrowest list that lets it work. |
| Filesystem scope | `--fs-root <dir>` confines the script to one directory. The check happens at the layer that opens files, so a builtin added tomorrow is scoped by construction rather than by remembering to call something. |
| `ct_eq` / `ct_eq_hex` | Constant-time comparison, for anything derived from a credential. `==` leaks the common-prefix length through exit timing. |
| `zeroize` | Non-optimizable memory wiping. The secrets store wipes its serialized buffer and shreds its backing file. |
| Bounded decompression | `gunzip`, `inflate`, `zip_extract` and `zip_read` refuse output over 100 MiB. A deflate stream can expand a thousandfold, so this bounds a remote denial of service. The bound is enforced *while* reading, because the allocation is what does the damage. |
| Bounded parsing | The parser refuses expression nesting past 64 levels, block nesting past 64 levels, and pointer-type nesting past 64 levels. A pathological input is a diagnostic, not a stack overflow. |
| SQL escaping is dialect-dependent | `sql_escape` is for ANSI SQL, PostgreSQL and SQLite, where a backslash is an ordinary character inside a literal. `sql_escape_mysql` is for MySQL's default `sql_mode`, where it is not. Using the wrong one does not fail loudly, so the lint rule names both rather than one. A parameterised query is better than either. |
| Weak-crypto warning | `md5`, `sha1`, `rot13` and `xor` print a deprecation warning on stderr, once per primitive. Deprecated, not removed: analysing a legacy protocol means calling them on purpose. |

### Statically, by `rakc lint`

Advisory by default (exit 0); `--deny` exits 1 for CI.

| Rule | Flags |
| --- | --- |
| `hardcoded-secret` | a literal assigned to a credential-shaped name, or matching a known token format (AWS `AKIA`, GitHub `ghp_`, Slack `xoxb-`, …) |
| `plaintext-url` | an `http://` URL |
| `weak-crypto` | md5, sha1, rot13, xor used as if secure |
| `secret-compare` | `==` between secret-looking values |
| `ffi-raw-pointer` | unchecked raw-pointer FFI |
| `insecure-transport` | plain TCP or WebSocket with no TLS |
| `unescaped-interpolation` | an f-string building a SQL or shell command around a value no `*_escape` wraps |

`unsafe-thin-reason` rejects a placeholder justification ("TODO", "trust me", …).
`rakc lint --audit` lists every `unsafe` block in a file with its reason, which is
the thing to read when reviewing a script you did not write.

## Running someone else's script

```console
rakc lint their_script.rak --deny      # read this first
rakc run their_script.rak --sandbox --allow net
```

Start with the narrowest `--allow` list. If the script needs `ffi` or `raw`, that is
a finding in itself: those capabilities reach arbitrary memory and the network stack
respectively, and a script that needs them is doing something you should read.

Read the `--audit` output before running anything with `unsafe` blocks in it. The
reason field is where a script says what it thinks it is exempt from, and reading it
costs one command.

For a package rather than a script, `oyvey run` sandboxes by default, granting
`fs_read` and nothing else:

```console
oyvey run                       # sandboxed: fs_read only
oyvey run --allow net           # add one capability
oyvey run --no-sandbox          # opt out entirely
```

`--allow` replaces the list rather than adding to it, so `--allow net` means
"net and nothing else", not "net on top of fs_read".

## Known gaps

Stated plainly, because a security document that only lists strengths is not useful.

* **`asm` is a capability, and it is not an assembler.** It was previously
  described here as an escape that the sandbox did not mediate. That is no
  longer true: `asm` is gated by its own `--allow asm` capability, and it
  reaches a fixed allow-list of read-only CPU queries (`cpuid_sse2`,
  `cpuid_aes`, `cpuid_rdrand`, `rdtsc`, `rdtscp_aux`, `tsc_invariant_hz`).
  There is no assembler behind it and no hand-written machine code, so there
  is no arbitrary code execution to gate. It is interpreter-only by design,
  so the VM has no `asm` surface at all.
* **A module body can see a name it never declared.** A Rak module that references
  an undefined name may resolve it against the importer's globals on the VM, where
  the interpreter refuses. See `docs/V8-KNOWN-ISSUES.md`. One root cause: the
  compiler has no table of builtin globals, so it cannot tell `len` from a leaked
  name.
* **Raw sockets are gated by capability, not by intent.** `net_raw_send` under
  `--allow raw` can spoof. There is no per-call-site justification the way `unsafe`
  has.
* **No DNSSEC validation** on `dns_query`, and no certificate pinning API.
* **`secret_set` has no expiry.** A secret lives until it is deleted or the process
  ends. `secret_rotate` is deliberately *not* implemented as a TTL: rotating a
  credential correctly means re-fetching it at the consumer, and a timer alone gives
  the appearance of rotation while a long-lived handle keeps serving the old value.
* **Parser size limits are not yet uniform.** `dns_query` and the PCAP reader have no
  per-message cap, and `binstruct` nesting is not depth-bounded.
* **The package manager's lockfile is not yet an integrity anchor.** A dependency's
  checksum in `oyvey.lock` is computed from the bytes that were just downloaded, so
  it detects local tampering but not a compromised upstream. The lockfile also pins
  a tag rather than a commit, so a force-moved tag changes the installed code
  without changing the recorded source. Dependency URLs are checked against a
  scheme allowlist before they reach `git`, so a manifest cannot smuggle a
  command in through one, but the content a URL resolves to is still trusted on
  the strength of TLS alone.
* **The sandbox does not apply to compiled output.** `rakc run` and `rakc vm`
  honour `--sandbox`; `rakc build --emit=exe` and the C target do not, because
  the capability system is a runtime construct. A compiled artefact runs with
  the full privileges of the account that built it.

## Related

`docs/SECURITY-AUDIT-2026-10.md` is a full assessment of this toolchain: what
was found, what was fixed, and what is still open. It is the longer version of
this document, and it is where the reasoning behind each of the entries above
is written down.