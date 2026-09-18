#![no_main]

use libfuzzer_sys::fuzz_target;

// JSON parser + pretty-print round-trip. Never panics on arbitrary bytes.
fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(v) = rak_stdlib::js::json_parse(&text) {
        let pretty = rak_stdlib::js::json_stringify(&v);
        let _ = pretty;
        // Navigate the pretty form with the string-based helpers.
        let _ = rak_stdlib::js::json_keys(&pretty);
        let _ = rak_stdlib::js::json_get(&pretty, "0");
        let _ = rak_stdlib::js::json_path(&pretty, "a.b");
        let _ = rak_stdlib::js::json_find_all(&pretty, "x");
    }
});