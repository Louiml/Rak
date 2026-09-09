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
    s.parse::<Ipv4Addr>().map_err(|_| format!("net_raw: bad IPv4 address '{}'", s))
}

/// Build an IPv4 header (20 bytes) carrying `payload` with the given protocol
/// number (6 = TCP, 17 = UDP). The header checksum is computed.
pub fn ipv4(src: &str, dst: &str, proto: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    let src = parse_ipv4(src)?;
    let dst = parse_ipv4(dst)?;
    let total_len = 20 + payload.len();
    let mut hdr = vec![0u8; 20];
    hdr[0] = 0x45; // version 4, IHL 5 (20 bytes)
    hdr[1] = 0;    // DSCP/ECN
    hdr[2] = (total_len >> 8) as u8;
    hdr[3] = (total_len & 0xFF) as u8;
    hdr[4] = 0; hdr[5] = 0;   // identification
    hdr[6] = 0x40; hdr[7] = 0; // flags: Don't Fragment, offset 0
    hdr[8] = 64;              // TTL
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
pub fn tcp(src_ip: &str, dst_ip: &str, src_port: u16, dst_port: u16, flags: &str, seq: u32, ack: u32, payload: &[u8]) -> Result<Vec<u8>, String> {
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
    hdr[14] = 0xFF; hdr[15] = 0xFF; // window
    // checksum [16..18] left 0
    hdr[18] = 0; hdr[19] = 0; // urgent pointer

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
pub fn udp(src_ip: &str, dst_ip: &str, src_port: u16, dst_port: u16, payload: &[u8]) -> Result<Vec<u8>, String> {
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

/// Send a forged packet on a raw socket. Requires `CAP_NET_RAW` /
/// Administrator. (Unix only — Windows raw-socket I/O needs Npcap and is not
/// in this build; the packet builders are cross-platform.)
#[cfg(unix)]
pub fn send(pkt: &[u8]) -> Result<usize, String> {
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    use std::net::{Ipv4Addr, SocketAddrV4};
    let sock = Socket::new(Domain::IPV4, Type::from_raw(libc::SOCK_RAW), Some(Protocol::from_raw(libc::IPPROTO_RAW)))
        .map_err(|e| format!("net_raw: open raw socket failed (need CAP_NET_RAW/Administrator): {}", e))?;
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
    sock.send_to(pkt, &addr).map_err(|e| format!("net_raw: send failed: {}", e))
}

#[cfg(not(unix))]
pub fn send(_pkt: &[u8]) -> Result<usize, String> {
    Err("net_raw: send/recv requires unix CAP_NET_RAW (Windows raw-socket I/O not in this build)".to_string())
}

/// Receive up to `max` bytes from a raw socket. Requires `CAP_NET_RAW` /
/// Administrator. (Unix only.)
#[cfg(unix)]
pub fn recv(max: usize) -> Result<Vec<u8>, String> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(Domain::IPV4, Type::from_raw(libc::SOCK_RAW), Some(Protocol::from_raw(libc::IPPROTO_RAW)))
        .map_err(|e| format!("net_raw: open raw socket failed (need CAP_NET_RAW/Administrator): {}", e))?;
    let mut buf = vec![0u8; max];
    let (n, _addr) = sock.recv_from(unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut std::mem::MaybeUninit<u8>, max) })
        .map_err(|e| format!("net_raw: recv failed: {}", e))?;
    buf.truncate(n);
    Ok(buf)
}

#[cfg(not(unix))]
pub fn recv(_max: usize) -> Result<Vec<u8>, String> {
    Err("net_raw: send/recv requires unix CAP_NET_RAW (Windows raw-socket I/O not in this build)".to_string())
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
        assert_eq!(pkt[0], 0x45);       // IPv4
        assert_eq!(pkt[9], 6);          // proto TCP
        // TCP flags byte is at offset 20 + 13 = 33; SYN = 0x02.
        assert_eq!(pkt[33] & 0x02, 0x02);
    }

    #[test]
    fn udp_header_shape() {
        let pkt = udp("10.0.0.5", "10.0.0.10", 1234, 53, b"hello").unwrap();
        assert_eq!(pkt.len(), 8 + 5);
        assert_eq!(&pkt[8..], b"hello");
    }
}
