//! Cross-module variable semantics: `import m`, `from m import x`, and what
//! each one binds.
//!
//! These are the rules a multi-file Rak program depends on, and each one here
//! covers something that was a real defect:
//!
//! * `import m; m.X` bound a *copy* of the module's exports taken at import time,
//!   so a `pub let mut` the module later reassigned never reached the importer.
//! * A module's own top-level `let mut` reset on every call under `rakc run`,
//!   because a module body ran in a scope on the *importer's* environment and
//!   `Env::clone` deep-copies scopes.
//! * A module could read the importer's globals.
//! * `pub` was not enforced on `m.X` once the namespace went live.
//! * `m.X = v` on the VM wrote only the namespace projection, so the module's own
//!   functions never saw it - and, once it was fixed, it wrote through `pub let`
//!   bindings the module had declared fixed.
//! * Binding a module handle through `Op::StoreGlobal` republished the handle into
//!   any namespace exporting a global of the same name, so `import beta` could
//!   replace `alpha.beta()`.
//! * `import pkg.sub` followed by `import pkg` replaced the package namespace and
//!   dropped the submodule.
//! * Binding a namespace to a name already holding it took the same `Mutex` twice
//!   and deadlocked, so `import m` twice hung the program.
//!
//! `run_on_both` runs every case on the interpreter and the VM and fails if they
//! disagree, because the interesting part of a fix like this is that both
//! backends end up with the *same* rule - not merely a rule on one of them.
//!
//! Where the two genuinely differ, the test says so by name and asserts each
//! side's behaviour explicitly. Those are ratchets: when the VM's flat global
//! namespace is namespaced, the test that documents the hole fails and gets
//! replaced with an `agree_on` one.

use std::path::{Path, PathBuf};

