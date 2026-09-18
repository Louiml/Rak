//! Application-layer VPN tunneling primitives.
//!
//! These helpers power the Rak `tunnel` keyword and the `vpn_*` builtins. They
//! are deliberately cross-platform: everything lives at the application/datagram
//! layer (TCP + UDP), so no kernel TUN/TAP device or raw-socket privilege is
//! required (which is Unix-gated in this stdlib). Windows works out of the box.
//!
//! The crypto stack is modern, forward-secret and AEAD-based:
//!   - X25519 (Curve25519 Diffie–Hellman) for ephemeral key agreement, and
//!   - ChaCha20-Poly1305 for authenticated encryption of tunnel datagrams,
//!   - HKDF-SHA256 for deriving session keys / rolling per-packet ratchet keys,
//!   - PBKDF2 (via HMAC) for deriving a pre-shared key from a passphrase.
//!
//! A tunneled datagram is `tunnel_frame(seq, payload)`:
//!   [ 8-byte big-endian sequence ] [ payload ... ]

use std::net::{SocketAddr, UdpSocket};
use std::sync::Mutex;
use std::time::Duration;

/// A connectionless UDP socket used as the outer transport of a VPN tunnel.
/// Wrapped in a `Mutex` so it can be shared cheaply across Rak `Value`s.
pub struct UdpTransport {
    pub socket: UdpSocket,
    /// Bound local address (ip:port), resolved from the OS after bind.
    pub local: SocketAddr,
}

impl UdpTransport {
    fn new(socket: UdpSocket) -> std::io::Result<Self> {
        let local = socket.local_addr()?;
        Ok(UdpTransport { socket, local })
    }
}

/// Bind a UDP socket to `addr` (e.g. `"127.0.0.1:12000"`). Returns the live
/// transport plus the actual bound socket address.
pub fn udp_bind(addr: &str) -> Result<(Mutex<UdpTransport>, SocketAddr), String> {
    let bind_addr: SocketAddr = addr
        .parse()
        .map_err(|_| format!("udp_bind: bad address '{}'", addr))?;
    let socket = UdpSocket::bind(bind_addr).map_err(|e| format!("udp_bind: {}", e))?;
    let transport = UdpTransport::new(socket).map_err(|e| format!("udp_bind: {}", e))?;
    let local = transport.local;
    Ok((Mutex::new(transport), local))
}

/// Bind a UDP socket on the loopback interface with an OS-assigned port.
pub fn udp_bind_ephemeral() -> Result<(Mutex<UdpTransport>, SocketAddr), String> {
    udp_bind("127.0.0.1:0")
}

/// Send `data` to `target` (ip:port). Returns the number of bytes written.
pub fn udp_send(transport: &Mutex<UdpTransport>, data: &[u8], target: &str) -> Result<usize, String> {
    let target: SocketAddr = target
        .parse()
        .map_err(|_| format!("udp_send: bad target '{}'", target))?;
    let t = transport.lock().map_err(|_| "udp_send: lock".to_string())?;
    t.socket
        .send_to(data, target)
        .map_err(|e| format!("udp_send: {}", e))
}

