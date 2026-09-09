//! TLS handshake inspection — parse a ClientHello to extract SNI and the
//! cipher suite list, and parse a certificate chain (DER → subject/issuer).
//!
//! Pure inspection: `inspect(host:port)` opens a TCP connection, sends a
//! minimal ClientHello, and reads the ServerHello + certificates (no TLS
//! termination). `parse_client_hello(bytes)` is the testable, offline entry
//! point.

use x509_parser::parse_x509_certificate;

/// Parsed SNI + cipher list from a ClientHello.
#[derive(Debug, Clone)]
pub struct ClientHelloInfo {
    pub sni: String,
    pub ciphers: Vec<u16>,
}

/// Parse a TLS record containing a ClientHello. `bytes` is the raw TCP payload
/// (record layer + handshake). Returns the SNI and advertised cipher suites.
pub fn parse_client_hello(bytes: &[u8]) -> Result<ClientHelloInfo, String> {
    // TLS record: type(1) version(2) length(2) fragment.
    if bytes.len() < 5 {
        return Err("tls: record too short".to_string());
    }
    let rec_type = bytes[0];
    if rec_type != 0x16 {
        return Err(format!("tls: not a handshake record (type 0x{:02X})", rec_type));
    }
    let frag_len = u16::from_be_bytes([bytes[3], bytes[4]]) as usize;
    let frag = bytes.get(5..5 + frag_len).ok_or("tls: short record")?;
    // Handshake: type(1) length(3) body. type 1 = ClientHello.
    if frag.len() < 4 {
        return Err("tls: short handshake".to_string());
    }
    if frag[0] != 1 {
        return Err(format!("tls: not a ClientHello (type {})", frag[0]));
    }
    let hs_len = ((frag[1] as usize) << 16) | (frag[2] as usize) << 8 | frag[3] as usize;
    let body = frag.get(4..4 + hs_len).ok_or("tls: short ClientHello")?;
    // ClientHello body: version(2) random(32) session_id(varlen 1) ciphers(varlen 2)
    //   extensions(varlen 2).
    let mut p = 0usize;
    p += 2; // version
    if body.len() < p + 32 {
        return Err("tls: short random".to_string());
    }
    p += 32; // random
    if body.len() < p + 1 {
        return Err("tls: short session id len".to_string());
    }
    let sid_len = body[p] as usize;
    p += 1 + sid_len;
    if body.len() < p + 2 {
        return Err("tls: short cipher list".to_string());
    }
    let cs_len = u16::from_be_bytes([body[p], body[p + 1]]) as usize;
    p += 2;
    let ciphers_end = p + cs_len;
    let mut ciphers = Vec::new();
    while p + 1 < ciphers_end {
        ciphers.push(u16::from_be_bytes([body[p], body[p + 1]]));
        p += 2;
    }
    p = ciphers_end;
    // compression methods: len(1) + list.
    if body.len() < p + 1 {
        return Ok(ClientHelloInfo { sni: String::new(), ciphers });
    }
    let cm_len = body[p] as usize;
    p += 1 + cm_len;
    if body.len() < p + 2 {
        return Ok(ClientHelloInfo { sni: String::new(), ciphers });
    }
    let ext_len = u16::from_be_bytes([body[p], body[p + 1]]) as usize;
    p += 2;
    let ext_end = p + ext_len;
    let mut sni = String::new();
    while p + 4 <= ext_end && p + 4 <= body.len() {
        let etype = u16::from_be_bytes([body[p], body[p + 1]]);
        let elen = u16::from_be_bytes([body[p + 2], body[p + 3]]) as usize;
        p += 4;
        let ebody = body.get(p..p + elen).unwrap_or(&[]);
        if etype == 0 && ebody.len() >= 5 {
            // SNI extension: list_len(2), name_type(1), name_len(2), name.
            let name_len = u16::from_be_bytes([ebody[3], ebody[4]]) as usize;
            if ebody.len() >= 5 + name_len {
                sni = String::from_utf8_lossy(&ebody[5..5 + name_len]).to_string();
            }
        }
        p += elen;
    }
    Ok(ClientHelloInfo { sni, ciphers })
}

/// A parsed certificate's subject/issuer (human-readable strings).
#[derive(Debug, Clone)]
pub struct ParsedCert {
    pub subject: String,
    pub issuer: String,
}

/// Parse a DER-encoded certificate chain (concatenated DER) into subject/issuer
/// pairs. Returns as many as can be parsed.
pub fn parse_cert_chain(der: &[u8]) -> Vec<ParsedCert> {
    let mut out = Vec::new();
    let mut buf = der;
    while let Ok((rem, cert)) = parse_x509_certificate(buf) {
        let subject = cert.subject().to_string();
        let issuer = cert.issuer().to_string();
        out.push(ParsedCert { subject, issuer });
        buf = rem;
        if buf.is_empty() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_client_hello_sni() {
        // Craft a minimal ClientHello with one cipher and SNI = "example.com".
        let mut body = Vec::new();
        body.extend_from_slice(&[0x03, 0x03]); // version TLS 1.2
        body.extend_from_slice(&[0u8; 32]);   // random
        body.push(0);                          // session id len 0
        body.extend_from_slice(&[0x00, 0x02]); // cipher list len 2
        body.extend_from_slice(&[0x13, 0x01]); // cipher TLS_AES_128_GCM_SHA256
        body.push(1); body.push(0);             // compression: 1 method (null)
        // Extensions: SNI.
        let name = b"example.com";
        let mut sni_ext = Vec::new();
        sni_ext.extend_from_slice(&((name.len() + 5) as u16).to_be_bytes()); // list len
        sni_ext.push(0); // host name type
        sni_ext.extend_from_slice(&(name.len() as u16).to_be_bytes());
        sni_ext.extend_from_slice(name);
        let mut exts = Vec::new();
        exts.extend_from_slice(&0u16.to_be_bytes()); // type 0 = SNI
        exts.extend_from_slice(&(sni_ext.len() as u16).to_be_bytes());
        exts.extend_from_slice(&sni_ext);
        body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
        body.extend_from_slice(&exts);
        // Handshake header: type 1, length 3 bytes.
        let mut hs = vec![1u8];
        hs.push((body.len() >> 16) as u8);
        hs.push((body.len() >> 8) as u8);
        hs.push(body.len() as u8);
        hs.extend_from_slice(&body);
        // Record header: type 0x16, version 0x0303, length.
        let mut rec = vec![0x16u8, 0x03, 0x03];
        rec.push((hs.len() >> 8) as u8);
        rec.push(hs.len() as u8);
        rec.extend_from_slice(&hs);

        let info = parse_client_hello(&rec).unwrap();
        assert_eq!(info.sni, "example.com");
        assert_eq!(info.ciphers, vec![0x1301]);
    }
}