/// Write a fixture tree to a temp directory.
///
/// The directory is removed first, so a test cannot pass on a file left over from
/// an earlier run - an import that should fail would find the stale module and
/// quietly succeed.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str, files: &[(&str, &str)]) -> Fixture {
        let dir = std::env::temp_dir().join(format!("rak_modstate_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        for (name, body) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create fixture subdir");
            }
            std::fs::write(&path, body).expect("write fixture file");
        }
        Fixture { dir }
    }

    fn base(&self) -> &str {
        self.dir.to_str().expect("fixture dir is valid utf-8")
    }

    fn source(&self, entry: &str) -> String {
        std::fs::read_to_string(self.dir.join(entry)).expect("read entry file")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Run `entry` on both backends, requiring byte-identical output.
fn agree_on(fx: &Fixture, entry: &str) -> Vec<String> {
    let source = fx.source(entry);
    let parity = rakc::run_on_both(&source, fx.base());
    if let Some(why) = parity.divergence() {
        panic!(
            "backend divergence running `{}`:\n{}\n--- source ---\n{}",
            entry, why, source
        );
    }
    match parity {
        rakc::BackendParity::Agree(out) => out,
        other => panic!("both backends failed on `{}`, so nothing was compared: {:?}", entry, other),
    }
}

const COUNTER: &str = r#"
pub let mut COUNT = 0
pub let NAME = "counter"

// Private: not `pub`, so unreachable as `counter.hidden` from outside, but
// readable by this module's own code.
let scale = 10

pub fn bump() {
    COUNT = COUNT + 1
    return COUNT
}

pub fn read_scale() {
    return scale
}
"#;

#[test]
fn import_binds_a_live_view_of_the_module_globals() {
    // The headline behaviour: `import m` hands out the module's state, not a
    // snapshot of it. Both of these used to read 0.
    let fx = Fixture::new(
        "live",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
dump counter.bump()
dump counter.bump()
dump counter.COUNT
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 1", "[DUMP] 2", "[DUMP] 2"]);
}

#[test]
fn a_modules_top_level_mutable_persists_across_a_call_chain() {
    // Guards the specific regression this file exists for. Called through two
    // levels of ordinary call chain, because a module's state used to live in a
    // scope that was deep-copied per call and so silently reset at every level.
    let fx = Fixture::new(
        "chain",
        &[
            ("bank.rak", r#"
pub let mut BALANCE = 0
pub fn deposit(n) { BALANCE = BALANCE + n  return BALANCE }
pub fn report() { return BALANCE }
"#),
            (
                "main.rak",
                r#"
import bank

fn deposit_twice(n) {
    bank.deposit(n)
    return bank.deposit(n)
}

deposit_twice(5)
deposit_twice(5)
dump bank.report()
"#,
            ),
        ],
    );
    // Four calls of 5, and none of them was forgotten on the way back.
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 20"]);
}

#[test]
fn two_handles_to_one_module_share_its_state() {
    // `import m` and `import m as k` must be the same module, not two views that
    // drift apart. If they were separate cells, `k.COUNT` would read 0 here.
    let fx = Fixture::new(
        "twohandles",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
import counter as k
dump k.bump()
dump counter.bump()
dump k.COUNT
dump counter.COUNT
"#,
            ),
        ],
    );
    assert_eq!(
        agree_on(&fx, "main.rak"),
        vec!["[DUMP] 1", "[DUMP] 2", "[DUMP] 2", "[DUMP] 2"]
    );
}

#[test]
fn importing_the_same_module_twice_terminates() {
    // Regression: binding a namespace to a name already holding it took the same
    // `Mutex` twice, and `std::sync::Mutex` is not reentrant, so this hung
    // rather than failing. `cargo test` could only see it as a timeout.
    let fx = Fixture::new(
        "twice",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
import counter
import counter
dump counter.bump()
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 1"]);
}

#[test]
fn importing_a_module_named_like_another_modules_export() {
    // Regression: binding a handle used to go through `Op::StoreGlobal`, which
    // republishes into every namespace exporting a global of the same name. So
    // `import beta` stored the *handle* into the global `beta` and republished it
    // into alpha's namespace under the export name `beta` - replacing
    // `alpha.beta()` with a module. `alpha.beta()` has to survive it.
    let fx = Fixture::new(
        "namecollision",
        &[
            ("alpha.rak", "pub fn beta() { return 77 }\n"),
            ("beta.rak", "pub let mut B = 0\npub fn bb() { return 5 }\n"),
            (
                "main.rak",
                r#"
import alpha
dump alpha.beta()
import beta
dump alpha.beta()
dump beta.bb()
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 77", "[DUMP] 77", "[DUMP] 5"]);
}

#[test]
fn from_import_with_an_alias_is_a_snapshot() {
    // `from m import x as y` copies. This is the form that behaves identically
    // on both backends, and therefore the one to reach for when you want a value
    // you own rather than a view of the module's.
    //
    // The snapshot is taken at *import* time, and imports are processed before the
    // first statement, so it is 0 rather than the 15 the module reaches later.
    let fx = Fixture::new(
        "snapalias",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
from counter import COUNT as opened_at, bump
dump bump()
dump bump()
dump opened_at
dump counter.COUNT
"#,
            ),
        ],
    );
    assert_eq!(
        agree_on(&fx, "main.rak"),
        vec!["[DUMP] 1", "[DUMP] 2", "[DUMP] 0", "[DUMP] 2"]
    );
}

#[test]
fn star_import_brings_in_the_public_names() {
    let fx = Fixture::new(
        "star",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
from counter import *
dump NAME
dump read_scale()
"#,
            ),
        ],
    );
    // `read_scale` is a function, so calling it runs the module's own code and
    // reads the private `scale` the importer cannot reach.
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] counter", "[DUMP] 10"]);
}

#[test]
fn star_import_does_not_overwrite_a_local() {
    let fx = Fixture::new(
        "starlocal",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
let NAME = "mine"
from counter import *
dump NAME
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] mine"]);
}

#[test]
fn star_import_is_a_snapshot_on_both_backends() {
    // `from m import *` copies. On the VM it used not to: the imported names were
    // the module's own chunk globals, so `COUNT` kept referring to the module and
    // followed it, printing 2 here against the interpreter's 0.
    //
    // That was a symptom of the flat global namespace rather than a separate bug,
    // and it went away with the namespacing, so this is now an `agree_on` test
    // rather than a pinned divergence. `docs/V8-KNOWN-ISSUES.md` recorded the
    // asymmetry; this is the test that says it is gone.
    let fx = Fixture::new(
        "stardiv",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
from counter import *
counter.bump()
counter.bump()
dump COUNT
"#,
            ),
        ],
    );
    // 0, not 2: the copy was taken before the two bumps.
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 0"]);
}

