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
pub mod log;
pub mod process;
pub mod secrets;
pub mod http_server;
pub mod websocket;

pub use net::*;
pub use crypto::*;
pub use encoding::*;
pub use recon::*;
pub use log::*;
pub use process::*;
pub use secrets::*;

/// Initialize all standard library modules
pub fn init() {
    // Placeholder for initialization logic
}