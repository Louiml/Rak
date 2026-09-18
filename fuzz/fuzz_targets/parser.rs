#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(tokens) = rakc::lexer::tokenize(&text) {
        let _ = rakc::parser::parse(&tokens, &text);
    }
});