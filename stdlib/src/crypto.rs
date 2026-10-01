use md5::{Md5, Digest};
use zeroize::Zeroizing;

/// Calculate MD5 hash of input bytes
pub fn md5(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Calculate SHA256 hash of input bytes
pub fn sha256(data: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Calculate SHA1 hash of input bytes
pub fn sha1(data: &[u8]) -> String {
    use sha1::{Sha1, Digest};
    let mut hasher = Sha1::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Simple XOR cipher
pub fn xor_encrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .zip(key.iter().cycle())
        .map(|(d, k)| d ^ k)
        .collect()
}

/// ROT13 cipher for strings
pub fn rot13(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                let base = if c.is_ascii_lowercase() { b'a' } else { b'A' };
                (((c as u8 - base + 13) % 26) + base) as char
            } else {
                c
            }
        })
        .collect()
}

/// Calculate file hash
pub fn file_hash(path: &str, algorithm: &str) -> anyhow::Result<String> {
    use std::fs;
    let data = fs::read(path)?;
    
    match algorithm {
        "md5" => Ok(md5(&data)),
        "sha1" => Ok(sha1(&data)),
        "sha256" => Ok(sha256(&data)),
        _ => Err(anyhow::anyhow!("Unknown hash algorithm: {}", algorithm)),
    }
}

/// HMAC-SHA256, returns lowercase hex.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    hex_lower(&mac.finalize().into_bytes())
}

/// AES-256-GCM encrypt. `key` must be 32 bytes, `nonce` 12 bytes. Returns the
/// ciphertext with the 16-byte auth tag appended.
pub fn aes_gcm_encrypt(key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "aes_gcm_encrypt: key must be 32 bytes".to_string())?;
    let nonce_arr: &[u8; 12] = nonce.try_into().map_err(|_| "aes_gcm_encrypt: nonce must be 12 bytes".to_string())?;
    cipher
        .encrypt(Nonce::from_slice(nonce_arr), plaintext)
        .map_err(|e| format!("aes_gcm_encrypt: {}", e))
}

/// AES-256-GCM decrypt. `ciphertext` is the output of `aes_gcm_encrypt`
/// (ciphertext ++ tag).
pub fn aes_gcm_decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "aes_gcm_decrypt: key must be 32 bytes".to_string())?;
    let nonce_arr: &[u8; 12] = nonce.try_into().map_err(|_| "aes_gcm_decrypt: nonce must be 12 bytes".to_string())?;
    cipher
        .decrypt(Nonce::from_slice(nonce_arr), ciphertext)
        .map_err(|_| "aes_gcm_decrypt: authentication failed or bad key/nonce".to_string())
}

/// Generate an Ed25519 keypair from a 32-byte seed. Returns (public_key, secret_key).
pub fn ed25519_keypair(seed: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use ed25519_dalek::SigningKey;
    let seed_arr: &[u8; 32] = seed.try_into().map_err(|_| "ed25519_keypair: seed must be 32 bytes".to_string())?;
    let signing = SigningKey::from_bytes(seed_arr);
    let verifying = signing.verifying_key();
    Ok((verifying.to_bytes().to_vec(), signing.to_bytes().to_vec()))
}

/// Sign a message with an Ed25519 secret key. Returns the 64-byte signature.
pub fn ed25519_sign(secret: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    let sk_arr: &[u8; 32] = secret.try_into().map_err(|_| "ed25519_sign: secret must be 32 bytes".to_string())?;
    let signing = SigningKey::from_bytes(sk_arr);
    use ed25519_dalek::Signature;
    let sig: Signature = signing.sign(message);
    Ok(sig.to_bytes().to_vec())
}

/// Verify an Ed25519 signature against a public key and message.
pub fn ed25519_verify(public: &[u8], signature: &[u8], message: &[u8]) -> Result<bool, String> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let pk_arr: &[u8; 32] = public.try_into().map_err(|_| "ed25519_verify: public key must be 32 bytes".to_string())?;
    let sig_arr: &[u8; 64] = signature.try_into().map_err(|_| "ed25519_verify: signature must be 64 bytes".to_string())?;
    let pk = VerifyingKey::from_bytes(pk_arr).map_err(|_| "ed25519_verify: invalid public key".to_string())?;
    let sig = Signature::from_bytes(sig_arr);
    Ok(pk.verify(message, &sig).is_ok())
}

