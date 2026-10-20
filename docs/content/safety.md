# Safety

Rak's position is that a limit is stated where it is measured. This page says
what is enforced, what is checked, and what is not enforced at all.

## The capability sandbox

`rakc run untrusted.rak --sandbox --allow net,fs_read` runs a program with
nothing but the capabilities it was granted. With `--sandbox` and no
`--allow`, every capability is denied and only pure computation works.

```text
sandbox: active (allow: net,fs_read)
```

A refusal names the capability and the fix:

```text
sandbox: builtin 'net_connect' blocked — capability 'net' not granted (rerun with --allow net)
```

### The capabilities

| Capability | Grants |
|---|---|
| `net` | `net_listen`, `net_accept`, `net_connect`, `net_local_addr`, the `tcp_`/`udp_`/`http_`/`ws_` families, the `dns_*` family, `fetch`, `scan_ports`, `scan_subdomains`, `ct_subdomains`, `whois_lookup` |
| `fs_read` | `read`, `file_read`, `read_lines`, `stream_csv`, `stream_jsonl`, `mmap_open` (read mode), `file_list`, `file_exists`, `file_size`, `zip_list`, `zip_read` |
| `fs_write` | `write`, `file_write`, `file_append`, `file_write_bytes`, `file_append_bytes`, `zip_write`, `mmap_write`, `file_delete`, `file_mkdir`, `file_copy`, `file_rename`, `log_init`, `dump x, "path"` |
| `process` | the `process_` family |
| `ffi` | the `ffi_` family, `extern_call`, and `extern "C" { … }` declarations |
| `raw` | `net_raw_send`, `net_raw_recv`, the `pcap_` family |
| `gui` | the `gui_` family |
| `secrets` | the `secret_` family, `env_get`, `env_set` |
| `asm` | inline assembly |
| `all` | every one of the above |

`fs` is accepted for `fs_write`, `proc` for `process`, and `raw_sockets` for
`raw`. An unknown name is a warning, not an error:
`sandbox: unknown capability 'x' (ignored)`.

The gate sits at the call site, and both backends enforce the same list.

**What is deliberately not gated.** Anything that cannot reach outside the
process: the packet *builders* (`net_raw_ipv4`, `net_raw_tcp`, `net_raw_udp`,
`net_raw_tcp_syn`, `net_raw_csum`, `net_raw_icmp*`, `net_raw_arp_*`), the DNS
*builders* (`dns_build`, `dns_parse`), `whois_parse`, `subdomain_enum`, the
`tunnel_*` crypto, and the hashing and constant-time helpers. A program can
compute all of those under a sandbox, because computing them has no effect on
anything.

**The sandbox is a process-wide filter, set once from the command line.** A
module cannot narrow its own privileges, and it cannot widen them either —
a program that runs under `--sandbox` runs its whole dependency tree under
it.

### The filesystem scope

`--fs-root DIR` is a separate limit from the capabilities. It confines paths,
independently of whether `fs_read` is granted:

```text
sandbox: path '/etc/passwd' is outside the filesystem scope '/tmp/work'
```

The path is resolved before the comparison, and the deepest existing ancestor
is canonicalised, so a symlink pointing out of the scope is refused too.

## Unsafe blocks

Some things cannot be checked, so they must be justified in the source:

```rak
unsafe "the pointer comes from ffi_alloc and the region is 4 bytes long" {
    let b = ffi_read(ptr, 0)
}
```

The reason string is **mandatory**. `rakc lint`'s `unsafe-thin-reason` rule
rejects a reason that is shorter than fifteen characters, or that contains
"trust me", "because", "todo", "fixme", "hack" or "n/a" — a justification
that says nothing is not a justification.

`rakc lint --audit` prints every unsafe block's reason in source order, so
the justifications can be reviewed as a list rather than hunted for.

Inline assembly (`asm`) is only legal inside an `unsafe` block, and the
parser enforces that. The `asm` capability gates the call, but nothing
mediates the instruction itself: the machine executes it. Treat every `asm`
call as a review boundary.

## The linter's security rules

`rakc lint` ships nine security rules on top of its five style rules. They
are advisory; `--deny` makes them fail the run.

| Rule | What it catches |
|---|---|
| `hardcoded-secret` | A credential-looking name assigned a string literal, or a literal that looks like a token (a known prefix, `password=`/`token=`/`api_key=`, or 32/40/64/128 hex digits). |
| `plaintext-url` | An `http://` URL whose host is not localhost. |
| `weak-crypto` | `md5`, `sha1`, `rot13` and repeating-key `xor`. |
| `secret-compare` | `==` on a value that looks secret — it leaks the common-prefix length through timing. |
| `ffi-raw-pointer` | Any `ffi_*` call other than `ffi_load` and the string conversions. |
| `insecure-transport` | `ws_connect` and `net_connect` — an unencrypted connection. |
| `unescaped-interpolation` | An f-string that looks like a SQL or shell command, interpolating a value that is not wrapped in an escape function. |
| `inline-asm` | Any `asm` call. |
| `unsafe-thin-reason` | An unsafe block whose justification is not a justification. |

