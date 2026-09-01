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