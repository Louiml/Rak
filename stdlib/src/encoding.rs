/// Encode bytes to hex string
pub fn hex_encode(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Decode hex string to bytes
pub fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        if let Ok(byte) = u8::from_str_radix(&hex[i..i+2], 16) {
            bytes.push(byte);
        } else {
            return None;
        }
    }
    Some(bytes)
}

/// Encode bytes to base64
pub fn base64_encode(data: &[u8]) -> String {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.encode(data)
}

/// Decode base64 to bytes
pub fn base64_decode(data: &str) -> Option<Vec<u8>> {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.decode(data).ok()
}

/// URL encode a string
pub fn url_encode(input: &str) -> String {
    urlencoding::encode(input).to_string()
}

/// URL decode a string
pub fn url_decode(input: &str) -> Option<String> {
    urlencoding::decode(input).ok().map(|s| s.to_string())
}

/// Convert string to bytes
pub fn to_bytes(input: &str) -> Vec<u8> {
    input.bytes().collect()
}

/// Convert bytes to string (lossy)
pub fn from_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}