**Where they have false negatives.** They are pattern matchers over the AST,
not a dataflow analysis. A secret assembled at runtime, a value that only
*becomes* secret after a call, or an interpolation whose template does not
look like SQL or a shell will pass. They are a review aid, not a guarantee.

The style rules are `unused-var`, `shadowed`, `unreachable`,
`missing-ret-type` and `duplicate-import`.

## Crypto

The primitives Rak actually has:

| Purpose | Builtins |
|---|---|
| Hashing | `sha256`, `hmac_sha256` (and `md5`/`sha1`, which print a deprecation warning to stderr) |
| Symmetric | `aes_gcm_encrypt` / `aes_gcm_decrypt` (AES-256-GCM), `chacha20_encrypt` / `chacha20_decrypt` (ChaCha20-Poly1305) |
| Signatures | `ed25519_keypair` / `ed25519_sign` / `ed25519_verify`, `ecdsa_keypair` / `ecdsa_sign` / `ecdsa_verify` (NIST P-256), `rsa_keypair` / `rsa_sign` / `rsa_verify` / `rsa_encrypt` / `rsa_decrypt` |
| Key agreement | `x25519_keypair` / `x25519_shared` |
| Key derivation | `tunnel_preshared_key` (PBKDF2), `kdf_next` (HKDF ratchet) |
| Constant time | `ct_eq`, `ct_eq_hex`, `ct_select` |

`md5` and `sha1` print this on stderr, once per primitive per process:

```text
rak: warning: `md5` is deprecated: MD5 is broken for anything adversarial. Use `sha256` unless you are analysing data that uses it, in which case suppress this with unsafe { "md5 is the subject of this analysis" { md5() } }
```

`ct_eq` compares bytes in time independent of their contents; `ct_eq_hex` does
the same for hex strings, case-insensitively; `ct_select` picks one of two
equal-length buffers without branching on the choice. Use them instead of `==`
on anything secret — see the `secret-compare` rule above.

`rsa_encrypt` and `rsa_decrypt` are OAEP with a label; `rsa_sign` is
PKCS#1 v1.5 with SHA-256.

## Escaping untrusted data

| Builtin | For |
|---|---|
| `sql_escape` | ANSI SQL, PostgreSQL, SQLite (and MySQL with `NO_BACKSLASH_ESCAPES`) |
| `sql_escape_mysql` | MySQL's default `sql_mode` |
| `shell_escape` | A shell command line |
| `html_escape` | HTML text and attributes |
| `regex_escape` | A regex pattern |

For SQL, a parameterised query is better than either escaper. For the shell,
an argv array is better than a command string. The linter's
`unescaped-interpolation` rule says so too, when it recognises a template that
looks like one or the other.

## Bounded decompression

Decompression is bounded at 100 MiB, and the bound is checked *while* reading,
not after. A gzip bomb is refused with a message that says what it thinks it
saw:

```text
gzip_decompress: output exceeds the 104857600-byte limit (this looks like a decompression bomb, not a gzip stream)
```

The same bound applies to zip entries.

## Zeroization

`zeroize(bytes)` wipes a buffer and returns it. The wipe is not elided by the
optimizer.

The secrets store has a stronger guarantee: `secret_delete` and
`secret_delete_all` overwrite the on-disk file with zeroes before removing
it, and wipe the in-memory values. The store itself is created with mode 0600
on unix. On Windows the file inherits the directory's ACL and the mode bits do
not apply.

## Packet forging

The packet builders are pure computation and are **not** gated by the
sandbox. Sending is: `net_raw_send` and `net_raw_recv` need the `raw`
capability, and on unix they need `CAP_NET_RAW` or root.

`net_raw_send` and `net_raw_recv` return a `Result` value rather than
raising, so a program can handle a refusal:

```rak
let sent = net_raw_send(pkt)
if sent is Err(e) { dump e }
```

## Fuzzing

Two fuzzers, with different jobs.

`rakc fuzz <target>` is a deterministic mutation loop that runs in ordinary
CI on any platform. It needs a debug build, because the harness uses
`catch_unwind` and the release profile sets `panic = "abort"`.

```text
rakc fuzz all
rakc fuzz dns --seed 0xC0FFEE
```

`fuzz/` holds the coverage-guided `cargo-fuzz` targets for long Linux or WSL
runs: the lexer, the parser, the evaluator, and the DNS, TLS, JSON, WebSocket,
tunnel, raw-packet, CSV, gzip and zip parsers.

## What Rak does not do

- It does not make a program safe to run untrusted code. The sandbox reduces
  what a program can reach; it does not make a bug in the program disappear.
- It does not verify a package's provenance. `oyvey audit` checks that the
  installed files match the lockfile's checksum, which is integrity, not
  trust: the checksum comes from the same source as the code.
- It does not sandbox a native library. A granted `ffi` capability hands a
  program the whole process, and the library's own bugs are not Rak's to
  contain.
- It does not have a memory-safe core. The compiler is Rust, but a Rak
  program that walks a raw pointer can still read out of bounds.
- It does not have control-flow integrity. The release binaries are built
  with ASLR, high-entropy VA, DEP and a CFG-compatible image, but the
  instrumented CFI that would actually enforce it is not available.
- It does not protect a value once it is in memory. `zeroize` is explicit and
  best-effort; a copy made by the runtime is not tracked.