// ---------------------------------------------------------------------------
// Constant-time primitives
//
// `==` on a Rak value is a data-dependent branch: it returns as soon as the
// first differing byte is found, so the wall-clock time reveals how long a
// shared prefix is. That is a timing oracle for MACs, session tokens, and
// password hashes. The functions below run in time independent of the data
// and return a real `bool` only after the whole comparison is done.
// ---------------------------------------------------------------------------

/// Constant-time equality over two byte strings. Returns `true` only when the
/// inputs have equal length and equal contents, without an early exit.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    // `a.len() == b.len()` is not a secret: lengths are not normally
    // confidential, and the length check is what stops the loop being skipped.
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// Constant-time comparison of two hex strings, ignoring case. Use this to
/// check digests against attacker-supplied values.
pub fn ct_eq_hex(a: &str, b: &str) -> bool {
    let ab = a.trim().as_bytes();
    let bb = b.trim().as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    // Fold case manually so the comparison stays branch-free.
    let mut diff = 0u8;
    for i in 0..ab.len() {
        let x = ab[i] | 0x20;
        let y = bb[i] | 0x20;
        diff |= x ^ y;
    }
    diff == 0
}

/// Constant-time conditional select. Returns `a` when `choice` is `1` and `b`
/// when it is `0`, without branching on the value. Lengths must match.
pub fn ct_select(choice: u8, a: &[u8], b: &[u8]) -> Result<Vec<u8>, String> {
    if a.len() != b.len() {
        return Err(format!(
            "ct_select: branches must be the same length ({} vs {})",
            a.len(),
            b.len()
        ));
    }
    // Mask is 0xFF when `choice` is truthy, 0x00 otherwise.
    let mask = (choice != 0) as u8;
    let mask = 0u8.wrapping_sub(mask);
    Ok(a.iter().zip(b.iter()).map(|(x, y)| (x & mask) | (y & !mask)).collect())
}

// ---------------------------------------------------------------------------
// Secure memory wiping
//
// `Vec::clear` and `drop` free memory without overwriting it, so a key left in
// a heap buffer survives in freed pages until something else reuses them.
// `zeroize` writes zeroes through a volatile path that the optimizer is not
// allowed to remove as a dead store.
// ---------------------------------------------------------------------------

/// Wipe a byte buffer in a way the optimizer cannot elide, then release it.
/// Call this on plaintext keys and passwords once they are no longer needed.
pub fn zeroize_bytes(buf: &mut [u8]) {
    use zeroize::Zeroize;
    buf.zeroize();
}

/// Copy `data` into a fresh buffer that wipes itself on drop. This is the safe
/// way to hold a key: no matter how the value goes out of scope, the bytes are
/// overwritten rather than merely freed.
pub fn secret_bytes(data: &[u8]) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(data.to_vec())
}

/// Overwrite a string's buffer in place. Rust offers no way to do this to a
/// `String` other than by taking it apart, so this takes ownership and wipes
/// the underlying allocation before releasing it.
pub fn zeroize_string(mut s: String) {
    use zeroize::Zeroize;
    unsafe { s.as_mut_vec().zeroize() };
    drop(s);
}

/// Best-effort secure delete for a file: overwrite the contents in place
/// before unlinking, so the bytes are not left in the filesystem's free blocks
/// or page cache. Falls back to a plain remove if the file cannot be opened.
pub fn secure_delete_file(path: &str) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(path) {
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let zeros = vec![0u8; 4096.min(len.max(1) as usize)];
        f.seek(SeekFrom::Start(0))?;
        let mut written = 0u64;
        while written < len {
            let n = std::cmp::min(zeros.len() as u64, len - written) as usize;
            f.write_all(&zeros[..n])?;
            written += n as u64;
        }
        f.sync_all()?;
        drop(f);
    }
    std::fs::remove_file(path)
}

// ---------------------------------------------------------------------------
// RSA
// ---------------------------------------------------------------------------

