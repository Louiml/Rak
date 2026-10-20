pub mod crypto;
pub mod dns;
pub mod encoding;
pub mod escape;
pub mod ffi;
pub mod file;
pub mod fs_scope;
pub mod http_server;
pub mod js;
pub mod log;
pub mod mmap;
pub mod net;
pub mod net_raw;
pub mod pcap;
pub mod process;
pub mod recon;
pub mod secrets;
pub mod stream_io;
pub mod tls;
pub mod tunnel;
// The HTML parser needs `scraper`, which needs `ahash`, which needs
// `getrandom` — whose browser backend pulls in `wasm-bindgen` and a pile of
// generated glue. The browser build leaves it out; the module keeps its shape
// either way, so a program that calls `html_links` there gets a clear answer
// instead of a missing name.
#[cfg(feature = "html")]
pub mod web;
#[cfg(not(feature = "html"))]
#[path = "web_stub.rs"]
pub mod web;
pub mod websocket;
// 0.8 feature packs
pub mod archive;
pub mod ctlogs;
pub mod datafmt;
// The random number generator needs `rand`, which needs `getrandom` — whose
// browser backend pulls in `wasm-bindgen` and generated glue. The browser build
// leaves it out; the module keeps its shape either way.
#[cfg(feature = "rand")]
pub mod randkit;
#[cfg(not(feature = "rand"))]
#[path = "randkit_stub.rs"]
pub mod randkit;
pub mod report;
pub mod timekit;
pub mod whois;
pub mod yara;

pub use crypto::*;
pub use encoding::*;
pub use log::*;
pub use net::*;
pub use process::*;
pub use recon::*;
pub use secrets::*;

/// Initialize all standard library modules
pub fn init() {
    // Placeholder for initialization logic
}
