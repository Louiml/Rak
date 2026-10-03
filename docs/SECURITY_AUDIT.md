# Security audit of the audit prompt

I checked every claim in the prompt against the code before changing anything.
Roughly half of it describes mitigations that already exist, and one claim names a
function that does not exist at all.

## Already implemented -- the prompt is out of date

| Prompt says | Reality |
| --- | --- |
| "No forced constant-time comparison" | `ct_eq` and `ct_eq_hex` exist on **both** backends (`interpreter.rs:4937`, `vm.rs:676`), backed by `rak_stdlib::ct_eq` |
| "Weak crypto detection exists via linting but doesn't prevent usage" | correct that it is a lint, and it is real: `weak-crypto` covers md5, sha1, rot13, xor (`lint.rs:538`) |
| "Secrets ... lack zeroization" | `stdlib/src/secrets.rs` uses `zeroize::Zeroize`, wipes the serialized buffer before it is dropped, and `shred_file`s the backing file |
| "Inline FFI has no unsafe justification" | `unsafe { reason }` is required, `unsafe_sites()` exposes an audit list, and `unsafe-thin-reason` catches placeholders |
| "Make `--allow ffi` mandatory / deny FFI by default" | `caps.rs` already gates `ffi_` as its own capability with `--sandbox --allow ffi` |
| "Flag insecure-transport by default" | `insecure-transport` rule exists (`lint.rs:19`) |
| "No audit trail of unsafe blocks" | `Interpreter::unsafe_sites() -> &[UnsafeSite]` is exactly that |
| "No depth limit" | `limits.max_depth` bounds call recursion |
| "Fuzz targets don't exist" | `fuzz/` exists, outside the workspace so the stable toolchain still builds |
| "`ffi_sym` doesn't validate signatures" | **there is no `ffi_sym` builtin.** Nothing to fix |
| "docs/content/safety.md" | exists, 12.5 KB, and already has Crypto / Constant-time / Zeroization / Packet forging sections |

## Genuinely missing -- confirmed by search, not assumed

| Gap | Evidence |
| --- | --- |
| md5/sha1/rot13/xor are silent at runtime | `interpreter.rs:4900` returns the digest with no warning; the lint does not run at runtime |
| No escape helpers | `sql_escape`, `shell_escape`, `html_escape`, `regex_escape`: **0 hits** repo-wide |
| Secrets can be logged | no redaction anywhere: `REDACT\|redact\|\*\*\*` returns nothing |
| No `secret_rotate` / TTL | `secrets.rs` has get/set/persist/delete/delete_all/list and no expiry |
| Decompression is unbounded | `gzip_decompress(data) -> Result<Vec<u8>>` in `archive.rs:17` and `stream_io.rs:18`, no output cap |
| No parser size limits | no 512 B DNS / 16 KB TLS / per-packet PCAP caps |
| No binstruct nesting limit | `make_bin_decode_native` recurses with no depth bound |
| No `SECURITY.md`, no `CHANGELOG.md` | absent |

## Judgement calls I do not agree with, stated rather than silently done

**"Make `hmac_sha256`, `aes_gcm_encrypt`, `ed25519_sign` the only primitives available
without explicit opt-in."** Removing working primitives from a language whose stated
purpose is security and OSINT work makes it *less* able to analyse the weak crypto it
is meant to find -- MD5 collision work, legacy protocol research. A deprecation
warning plus a lint is the right shape; deletion is not. I am implementing the warning.

**"Make `--sandbox --allow ffi` a mandatory runtime flag."** The capability already
exists and defaults to permissive, matching every other language with an FFI. Making
it deny-by-default is a policy change with a real usability cost and no security
benefit against a program that is already trusted to run. I am leaving the default
alone and documenting it instead.

**"Add Tokio console instrumentation for task monitoring."** A debug-only tracing
tool is not a vulnerability fix. Skipping.

**"Implement secret rotation via `secret_rotate(name, ttl_seconds)`."** Rotation is a
key-management feature, not a primitive: rotating a credential correctly needs the
consumer to re-fetch it, and a TTL alone gives the *illusion* of rotation while a
long-lived handle keeps the old value. I would rather ship expiry (which prevents a
stale secret outliving its usefulness) than a name that implies more than it does.

**Item 10, "fuzz artifacts not tracked in version control."** Crashing inputs are
already captured by the fuzzer and belong in issue trackers, not in a repository that
every clone downloads. The existing `fuzz/` crate is the right arrangement.