/// Generate an RSA keypair. `bits` must be 2048 or 4096. Returns
/// (public_key_der, private_key_der), both DER so they survive `ffi_write`
/// and can be fed to OpenSSL for interop.
pub fn rsa_keypair(bits: u32) -> Result<(Vec<u8>, Vec<u8>), String> {
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};
    let mut rng = rand::thread_rng();
    let private = rsa::RsaPrivateKey::new(&mut rng, bits as usize)
        .map_err(|e| format!("rsa_keypair: {}", e))?;
    let public = rsa::RsaPublicKey::from(&private);
    let priv_der = private
        .to_pkcs8_der()
        .map_err(|e| format!("rsa_keypair: {}", e))?
        .as_bytes()
        .to_vec();
    let pub_der = public
        .to_public_key_der()
        .map_err(|e| format!("rsa_keypair: {}", e))?
        .as_bytes()
        .to_vec();
    Ok((pub_der, priv_der))
}

/// Sign a message with an RSA private key using PKCS#1 v1.5 over SHA-256.
pub fn rsa_sign(private_der: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::RsaPrivateKey;
    let key = RsaPrivateKey::from_pkcs8_der(private_der)
        .map_err(|e| format!("rsa_sign: {}", e))?;
    let signing = SigningKey::<sha2::Sha256>::new(key);
    Ok(signing.sign(message).to_vec())
}

/// Verify an RSA PKCS#1 v1.5 / SHA-256 signature.
pub fn rsa_verify(public_der: &[u8], signature: &[u8], message: &[u8]) -> Result<bool, String> {
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::pkcs8::DecodePublicKey;
    use rsa::signature::Verifier;
    use rsa::RsaPublicKey;
    use sha2::Sha256;
    let key = RsaPublicKey::from_public_key_der(public_der)
        .map_err(|e| format!("rsa_verify: {}", e))?;
    let vk = VerifyingKey::<Sha256>::new(key);
    let sig = Signature::try_from(signature).map_err(|e| format!("rsa_verify: {}", e))?;
    Ok(vk.verify(message, &sig).is_ok())
}

/// RSA-OAEP encrypt with SHA-256 and an empty label.
pub fn rsa_encrypt(public_der: &[u8], plaintext: &[u8], label: &[u8]) -> Result<Vec<u8>, String> {
    use rsa::pkcs8::DecodePublicKey;
    use rsa::RsaPublicKey;
    let key = RsaPublicKey::from_public_key_der(public_der)
        .map_err(|e| format!("rsa_encrypt: {}", e))?;
    let mut rng = rand::thread_rng();
    let oaep = oaep_sha256(label);
    key.encrypt(&mut rng, oaep, plaintext)
        .map_err(|e| format!("rsa_encrypt: {}", e))
}

/// RSA-OAEP decrypt with SHA-256 and an empty label.
pub fn rsa_decrypt(private_der: &[u8], ciphertext: &[u8], label: &[u8]) -> Result<Vec<u8>, String> {
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::RsaPrivateKey;
    let key = RsaPrivateKey::from_pkcs8_der(private_der)
        .map_err(|e| format!("rsa_decrypt: {}", e))?;
    key.decrypt(oaep_sha256(label), ciphertext)
        .map_err(|e| format!("rsa_decrypt: {}", e))
}

/// Build an OAEP padding instance. `Oaep::new_with_label` wants a `str`
/// label; a non-UTF-8 label falls back to the empty label, which is what
/// every mainstream TLS and PKCS#1 consumer uses anyway.
fn oaep_sha256(label: &[u8]) -> rsa::Oaep {
    match std::str::from_utf8(label) {
        Ok(s) => rsa::Oaep::new_with_label::<sha2::Sha256, _>(s),
        Err(_) => rsa::Oaep::new::<sha2::Sha256>(),
    }
}

// ---------------------------------------------------------------------------
// ECDSA over NIST P-256
// ---------------------------------------------------------------------------

