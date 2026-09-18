# Examples

All examples live in `examples/`. Run any of them with `rakc run
examples/<name>.rak` (and most also with `rakc vm`).

| Example | What it shows |
|---------|---------------|
| `hello.rak` | The whole program: `dump "Hello, World"` |
| `quick.rak` | Syntax tour |
| `osint_scan.rak` | Port scanning with `scan` / `scan_ports` |
| `echo_server.rak` | TCP echo server (`net_listen`/`net_accept`) |
| `sql_server.rak` | A SQL server written in Rak |
| `sql_client.rak` | CREATE, INSERT, SELECT, UPDATE, DELETE against the server |
| `self_host.rak` | A Rak interpreter written in Rak |
| `bench.rak` | Interpreter vs VM benchmark |
| `gui_demo.rak` | GUI window demo (`--features gui`) |
| `json_demo.rak` | JSON parse and stringify |
| `closures.rak` | Lexical closures and recursion |
| `vm_features.rak` | Map, tuple, index, interpolation, match on the VM |
| `pipeline.rak` | Pipeline operator (`\|>`) |
| `regex.rak` | Regex literals and matching |
| `binary_patterns.rak` | Binary byte-pattern matching |
| `traits.rak` | Display/Iterable/Index trait protocols |
| `ffi.rak` | FFI: extern bindings + raw memory (interpreter + VM) |
| `ffi_dynamic.rak` | FFI dynamic loader (lib.call/lib.sym/lib.close) |
| `mmap.rak` | Memory-mapped files: zero-copy slice/search/lines |
| `async.rak` | Async event loop: async fn / await / tcp_probe |
| `async_orchestration.rak` | 0.7: `await_all`, `select`, `timeout`, `task_group` |
| `streaming.rak` | 0.7: pull-based streams, `read_lines`, `tcp_stream` |
| `cli_greeter.rak` | 0.7: `fn main(argv) -> int`, `parse_args`, exit codes |
| `security_workflow.rak` | 0.7: mixed workflow (streams + async + errors) |
| `net_raw.rak` | Raw sockets: forge IPv4/TCP/UDP packets |
| `parsers.rak` | DNS / TLS / PCAP wire-format parsers |
| `macros.rak` | Compile-time macros: macro / name! / const |
| `import_demo.rak` | Python-style import / from / export (with mymod/ package) |
| `forensic_structs.rak` | binstruct decode/encode round-trip + evidence provenance |
| `stdlib_demo.rak` | String, array, and math builtins |
| `VPN/keyexchange.rak` | X25519 key agreement (both backends) |
| `VPN/psk_tunnel.rak` | The `tunnel` keyword end to end |
| `VPN/udp_dtls_like.rak` | Encrypted UDP transport |
| `VPN/packet_builder.rak` | Raw packet forging |
| `VPN/vpn_over_http.rak` | Covert HTTP relay |

## SQL server

```bash
rakc run examples/sql_server.rak   # listens on 127.0.0.1:18393
rakc run examples/sql_client.rak   # CREATE, INSERT, SELECT, UPDATE, DELETE
```

The engine supports CREATE TABLE, INSERT, SELECT with WHERE and column
projection, UPDATE, DELETE, and DROP. Storage is file-backed with a simple
line format. The server handles each connection inline, parsing SQL and
returning JSON.

## Self-hosting

```bash
rakc run examples/self_host.rak    # a Rak interpreter written in Rak
```

It reads Rak source, lexes it, parses it, and evaluates it. The output of
`1 + 2 * 3` is 7. The output of `let x = 5; let y = 7; dump x * y - 1` is 34.
