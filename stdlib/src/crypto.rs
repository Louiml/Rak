use md5::{Md5, Digest};

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

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}