/// Receive up to `max` bytes from any peer. Waits up to `timeout_ms`
/// (0 = block forever). Returns `(data, peer_addr)`.
pub fn udp_recv(
    transport: &Mutex<UdpTransport>,
    max: usize,
    timeout_ms: u64,
) -> Result<Option<(Vec<u8>, SocketAddr)>, String> {
    let t = transport.lock().map_err(|_| "udp_recv: lock".to_string())?;
    if timeout_ms > 0 {
        t.socket
            .set_read_timeout(Some(Duration::from_millis(timeout_ms)))
            .map_err(|e| format!("udp_recv: {}", e))?;
    } else {
        t.socket
            .set_read_timeout(None)
            .map_err(|e| format!("udp_recv: {}", e))?;
    }
    let mut buf = vec![0u8; max];
    match t.socket.recv_from(&mut buf) {
        Ok((n, addr)) => {
            buf.truncate(n);
            Ok(Some((buf, addr)))
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(e) => Err(format!("udp_recv: {}", e)),
    }
}

/// The local bound address (ip:port) of a UDP transport.
pub fn udp_local_addr(transport: &Mutex<UdpTransport>) -> String {
    match transport.lock() {
        Ok(t) => t.local.to_string(),
        Err(_) => String::from("<locked>"),
    }
}

// ---------------------------------------------------------------------------
// Datagram framing
// ---------------------------------------------------------------------------

/// Prefix `payload` with an 8-byte big-endian sequence number, producing the
/// on-the-wire tunnel frame. Sequence numbers detect reordering/dropped frames.
pub fn tunnel_frame(seq: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Parse a tunnel frame produced by `tunnel_frame`. Returns (sequence, payload).
pub fn tunnel_unframe(frame: &[u8]) -> Result<(u64, Vec<u8>), String> {
    if frame.len() < 8 {
        return Err("tunnel_unframe: frame too short".to_string());
    }
    let mut seq_bytes = [0u8; 8];
    seq_bytes.copy_from_slice(&frame[..8]);
    let seq = u64::from_be_bytes(seq_bytes);
    Ok((seq, frame[8..].to_vec()))
}

// ---------------------------------------------------------------------------
// X25519 key agreement
// ---------------------------------------------------------------------------

/// Generate an X25519 keypair from a 32-byte seed. Returns (public_key, secret_key).
pub fn x25519_keypair(seed: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use x25519_dalek::{PublicKey, StaticSecret};
    let seed_arr: &[u8; 32] = seed
        .try_into()
        .map_err(|_| "x25519_keypair: seed must be 32 bytes".to_string())?;
    let secret = StaticSecret::from(*seed_arr);
    let public = PublicKey::from(&secret);
    Ok((public.to_bytes().to_vec(), secret.to_bytes().to_vec()))
}

/// Compute the shared secret between this secret key and a peer public key.
pub fn x25519_shared(secret: &[u8], peer_public: &[u8]) -> Result<Vec<u8>, String> {
    use x25519_dalek::{PublicKey, StaticSecret};
    let secret_arr: &[u8; 32] = secret
        .try_into()
        .map_err(|_| "x25519_shared: secret must be 32 bytes".to_string())?;
    let public_arr: &[u8; 32] = peer_public
        .try_into()
        .map_err(|_| "x25519_shared: public key must be 32 bytes".to_string())?;
    let secret = StaticSecret::from(*secret_arr);
    let public = PublicKey::from(*public_arr);
    Ok(secret.diffie_hellman(&public).as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// ChaCha20-Poly1305 authenticated encryption
// ---------------------------------------------------------------------------

/// ChaCha20-Poly1305 encrypt. `key` must be 32 bytes, `nonce` 12 bytes.
/// `aad` is authenticated but not encrypted (may be empty). Returns the
/// ciphertext with the 16-byte auth tag appended (mirrors `aes_gcm_encrypt`).
pub fn chacha20_encrypt(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, String> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "chacha20_encrypt: key must be 32 bytes".to_string())?;
    let nonce_arr: &[u8; 12] = nonce
        .try_into()
        .map_err(|_| "chacha20_encrypt: nonce must be 12 bytes".to_string())?;
    cipher
        .encrypt(Nonce::from_slice(nonce_arr), aead_payload(aad, plaintext))
        .map_err(|_| "chacha20_encrypt: encryption failed".to_string())
}

/// ChaCha20-Poly1305 decrypt. `ciphertext` is the output of `chacha20_encrypt`
/// (ciphertext ++ tag), using the same `aad`.
pub fn chacha20_decrypt(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, String> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "chacha20_decrypt: key must be 32 bytes".to_string())?;
    let nonce_arr: &[u8; 12] = nonce
        .try_into()
        .map_err(|_| "chacha20_decrypt: nonce must be 12 bytes".to_string())?;
    cipher
        .decrypt(Nonce::from_slice(nonce_arr), aead_payload(aad, ciphertext))
        .map_err(|_| "chacha20_decrypt: authentication failed or bad key/nonce".to_string())
}

fn aead_payload<'a, 'b>(aad: &'a [u8], plaintext: &'b [u8]) -> chacha20poly1305::aead::Payload<'b, 'a> {
    chacha20poly1305::aead::Payload { msg: plaintext, aad }
}

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derive `length` bytes from a shared secret via HKDF-SHA256, using an
/// optional `salt` and `info` context. This turns X25519 output into a
/// uniformly random session key for the AEAD phase.
pub fn hkdf_derive(secret: &[u8], salt: &[u8], info: &[u8], length: u32) -> Result<Vec<u8>, String> {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let hk = if salt.is_empty() {
        Hkdf::<Sha256>::new(None, secret)
    } else {
        Hkdf::<Sha256>::new(Some(salt), secret)
    };
    let mut out = vec![0u8; length as usize];
    hk.expand(info, &mut out)
        .map_err(|e| format!("hkdf_derive: {}", e))?;
    Ok(out)
}

/// Derive a pre-shared key from a passphrase via PBKDF2-HMAC-SHA256 with a
/// random `salt`. `iterations` slows down brute-force attacks.
pub fn psk_derive(
    passphrase: &str,
    salt: &[u8],
    iterations: u32,
    length: u32,
) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; length as usize];
    pbkdf2_hmac_sha256(passphrase.as_bytes(), salt, iterations, &mut out);
    Ok(out)
}

fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    // PBKDF2-HMAC-SHA256 via repeated HMAC (rebound from hmac+sha2 crates).
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let blocks = (out.len() + 31) / 32;
    let mut block = vec![0u8; out.len()];
    let mut offset = 0usize;
    for block_index in 1..=blocks {
        let mut mac = HmacSha256::new_from_slice(password).unwrap();
        mac.update(salt);
        mac.update(&[(block_index >> 24) as u8, (block_index >> 16) as u8, (block_index >> 8) as u8, block_index as u8]);
        let mut u = mac.finalize().into_bytes().to_vec();
        let mut t = u.clone();
        for _ in 1..iterations {
            let mut mac2 = HmacSha256::new_from_slice(password).unwrap();
            mac2.update(&u);
            u = mac2.finalize().into_bytes().to_vec();
            for i in 0..t.len() {
                t[i] ^= u[i];
            }
        }
        // Copy as much of block `t` as fits.
        let copy_len = (out.len() - offset).min(t.len());
        block[offset..offset + copy_len].copy_from_slice(&t[..copy_len]);
        offset += copy_len;
    }
    out.copy_from_slice(&block[..out.len()]);
}

/// Derive a rolling per-packet key for the next `counter` slot using HKDF-SHA256
/// `expand(prev_key, info="rak:tunnel").` Updating the key each packet gives a
/// forward-secret ratchet even if one slot's key leaks.
pub fn kdf_next(prev_key: &[u8], counter: u32, length: u32) -> Result<Vec<u8>, String> {
    let info = format!("rak:tunnel:{}", counter).into_bytes();
    hkdf_derive(prev_key, b"rak:tunnel", &info, length)
}

/// A single 96-bit nonce for a given packet sequence, deterministic and
/// collision-free for a given key as long as `seq` is never reused.
pub fn nonce_for(seq: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&seq.to_be_bytes());
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED_A: [u8; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    ];
    const SEED_B: [u8; 32] = [
        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
    ];

    #[test]
    fn x25519_both_sides_agree() {
        let (a_pub, a_sec) = x25519_keypair(&SEED_A).unwrap();
        let (b_pub, b_sec) = x25519_keypair(&SEED_B).unwrap();
        let shared_a = x25519_shared(&a_sec, &b_pub).unwrap();
        let shared_b = x25519_shared(&b_sec, &a_pub).unwrap();
        assert_eq!(shared_a.len(), 32);
        assert_eq!(shared_a, shared_b);
    }

    #[test]
    fn chacha20_round_trip_and_tamper() {
        let key = psk_derive("test-pass", b"salt", 1000, 32).unwrap();
        let nonce = nonce_for(1);
        let aad = b"rak:v1";
        let ct = chacha20_encrypt(&key, &nonce, aad, b"hello vpn").unwrap();
        assert_eq!(ct.len(), 9 + 16); // payload + tag
        let pt = chacha20_decrypt(&key, &nonce, aad, &ct).unwrap();
        assert_eq!(pt, b"hello vpn");
        // Wrong AAD must fail authentication.
        assert!(chacha20_decrypt(&key, &nonce, b"tampered", &ct).is_err());
    }

    #[test]
    fn framing_round_trip() {
        let frame = tunnel_frame(42, b"payload");
        assert_eq!(frame.len(), 8 + 7);
        let (seq, payload) = tunnel_unframe(&frame).unwrap();
        assert_eq!(seq, 42);
        assert_eq!(payload, b"payload");
        assert!(tunnel_unframe(&[0u8; 4]).is_err());
    }

    #[test]
    fn kdf_ratchet_changes_key() {
        let key = psk_derive("pass", b"s", 1000, 32).unwrap();
        let k1 = kdf_next(&key, 1, 32).unwrap();
        let k2 = kdf_next(&key, 2, 32).unwrap();
        assert_ne!(k1, k2);
        assert_ne!(key, k1);
    }
}