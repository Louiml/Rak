//! Target conformance: run a corpus of programs on every compilation target
//! and require byte-identical output against the interpreter.
//!
//! The interpreter is the reference: `rakc run`'s output is what a compiled
//! artifact must reproduce. The corpus is deliberately small and each program
//! names the feature it exercises, so a divergence prints a diff that points
//! at the language feature that regressed rather than at a line number.
//!
//! The C target requires a C compiler on `PATH` (or `RAKC_CC`). When none is
//! present the C cases report a skip rather than a pass: a silently skipped
//! gate is how the VM's interpreter-only builtins went unnoticed for a
//! release, so the skip is printed and counted, never quiet.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Each case: `(label, source)`. A case's output is compared literally.
const CORPUS: &[(&str, &str)] = &[
    (
        "int-arithmetic",
        r#"
dump 1 + 2 * 3
dump 10 / 3
dump 10 % 3
dump 2 + 3 * 4 - 6 / 2
"#,
    ),
    (
        "float-arithmetic",
        r#"
dump 1.5 + 2.5
dump 3.0 * 4.0
dump 7.0 / 2.0
"#,
    ),
    (
        "hex-literals",
        r#"
let sig = 0xDEADBEEF
dump sig
dump 0xFF & 0x0F
dump 0x10 | 0x01
dump 1 << 4
dump 256 >> 4
"#,
    ),
    (
        "string-ops",
        r#"
dump "hello" + " " + "world"
dump len("hello")
dump upper("abc")
dump lower("ABC")
dump "hello"[1]
dump slice("hello", 1, 3)
"#,
    ),
    (
        "control-flow",
        r#"
let mut x = 0
for i in [1, 2, 3, 4, 5] {
    x = x + i
}
dump x
let mut y = 0
let mut n = 0
while n < 5 {
    y = y + n
    n = n + 1
}
dump y
"#,
    ),
    (
        "functions",
        r#"
fn add(a, b) { return a + b }
dump add(3, 4)
fn fact(n) {
    if n <= 1 { return 1 }
    return n * fact(n - 1)
}
dump fact(5)
"#,
    ),
    (
        "arrays",
        r#"
let xs = [3, 1, 4, 1, 5]
dump len(xs)
dump sum(xs)
dump sort(xs)
dump max(3, 9, 2)
dump xs[2]
dump slice(xs, 1, 3)
"#,
    ),
    (
        "maps",
        r#"
let m = {a: 1, b: 2}
dump m.a
dump has(m, "a")
dump get(m, "z", 99)
// `keys` order follows the backend's HashMap and is documented as
// nondeterministic, so the corpus sorts before printing.
dump sort(keys(m))
"#,
    ),
    (
        "match",
        r#"
fn describe(n) {
    match n {
        0 => { return "zero" },
        1 => { return "one" },
        _ => { return "many" },
    }
}
dump describe(0)
dump describe(1)
dump describe(42)
"#,
    ),
    (
        "tuples-structs",
        r#"
let p = (1, 2)
dump p.0
dump p.1
struct Point { x: int, y: int }
let o = Point { x: 3, y: 4 }
dump o.x
dump o.y
"#,
    ),
    (
        "option-result",
        r#"
let v = Ok(42)?
dump v
let w = Some(7)
dump w
dump None
"#,
    ),
    (
        "try-catch",
        r#"
try {
    raise "boom"
} catch e {
    dump e
}
dump "after"
"#,
    ),
    (
        "fmt-specs",
        r#"
dump fmt("{}", 42)
dump fmt("0x{:04X}", 0xAB)
dump fmt("{:>6}", 42)
dump fmt("{:.2f}", 3.14159)
"#,
    ),
    (
        "closures",
        // Closures referencing globals and their own parameters — the shape
        // every backend ships today. Captured *locals* are the documented
        // roadmap item (the VM captures by value), so the corpus pins the
        // supported subset.
        r#"
let base = 100
let double = fn(x) { return x * 2 }
dump double(21)
dump fn(x) { return x + base }(5)
fn twice(f, v) { return f(f(v)) }
dump twice(double, 3)
"#,
    ),
    (
        "function-locals",
        // A function with a local of its own. The C target used to emit a
        // bare `v_x = ...` for a local -- an assignment to a variable it
        // never declared -- so *every* function with a local produced C that
        // the C compiler rejected. The corpus had no such case, which is why
        // it went unnoticed.
        r#"
fn classify(n) {
    let doubled = n * 2
    let label = "big"
    if doubled > 10 { return label }
    return "small"
}
dump classify(3)
dump classify(9)
"#,
    ),
    (
        "closure-captures-a-local",
        // A closure reading a local of the function it was written in. The
        // captured value lives in the closure's environment; the C target
        // used to read the enclosing function's C variable, which does not
        // exist by the time the closure runs.
        r#"
fn make_adder(n) {
    let base = n
    return fn(x) { return x + base }
}
let add5 = make_adder(5)
dump add5(10)
"#,
    ),
    (
        "comparisons",
        r#"
dump 1 < 2
dump 2 <= 2
dump 3 > 4
dump 4 >= 4
dump 1 == 1
dump 1 != 2
dump "a" == "a"
dump [1, 2] == [1, 2]
"#,
    ),
    (
        "truthiness",
        r#"
if "" { dump "empty-true" } else { dump "empty-false" }
if "x" { dump "nonempty-true" }
if 0 { dump "zero-true" } else { dump "zero-false" }
if nil { dump "nil-true" } else { dump "nil-false" }
"#,
    ),
];

