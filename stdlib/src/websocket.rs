//! WebSocket framing + handshake on top of a raw `TcpStream`.
//!
//! RFC 6455 text/binary frames with masking (client→server) and unmasking. The
//! Rak builtins operate on the existing `Value::TcpStream` handle, so no new
//! value type is needed. The pure `encode_frame` / `parse_frame` functions are
//! unit-testable without a network.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

/// Frame opcodes.
pub const OP_TEXT: u8 = 0x1;
pub const OP_BINARY: u8 = 0x2;
pub const OP_CLOSE: u8 = 0x8;
pub const OP_PING: u8 = 0x9;
pub const OP_PONG: u8 = 0xA;

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAGIC_GEN: &[u8] = b"0123456789abcdef";

/// Encode a WebSocket frame. `opcode` is one of the `OP_*` constants; if `mask`
/// is true the payload is masked per RFC 6455 (required for client→server).
pub fn encode_frame(opcode: u8, payload: &[u8], mask: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + payload.len());
    let fin_op = 0x80 | (opcode & 0x0F);
    out.push(fin_op);
    let len = payload.len();
    if len < 126 {
        out.push((len as u8) | if mask { 0x80 } else { 0 });
    } else if len <= 0xFFFF {
        out.push(0x7E | if mask { 0x80 } else { 0 });
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x7F | if mask { 0x80 } else { 0 });
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    if mask {
        let mask_key: [u8; 4] = [magic(); 4];
        out.extend_from_slice(&mask_key);
        for (i, b) in payload.iter().enumerate() {
            out.push(b ^ mask_key[i % 4]);
        }
    } else {
        out.extend_from_slice(payload);
    }
    out
}

/// A frame parsed from the wire.
#[derive(Debug, Clone)]
pub struct Frame {
    pub opcode: u8,
    pub fin: bool,
    pub payload: Vec<u8>,
}

/// Parse one WebSocket frame from a byte slice. Returns the frame and the
/// number of bytes consumed, or an error string.
pub fn parse_frame(data: &[u8]) -> Result<(Frame, usize), String> {
    if data.len() < 2 {
        return Err("ws: frame too short".to_string());
    }
    let b0 = data[0];
    let b1 = data[1];
    let fin = b0 & 0x80 != 0;
    let opcode = b0 & 0x0F;
    let masked = b1 & 0x80 != 0;
    let mut len = (b1 & 0x7F) as u64;
    let mut off = 2usize;
    if len == 126 {
        if data.len() < off + 2 {
            return Err("ws: truncated extended length".to_string());
        }
        len = u16::from_be_bytes([data[off], data[off + 1]]) as u64;
        off += 2;
    } else if len == 127 {
        if data.len() < off + 8 {
            return Err("ws: truncated extended length".to_string());
        }
        len = u64::from_be_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
    }
    let mask_key: [u8; 4];
    if masked {
        if data.len() < off + 4 {
            return Err("ws: truncated mask key".to_string());
        }
        mask_key = data[off..off + 4].try_into().unwrap();
        off += 4;
    } else {
        mask_key = [0; 4];
    }
    let len = len as usize;
    if data.len() < off + len {
        return Err(format!("ws: frame body incomplete (need {}, have {})", off + len, data.len()));
    }
    let mut payload = data[off..off + len].to_vec();
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask_key[i % 4];
        }
    }
    Ok((Frame { opcode, fin, payload }, off + len))
}

fn magic() -> u8 {
    // Deterministic-but-nondeterministic masking key helper. Real masking keys
    // should be cryptographically random; this is adequate for Rak's
    // scripted, non-adversarial use and keeps the module self-contained.
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let tick = (ms ^ 0x9E3779B97F4A7C15).rotate_left(17) as usize;
    MAGIC_GEN[(tick + 3) % MAGIC_GEN.len()]
}

