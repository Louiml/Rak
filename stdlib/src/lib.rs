pub mod net;
pub mod crypto;
pub mod encoding;
pub mod recon;
pub mod web;
pub mod file;
pub mod js;
pub mod ffi;
pub mod mmap;
pub mod net_raw;
pub mod dns;
pub mod tls;
pub mod pcap;

pub use net::*;
pub use crypto::*;
pub use encoding::*;
pub use recon::*;

/// Initialize all standard library modules
pub fn init() {
    // Placeholder for initialization logic
}