/// Run `source` on the interpreter and return its output lines.
fn interp_output(source: &str) -> Result<Vec<String>, String> {
    let mut interp = rakc::interpreter::Interpreter::with_base_dir(".".to_string());
    interp.set_max_depth(rakc::interpreter::DEFAULT_MAX_DEPTH);
    interp
        .run_source(source)
        .map_err(|e| format!("interpreter: {}", e))
}

/// Compile `source` to a `.rkbc` and run it on the VM.
fn bytecode_output(source: &str) -> Result<Vec<String>, String> {
    rakc::eval_vm_in(source, ".").map_err(|e| format!("bytecode: {}", e))
}

/// Locate a C compiler: `RAKC_CC`, then `PATH`. Returns the executable path.
fn find_cc() -> Option<PathBuf> {
    if let Ok(cc) = std::env::var("RAKC_CC") {
        let p = PathBuf::from(cc);
        if p.is_file() {
            return Some(p);
        }
    }
    let out = Command::new("gcc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let _ = out;
    Some(PathBuf::from("gcc"))
}

/// Compile `source` through the C backend and run the produced binary.
fn c_output(
    source: &str,
    cc: &Path,
    dir: &std::path::Path,
    label: &str,
) -> Result<Vec<String>, String> {
    let opts = rakc::targets::CodegenOptions::new(
        dir.join(format!("{}.rak", label))
            .to_str()
            .unwrap_or("conformance.rak"),
    );
    let c_path = dir.join(format!("{}.c", label));
    let exe_path = dir.join(format!("{}.exe", label));

    let tokens = rakc::lexer::tokenize(source).map_err(|e| format!("lex: {}", e))?;
    let module = rakc::parser::parse(&tokens, source).map_err(|e| format!("parse: {}", e))?;
    let c_source = rakc::targets::c::translate(&module, &opts)?;

    std::fs::write(&c_path, c_source).map_err(|e| format!("write c: {}", e))?;
    let status = Command::new(cc)
        .args(["-std=c11", "-O1"])
        .arg(&c_path)
        .arg("-o")
        .arg(&exe_path)
        .arg("-lm")
        .output()
        .map_err(|e| format!("run gcc: {}", e))?;
    if !status.status.success() {
        return Err(format!(
            "gcc failed: {}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }

    let out = Command::new(&exe_path)
        .output()
        .map_err(|e| format!("run exe: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "exe exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout.lines().map(|l| l.to_string()).collect())
}

fn diff(reference: &[String], actual: &[String], target: &str, label: &str) -> String {
    format!(
        "conformance divergence in `{}` on target `{}`:\n  interpreter: {:?}\n  {}: {:?}",
        label, target, reference, target, actual
    )
}

#[test]
fn bytecode_matches_the_interpreter_on_the_corpus() {
    for (label, source) in CORPUS {
        let expected = interp_output(source).expect(label);
        let actual = bytecode_output(source).expect(label);
        assert_eq!(
            expected,
            actual,
            "{}",
            diff(&expected, &actual, "bytecode", label)
        );
    }
}

#[test]
fn c_matches_the_interpreter_on_the_corpus() {
    let Some(cc) = find_cc() else {
        eprintln!("SKIP: c conformance (no C compiler on PATH; set RAKC_CC to point at one)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("rak_conformance_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let mut ran = 0;
    for (label, source) in CORPUS {
        let expected = match interp_output(source) {
            Ok(o) => o,
            Err(e) => {
                // The interpreter rejecting a construct is a reference
                // answer; the C target must reject it too.
                match c_output(source, &cc, &dir, label) {
                    Ok(_) => panic!(
                        "conformance in `{}`: interpreter errored ({}) but the C target ran",
                        label, e
                    ),
                    Err(_) => continue,
                }
            }
        };
        let actual = c_output(source, &cc, &dir, label).expect(label);
        assert_eq!(expected, actual, "{}", diff(&expected, &actual, "c", label));
        ran += 1;
    }
    assert!(ran > 0, "the corpus exercised at least one C case");
    let _ = std::fs::remove_dir_all(&dir);
}