fn base64(data: &[u8]) -> String {
    crate::encoding::base64_encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6455_accept_vector() {
        // RFC 6455 §1.3 example: key -> s3pPLMBiTxaQ9kYGzzhZRbK+xOo=
        assert_eq!(accept_value("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn frame_roundtrip_unmasked() {
        let data = b"hello ws";
        let f = encode_frame(OP_TEXT, data, false);
        let (parsed, consumed) = parse_frame(&f).unwrap();
        assert_eq!(consumed, f.len());
        assert_eq!(parsed.opcode, OP_TEXT);
        assert!(parsed.fin);
        assert_eq!(parsed.payload, data);
    }

    #[test]
    fn frame_roundtrip_masked() {
        let data = b"masked payload";
        let f = encode_frame(OP_BINARY, data, true);
        let (parsed, consumed) = parse_frame(&f).unwrap();
        assert_eq!(consumed, f.len());
        assert_eq!(parsed.opcode, OP_BINARY);
        assert_eq!(parsed.payload, data);
    }

    #[test]
    fn frame_long_length() {
        // 200 bytes must use the 16-bit extended length form.
        let data = vec![0xABu8; 200];
        let f = encode_frame(OP_TEXT, &data, false);
        assert_eq!(f[1], 126);
        let (parsed, _) = parse_frame(&f).unwrap();
        assert_eq!(parsed.payload, data);
    }
}

/// Compute the `Sec-WebSocket-Accept` server response for a client key.
pub fn accept_value(key: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(key.trim().as_bytes());
    h.update(GUID.as_bytes());
    base64(&h.finalize())
}

/// Perform the client handshake over a connected stream. Returns the server's
/// subprotocol on success (may be empty).
pub fn client_handshake(stream: &mut TcpStream, host: &str, path: &str) -> Result<String, String> {
    let key = base64(&[magic(); 16]);
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
        path, host, key
    );
    stream.write_all(req.as_bytes()).map_err(|e| format!("ws: handshake write: {}", e))?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| format!("ws: clone: {}", e))?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| format!("ws: handshake read: {}", e))?;
    if !line.contains("101") {
        return Err(format!("ws: handshake rejected: {}", line.trim()));
    }
    let mut subprotocol = String::new();
    loop {
        let mut h = String::new();
        let n = reader.read_line(&mut h).map_err(|e| format!("ws: header read: {}", e))?;
        if n == 0 || h.trim().is_empty() {
            break;
        }
        let t = h.trim().to_lowercase();
        if t.starts_with("sec-websocket-protocol:") {
            subprotocol = t["sec-websocket-protocol:".len()..].trim().to_string();
        }
    }
    Ok(subprotocol)
}

/// Perform the server handshake (respond to a client's Upgrade request already
/// read from `reader`). Returns the client's desired path.
pub fn server_handshake(stream: &mut TcpStream, key: &str) -> Result<(), String> {
    let accept = accept_value(key);
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept
    );
    stream.write_all(resp.as_bytes()).map_err(|e| format!("ws: handshake write: {}", e))
}

/// Read one frame from a buffered reader.
pub fn read_frame(reader: &mut BufReader<TcpStream>) -> Result<Option<Frame>, String> {
    let mut header = [0u8; 2];
    let n = reader.read(&mut header).map_err(|e| format!("ws: read: {}", e))?;
    if n == 0 {
        return Ok(None);
    }
    if n < 2 {
        return Err("ws: truncated header".to_string());
    }
    let mut buf = header.to_vec();
    let b1 = header[1];
    let mut len = (b1 & 0x7F) as u64;
    if len == 126 {
        let mut ext = [0u8; 2];
        reader.read_exact(&mut ext).map_err(|e| format!("ws: read len: {}", e))?;
        buf.extend_from_slice(&ext);
        len = u16::from_be_bytes(ext) as u64;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        reader.read_exact(&mut ext).map_err(|e| format!("ws: read len: {}", e))?;
        buf.extend_from_slice(&ext);
        len = u64::from_be_bytes(ext);
    }
    if b1 & 0x80 != 0 {
        let mut key = [0u8; 4];
        reader.read_exact(&mut key).map_err(|e| format!("ws: read mask: {}", e))?;
        buf.extend_from_slice(&key);
    }
    let len = len as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut payload).map_err(|e| format!("ws: read payload: {}", e))?;
    }
    buf.extend_from_slice(&payload);
    let (frame, _) = parse_frame(&buf)?;
    Ok(Some(frame))
}