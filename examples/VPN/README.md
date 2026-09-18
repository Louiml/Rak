# Rak &mdash; VPN examples

Rak ships an **application-layer VPN / encrypted-tunnel toolkit** so you can build
secure transport channels entirely in the language &mdash; no kernel TUN/TAP device
and no raw-socket privileges required, which means it works on **Windows** out of
the box (raw sockets are Unix-gated in this stdlib).

The core crypto is modern AEAD + forward-secret key agreement:

| Primitive | Rak builtin |
|-----------|-------------|
| X25519 (Curve25519 ECDH) keypair | `x25519_keypair(seed)` |
| X25519 shared secret | `x25519_shared(secret, peer_pub)` |
| ChaCha20-Poly1305 encrypt/decrypt | `chacha20_encrypt(key, nonce, aad, data)` / `chacha20_decrypt(...)` |
| PBKDF2-HMAC-SHA256 pre-shared key | `tunnel_preshared_key(pass, salt, iters, len)` |
| HKDF-SHA256 per-packet ratchet | `kdf_next(prev_key, counter, len)` |
| Tunnel datagram framing (seq + payload) | `tunnel_frame(seq, payload)` / `tunnel_unframe(frame)` |
| UDP transport (the outer conduit) | `udp_bind(addr)` / `udp_send` / `udp_recv` / `udp_local_addr` |

### The `tunnel` keyword

```rak
tunnel link "s3cret passphrase" {
    // `link`        -> 32-byte session key (Bytes)
    // `link_udp`    -> bound UDP transport handle
    // `link_addr`   -> the bound "ip:port" string
    // `tunnel_key`  -> alias for the session key
}
```

Inside the block an encrypted UDP conduit is already bound. Combine it with the
crypto builtins to send authenticated, replay-protected datagrams.

## Scripts

Run each with the interpreter (the `tunnel` keyword, UDP transport, `fetch` and
`try/catch` live in the interpreter backend). VM notes below.

```
rakc run  examples/VPN/<script>.rak     # interpreter (full support)
rakc vm   examples/VPN/<script>.rak     # bytecode VM (crypto builtins only)
```

| Script | What it shows | VM? |
|--------|---------------|-----|
| `keyexchange.rak` | Out-of-band X25519 ECDH between two parties; both derive the **same** shared secret, and a passive eavesdropper without a secret fails. | yes |
| `chacha_tunnel.rak` | ChaCha20-Poly1305 round-trip + a tamper-detection failure case (via `try/catch`), plus datagram framing + per-packet ratchet. | no (uses `try/catch`) |
| `psk_tunnel.rak` | The `tunnel` keyword: derive a session key, open an encrypted UDP conduit, and encrypt a datagram. | no (`tunnel` keyword) |
| `udp_dtls_like.rak` | Two local UDP sockets negotiating an X25519 handshake and exchanging an authenticated encrypted datagram (DTLS-style data path). | no (`udp_*`) |
| `packet_builder.rak` | Encrypt + frame a tunnel datagram and wrap it in a forged UDP/IP packet for raw send (computation-only). | no (`net_raw_*`) |
| `vpn_over_http.rak` | Covert transport: shuttle tunneled, encrypted frames through the built-in HTTP server via a background `spawn`. | no (`fetch`/server) |

## Security notes

- **Never hardcode real keys/passphrases.** In production pull them from the
  secrets store: `secret_get("vpn-psk")`.
- `x25519_keypair` is deterministic given a 32-byte seed; use fresh random seeds
  for ephemeral handshake keys (see `rand`/entropy builtins).
- Each packet should use a **monotonic, never-reused** `seq` for `tunnel_frame`
  so the AEAD nonce never repeats under the same key.
- For latency on lossy links prefer UDP; for reliable transport wrap the same
  framing inside a TCP stream.

These examples are teaching tools for building VPN tunnels, not a drop-in
production VPN. Understand the protocols before deploying in hostile environments.