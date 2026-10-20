# Examples

Every example lives in `examples/`. Run one with `rakc run
examples/<name>.rak`; most also run under `rakc vm`, and the ones that do not
are the interpreter-only features.

## The single-file examples

| File | Shows |
|---|---|
| `hello.rak` | The smallest program. |
| `quick.rak` | A stdlib smoke test: hashing, hex, base64, `fmt`. |
| `core_demo.rak` | The core language: closures, traits, structs, enums, errors. |
| `stdlib_demo.rak` | Strings, arrays, math. |
| `batteries_demo.rak` | Time, seeded randomness, CSV, gzip. |
| `closures.rak` | Lexical closures and recursion. |
| `pipeline.rak` | The pipeline operator. |
| `regex.rak` | Regex literals and the regex builtins. |
| `binary_patterns.rak` | Matching byte patterns. |
| `traits.rak` | The Display, Iterable, Index and IndexMut protocols. |
| `macros.rak` | Compile-time macros and constants. |
| `vm_features.rak` | Maps, tuples, indexing, interpolation, `match` on the VM. |
| `json_demo.rak` | JSON parsing and serialisation. |
| `forensic_structs.rak` | `binstruct` round-trip, and evidence provenance. |
| `osint_scan.rak` | A port scan. |
| `osint_demo.rak` | The OSINT toolkit. |
| `osint_pack_demo.rak` | The OSINT pack: WHOIS, certificate transparency, YARA. |
| `crypto_demo.rak` | Hashing, AES-GCM, Ed25519, ECDSA, RSA. |
| `secure_usage.rak` | The safe way to use the crypto builtins. |
| `security_workflow.rak` | An end-to-end investigation. |
| `net_audit.rak` | A network audit. |
| `logging_demo.rak` | Structured JSON logging. |
| `secrets_demo.rak` | The secrets store. |
| `process_demo.rak` | Spawning and reaping a child process. |
| `ffi.rak` | FFI: `extern` bindings and raw memory. |
| `ffi_dynamic.rak` | The dynamic loader: `lib.call`, `lib.sym`, `lib.close`. |
| `mmap.rak` | Memory-mapped files: zero-copy slices, search and lines. |
| `async.rak` | The async event loop: `async fn`, `await`, `tcp_probe`. |
| `async_orchestration.rak` | `await_all`, `select`, `timeout`, `task_group`. |
| `net_raw.rak` | Forging IPv4, TCP, UDP, ICMP and ARP packets. |
| `parsers.rak` | The DNS, TLS and PCAP wire-format parsers. |
| `streaming.rak` | Lazy streams over files and sockets. |
| `import_demo.rak` | Python-style `import`, `from` and `export`. |
| `gui_demo.rak` | A native GUI window. |
| `cli_greeter.rak` | A CLI with `fn main(argv)`. |
| `self_host.rak` | A Rak interpreter written in Rak. |
| `sql_server.rak` | A SQL server in Rak. |
| `sql_client.rak` | Talking to that server. |
| `echo_server.rak` | A TCP echo server. |
| `echo_client.rak` | Talking to that server. |
| `http_server_demo.rak` | The pull-based HTTP server. |
| `ws_server_demo.rak` | A WebSocket echo server. |
| `ws_client_demo.rak` | Talking to that server. |
| `dap_demo.rak` | Driving a debug session over the Debug Adapter Protocol. |
| `bench.rak` | A benchmark loop. |

Some of these need something to talk to, a display, or a network, and are
skipped by the parity suite for that reason: `bench`, `echo_server`,
`http_server_demo`, `ws_server_demo`, `gui_demo`, `echo_client`,
`ws_client_demo`, `sql_client`, `sql_server` and `dap_demo`.

## The example projects

`examples/` is not flat. Four directories hold multi-file projects.

### `hexhunt/`

A multi-module binary signature scanner. It declares a signature database, a
sweep over a capture file, and a report, and it runs identically on both
backends.

```bash
rakc run examples/hexhunt/src/main.rak capture.bin
```

Its README covers the layout, and its `ensures` clause
(`ensures len(result) <= len(data)`) is checked on both backends.

### `modstate/`

The rules of module state, in one place: a live `import m` view against a
copying `from m import x`, module-level `let mut`, `pub` as the visibility
boundary, and `pub use` copying.

### `VPN/`

The encrypted tunnel, end to end: a ChaCha20 tunnel over UDP, and a variant
that carries it over HTTP.

### `bench/`

Six benchmark workloads — arithmetic, arrays, bytes, Fibonacci, maps and
strings — used by `rakc bench` to compare the interpreter and the VM.

## The SQL server

```bash
rakc run examples/sql_server.rak   # listens on 127.0.0.1:18393
rakc run examples/sql_client.rak    # CREATE, INSERT, SELECT, UPDATE, DELETE
```

The engine supports CREATE TABLE, INSERT, SELECT with WHERE and column
projection, UPDATE, DELETE and DROP. Storage is file-backed with a simple
line format. The server handles each connection inline, parsing SQL and
returning JSON.

## Self-hosting

```bash
rakc run examples/self_host.rak    # a Rak interpreter written in Rak
```

It reads Rak source, lexes it, parses it, and evaluates it. The output of
`1 + 2 * 3` is 7. The output of `let x = 5; let y = 7; dump x * y - 1` is 34.