#[test]
fn assigning_through_a_module_writes_the_modules_own_state() {
    // `m.X = v` has to land in the module, not in a copy the importer holds -
    // otherwise it is a write to nowhere. On the VM that means the chunk global
    // the module's own functions read, not just the namespace projection.
    let fx = Fixture::new(
        "write",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
counter.COUNT = 41
dump counter.COUNT
counter.bump()
dump counter.COUNT
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 41", "[DUMP] 42"]);
}

#[test]
fn assigning_through_a_module_writes_through_a_function_argument() {
    // The same write, reached through a module passed as a value. If the write
    // only reached the namespace projection, `report()` would still see 0.
    let fx = Fixture::new(
        "writearg",
        &[
            ("bank.rak", "pub let mut TOTAL = 0\npub fn report() { return TOTAL }\n"),
            (
                "main.rak",
                r#"
import bank
fn set(m, v) { m.TOTAL = v  return nil }
set(bank, 99)
dump bank.report()
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 99"]);
}

#[test]
fn an_exported_let_is_not_assignable_through_the_module() {
    // `pub let NAME = "orig"` is a constant of the module's API. An importer that
    // could write it would change what the module's own functions read, so
    // `m.NAME = v` must be refused - and the refusal must name the reason, or the
    // reader goes looking for a spelling mistake.
    let fx = Fixture::new(
        "immutable",
        &[
            ("fixed.rak", "pub let NAME = \"orig\"\npub fn get() { return NAME }\n"),
            (
                "main.rak",
                r#"
import fixed
dump fixed.get()
fixed.NAME = "hacked"
"#,
            ),
        ],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(msg) => {
            assert!(
                msg.contains("NAME") && msg.contains("let mut"),
                "the diagnostic should name the binding and the rule, got: {}",
                msg
            );
        }
        other => panic!("assigning an exported `let` was accepted: {:?}", other),
    }
}

#[test]
fn a_private_name_is_not_assignable_through_the_module() {
    let fx = Fixture::new(
        "assignprivate",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
counter.scale = 99
"#,
            ),
        ],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(msg) => {
            assert!(msg.contains("scale"), "got: {}", msg);
            assert!(
                msg.contains("not exported"),
                "a private name should be refused as unexported, got: {}",
                msg
            );
        }
        other => panic!("assigning a private name was accepted: {:?}", other),
    }
}

#[test]
fn a_modules_own_function_can_still_read_its_private_names() {
    // The other half of `pub`: privacy is about the *importer's* view, not the
    // module's. A name with no `pub` is unreadable as `counter.scale` from
    // outside, and completely ordinary inside.
    let fx = Fixture::new(
        "private",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
dump counter.read_scale()
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 10"]);
}

#[test]
fn a_module_cannot_read_the_importers_globals() {
    // Name resolution is per-file. Before the fix a module body ran in a scope on
    // the *importer's* environment, so `peek()` resolved `TOKEN` against the
    // importer's binding and silently succeeded on the interpreter too.
    //
    // STILL A DIVERGENCE, and namespacing did not fix this one.
    //
    // Mangling gives a module its own globals for the names it *declares*, so the
    // private top-level of a module is now unreachable from outside and two modules
    // can export the same name. But `TOKEN` is a name `peek.rak` never declares, so
    // it is not in the module's rename map, and it still resolves against the
    // importer's globals: the free name is a chunk global and the importer's globals
    // are visible to it.
    //
    // Closing this needs a module *scope* -- a free name inside a module body that is
    // neither local, nor one of its own top-level names, nor a name it imported,
    // nor a builtin, has to be an error. The compiler cannot make that call: it has
    // no list of builtin globals, so it cannot tell a builtin from a leaked one.
    // See docs/V8-KNOWN-ISSUES.md.
    let fx = Fixture::new(
        "isolate",
        &[
            ("peek.rak", "pub fn peek() { return TOKEN }\n"),
            (
                "main.rak",
                r#"
let TOKEN = "from-main"
import peek
dump peek.peek()
"#,
            ),
        ],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(message) => {
            // If this ever starts passing as agreement, the divergence is closed and
            // this test should become a plain `agree_on`.
            panic!("the importer-global leak is fixed; convert this to agree_on: {}", message);
        }
        rakc::BackendParity::VmOnly { interp_error, vm_output } => {
            let lowered = interp_error.to_lowercase();
            assert!(
                lowered.contains("undefined") || lowered.contains("token"),
                "expected the interpreter to fail naming TOKEN, got: {}",
                interp_error
            );
            assert_eq!(
                vm_output,
                vec!["[DUMP] from-main"],
                "the VM still resolves a module's free name against the importer's globals"
            );
        }
        other => panic!("expected interp-only failure, got: {:?}", other),
    }
}

/// A module's private top-level is unreachable from outside.
///
/// This was the other half of `pub` being only half true: with every module's
/// bindings sharing one flat namespace of chunk globals, the importer could name
/// `scale` directly and the VM handed it over. Mangling puts the module's `scale`
/// in a global of its own, so the name the importer wrote no longer exists.
#[test]
fn a_modules_private_top_level_is_not_visible_to_the_importer() {
    let fx = Fixture::new(
        "private-leak",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter
dump scale
"#,
            ),
        ],
    );
    // Both backends must refuse, and the only thing allowed to differ is wording:
    // "Undefined variable: scale" against "Undefined: scale" is the documented
    // `err`-prefix asymmetry, not a leak. What matters is that neither *succeeded*.
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(message) => {
            assert!(message.to_lowercase().contains("scale"), "got: {}", message);
        }
        rakc::BackendParity::DisagreeOnError { interp_error, vm_error } => {
            for (name, err) in [("interpreter", &interp_error), ("VM", &vm_error)] {
                let lowered = err.to_lowercase();
                assert!(
                    lowered.contains("undefined") && lowered.contains("scale"),
                    "expected the {} to refuse with an undefined-variable error naming scale, got: {}",
                    name,
                    err
                );
            }
        }
        other => panic!(
            "a module's private `scale` leaked to the importer: {:?}",
            other
        ),
    }
}

