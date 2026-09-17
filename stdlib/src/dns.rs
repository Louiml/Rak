//! DNS wire-format builder/parser and UDP query.
//!
//! Hand-rolled (no external DNS crate) so it works in air-gapped OSINT setups.
//! Record types A/AAAA/MX/TXT/CNAME/PTR/NS. Compression pointers are decoded.

use std::net::UdpSocket;

#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub rtype: String,
    pub ttl: u32,
    pub rdata: String,
}

#[derive(Debug, Clone)]
pub struct DnsResponse {
    pub answers: Vec<DnsRecord>,
    pub truncated: bool,
}

/// Map a record-type string to its DNS type number.
pub fn rtype_num(rtype: &str) -> u16 {
    match rtype.to_uppercase().as_str() {
        "A" => 1,
        "NS" => 2,
        "CNAME" => 5,
        "PTR" => 12,
        "MX" => 15,
        "TXT" => 16,
        "AAAA" => 28,
        "SOA" => 6,
        _ => 1,
    }
}

fn rtype_name(t: u16) -> String {
    match t {
        1 => "A", 2 => "NS", 5 => "CNAME", 6 => "SOA", 12 => "PTR",
        15 => "MX", 16 => "TXT", 28 => "AAAA", _ => "OTHER",
    }.to_string()
}

/// Encode a dotted name into DNS label format.
fn encode_name(name: &str, out: &mut Vec<u8>) {
    for label in name.trim_matches('.').split('.') {
        if label.is_empty() {
            continue;
        }
        let b = label.as_bytes();
        out.push(b.len() as u8);
        out.extend_from_slice(b);
    }
    out.push(0);
}

/// Decode a name starting at `offset`, following compression pointers. Returns
/// (name, next offset after the name's own bytes — not after the pointer).
fn decode_name(msg: &[u8], offset: usize) -> Result<(String, usize), String> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = offset;
    let mut jumped = false;
    let mut after = offset;
    let mut hops = 0;
    loop {
        if pos >= msg.len() {
            return Err("dns: truncated name".to_string());
        }
        let len = msg[pos];
        if len == 0 {
            pos += 1;
            if !jumped {
                after = pos;
            }
            break;
        }
        if len & 0xC0 == 0xC0 {
            // Compression pointer.
            if pos + 1 >= msg.len() {
                return Err("dns: truncated pointer".to_string());
            }
            let ptr = ((len as usize & 0x3F) << 8) | msg[pos + 1] as usize;
            if !jumped {
                after = pos + 2;
            }
            pos = ptr;
            jumped = true;
            hops += 1;
            if hops > 16 {
                return Err("dns: too many compression hops".to_string());
            }
            continue;
        }
        let end = pos + 1 + len as usize;
        if end > msg.len() {
            return Err("dns: truncated label".to_string());
        }
        labels.push(String::from_utf8_lossy(&msg[pos + 1..end]).to_string());
        pos = end;
    }
    Ok((labels.join("."), after))
}

