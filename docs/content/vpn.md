# VPN and encrypted tunneling

Rak has a `tunnel` statement: a cross-platform application-layer VPN that
needs no raw sockets and no privileged port. It binds a UDP socket, derives a
session key from a passphrase, and sends ChaCha20-Poly1305 frames over it.

## The statement

```rak
tunnel mytunnel "my passphrase" {
    let peer = mytunnel_addr
    tunnel_send(mytunnel, "hello", peer)
}
```

The passphrase may be a string literal or the name of a variable holding one.
Inside the body, the statement binds:

| Name | Is |
|---|---|
| `mytunnel` | The 32-byte session key. |
| `mytunnel_udp` | The UDP transport. |
| `mytunnel_addr` | The address the transport is bound to. |
| `tunnel_key` | A stable alias for the key. |

## The crypto

| Builtin | Does |
|---|---|
| `tunnel_preshared_key(password, salt, iters?, len?)` | PBKDF2-HMAC-SHA256. The default iteration count is 100,000. |
| `kdf_next(prev_key, counter?, len?)` | The next key in an HKDF ratchet. |
| `tunnel_frame(seq, payload)` | Frame a payload with its sequence number. |
| `tunnel_unframe(frame)` | `[seq, payload]`. |
| `tunnel_nonce(seq)` | The 12-byte nonce for a sequence number. |
| `x25519_keypair(seed)` | A Curve25519 key pair. |
| `x25519_shared(secret, peer_public)` | The shared secret. |
| `chacha20_encrypt(key, nonce, aad, plaintext)` | ChaCha20-Poly1305. |
| `chacha20_decrypt(key, nonce, aad, ciphertext)` | The same, decrypting. |

The key exchange is a full walkthrough: derive a preshared key from the
passphrase, exchange X25519 public keys over the tunnel, mix the shared
secret with the preshared key, and then ratchet the key forward with
`kdf_next` for every frame.

```rak
let psk = tunnel_preshared_key("passphrase", salt, 100000, 32)
let (my_pk, my_sk) = x25519_keypair(seed)
tunnel_send(mytunnel, my_pk, peer)

let peer_pk = tunnel_recv(mytunnel, peer)
let shared = x25519_shared(my_sk, peer_pk)
let key = kdf_next(psk, 0, 32)
```

## UDP

The transport underneath is a plain UDP socket, with builtins of its own:

| Builtin | Does |
|---|---|
| `udp_bind(addr?)` | `[transport, local_address]`. |
| `udp_send(transport, data, target)` | Send. Returns the byte count. |
| `udp_recv(transport, max?, timeout_ms?)` | `[bytes, address]`, or nil on a timeout. |
| `udp_local_addr(transport)` | The bound address. |

A receive timeout is `nil`, not an error: a timeout is the ordinary outcome
of waiting on a network.

## Security notes

- The passphrase is stretched with PBKDF2 before use. The default iteration
  count is 100,000, and it is worth raising.
- Every frame is authenticated with ChaCha20-Poly1305, and the sequence
  number is part of the nonce. A frame that fails authentication raises.
- The nonce is derived from the sequence number, so a key must never be reused
  with a sequence number it has already been used with — which is what the
  ratchet is for.
- This is an application-layer tunnel over UDP, not a VPN in the operating
  system's sense. It protects the payload; it does not change the routing
  table, and traffic metadata is still visible.

See [Networking and protocols](networking.html) for the rest of the network
builtins, and [Safety](safety.html) for the crypto inventory.