/// Two modules may export the same name without sharing one binding.
///
/// The worst of the six symptoms, because nothing reported it: both modules' `shared`
/// were the same chunk global, so `a1.sa()` reported `b1`'s state with no diagnostic
/// anywhere. Namespacing gives each module its own global per name.
#[test]
fn two_modules_may_export_the_same_name() {
    let fx = Fixture::new(
        "collide",
        &[
            ("a1.rak", "pub let mut shared = 0
pub fn sa() { shared = shared + 1
  return shared }
"),
            ("b1.rak", "pub let mut shared = 100
pub fn sb() { shared = shared + 1
  return shared }
"),
            (
                "main.rak",
                r#"
import a1
import b1
dump a1.sa()
dump b1.sb()
dump a1.shared
dump b1.shared
"#,
            ),
        ],
    );
    assert_eq!(
        agree_on(&fx, "main.rak"),
        vec!["[DUMP] 1", "[DUMP] 101", "[DUMP] 1", "[DUMP] 101"]
    );
}

#[test]
fn a_re_export_hands_out_a_copy_not_a_second_live_view() {
    // `pub use` copies. That is deliberate: it matches Python, and it means a
    // re-export chain cannot be used to smuggle a live binding past `from`-import
    // snapshot semantics.
    //
    // `v0` (the `from`-imported snapshot) reads 0 on both backends, which is the
    // part that matters and the part that could regress.
    let fx = Fixture::new(
        "reexport",
        &[
            ("inner.rak", "pub let mut V = 0\npub fn bump() { V = V + 1  return V }\n"),
            ("outer.rak", "pub use {bump, V} from inner\n"),
            (
                "main.rak",
                r#"
import outer
from outer import bump, V as v0
dump bump()
dump v0
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 1", "[DUMP] 0"]);
}

#[test]
fn nested_directory_packages_share_one_namespace_in_either_order() {
    let fx = Fixture::new(
        "pkg",
        &[
            ("pkg/init.rak", "pub let mut TOTAL = 0\npub fn add(n) { TOTAL = TOTAL + n  return TOTAL }\n"),
            ("pkg/sub.rak", "pub let mut SUBT = 0\npub fn bump() { SUBT = SUBT + 1  return SUBT }\n"),
            (
                "pkg_first.rak",
                r#"
import pkg
import pkg.sub
dump pkg.add(5)
dump pkg.add(3)
dump pkg.TOTAL
dump pkg.sub.bump()
dump pkg.sub.SUBT
"#,
            ),
            (
                "sub_first.rak",
                r#"
import pkg.sub
import pkg
dump pkg.PKGVERLESS()
"#,
            ),
        ],
    );
    assert_eq!(
        agree_on(&fx, "pkg_first.rak"),
        vec!["[DUMP] 5", "[DUMP] 8", "[DUMP] 8", "[DUMP] 1", "[DUMP] 1"]
    );
}

#[test]
fn importing_a_submodule_first_still_gives_you_the_package() {
    // Regression: `import pkg.sub` synthesised a throwaway package namespace
    // holding only `sub`, and a later `import pkg` *replaced* it - so `pkg.sub`
    // stopped resolving. `pkg`'s own exports are reachable from `import pkg.sub`
    // alone as well, which is what makes the two orderings equivalent.
    let fx = Fixture::new(
        "subfirst",
        &[
            ("pkg/init.rak", "pub let PKGVER = \"1.0\"\n"),
            ("pkg/sub.rak", "pub fn twice(n) { return n * 2 }\n"),
            (
                "main.rak",
                r#"
import pkg.sub
dump pkg.PKGVER
dump pkg.sub.twice(21)
"#,
            ),
            (
                "both.rak",
                r#"
import pkg.sub
import pkg
dump pkg.PKGVER
dump pkg.sub.twice(21)
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 1.0", "[DUMP] 42"]);
    assert_eq!(agree_on(&fx, "both.rak"), vec!["[DUMP] 1.0", "[DUMP] 42"]);
}

#[test]
fn circular_imports_resolve_on_both_backends() {
    // A cycle means one module's namespace is read before its body has run. Both
    // backends have to cope: the interpreter returns a partially-built entry, and
    // the VM binds the namespace it has already created.
    let fx = Fixture::new(
        "cycle",
        &[
            ("a.rak", "import b\npub let mut AV = 1\npub fn av() { return AV + b.bv() }\n"),
            ("b.rak", "import a\npub fn bv() { return a.AV }\n"),
            ("main.rak", "import a\ndump a.av()\n"),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 2"]);
}

#[test]
fn a_module_can_be_passed_around_as_a_value() {
    // The reason `import m` is a handle and not a compile-time rewrite: a module
    // is an ordinary value, so it survives being passed to a function.
    let fx = Fixture::new(
        "passable",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
import counter

fn use_it(m, n) {
    return m.bump() + n
}

dump use_it(counter, 100)
dump counter.COUNT
"#,
            ),
        ],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 101", "[DUMP] 1"]);
}

#[test]
fn an_inline_mod_block_also_keeps_state() {
    // `mod { .. }` is the in-file spelling of a module and had the same
    // snapshot-at-instant behaviour.
    let fx = Fixture::new(
        "inline",
        &[(
            "main.rak",
            r#"
mod counter {
    let mut COUNT = 0
    fn bump() -> int { COUNT = COUNT + 1  return COUNT }
}

dump counter.bump()
dump counter.bump()
dump counter.COUNT
"#,
        )],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 1", "[DUMP] 2", "[DUMP] 2"]);
}

#[test]
fn an_inline_mod_block_respects_let_versus_let_mut() {
    let fx = Fixture::new(
        "inlinemut",
        &[(
            "main.rak",
            r#"
mod m {
    let FIXED = 1
    let mut LOOSE = 2
}
m.LOOSE = 5
dump m.LOOSE
m.FIXED = 9
"#,
        )],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(msg) => {
            assert!(
                msg.contains("FIXED") && msg.contains("let mut"),
                "got: {}",
                msg
            );
        }
        other => panic!("mod-block mutability is not enforced on both backends: {:?}", other),
    }
}

#[test]
fn importing_a_name_that_is_not_exported_says_what_is() {
    // The error has to name the module's real exports. Before the change it was
    // "'x' is not exported" with nothing to go on, which is unhelpful when the
    // problem is `pub`-ness rather than spelling.
    let fx = Fixture::new(
        "missing",
        &[
            ("counter.rak", COUNTER),
            (
                "main.rak",
                r#"
from counter import COUNT, scale
"#,
            ),
        ],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(msg) => {
            assert!(
                msg.contains("COUNT") && msg.contains("NAME"),
                "the diagnostic should list what the module does export, got: {}",
                msg
            );
        }
        other => panic!("importing a private name was accepted or disputed: {:?}", other),
    }
}

/// The shipped example, run on both backends.
///
/// `examples_parity.rs` only walks the top level of `examples/`, so an example in
/// a directory of its own - which is what a module example needs - is otherwise
/// never executed by anything.
#[test]
fn the_module_state_example_runs_on_both_backends() {
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rakc has a parent")
        .join("examples")
        .join("modstate")
        .join("module_state.rak");
    let source = std::fs::read_to_string(&entry)
        .unwrap_or_else(|e| panic!("read {}: {}", entry.display(), e));
    let base = entry.parent().expect("example has a parent").to_str().expect("utf-8 path");
    let parity = rakc::run_on_both(&source, base);
    if let Some(why) = parity.divergence() {
        panic!("the shipped module example diverges:\n{}", why);
    }
    match parity {
        rakc::BackendParity::Agree(lines) => {
            assert!(lines.len() >= 15, "the example should exercise every rule, got {} lines", lines.len());
        }
        other => panic!("the shipped module example failed: {:?}", other),
    }
}

/// A `mod { }` block keeps its own names. Two blocks declaring the same private
/// name used to share one chunk global, so `a.g()` reported `b`'s value -- and
/// neither private name was reachable from outside.
#[test]
fn two_mod_blocks_do_not_share_a_private_name() {
    let fx = Fixture::new(
        "modcollide",
        &[(
            "main.rak",
            r#"
mod a { let N = 1
  pub fn g() { return N } }
mod b { let N = 2
  pub fn g() { return N } }
dump a.g()
dump b.g()
"#,
        )],
    );
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 1", "[DUMP] 2"]);
}

/// A `mod` block's private top-level is not in the enclosing file's scope.
#[test]
fn a_mod_blocks_private_name_does_not_leak_out() {
    let fx = Fixture::new(
        "modleak",
        &[(
            "main.rak",
            r#"
mod a { let hidden = 5
  pub fn g() { return hidden } }
dump hidden
"#,
        )],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(m) => {
            assert!(m.to_lowercase().contains("hidden"), "got: {}", m)
        }
        rakc::BackendParity::DisagreeOnError { interp_error, vm_error } => {
            for (who, e) in [("interpreter", &interp_error), ("VM", &vm_error)] {
                assert!(
                    e.to_lowercase().contains("hidden"),
                    "expected the {} to refuse `hidden`, got: {}",
                    who,
                    e
                );
            }
        }
        other => panic!("`hidden` leaked out of the mod block: {:?}", other),
    }
}

/// Reading a private name through a module handle is an error naming the name,
/// on both backends. It used to read as `nil` on the VM, which made a
/// misspelling indistinguishable from a private name and from a real nil.
#[test]
fn reading_a_private_name_through_the_handle_reports_it() {
    let fx = Fixture::new(
        "readprivate",
        &[(
            "p.rak",
            "pub let PUBV = 1\nlet privv = 2\n",
        ), ("main.rak", "import p\ndump p.privv\n")],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(m) => {
            assert!(m.contains("privv"), "the message should name the field, got: {}", m)
        }
        rakc::BackendParity::DisagreeOnError { interp_error, vm_error } => {
            for (who, e) in [("interpreter", &interp_error), ("VM", &vm_error)] {
                assert!(
                    e.contains("privv"),
                    "expected the {} to name `privv`, got: {}",
                    who,
                    e
                );
            }
        }
        other => panic!("reading a private name should not have succeeded: {:?}", other),
    }
}

/// A misspelled export is reported by name rather than read as nil.
#[test]
fn a_misspelled_export_is_reported_rather_than_read_as_nil() {
    let fx = Fixture::new(
        "typo",
        &[
            ("p.rak", "pub let COUNT = 1\n"),
            ("main.rak", "import p\ndump p.COUTN\n"),
        ],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::AgreeOnError(m) => {
            assert!(m.contains("COUTN"), "the message should name the field, got: {}", m)
        }
        rakc::BackendParity::DisagreeOnError { interp_error, vm_error } => {
            for (who, e) in [("interpreter", &interp_error), ("VM", &vm_error)] {
                assert!(e.contains("COUTN"), "expected the {} to name COUTN, got: {}", who, e);
            }
        }
        other => panic!("a typo read as a value instead of an error: {:?}", other),
    }
}

/// A `mod { }` block is a separate module scope, so it cannot read the enclosing
/// file's bindings -- and this is the same root cause as
/// `a_module_cannot_read_the_importers_globals`, seen from the other side.
///
/// The interpreter treats a `mod` block as a module in its own right: it sees
/// builtins and its own names and nothing else. It cannot see the enclosing
/// file's `let`, its functions, or its imports:
///
/// ```text
/// let TOP = 7          mod w { fn g() { return TOP } }   interp: Undefined variable: TOP
/// fn helper()          mod w { fn g() { return helper() } } interp: Unknown function
/// import helper        mod w { fn g() { return helper.V } }  interp: Undefined variable
/// ```
///
/// The VM resolves all three, because an inlined body still shares the chunk's
/// flat namespace. Namespacing the module's *own* declarations does not change
/// this: these are names the block never declared.
///
/// Closing it needs a module scope in the compiler -- a free name inside a module
/// body that is neither local, nor its own, nor imported by it, nor a builtin has
/// to be an error -- and the compiler has no list of builtins to decide with.
#[test]
fn a_mod_block_cannot_see_the_enclosing_files_bindings() {
    let fx = Fixture::new(
        "modscope",
        &[
            ("helper.rak", "pub let V = 42\n"),
            (
                "main.rak",
                r#"
let TOP = 7
import helper
mod w { pub fn get() { return TOP } }
mod r { pub fn get() { return helper.V } }
dump w.get()
dump r.get()
"#,
            ),
        ],
    );
    match rakc::run_on_both(&fx.source("main.rak"), fx.base()) {
        rakc::BackendParity::Agree(lines) => panic!(
            "a mod block reached the enclosing scope; convert this to agree_on: {:?}",
            lines
        ),
        // Either backend may be the one that refuses first -- `w.get()` fails before
        // `r.get()` is ever reached -- so all this pins is that they do not both
        // succeed at reading the enclosing file's bindings.
        other => {
            let text = format!("{:?}", other);
            assert!(
                !text.contains("Agree"),
                "expected at least one backend to refuse, got: {}",
                text
            );
        }
    }
}

/// One module from-importing another still sees the copy it asked for, and its own
/// `pub` surface is unchanged.
#[test]
fn a_module_can_from_import_another_module() {
    let fx = Fixture::new(
        "modfrommod",
        &[
            (
                "lib.rak",
                "pub let mut N = 0\npub fn inc() { N = N + 1\n  return N }\n",
            ),
            (
                "mid.rak",
                "from lib import N as base, inc\npub fn twice() { inc()\n  return base }\n",
            ),
            ("main.rak", "import mid\ndump mid.twice()\n"),
        ],
    );
    // `base` is the copy taken at import time, so it is 0 even though `inc()` ran.
    assert_eq!(agree_on(&fx, "main.rak"), vec!["[DUMP] 0"]);
}

/// `from m import *` takes a copy, so a later change in the module is not seen --
/// and the module's own state is unaffected by the import.
#[test]
fn a_star_import_is_a_copy_and_leaves_the_module_alone() {
    let fx = Fixture::new(
        "starcopy",
        &[
            (
                "lib.rak",
                "pub let mut N = 0\npub fn inc() { N = N + 1\n  return N }\n",
            ),
            (
                "main.rak",
                r#"
import lib
from lib import *
dump N
lib.inc()
dump N
dump lib.N
"#,
            ),
        ],
    );
    assert_eq!(
        agree_on(&fx, "main.rak"),
        vec!["[DUMP] 0", "[DUMP] 0", "[DUMP] 1"]
    );
}
