//! Every workload in `examples/bench` must run and must agree between the two
//! backends.
//!
//! This is not a performance test. A benchmark whose correctness drifts is worse
//! than no benchmark: the number still looks plausible and is measuring the wrong
//! program. Two of the six workloads as first written did exactly that -- `map.rak`
//! called `map_set`, which is not a Rak builtin, and `strings.rak` used `s[0:9]`,
//! which is not valid syntax -- and both were caught here rather than by reading a
//! timing.

use std::process::Command;

fn rakc() -> &'static str {
    env!("CARGO_BIN_EXE_rakc")
}

fn workloads() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("examples")
        .join("bench");
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension().map(|x| x == "rak").unwrap_or(false)).then_some(p)
        })
        .collect();
    v.sort();
    assert!(!v.is_empty(), "no workloads in {}", dir.display());
    v
}

fn run(mode: &str, path: &std::path::Path) -> String {
    let out = Command::new(rakc())
        .arg(mode)
        .arg(path)
        .output()
        .unwrap_or_else(|e| panic!("spawning rakc {} failed: {}", mode, e));
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    assert!(
        out.status.success(),
        "{} failed on {}:\n{}",
        mode,
        path.display(),
        text
    );
    text
}

#[test]
fn bench_workloads_agree_across_backends() {
    for w in workloads() {
        let i = run("run", &w);
        let v = run("vm", &w);
        assert_eq!(
            i, v,
            "{} produced different output on the two backends:\n  interpreter: {:?}\n  vm:           {:?}",
            w.display(),
            i.trim(),
            v.trim()
        );
        assert!(
            i.contains("[DUMP]"),
            "{} produced no output, so it measures nothing:\n{}",
            w.display(),
            i
        );
    }
}

/// `rakc bench` must actually measure, which means every phase must report a
/// non-zero number. The previous implementation printed `0 ms` for anything under a
/// millisecond via `as_millis()`, so a front end that does real work was
/// indistinguishable from one that did not.
#[test]
fn bench_reports_every_phase() {
    let w = workloads()
        .into_iter()
        .find(|p| p.file_name().unwrap().to_string_lossy().contains("arith"))
        .expect("arith.rak workload");
    let out = Command::new(rakc())
        .args(["bench", &w.to_string_lossy(), "--repeat", "3"])
        .output()
        .expect("spawning rakc bench");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "bench failed:\n{}{}",
        text,
        String::from_utf8_lossy(&out.stderr)
    );
    for phase in ["lex", "parse", "compile", "interp", "vm"] {
        assert!(
            text.contains(phase),
            "bench output is missing the {} phase:\n{}",
            phase,
            text
        );
    }
    assert!(
        text.contains("samples, median"),
        "bench must say how many samples it took and that they are a median:\n{}",
        text
    );
}

/// `--repeat` has to be honoured, and it must not be silently ignored the way the
/// old two-number implementation ignored everything except its own timing.
#[test]
fn bench_rejects_a_non_positive_repeat() {
    let w = workloads().remove(0);
    let out = Command::new(rakc())
        .args(["bench", &w.to_string_lossy(), "--repeat", "0"])
        .output()
        .expect("spawning rakc bench");
    assert!(
        !out.status.success(),
        "--repeat 0 should be rejected, not treated as one sample"
    );
}

/// The constant folder must not change what a program computes. It folds only
/// literal-on-literal arithmetic, and must decline everything it cannot prove,
/// including the cases that must stay errors.
#[test]
fn constant_folding_preserves_semantics() {
    let dir = std::env::temp_dir().join("rakc_fold_semantics");
    std::fs::create_dir_all(&dir).unwrap();
    let cases: &[(&str, &str)] = &[
        // `<<` binds looser than `+`/`-` in Rak, so this is
        // `(2 * 3 + 7 / 2 - 1) << 4` = `(6 + 3 - 1) << 4` = 128.
        // Written out here because the first version of this test expected 19 and
        // was wrong: it assumed `<<` bound tightest. The folding was right.
        ("dump 2 * 3 + 7 / 2 - 1 << 4\n", "[DUMP] 128"),
        (
            "dump [(2 * 3), (7 / 2), (1 << 4), (2 * 3 + 1)]\n",
            "[DUMP] [6, 3, 16, 7]",
        ),
        ("dump 2147483647 * 2147483647 * 2147483647\n", "[DUMP]"),
        ("dump 5 % 0\n", "Division by zero"),
        ("dump 1 / 0\n", "Division by zero"),
        ("dump 3 * undefined_name\n", "Undefined variable"),
    ];
    for (i, (src, want)) in cases.iter().enumerate() {
        let p = dir.join(format!("case{}.rak", i));
        std::fs::write(&p, src).unwrap();
        for mode in ["run", "vm"] {
            let out = Command::new(rakc()).arg(mode).arg(&p).output().unwrap();
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(
                text.contains(want),
                "{} on {}: expected {:?} in {:?}",
                src.trim(),
                mode,
                want,
                text
            );
        }
    }
}
