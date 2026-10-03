//! Runtime deprecation for weak crypto, and contextual escaping.
//!
//! Both run on both backends, and the tests here require the two to agree -- a
//! security diagnostic that appears on one backend only is worse than none, because
//! a program that behaves differently under `rakc run` and `rakc vm` cannot be
//! reasoned about.

/// Run `source` on both backends, requiring identical stdout.
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

/// Run on both backends and return `(stdout, stderr)`.
///
/// Warnings go to stderr on purpose -- stdout is a program's data -- so these
/// tests have to look at the two streams separately.
///
/// The source goes through a temporary file rather than stdin: the help text
/// advertises `-` as "read from stdin", but passing it makes the argument parser
/// report "only flags were given", and a test that depends on a broken path is
/// testing the wrong thing.
fn agree_with_stderr(source: &str) -> (Vec<String>, Vec<String>) {
    // A unique file per call. The tests run in parallel threads inside one binary,
    // and a shared filename means each test executes whichever sibling wrote last
    // -- which showed up as an md5 assertion seeing a sha256 source, and as a
    // stderr assertion seeing a different primitive's warning.
    let dir = std::env::temp_dir().join(format!("rak_hardening_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = dir.join(format!("case_{}_{}.rak", std::process::id(), n));
    std::fs::write(&path, source).expect("write the case file");

    let mut outs = Vec::new();
    let mut errs = Vec::new();
    for backend in ["run", "vm"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rakc"))
            .arg(backend)
            .arg(&path)
            .output()
            .unwrap_or_else(|e| panic!("spawn {backend}: {e}"));
        outs.push(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|l| l.to_string())
                .collect::<Vec<_>>(),
        );
        errs.push(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .map(|l| l.to_string())
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        outs[0], outs[1],
        "stdout differs:\n{:?}\n{:?}",
        outs[0], outs[1]
    );
    assert_eq!(
        errs[0], errs[1],
        "stderr differs between backends:\n{:?}\n{:?}",
        errs[0], errs[1]
    );
    let _ = std::fs::remove_file(&path);
    (outs[0].clone(), errs[0].clone())
}

#[test]
fn sql_escape_neutralises_a_quote() {
    let out = agree(
        r#"
dump sql_escape("O'Brien")
dump sql_escape("harmless")
"#,
    );
    assert_eq!(out, vec!["[DUMP] O''Brien", "[DUMP] harmless"]);
}

#[test]
fn shell_escape_quotes_the_argument() {
    let out = agree(
        r#"
dump shell_escape("it's")
dump shell_escape("plain")
dump shell_escape("")
"#,
    );
    assert_eq!(
        out,
        vec!["[DUMP] 'it'\\''s'", "[DUMP] 'plain'", "[DUMP] ''"]
    );
}

#[test]
fn html_escape_encodes_the_markup_characters() {
    let out = agree(
        r#"
dump html_escape("<script>alert('x')</script>")
"#,
    );
    assert_eq!(
        out,
        vec!["[DUMP] &lt;script&gt;alert(&#x27;x&#x27;)&lt;&#x2F;script&gt;"]
    );
}

#[test]
fn regex_escape_defuses_a_metacharacter() {
    let out = agree(
        r#"
dump regex_escape("a.c*")
"#,
    );
    assert_eq!(out, vec!["[DUMP] a\\.c\\*"]);
}

#[test]
fn an_injection_attempt_survives_escaping_only_as_text() {
    // The property, stated as a round trip rather than as a shape: the escaped
    // payload is still the original text, so a *parameterised* query would treat
    // it as data. What is being checked here is that the quotes are balanced, so
    // it cannot terminate a literal by itself.
    let out = agree(
        r#"
let hostile = "x'; DROP TABLE users; --"
dump sql_escape(hostile)
"#,
    );
    let escaped = out[0].trim_start_matches("[DUMP] ");
    assert_eq!(escaped, "x''; DROP TABLE users; --");
    assert_eq!(
        escaped.matches('\'').count() % 2,
        0,
        "quotes must stay paired so the literal never closes"
    );
}

#[test]
fn md5_warns_on_stderr_and_still_returns_the_digest() {
    let (out, err) = agree_with_stderr("dump md5(\"hello\")\n");
    // The digest is unchanged: this is a deprecation, not a removal.
    assert_eq!(out, vec!["[DUMP] 5d41402abc4b2a76b9719d911017c592"], "got {out:?}");
    let warning = err.join("\n");
    assert!(warning.contains("md5"), "the warning must name the primitive: {warning}");
    assert!(warning.contains("deprecated"), "{warning}");
    assert!(
        warning.contains("sha256"),
        "the warning should say what to use instead: {warning}"
    );
}

#[test]
fn sha1_warns_too() {
    let (_, err) = agree_with_stderr("dump sha1(\"hello\")\n");
    let warning = err.join("\n");
    assert!(warning.contains("sha1"), "{warning}");
    assert!(warning.contains("collision"), "{warning}");
}

#[test]
fn sha256_does_not_warn() {
    // The fix has to stay quiet, or the warning trains people to ignore it.
    let (_, err) = agree_with_stderr("dump sha256(\"hello\")\n");
    assert!(
        err.is_empty(),
        "sha256 must not warn: {:?}",
        err
    );
}

#[test]
fn the_warning_is_not_on_stdout() {
    // A warning on stdout would corrupt a pipeline and fail a parity comparison for
    // a reason that has nothing to do with what is being tested.
    let (out, err) = agree_with_stderr("dump md5(\"x\")\n");
    assert!(!out.iter().any(|l| l.contains("deprecated")), "{out:?}");
    assert!(err.iter().any(|l| l.contains("deprecated")), "{err:?}");
}

#[test]
fn a_warning_is_emitted_once_per_primitive_not_once_per_call() {
    // A digest inside a loop would otherwise print a warning per iteration.
    let (_, err) = agree_with_stderr(
        r#"
let mut i = 0
while i < 50 { let x = md5("hello")  i = i + 1 }
dump "done"
"#,
    );
    let count = err.iter().filter(|l| l.contains("md5")).count();
    assert_eq!(count, 1, "expected exactly one warning, got {count}: {err:?}");
}

#[test]
fn escaping_does_not_need_a_capability() {
    // Pure string functions: requiring a capability would be noise, and gating
    // them would make the safe path the awkward one.
    let out = agree("dump sql_escape(\"a'b\")\n");
    assert_eq!(out, vec!["[DUMP] a''b"]);
}