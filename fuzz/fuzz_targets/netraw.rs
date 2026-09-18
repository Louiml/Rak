#![no_main]

use libfuzzer_sys::fuzz_target;

// Raw packet header builders: arbitrary IP/payload must not panic / overflow.
fuzz_target!(|data: &[u8]| {
    let ip = String::from_utf8_lossy(&data[..data.len().min(16)]);
    let payload = &data[..data.len().min(256)];
    let _ = rak_stdlib::net_raw::csum16(data);
    let _ = rak_stdlib::net_raw::ipv4(&ip, &ip, 6, payload);
    let _ = rak_stdlib::net_raw::tcp(&ip, &ip, 1234, 80, "S", 1, 0, payload);
    let _ = rak_stdlib::net_raw::udp(&ip, &ip, 1234, 53, payload);
    let _ = rak_stdlib::net_raw::tcp_syn(&ip, &ip, 1234, 80);
});