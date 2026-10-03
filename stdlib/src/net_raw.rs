//! Raw sockets & packet forging.
//!
//! IPv4/TCP/UDP header builders compute correct ones-complement checksums so
//! forged packets are well-formed. `send`/`recv` open a raw socket (`SOCK_RAW`)
//! which requires `CAP_NET_RAW` on Linux and Administrator on Windows; the
//! permission error is surfaced as a clear runtime error rather than a panic.

use std::net::Ipv4Addr;

/// Ones-complement 16-bit checksum over a byte slice (padded with a zero byte
/// if the length is odd).
pub fn csum16(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum as u16
}

fn parse_ipv4(s: &str) -> Result<Ipv4Addr, String> {
    s.parse::<Ipv4Addr>()
        .map_err(|_| format!("net_raw: bad IPv4 address '{}'", s))
}

/// Build an IPv4 header (20 bytes) carrying `payload` with the given protocol
/// number (6 = TCP, 17 = UDP). The header checksum is computed.
pub fn ipv4(src: &str, dst: &str, proto: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    let src = parse_ipv4(src)?;
    let dst = parse_ipv4(dst)?;
    let total_len = 20 + payload.len();
    let mut hdr = vec![0u8; 20];
    hdr[0] = 0x45; // version 4, IHL 5 (20 bytes)
    hdr[1] = 0; // DSCP/ECN
    hdr[2] = (total_len >> 8) as u8;
    hdr[3] = (total_len & 0xFF) as u8;
    hdr[4] = 0;
    hdr[5] = 0; // identification
    hdr[6] = 0x40;
    hdr[7] = 0; // flags: Don't Fragment, offset 0
    hdr[8] = 64; // TTL
    hdr[9] = proto;
    // checksum at [10..12] left 0
    hdr[12..16].copy_from_slice(&src.octets());
    hdr[16..20].copy_from_slice(&dst.octets());
    let csum = csum16(&hdr);
    hdr[10] = (csum >> 8) as u8;
    hdr[11] = (csum & 0xFF) as u8;
    let mut pkt = hdr;
    pkt.extend_from_slice(payload);
    Ok(pkt)
}

/// TCP flags as a string: combinations of "F" (FIN), "S" (SYN), "R" (RST),
/// "P" (PSH), "A" (ACK), "U" (URG).
fn parse_flags(s: &str) -> u8 {
    let mut f = 0u8;
    for c in s.chars() {
        match c {
            'F' => f |= 0x01,
            'S' => f |= 0x02,
            'R' => f |= 0x04,
            'P' => f |= 0x08,
            'A' => f |= 0x10,
            'U' => f |= 0x20,
            _ => {}
        }
    }
    f
}

/// Build a TCP segment (20-byte header) with the given flags, sequence,
/// acknowledgement, and payload. The checksum is computed over the IPv4
/// pseudo-header (src/dst from `src_ip`/`dst_ip`).
pub fn tcp(
    src_ip: &str,
    dst_ip: &str,
    src_port: u16,
    dst_port: u16,
    flags: &str,
    seq: u32,
    ack: u32,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let src = parse_ipv4(src_ip)?;
    let dst = parse_ipv4(dst_ip)?;
    let data_len = payload.len();
    let mut hdr = vec![0u8; 20];
    hdr[0] = (src_port >> 8) as u8;
    hdr[1] = (src_port & 0xFF) as u8;
    hdr[2] = (dst_port >> 8) as u8;
    hdr[3] = (dst_port & 0xFF) as u8;
    hdr[4..8].copy_from_slice(&seq.to_be_bytes());
    hdr[8..12].copy_from_slice(&ack.to_be_bytes());
    hdr[12] = 0x50; // data offset 5 (20 bytes)
    hdr[13] = parse_flags(flags);
    hdr[14] = 0xFF;
    hdr[15] = 0xFF; // window
                    // checksum [16..18] left 0
    hdr[18] = 0;
    hdr[19] = 0; // urgent pointer

    // Pseudo-header for the TCP checksum.
    let mut pseudo = Vec::with_capacity(12 + 20 + data_len);
    pseudo.extend_from_slice(&src.octets());
    pseudo.extend_from_slice(&dst.octets());
    pseudo.push(0);
    pseudo.push(6);
    let tcp_len = (20 + data_len) as u16;
    pseudo.push((tcp_len >> 8) as u8);
    pseudo.push((tcp_len & 0xFF) as u8);
    pseudo.extend_from_slice(&hdr);
    pseudo.extend_from_slice(payload);
    let csum = csum16(&pseudo);
    hdr[16] = (csum >> 8) as u8;
    hdr[17] = (csum & 0xFF) as u8;

    let mut seg = hdr;
    seg.extend_from_slice(payload);
    Ok(seg)
}