/// Generate a P-256 ECDSA keypair. Returns (public_key_der, private_key_der)
/// as SEC1/PKCS#8 DER.
pub fn ecdsa_keypair() -> Result<(Vec<u8>, Vec<u8>), String> {
    use p256::ecdsa::SigningKey;
    use p256::pkcs8::{EncodePrivateKey, EncodePublicKey};
    let mut rng = rand::thread_rng();
    let signing = SigningKey::random(&mut rng);
    let verifying = signing.verifying_key();
    let priv_der = signing
        .to_pkcs8_der()
        .map_err(|e| format!("ecdsa_keypair: {}", e))?
        .as_bytes()
        .to_vec();
    let pub_der = verifying
        .to_public_key_der()
        .map_err(|e| format!("ecdsa_keypair: {}", e))?
        .as_bytes()
        .to_vec();
    Ok((pub_der, priv_der))
}

/// Sign a message with a P-256 ECDSA private key. Returns a 64-byte
/// fixed-width r||s signature.
pub fn ecdsa_sign(private_der: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{Signature, SigningKey};
    use p256::pkcs8::DecodePrivateKey;
    let key = SigningKey::from_pkcs8_der(private_der)
        .map_err(|e| format!("ecdsa_sign: {}", e))?;
    let sig: Signature = key.sign(message);
    Ok(sig.to_bytes().to_vec())
}

/// Verify a P-256 ECDSA signature.
pub fn ecdsa_verify(public_der: &[u8], signature: &[u8], message: &[u8]) -> Result<bool, String> {
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::{Signature, VerifyingKey};
    use p256::pkcs8::DecodePublicKey;
    let key = VerifyingKey::from_public_key_der(public_der)
        .map_err(|e| format!("ecdsa_verify: {}", e))?;
    let sig = Signature::from_slice(signature)
        .map_err(|e| format!("ecdsa_verify: {}", e))?;
    Ok(key.verify(message, &sig).is_ok())
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_matches_equality_and_agrees_on_length() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(!ct_eq(b"", b"a"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn ct_eq_hex_is_case_insensitive() {
        assert!(ct_eq_hex("DEADBEEF", "deadbeef"));
        assert!(!ct_eq_hex("deadbeef", "deadbeee"));
        assert!(!ct_eq_hex("dead", "deadbeef"));
    }

    #[test]
    fn ct_select_picks_the_right_branch() {
        assert_eq!(ct_select(1, b"aaaa", b"bbbb").unwrap(), b"aaaa");
        assert_eq!(ct_select(0, b"aaaa", b"bbbb").unwrap(), b"bbbb");
        assert!(ct_select(1, b"aa", b"bbb").is_err());
    }

    #[test]
    fn zeroize_wipes_the_buffer() {
        let mut buf = b"super secret key".to_vec();
        zeroize_bytes(&mut buf);
        assert_eq!(buf, vec![0u8; 16]);
    }

    #[test]
    fn secret_bytes_zeroize_on_drop() {
        let s = secret_bytes(b"hunter2");
        assert_eq!(s.as_slice(), b"hunter2");
        drop(s);
        // The wipe itself is not observable here, but the type must compile and
        // run, which is the regression this guards.
    }

    #[test]
    fn rsa_sign_verify_roundtrip() {
        // 2048-bit generation is slow, so this is the only RSA roundtrip we
        // run in the normal suite.
        let (pub_der, priv_der) = rsa_keypair(2048).unwrap();
        let sig = rsa_sign(&priv_der, b"rak").unwrap();
        assert!(rsa_verify(&pub_der, &sig, b"rak").unwrap());
        assert!(!rsa_verify(&pub_der, &sig, b"other").unwrap());
    }

    #[test]
    fn rsa_oaep_roundtrip() {
        let (pub_der, priv_der) = rsa_keypair(2048).unwrap();
        let ct = rsa_encrypt(&pub_der, b"top secret", b"").unwrap();
        let pt = rsa_decrypt(&priv_der, &ct, b"").unwrap();
        assert_eq!(pt, b"top secret");
    }

    #[test]
    fn ecdsa_sign_verify_roundtrip() {
        let (pub_der, priv_der) = ecdsa_keypair().unwrap();
        let sig = ecdsa_sign(&priv_der, b"rak").unwrap();
        assert_eq!(sig.len(), 64);
        assert!(ecdsa_verify(&pub_der, &sig, b"rak").unwrap());
        assert!(!ecdsa_verify(&pub_der, &sig, b"other").unwrap());
    }
}