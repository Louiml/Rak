# Networking & protocols

## TCP

```rak
let listener = net_listen("127.0.0.1:18392")
let conn = net_accept(listener)
let stream = conn.0
tcp_write(stream, "hello\n")
dump tcp_read_line(stream)
tcp_close(stream)
```

`net_listen`, `net_accept`, `net_connect`, `net_local_addr`, `tcp_read`,
`tcp_read_line`, `tcp_write`, `tcp_close`. Also `tcp_stream(addr)` (0.7) for a
pull-based stream, and `tcp_read_line` for line protocols.

## HTTP server (pull-based)

A minimal HTTP/1.1 server. Requests are queued and read on the main thread, so
handlers can use arbitrary Rak logic (closures, etc.). Works on both backends.

```rak
http_server_start("127.0.0.1", 8080)
loop {
    let req = http_server_poll()          // nil when idle; never blocks
    if req == nil { sleep(10) }
    else {
        if req.path == "/users" {
            http_server_respond(req.id, 200, { "Content-Type": "application/json" }, "{\"ok\":true}")
        } else {
            http_server_respond(req.id, 404, {}, "not found")
        }
    }
}
http_server_stop()
```

`http_server_start(addr, port)` returns the bound port; `http_server_poll()`
returns `{id, method, path, query, headers, body}` or `nil`.

## WebSocket

RFC 6455 framing + handshake built on the TCP stream handle.
Interpreter-only (the VM has no TCP layer).

```rak
// server: accept + handshake + echo
let list = net_listen("127.0.0.1:19001")
let (stream, _) = net_accept(list)
ws_handshake(stream)
loop {
    let m = ws_recv(stream)
    if m == nil { break }
    if m.opcode == 1 { ws_send(stream, m.payload, false) }
}

// client
let ws = ws_connect("ws://127.0.0.1:19001/echo")
ws_send(ws, "hello", true)
dump string(ws_recv(ws).payload)   // hello
```

Client-to-server frames must be masked (`mask: true`); servers pass `false`.

## Raw sockets & packet forging

Build IPv4/TCP/UDP headers with correct ones-complement checksums and send
them on a raw socket. The packet builders are pure computation and run
anywhere; `net_raw_send`/`net_raw_recv` need `CAP_NET_RAW`/Administrator (unix)
and return a `Result`.

```rak
let pkt = net_raw_tcp_syn("10.0.0.5", "10.0.0.10", 12345, 80)
dump len(pkt)          // 40 bytes
dump pkt[0]            // 0x45 (IPv4)
dump pkt[33]           // 0x02 (SYN flag)
dump net_raw_send(pkt) // Ok(40) on a privileged unix box, Err(...) otherwise

let tcp_seg = net_raw_tcp("10.0.0.5", "10.0.0.10", 12345, 80, "SA", 0x1A2B3C4D, 0, b"hello")
let ip_pkt = net_raw_ipv4("10.0.0.5", "10.0.0.10", 6, tcp_seg)
let udp_seg = net_raw_udp("10.0.0.5", "10.0.0.10", 1234, 53, b"\x00")
dump net_raw_csum(pkt[0..20])  // 0 (the IP header is self-checking)
```

Send/recv are unix-only in this build (Windows returns a clear `Err`). An
invalid checksum is not an error — forging malformed packets is intentional.

## Protocol parsers (DNS / TLS / PCAP)

Hand-rolled DNS wire-format builder/parser + UDP query (no external DNS crate
— works air-gapped). TLS ClientHello SNI extraction and DER cert-chain parsing.
PCAP offline capture behind the `pcap` cargo feature (libpcap/Npcap).

```rak
dump dns_query("example.com", "A")       // Ok({answers: [...], truncated})
let q = dns_build("example.com", "A")    // raw query bytes (offline)
let info = tls_parse_client_hello(bytes) // {sni, ciphers}
let certs = tls_parse_cert_chain(der)    // [{subject, issuer}, ...]
let h = pcap_open("capture.pcap")?       // Ok(<pcap>) or Err(...)
let pkt = pcap_next(h)                   // {timestamp, linktype, payload} or nil
```

DNS truncation (TC flag) shows up as `truncated: true`. All parsers
bounds-check every slice and return `Result`s so they degrade gracefully
offline / without the feature.

## UDP

`udp_bind(addr)` returns a `(transport, "ip:port")` pair;
`udp_send(transport, data, target)`, `udp_recv(transport, max, timeout_ms)`
(returns `nil` on timeout), `udp_local_addr(transport)`. Interpreter-only;
used as the conduit under the [VPN toolkit](vpn.html).