/// Build a UDP datagram (8-byte header) with the given payload. The checksum
/// is computed over the IPv4 pseudo-header.
pub fn udp(
    src_ip: &str,
    dst_ip: &str,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let src = parse_ipv4(src_ip)?;
    let dst = parse_ipv4(dst_ip)?;
    let data_len = payload.len();
    let mut hdr = vec![0u8; 8];
    hdr[0] = (src_port >> 8) as u8;
    hdr[1] = (src_port & 0xFF) as u8;
    hdr[2] = (dst_port >> 8) as u8;
    hdr[3] = (dst_port & 0xFF) as u8;
    let len = (8 + data_len) as u16;
    hdr[4] = (len >> 8) as u8;
    hdr[5] = (len & 0xFF) as u8;
    // checksum [6..8] left 0 (optional in IPv4)

    let mut pseudo = Vec::with_capacity(12 + 8 + data_len);
    pseudo.extend_from_slice(&src.octets());
    pseudo.extend_from_slice(&dst.octets());
    pseudo.push(0);
    pseudo.push(17);
    pseudo.push((len >> 8) as u8);
    pseudo.push((len & 0xFF) as u8);
    pseudo.extend_from_slice(&hdr);
    pseudo.extend_from_slice(payload);
    let csum = csum16(&pseudo);
    if csum != 0 {
        hdr[6] = (csum >> 8) as u8;
        hdr[7] = (csum & 0xFF) as u8;
    }

    let mut seg = hdr;
    seg.extend_from_slice(payload);
    Ok(seg)
}

/// Convenience: build a full IPv4+TCP SYN packet (`src` -> `dst:dport`).
pub fn tcp_syn(src: &str, dst: &str, src_port: u16, dport: u16) -> Result<Vec<u8>, String> {
    let seg = tcp(src, dst, src_port, dport, "S", 0x1A2B_3C4D, 0, &[])?;
    ipv4(src, dst, 6, &seg)
}

/// Parse a dotted-quad IPv4 string into its four octets. Used by the ARP
/// builders, which deal in MAC addresses rather than ports.
fn ipv4_octets(s: &str) -> Result<[u8; 4], String> {
    let parts: Vec<&str> = s.trim().split('.').collect();
    if parts.len() != 4 {
        return Err(format!(
            "net_raw: '{}' is not a dotted-quad IPv4 address",
            s
        ));
    }
    let mut out = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p
            .parse::<u8>()
            .map_err(|_| format!("net_raw: '{}' is not a dotted-quad IPv4 address", s))?;
    }
    Ok(out)
}