/// Build a DNS query message for `name`/`rtype`.
pub fn build_query(name: &str, rtype: &str) -> Vec<u8> {
    let mut msg = Vec::with_capacity(32);
    // Header: id=0x1234, flags=0x0100 (RD), qdcount=1, rest 0.
    msg.extend_from_slice(&[0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
    encode_name(name, &mut msg);
    msg.extend_from_slice(&rtype_num(rtype).to_be_bytes());
    msg.extend_from_slice(&1u16.to_be_bytes()); // class IN
    msg
}

/// Parse a DNS response message into answers.
pub fn parse_response(msg: &[u8]) -> Result<DnsResponse, String> {
    if msg.len() < 12 {
        return Err("dns: response too short".to_string());
    }
    let flags = u16::from_be_bytes([msg[2], msg[3]]);
    let truncated = flags & 0x0200 != 0;
    let qdcount = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let ancount = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut pos = 12usize;
    // Skip questions.
    for _ in 0..qdcount {
        let (_, next) = decode_name(msg, pos)?;
        pos = next + 4; // type + class
    }
    let mut answers = Vec::with_capacity(ancount);
    for _ in 0..ancount {
        let (name, next) = decode_name(msg, pos).unwrap_or(("".to_string(), pos));
        pos = next;
        if pos + 10 > msg.len() {
            return Err("dns: truncated answer".to_string());
        }
        let t = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
        let ttl = u32::from_be_bytes([msg[pos + 4], msg[pos + 5], msg[pos + 6], msg[pos + 7]]);
        let rdlen = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > msg.len() {
            return Err("dns: truncated rdata".to_string());
        }
        let rdata = match t {
            1 => {
                // A record: 4 bytes -> IPv4.
                if rdlen == 4 {
                    format!("{}.{}.{}.{}", msg[pos], msg[pos + 1], msg[pos + 2], msg[pos + 3])
                } else {
                    String::from_utf8_lossy(&msg[pos..pos + rdlen]).to_string()
                }
            }
            28 => {
                // AAAA: 16 bytes -> IPv6 (colon form, simple).
                if rdlen == 16 {
                    let segs: Vec<String> = (0..8).map(|i| {
                        format!("{:02x}{:02x}", msg[pos + 2 * i], msg[pos + 2 * i + 1])
                    }).collect();
                    segs.join(":")
                } else {
                    String::from_utf8_lossy(&msg[pos..pos + rdlen]).to_string()
                }
            }
            5 | 2 | 12 | 6 => {
                // CNAME / NS / PTR / SOA-name: a domain name.
                let (n, _) = decode_name(msg, pos).unwrap_or((String::from_utf8_lossy(&msg[pos..pos + rdlen]).to_string(), pos + rdlen));
                n
            }
            15 => {
                // MX: preference u16 + name.
                if rdlen >= 3 {
                    let pref = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
                    let (n, _) = decode_name(msg, pos + 2).unwrap_or_default();
                    format!("{} {}", pref, n)
                } else {
                    String::new()
                }
            }
            _ => String::from_utf8_lossy(&msg[pos..pos + rdlen]).to_string(),
        };
        answers.push(DnsRecord { name, rtype: rtype_name(t), ttl, rdata });
        pos += rdlen;
    }
    Ok(DnsResponse { answers, truncated })
}

/// Query a DNS server (default `8.8.8.8:53`) for `name`/`rtype`.
pub fn query(name: &str, rtype: &str, server: Option<&str>) -> Result<DnsResponse, String> {
    let server = server.unwrap_or("8.8.8.8:53").to_string();
    let q = build_query(name, rtype);
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("dns: bind: {}", e))?;
    sock.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
    sock.send_to(&q, &server).map_err(|e| format!("dns: send: {}", e))?;
    let mut buf = vec![0u8; 4096];
    let (n, _) = sock.recv_from(&mut buf).map_err(|e| format!("dns: recv: {}", e))?;
    parse_response(&buf[..n])
}

/// Resolve a hostname, returning all A/AAAA address strings.
pub fn resolve(name: &str, server: Option<&str>) -> Result<Vec<String>, String> {
    let mut addrs = Vec::new();
    for rtype in ["A", "AAAA"] {
        if let Ok(resp) = query(name, rtype, server) {
            for r in resp.answers {
                let v = r.rdata.clone();
                if !addrs.contains(&v) {
                    addrs.push(v);
                }
            }
        }
    }
    Ok(addrs)
}

/// Reverse-DNS lookup for an IPv4 or IPv6 address. Returns PTR names.
pub fn reverse(ip: &str, server: Option<&str>) -> Result<Vec<String>, String> {
    let ptr_name = ptr_for(ip)?;
    let resp = query(&ptr_name, "PTR", server)?;
    Ok(resp.answers.into_iter().map(|r| r.rdata).filter(|s| !s.is_empty()).collect())
}

fn ptr_for(ip: &str) -> Result<String, String> {
    if ip.contains(':') {
        // IPv6: split on '::' if present, expand to 8 groups of 4 hex digits,
        // then reverse each nibble across the whole address per RFC 3596.
        let (head, tail) = match ip.find("::") {
            Some(i) => (&ip[..i], &ip[i + 2..]),
            None => (ip, ""),
        };
        let head_groups: Vec<&str> = if head.is_empty() { Vec::new() } else { head.split(':').collect() };
        let tail_groups: Vec<&str> = if tail.is_empty() { Vec::new() } else { tail.split(':').collect() };
        let missing = 8usize.saturating_sub(head_groups.len() + tail_groups.len());
        let mut groups: Vec<String> = Vec::with_capacity(8);
        for g in &head_groups {
            groups.push(format!("{:0>4}", g));
        }
        for _ in 0..missing {
            groups.push("0000".to_string());
        }
        for g in &tail_groups {
            groups.push(format!("{:0>4}", g));
        }
        if groups.len() != 8 {
            return Err(format!("dns_reverse: bad IPv6 '{}'", ip));
        }
        let mut nibbles = String::new();
        for g in &groups {
            nibbles.push_str(&g.to_lowercase());
        }
        let rev: String = nibbles.chars().rev().collect();
        let dotted = rev.chars().map(|c| c.to_string()).collect::<Vec<_>>().join(".");
        Ok(format!("{}.ip6.arpa", dotted))
    } else {
        let parts: Vec<&str> = ip.split('.').collect();
        if parts.len() != 4 {
            return Err(format!("dns_reverse: bad IPv4 '{}'", ip));
        }
        let mut rev: Vec<String> = parts.iter().rev().map(|s| s.to_string()).collect();
        rev.push("in-addr.arpa".to_string());
        Ok(rev.join("."))
    }
}

/// Query all supported record types for a name, returning flattened records.
pub fn records(name: &str, server: Option<&str>) -> Result<Vec<DnsRecord>, String> {
    let mut out = Vec::new();
    for rtype in ["A", "AAAA", "MX", "TXT", "CNAME", "NS", "PTR", "SOA"] {
        if let Ok(resp) = query(name, rtype, server) {
            out.extend(resp.answers);
        }
    }
    Ok(out)
}

/// Walk a domain's common subdomains, returning those that resolve (have any
/// record). `prefixes` is a comma-separated dictionary.
pub fn walk(domain: &str, prefixes: &[String], server: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    for p in prefixes {
        let sub = format!("{}.{}", p, domain);
        if query(&sub, "A", server).map(|r| !r.answers.is_empty()).unwrap_or(false) {
            out.push(sub);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_query_shape() {
        let q = build_query("example.com", "A");
        assert_eq!(&q[..2], &[0x12, 0x34]); // id
        assert_eq!(q[2], 0x01); // RD flag
        // Name starts at offset 12: 7example3com0 (13 bytes) -> type at 25.
        assert_eq!(q[12], 7);
        assert_eq!(&q[25..27], &[0, 1]); // type A = 1 (big-endian)
        assert_eq!(&q[27..29], &[0, 1]); // class IN
    }

    #[test]
    fn parse_crafted_a_response() {
        // Craft a minimal DNS response: 1 question (example.com A), 1 answer
        // (example.com A 60 1.2.3.4).
        let mut msg = build_query("example.com", "A");
        // Set ancount=1.
        msg[6] = 0; msg[7] = 1;
        // Answer: name pointer to offset 12, type A, class IN, ttl 60, rdlen 4, rdata 1.2.3.4.
        let mut ans = vec![0xC0, 0x0C];          // pointer to offset 12
        ans.extend_from_slice(&1u16.to_be_bytes()); // type A
        ans.extend_from_slice(&1u16.to_be_bytes()); // class IN
        ans.extend_from_slice(&60u32.to_be_bytes()); // ttl
        ans.extend_from_slice(&4u16.to_be_bytes()); // rdlen
        ans.extend_from_slice(&[1, 2, 3, 4]);
        msg.extend_from_slice(&ans);
        let resp = parse_response(&msg).unwrap();
        assert!(!resp.truncated);
        assert_eq!(resp.answers.len(), 1);
        assert_eq!(resp.answers[0].rtype, "A");
        assert_eq!(resp.answers[0].rdata, "1.2.3.4");
        assert_eq!(resp.answers[0].ttl, 60);
    }
}
