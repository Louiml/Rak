# VPN & encrypted tunneling

Rak ships an application-layer VPN toolkit: forward-secret key agreement
(X25519), AEAD encryption (ChaCha20-Poly1305), key derivation (PBKDF2 + HKDF),
tunnel datagram framing, and an encrypted UDP transport. Everything lives at
the application layer, so no kernel TUN/TAP device or raw-socket privilege is
required — it works on **Windows** out of the box.

## The tunnel keyword

```rak
tunnel link "passphrase" {
    // link       -> 32-byte session key (Bytes)
    // link_udp   -> bound encrypted UDP transport handle
    // link_addr  -> bound "ip:port" string
    // tunnel_key -> alias for the session key
    udp_send(link_udp, b"ping", peer_addr)
}
```

`tunnel <name> <passphrase> { ... }` is a scoped statement: at runtime it
derives a 32-byte session key via PBKDF2-HMAC-SHA256 (`psk_derive`), binds an
ephemeral UDP transport, defines `<name>`, `<name>_udp`, `<name>_addr`, and
`tunnel_key` inside the block scope, then pops the scope when the body ends —
so the names are undefined outside by design.

The bytecode VM does not implement `tunnel` (it has no UDP transport value) and
returns a clear `VM does not support 'tunnel' statement` error.

## Crypto builtins

| Function | Purpose |
|----------|---------|
| `x25519_keypair(seed) -> (pub, sec)` | Deterministic Curve25519 keypair from a 32-byte seed. |
| `x25519_shared(secret, peer_pub) -> bytes` | ECDH shared secret. |
| `chacha20_encrypt(key, nonce, aad, plain) -> bytes` | AEAD ciphertext ++ 16-byte tag. |
| `chacha20_decrypt(key, nonce, aad, ct) -> bytes` | AEAD decrypt / verify (raises on auth failure). |
| `tunnel_preshared_key(pass, salt, iters, len) -> bytes` | PBKDF2-HMAC-SHA256 session key. |
| `hkdf_derive(secret, salt, info, len) -> bytes` | HKDF-SHA256 extract/expand. |
| `kdf_next(prev, counter, len) -> bytes` | Rolling per-packet ratchet key. |
| `tunnel_frame(seq, payload) -> bytes` | `[seq(8)] ++ payload` framing. |
| `tunnel_unframe(frame) -> (seq, payload)` | Parse + validate a frame. |
| `tunnel_nonce(seq) -> bytes(12)` | Deterministic 12-byte AEAD nonce. |

The UDP transport builtins (`udp_bind`/`udp_send`/`udp_recv`/`udp_local_addr`)
are interpreter-only. The pure-crypto set runs on both backends.

## Key exchange walkthrough

```rak
// both sides
let keys = x25519_keypair(seed)          // (pub, sec)
// exchange only the public keys, then:
let shared = x25519_shared(keys.1, peer_pub)   // both sides derive the same 32 bytes
```

## Security notes

- Keys must be fresh/random; never reuse a nonce under the same key (roll via
  `kdf_next` and `tunnel_nonce(seq)`).
- `tunnel_frame` uses a monotonic `seq` for replay/reorder detection and to
  avoid nonce reuse.
- In production, pull passphrases from the secrets store (`secret_get`) — never
  hardcode them.
- `chacha20_encrypt`/`decrypt` require key = 32 bytes and nonce = 12 bytes;
  decrypt raises on auth-tag mismatch or bad key/nonce.
- `tunnel_unframe` rejects frames shorter than 8 bytes.

See `examples/VPN/` — `keyexchange.rak` (both backends), `psk_tunnel.rak`,
`udp_dtls_like.rak`, `packet_builder.rak`, and `vpn_over_http.rak`.