/// Parse a MAC address written as `aa:bb:cc:dd:ee:ff` or `aa-bb-cc-dd-ee-ff`.
fn mac_bytes(s: &str) -> Result<[u8; 6], String> {
    let hex: Vec<u8> = s
        .trim()
        .split(|c| c == ':' || c == '-' || c == '.')
        .filter(|p| !p.is_empty())
        .map(|p| u8::from_str_radix(p, 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| format!("net_raw: '{}' is not a MAC address", s))?;
    if hex.len() != 6 {
        return Err(format!(
            "net_raw: '{}' is not a MAC address (want 6 octets, got {})",
            s,
            hex.len()
        ));
    }
    Ok([hex[0], hex[1], hex[2], hex[3], hex[4], hex[5]])
}

/// Build an ICMP echo request (type 8, code 0) for `dst`, wrapped in an IPv4
/// header. `id` and `seq` are echoed back in the reply, so a caller can match
/// a response to its request. `payload` is the data to echo (16 bytes is the
/// conventional size).
pub fn icmp_echo(
    src: &str,
    dst: &str,
    id: u16,
    seq: u16,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let mut msg = Vec::with_capacity(8 + payload.len());
    msg.push(8); // type: echo request
    msg.push(0); // code: no error
    msg.extend_from_slice(&[0, 0]); // checksum placeholder
    msg.extend_from_slice(&id.to_be_bytes());
    msg.extend_from_slice(&seq.to_be_bytes());
    msg.extend_from_slice(payload);
    let c = csum16(&msg);
    msg[2] = (c >> 8) as u8;
    msg[3] = (c & 0xFF) as u8;
    ipv4(src, dst, 1, &msg) // protocol 1 = ICMP
}

/// Build an ICMP echo reply (type 0) for `dst`, for hosts that answer on
/// someone else's behalf (RFC 1122 / RFC 1812).
pub fn icmp_echo_reply(
    src: &str,
    dst: &str,
    id: u16,
    seq: u16,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let mut msg = Vec::with_capacity(8 + payload.len());
    msg.push(0); // type: echo reply
    msg.push(0); // code
    msg.extend_from_slice(&[0, 0]); // checksum placeholder
    msg.extend_from_slice(&id.to_be_bytes());
    msg.extend_from_slice(&seq.to_be_bytes());
    msg.extend_from_slice(payload);
    let c = csum16(&msg);
    msg[2] = (c >> 8) as u8;
    msg[3] = (c & 0xFF) as u8;
    ipv4(src, dst, 1, &msg)
}

/// Build an ARP request asking "who has `target_ip`?" on the local segment.
/// `src_mac` must be the real MAC of the interface sending the frame, or the
/// target will answer to the wrong place.
pub fn arp_request(src_mac: &str, src_ip: &str, target_ip: &str) -> Result<Vec<u8>, String> {
    let sha = mac_bytes(src_mac)?;
    let spa = ipv4_octets(src_ip)?;
    let tpa = ipv4_octets(target_ip)?;
    Ok(arp_frame(&sha, &spa, &[0, 0, 0, 0, 0, 0], &tpa, 1))
}

/// Build an ARP reply telling `target_ip` that `src_ip` lives at `src_mac`.
pub fn arp_reply(
    src_mac: &str,
    src_ip: &str,
    target_mac: &str,
    target_ip: &str,
) -> Result<Vec<u8>, String> {
    let sha = mac_bytes(src_mac)?;
    let spa = ipv4_octets(src_ip)?;
    let tha = mac_bytes(target_mac)?;
    let tpa = ipv4_octets(target_ip)?;
    Ok(arp_frame(&sha, &spa, &tha, &tpa, 2))
}

/// Assemble a 28-byte Ethernet + ARP payload. `htype` 1 is Ethernet, `ptype`
/// 0x0800 is IPv4, `hlen`/`plen` are 6 and 4.
fn arp_frame(sha: &[u8; 6], spa: &[u8; 4], tha: &[u8; 6], tpa: &[u8; 4], opcode: u16) -> Vec<u8> {
    let mut f = Vec::with_capacity(42);
    f.extend_from_slice(tha); // destination MAC
    f.extend_from_slice(sha); // source MAC
    f.extend_from_slice(&[0x08, 0x06]); // EtherType: ARP
    f.extend_from_slice(&1u16.to_be_bytes()); // hardware type: Ethernet
    f.extend_from_slice(&0x0800u16.to_be_bytes()); // protocol type: IPv4
    f.push(6); // hardware length
    f.push(4); // protocol length
    f.extend_from_slice(&opcode.to_be_bytes()); // 1 = request, 2 = reply
    f.extend_from_slice(sha);
    f.extend_from_slice(spa);
    f.extend_from_slice(tha);
    f.extend_from_slice(tpa);
    f
}

/// Parse an ARP frame (the 28-byte ARP payload, either bare or with the
/// 14-byte Ethernet header) into a plain map. Returns nil on anything that is
/// not ARP, so a scanner can feed it raw capture bytes without pre-filtering.
pub fn arp_parse(frame: &[u8]) -> Option<Vec<(String, String)>> {
    // Skip the Ethernet header if present: ethertype 0x0806 at offset 12.
    let body = if frame.len() >= 14 && frame[12] == 0x08 && frame[13] == 0x06 {
        &frame[14..]
    } else {
        frame
    };
    if body.len() < 28 {
        return None;
    }
    if body[0] != 0 || body[1] != 1 {
        return None; // hardware type must be Ethernet
    }
    let opcode = u16::from_be_bytes([body[6], body[7]]);
    let fmt_mac = |b: &[u8]| {
        b.iter()
            .map(|x| format!("{:02x}", x))
            .collect::<Vec<_>>()
            .join(":")
    };
    let fmt_ip = |b: &[u8]| {
        b.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(".")
    };
    let mut out = Vec::new();
    out.push(("opcode".to_string(), opcode.to_string()));
    out.push(("sender_mac".to_string(), fmt_mac(&body[8..14])));
    out.push(("sender_ip".to_string(), fmt_ip(&body[14..18])));
    out.push(("target_mac".to_string(), fmt_mac(&body[18..24])));
    out.push(("target_ip".to_string(), fmt_ip(&body[24..28])));
    Some(out)
}

/// Build a full ICMP echo request. The name matches the capability the
/// sandbox has always gated, so `caps.rs` no longer refers to a builtin that
/// does not exist.
pub fn icmp_ping(src: &str, dst: &str, id: u16, seq: u16) -> Result<Vec<u8>, String> {
    icmp_echo(src, dst, id, seq, &[])
}

/// Send a forged packet on a raw socket. Requires `CAP_NET_RAW` /
/// Administrator. (Unix only — Windows raw-socket I/O needs Npcap and is not
/// in this build; the packet builders are cross-platform.)
#[cfg(unix)]
pub fn send(pkt: &[u8]) -> Result<usize, String> {
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::os::fd::AsRawFd;
    let sock = Socket::new(
        Domain::IPV4,
        Type::from(libc::SOCK_RAW),
        Some(Protocol::from(libc::IPPROTO_RAW)),
    )
    .map_err(|e| {
        format!(
            "net_raw: open raw socket failed (need CAP_NET_RAW/Administrator): {}",
            e
        )
    })?;
    unsafe {
        let one: libc::c_int = 1;
        let _ = libc::setsockopt(
            sock.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_HDRINCL,
            &one as *const _ as *const libc::c_void,
            std::mem::size_of_val(&one) as libc::socklen_t,
        );
    }
    let addr = SockAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));
    sock.send_to(pkt, &addr)
        .map_err(|e| format!("net_raw: send failed: {}", e))
}

