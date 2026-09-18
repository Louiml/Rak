#![no_main]

use libfuzzer_sys::fuzz_target;

// Full interpreter evaluation of arbitrary source — exercises the lexer, parser,
// tree-walking evaluator, and many builtins together. Input size is capped by the
// fuzzer (default 4096 bytes), so this stays bounded.
fuzz_target!(|data: &[u8]| {
    if data.len() > 2048 {
        return;
    }
    let text = String::from_utf8_lossy(data);
    let _ = rakc::eval(&text);
});