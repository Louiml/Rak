//! `fmt` format specs, on both backends.
//!
//! `fmt` used to pick a rendering by asking whether the spec text merely *contained*
//! certain characters: `spec.contains(":04X")`, then `":08X"`, then any `x`/`X`. That
//! recognised exactly two widths and no other type, so `{:02X}` -- the width you
//! want for a hex dump -- silently fell through to the default rendering and printed
//! `5` where it should print `05`.
//!
//! The VM's copy had drifted further: it had no float branch at all, so
//! `fmt("{:.2f}", x)` printed a rounded value on the interpreter and the raw float on
//! the VM.
//!
//! Both copies are now one shared parser (`rakc/src/fmt_spec.rs`). This file exists
//! mainly so the two backends cannot drift apart again: every spec goes through
//! `run_on_both`, which fails on any disagreement.

/// Run `source` on both backends, requiring byte-identical output.
fn agree(source: &str) -> Vec<String> {
    let parity = rakc::run_on_both(source, ".");
    if let Some(why) = parity.divergence() {
        panic!(
            "backend divergence:\n{}\n--- source ---\n{}",
            why, source
        );
    }
    match parity {
        rakc::BackendParity::Agree(out) => out,
        other => panic!("both backends failed, so nothing was compared: {:?}", other),
    }
}

#[test]
fn hex_widths_zero_pad() {
    // The bug that motivated all of this: `{:02X}` used to print `5`.
    let out = agree(
        r#"
dump fmt("{:02X}", 5)
dump fmt("{:02x}", 5)
dump fmt("{:04X}", 90)
dump fmt("{:08X}", 3735928559)
dump fmt("{:X}", 255)
dump fmt("{:x}", 255)
"#,
    );
    assert_eq!(
        out,
        vec![
            "[DUMP] 05",
            "[DUMP] 05",
            "[DUMP] 005A",
            "[DUMP] DEADBEEF",
            "[DUMP] FF",
            "[DUMP] ff",
        ]
    );
}

#[test]
fn radix_specs_and_the_alternate_prefix() {
    let out = agree(
        r#"
dump fmt("{:b}", 5)
dump fmt("{:o}", 8)
dump fmt("{:d}", 42)
dump fmt("{:#x}", 255)
dump fmt("{:#b}", 5)
dump fmt("{:08b}", 5)
"#,
    );
    assert_eq!(
        out,
        vec![
            "[DUMP] 101",
            "[DUMP] 10",
            "[DUMP] 42",
            "[DUMP] 0xff",
            "[DUMP] 0b101",
            "[DUMP] 00000101",
        ]
    );
}

#[test]
fn float_specs_agree_across_backends() {
    // This is the spec the VM never had a branch for.
    let out = agree(
        r#"
dump fmt("{:.2f}", 3.14159)
dump fmt("{:.0f}", 2.5)
dump fmt("{:e}", 1234.5)
dump fmt("{:8.3f}", 1.5)
"#,
    );
    assert_eq!(
        out,
        vec![
            "[DUMP] 3.14",
            "[DUMP] 2",
            "[DUMP] 1.234500e3",
            "[DUMP]    1.500",
        ]
    );
}

#[test]
fn width_and_alignment() {
    let out = agree(
        r#"
dump fmt("{:>6}", 42)
dump fmt("{:<6}", 42)
dump fmt("{:^7}", "ab")
dump fmt("{:*^7}", "ab")
dump fmt("{:*<6}", "ab")
dump fmt("[{:5}]", "ab")
"#,
    );
    assert_eq!(
        out,
        vec![
            "[DUMP]     42",
            "[DUMP] 42    ",
            "[DUMP]   ab   ",
            "[DUMP] **ab***",
            "[DUMP] ab****",
            "[DUMP] [ab   ]",
        ]
    );
}

#[test]
fn braces_escapes_are_untouched_and_arguments_are_consumed_in_order() {
    let out = agree(
        r#"
dump fmt("{{literal}} {0}", 7)
dump fmt("{} {} {}", "a", "b", "c")
"#,
    );
    assert_eq!(out, vec!["[DUMP] {literal} 7", "[DUMP] a b c"]);
}

/// `{1}` looks like a positional index and is not one. Rak fills placeholders in
/// order, so this is `x` then `y`. Pinned because a parser that started honouring
/// indices would silently change the output of every existing `fmt` call.
#[test]
fn a_numeric_placeholder_is_not_a_positional_index() {
    let out = agree(r#"dump fmt("{1}{0}", "x", "y")"#);
    assert_eq!(out, vec!["[DUMP] xy"]);
}

#[test]
fn a_spec_with_no_arguments_is_left_alone() {
    // A placeholder with nothing to fill it must not eat the next argument's slot.
    let out = agree(
        r#"
dump fmt("{} {}", "only")
dump fmt("no placeholders", 1, 2)
"#,
    );
    assert_eq!(out, vec!["[DUMP] only ", "[DUMP] no placeholders"]);
}

#[test]
fn zero_padding_keeps_a_negative_sign_outside_the_padding() {
    let out = agree(
        r#"
dump fmt("{:05}", -42)
dump fmt("{:+}", 42)
"#,
    );
    assert_eq!(out, vec!["[DUMP] -0042", "[DUMP] +42"]);
}

#[test]
fn bytes_format_as_hex_pairs() {
    // The hex-editor use case: a row of bytes rendered one `{:02X}` at a time.
    let out = agree(
        r#"
let row = bytes([0, 15, 16, 255])
let mut s = ""
for b in row { s = s + fmt("{:02X}", b) }
dump s
"#,
    );
    assert_eq!(out, vec!["[DUMP] 000F10FF"]);
}