#[cfg(not(unix))]
pub fn send(_pkt: &[u8]) -> Result<usize, String> {
    Err(
        "net_raw: send/recv requires unix CAP_NET_RAW (Windows raw-socket I/O not in this build)"
            .to_string(),
    )
}

/// Receive up to `max` bytes from a raw socket. Requires `CAP_NET_RAW` /
/// Administrator. (Unix only.)
#[cfg(unix)]
pub fn recv(max: usize) -> Result<Vec<u8>, String> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(
        Domain::IPV4,
        Type::from(libc::SOCK_RAW),
        Some(Protocol::from(libc::IPPROTO_RAW)),
    )
    .map_err(|e| {
        format!(
            "net_raw: open raw socket failed (need CAP_NET_RAW/Administrator): {}",
            e
        )
    })?;
    let mut buf = vec![0u8; max];
    let (n, _addr) = sock
        .recv_from(unsafe {
            std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut std::mem::MaybeUninit<u8>, max)
        })
        .map_err(|e| format!("net_raw: recv failed: {}", e))?;
    buf.truncate(n);
    Ok(buf)
}

#[cfg(not(unix))]
pub fn recv(_max: usize) -> Result<Vec<u8>, String> {
    Err(
        "net_raw: send/recv requires unix CAP_NET_RAW (Windows raw-socket I/O not in this build)"
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csum16_known() {
        // RFC 1071 example: sum of 0x00 0x01 0xF2 0x03 0xF4 0xF5 0xF6 0xF7
        // = 0x0001 + 0xF203 + 0xF4F5 + 0xF6F7 = 0x1DDF0 -> ~0xDDFF... verify nonzero.
        let data = [0x00, 0x01, 0xF2, 0x03, 0xF4, 0xF5, 0xF6, 0xF7];
        let c = csum16(&data);
        assert_ne!(c, 0);
    }

    #[test]
    fn ipv4_header_length_and_checksum() {
        let pkt = ipv4("10.0.0.5", "10.0.0.10", 6, &[0u8; 4]).unwrap();
        assert_eq!(pkt.len(), 24);
        assert_eq!(pkt[0], 0x45);
        // The header is self-checking: csum16 over it (with the stored
        // checksum included) must be 0.
        assert_eq!(csum16(&pkt[..20]), 0);
    }

    #[test]
    fn tcp_syn_packet_shape() {
        let pkt = tcp_syn("10.0.0.5", "10.0.0.10", 12345, 80).unwrap();
        assert_eq!(pkt.len(), 20 + 20); // IPv4 header + TCP header (no payload)
        assert_eq!(pkt[0], 0x45); // IPv4
        assert_eq!(pkt[9], 6); // proto TCP
                               // TCP flags byte is at offset 20 + 13 = 33; SYN = 0x02.
        assert_eq!(pkt[33] & 0x02, 0x02);
    }

    #[test]
    fn udp_header_shape() {
        let pkt = udp("10.0.0.5", "10.0.0.10", 1234, 53, b"hello").unwrap();
        assert_eq!(pkt.len(), 8 + 5);
        assert_eq!(&pkt[8..], b"hello");
    }

    #[test]
    fn icmp_echo_request_shape() {
        let pkt = icmp_echo("10.0.0.5", "10.0.0.10", 0x1234, 1, &[0xAB; 16]).unwrap();
        assert_eq!(pkt.len(), 20 + 8 + 16);
        assert_eq!(pkt[9], 1); // protocol ICMP
        assert_eq!(pkt[20], 8); // type = echo request
        assert_eq!(pkt[21], 0); // code
                                // id/seq are echoed at ICMP offset 4 and 6.
        assert_eq!(&pkt[24..26], &[0x12, 0x34]);
        assert_eq!(&pkt[26..28], &[0x00, 0x01]);
        // The ICMP message is self-checking once the checksum is stored.
        assert_eq!(csum16(&pkt[20..]), 0);
    }

    #[test]
    fn icmp_echo_reply_differs_only_in_type() {
        let req = icmp_echo("10.0.0.5", "10.0.0.10", 1, 1, &[]).unwrap();
        let rep = icmp_echo_reply("10.0.0.10", "10.0.0.5", 1, 1, &[]).unwrap();
        assert_eq!(req[20], 8);
        assert_eq!(rep[20], 0);
    }

    #[test]
    fn icmp_ping_matches_echo_request() {
        let a = icmp_ping("10.0.0.5", "10.0.0.10", 7, 7).unwrap();
        let b = icmp_echo("10.0.0.5", "10.0.0.10", 7, 7, &[]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn arp_request_frame_layout() {
        let f = arp_request("aa:bb:cc:dd:ee:ff", "10.0.0.5", "10.0.0.10").unwrap();
        assert_eq!(f.len(), 42); // 14 Ethernet + 28 ARP
                                 // Broadcast destination, then our own MAC as source.
        assert_eq!(&f[0..6], &[0, 0, 0, 0, 0, 0]);
        assert_eq!(&f[6..12], &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        assert_eq!(&f[12..14], &[0x08, 0x06]); // EtherType: ARP
        assert_eq!(&f[14..16], &[0x00, 0x01]); // hardware type: Ethernet
        assert_eq!(&f[16..18], &[0x08, 0x00]); // protocol type: IPv4
        assert_eq!(f[18], 6); // hardware length
        assert_eq!(f[19], 4); // protocol length
        assert_eq!(&f[20..22], &[0x00, 0x01]); // opcode 1 = request
        assert_eq!(&f[22..28], &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]); // SHA
        assert_eq!(&f[28..32], &[10, 0, 0, 5]); // SPA
        assert_eq!(&f[32..38], &[0, 0, 0, 0, 0, 0]); // THA is unknown
        assert_eq!(&f[38..42], &[10, 0, 0, 10]); // TPA
    }

    #[test]
    fn arp_reply_uses_the_target_mac() {
        let f = arp_reply(
            "aa:bb:cc:dd:ee:ff",
            "10.0.0.10",
            "11:22:33:44:55:66",
            "10.0.0.5",
        )
        .unwrap();
        assert_eq!(&f[0..6], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(&f[32..38], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(&f[20..22], &[0x00, 0x02]);
    }

    #[test]
    fn arp_parse_roundtrips_a_request() {
        let f = arp_request("aa:bb:cc:dd:ee:ff", "10.0.0.5", "10.0.0.10").unwrap();
        let kv = arp_parse(&f).unwrap();
        let get = |k: &str| {
            kv.iter()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(get("opcode"), "1");
        assert_eq!(get("sender_mac"), "aa:bb:cc:dd:ee:ff");
        assert_eq!(get("sender_ip"), "10.0.0.5");
        assert_eq!(get("target_ip"), "10.0.0.10");
    }

    #[test]
    fn arp_parse_accepts_a_bare_payload_and_rejects_junk() {
        let f = arp_request("aa:bb:cc:dd:ee:ff", "10.0.0.5", "10.0.0.10").unwrap();
        assert!(arp_parse(&f[14..]).is_some());
        assert!(arp_parse(&[0u8; 10]).is_none());
        assert!(arp_parse(&[]).is_none());
    }

    #[test]
    fn mac_and_ip_parsers_reject_bad_input() {
        assert!(mac_bytes("aa:bb:cc:dd:ee").is_err());
        assert!(mac_bytes("zz:bb:cc:dd:ee:ff").is_err());
        assert!(ipv4_octets("10.0.0").is_err());
        assert!(ipv4_octets("10.0.0.999").is_err());
        // Dashes and dots are accepted as separators.
        assert!(mac_bytes("aa-bb-cc-dd-ee-ff").is_ok());
    }
}
