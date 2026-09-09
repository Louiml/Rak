//! PCAP live/offline capture, gated behind the `pcap` Cargo feature (needs
//! libpcap on Linux / Npcap on Windows to build).
//!
//! When the feature is off, the stub functions return a clear "not built"
//! error. When on, `open(path)` reads an offline capture and `next(handle)`
//! yields `{timestamp, linktype, payload}` packets.

#[cfg(feature = "pcap")]
use pcap::Capture;

/// An open PCAP capture handle.
#[cfg(feature = "pcap")]
pub struct PcapHandle {
    cap: Capture<pcap::Offline>,
}

#[cfg(not(feature = "pcap"))]
pub struct PcapHandle;

/// One captured packet: timestamp (epoch µs), linktype, and raw payload.
#[derive(Debug, Clone)]
pub struct PcapPacket {
    pub timestamp: i64,
    pub linktype: i64,
    pub payload: Vec<u8>,
}

#[cfg(feature = "pcap")]
pub fn open(path: &str) -> Result<PcapHandle, String> {
    let cap = Capture::from_file(path).map_err(|e| format!("pcap: open '{}': {}", path, e))?;
    Ok(PcapHandle { cap })
}

#[cfg(not(feature = "pcap"))]
pub fn open(_path: &str) -> Result<PcapHandle, String> {
    Err("pcap: not built (build rak-stdlib with --features pcap; needs libpcap/Npcap)".to_string())
}

#[cfg(feature = "pcap")]
pub fn next(h: &mut PcapHandle) -> Option<PcapPacket> {
    match h.cap.next_packet() {
        Ok(p) => {
            let ts = p.timestamp();
            let micros = ts.timestamp() as i64 * 1_000_000 + ts.timestamp_submicros() as i64 / 1000;
            Some(PcapPacket {
                timestamp: micros,
                linktype: 1, // Ethernet, by default
                payload: p.data.to_vec(),
            })
        }
        Err(_) => None,
    }
}

#[cfg(not(feature = "pcap"))]
pub fn next(_h: &mut PcapHandle) -> Option<PcapPacket> {
    None
}
