//! Module namespaces — the live global bindings behind `import m`.
//!
//! Rak resolves `import m` to a *handle*, not a copy: `m.X` reads whatever the
//! module's own code last stored in `X`. That is the behaviour `import m; m.X`
//! has in Python, and it is what makes a module usable as shared state rather
//! than as a bag of constants copied at import time.
//!
//! # Why this is generic
//!
//! The two backends have separate `Value` enums (`interpreter::Value` and
//! `value::Value`) that share no variants, so a namespace holding one cannot
//! hold the other. Parameterising over `V` lets both backends use the same
//! bookkeeping — the public/private split, the live cell, the lookup rules —
//! without either of them reimplementing it and the two drifting apart. This is
//! the same arrangement as `crate::setrepr::SetRepr`, which is generic for the
//! same reason.
//!
//! # The two access rules
//!
//! * `import m` hands out the namespace itself, so `m.X` and `m.X = v` read and
//!   write the module's real state, and they keep doing so after the importing
//!   script has moved on or the module has been passed to another function.
//! * `from m import X` copies the value out, once, at import time. If the module
//!   later rebinds `X`, the already-imported name does not follow. This is
//!   Python's rule and it is deliberate: a live binding for a `from`-imported
//!   name would be a second, invisible way for a module to mutate a caller's
//!   variables, and it would make "who changed this?" unanswerable.
//!
//! Use `import m` when you want to follow the module's state; use `from m
//! import X` when you want a value you own.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// A module's namespace: its global bindings, and which of them it exports.
///
/// `values` is the *same* cell the module's own functions close over, so a
/// `pub let mut` that the module reassigns is visible through `m.X` the moment it
/// happens. `public` records the names marked `pub`/`export`; a module's other
/// top-level bindings live in `values` too but stay unreachable from outside,
/// which is what `pub` has always meant.
pub struct ModuleNamespace<V> {
    pub values: Arc<Mutex<HashMap<String, V>>>,
    pub public: HashSet<String>,
    /// Exported names the module declared `let mut`, rather than `let`.
    ///
    /// `m.X = v` writes the module's real state, so it has to respect the same
    /// rule a write inside the module does: `pub let NAME = "x"` is a constant of
    /// the module's API and an importer must not be able to move it, because the
    /// module's own functions read it. Without this, `pub` was enforced while
    /// `let` was not, which is the more surprising of the two holes — the importer
    /// silently changes something the module believes is fixed.
    pub mutable: HashSet<String>,
    /// Where each export's value actually lives, for backends whose module
    /// bindings are not the cell.
    ///
    /// The interpreter gives a module its own `Env`, so `values` *is* the module's
    /// state and a write through `m.X` needs nowhere else to go. The VM inlines a
    /// module body into the same chunk, so its exports are ordinary globals and
    /// `values` is a projection of them that `Op::StoreGlobal` keeps up to date.
    /// A write that only reached `values` would be overwritten by the next store to
    /// the global, so the VM needs to know which global backs each export and write
    /// through to it.
    ///
    /// Empty on the interpreter, and empty on the VM until `MakeModule` has run, so
    /// a write before the mapping exists falls back to the projection rather than
    /// failing.
    backing: HashMap<String, String>,
}

impl<V> Clone for ModuleNamespace<V> {
    fn clone(&self) -> Self {
        ModuleNamespace {
            values: self.values.clone(),
            public: self.public.clone(),
            mutable: self.mutable.clone(),
            backing: self.backing.clone(),
        }
    }
}

impl<V> Default for ModuleNamespace<V> {
    fn default() -> Self {
        ModuleNamespace::empty()
    }
}

impl<V> ModuleNamespace<V> {
    /// A namespace whose bindings live in `values`.
    pub fn new(values: Arc<Mutex<HashMap<String, V>>>) -> Self {
        ModuleNamespace {
            values,
            public: HashSet::new(),
            mutable: HashSet::new(),
            backing: HashMap::new(),
        }
    }

    /// An empty namespace with a cell of its own.
    pub fn empty() -> Self {
        ModuleNamespace::new(Arc::new(Mutex::new(HashMap::new())))
    }

    /// Read an exported name. `None` for a private or an unknown one, so
    /// `m.private` and `m.nosuchname` are indistinguishable from outside — which
    /// is the point.
    pub fn get(&self, name: &str) -> Option<V>
    where
        V: Clone,
    {
        if !self.is_public(name) {
            return None;
        }
        self.values.lock().unwrap().get(name).cloned()
    }

    /// Whether `name` is part of the module's public surface.
    ///
    /// Separate from `get` so a caller that only needs the answer does not clone
    /// the stored value to find out.
    pub fn is_public(&self, name: &str) -> bool {
        self.public.contains(name)
    }

    /// Whether `m.name = v` would be allowed.
    ///
    /// Both halves matter: `pub` decides the name is reachable at all, and the
    /// module's own `let` / `let mut` decides whether it may be written.
    pub fn is_writable(&self, name: &str) -> bool {
        self.is_public(name) && self.mutable.contains(name)
    }

    /// Write through `m.X = v`. Returns false for a private name, an unknown one,
    /// or one the module declared `let` rather than `let mut` — in which case the
    /// caller reports the reason; see [`ModuleNamespace::refusal`].
    pub fn set(&self, name: &str, value: V) -> bool {
        if !self.is_writable(name) {
            return false;
        }
        self.values.lock().unwrap().insert(name.to_string(), value);
        true
    }

    /// Why a `set` was refused, so the caller can say which rule applied instead
    /// of reporting one catch-all.
    /// Why a read of `name` through a module handle is refused.
    ///
    /// Separate from `refusal`, which is about assignment: reading a name the module
    /// did not export has nothing to do with `let` versus `let mut`, and reusing the
    /// assignment message would blame the wrong rule.
    pub fn read_refusal(&self, name: &str) -> String {
        if self.is_public(name) {
            // Public, so it is present; the caller reached here only because the
            // value is somehow absent. Say what is actually true rather than
            // claiming it is unexported.
            return format!("'{}' is exported but has no value yet", name);
        }
        format!("'{}' is not exported from this module", name)
    }

    pub fn refusal(&self, name: &str) -> String {
        if !self.is_public(name) {
            return format!("'{}' is not exported from this module, so it cannot be assigned", name);
        }
        format!(
            "'{}' is declared `let` in its module, so it is immutable; the module must \
             declare `pub let mut` for it to be assignable through the module",
            name
        )
    }

    /// Record a name as exported. Called once per `pub`/`export` declaration
    /// while the module body runs.
    pub fn mark_public(&mut self, name: &str) {
        self.public.insert(name.to_string());
    }

    /// Record a name as exported *and* writable, from a `pub let mut`.
    pub fn mark_public_mut(&mut self, name: &str) {
        self.public.insert(name.to_string());
        self.mutable.insert(name.to_string());
    }

    /// Record several names as exported, all writable. Used by `mod` blocks,
    /// where every top-level name is exported and the block's own `let mut` says
    /// which of them may be written.
    pub fn mark_public_mut_all(&mut self, names: impl IntoIterator<Item = String>) {
        for n in names {
            self.mark_public_mut(&n);
        }
    }

    /// Mark several names writable without changing what is public. Used to apply
    /// a module's `let mut` to names a re-export has already published.
    pub fn mark_mutable_all(&mut self, names: impl IntoIterator<Item = String>) {
        for n in names {
            self.mutable.insert(n);
        }
    }

    /// Record several names as exported.
    pub fn mark_public_all(&mut self, names: impl IntoIterator<Item = String>) {
        for n in names {
            self.mark_public(&n);
        }
    }
    /// Every exported name with its current value, for `from m import *`.
    pub fn exports(&self) -> Vec<(String, V)>
    where
        V: Clone,
    {
        let values = self.values.lock().unwrap();
        let mut out: Vec<(String, V)> = self
            .public
            .iter()
            .filter_map(|n| values.get(n).map(|v| (n.clone(), v.clone())))
            .collect();
        // Sorted, so a star-import binds names in a predictable order and a
        // program that dumps them does not produce different output run to run.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The exported names, sorted, for the error when one is missing.
    pub fn public_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.public.iter().cloned().collect();
        names.sort();
        names
    }

    /// Store a value bypassing both the export and the mutability checks.
    ///
    /// For filling a namespace while its module body is still running, and for the
    /// VM republishing a chunk global into every namespace that exposes it. Neither
    /// is a write *through* the module, so neither should be judged against the
    /// module's `pub`/`let mut` rules.
    pub fn store(&self, name: &str, value: V) {
        self.values.lock().unwrap().insert(name.to_string(), value);
    }

    /// Record which global backs each export. Called once by the VM, when the
    /// namespace is created.
    pub fn set_backing(&mut self, backing: HashMap<String, String>) {
        self.backing = backing;
    }

    /// The global that backs `export`, if this namespace has been told.
    pub fn backing_global(&self, export: &str) -> Option<&str> {
        self.backing.get(export).map(|s| s.as_str())
    }

    /// Store `value` under `export` and mark it public, used when a re-export adds
    /// a name to a module that is still loading.
    pub fn publish(&mut self, export: &str, value: V) {
        self.store(export, value);
        self.mark_public(export);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_names_are_invisible_and_unwritable() {
        let mut ns = ModuleNamespace::<i64>::empty();
        ns.store("hidden", 1);
        ns.store("open", 2);
        ns.store("fixed", 3);
        ns.mark_public("open");
        ns.mark_public("fixed");
        ns.mark_public_mut("open");

        assert_eq!(ns.get("open"), Some(2));
        assert_eq!(ns.get("fixed"), Some(3));
        assert_eq!(ns.get("hidden"), None, "a private name must not read through");
        assert_eq!(ns.get("nosuch"), None);

        // `pub let mut` is writable through the handle.
        assert!(ns.set("open", 9));
        assert_eq!(ns.get("open"), Some(9));

        assert!(!ns.set("hidden", 9), "a private name must not be writable through");
    }

    #[test]
    fn an_exported_let_is_not_writable_through_the_handle() {
        // `pub let NAME = "x"` is a constant of the module's API. An importer
        // that could write it would change what the module's own functions read,
        // so `m.NAME = v` has to be refused - and the refusal has to say which of
        // the two rules applied, so the caller is not left guessing.
        let mut ns = ModuleNamespace::<&str>::empty();
        ns.store("NAME", "orig");
        ns.mark_public("NAME");

        assert_eq!(ns.get("NAME"), Some("orig"), "reading is fine");
        assert!(!ns.set("NAME", "hacked"));
        assert_eq!(ns.get("NAME"), Some("orig"), "the value must be untouched");
        assert!(
            ns.refusal("NAME").contains("let mut"),
            "got: {}",
            ns.refusal("NAME")
        );
        // A name that is not exported at all gets the other message.
        assert!(ns.refusal("nope").contains("not exported"));
    }

    #[test]
    fn exports_are_sorted_so_star_imports_are_deterministic() {
        let mut ns = ModuleNamespace::<i64>::empty();
        ns.store("zeta", 1);
        ns.store("alpha", 2);
        ns.mark_public_all(["alpha".to_string(), "zeta".to_string()]);

        let names: Vec<String> = ns.exports().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["alpha".to_string(), "zeta".to_string()]);
        // The private binding is in `values` but must not be exported.
        assert_eq!(ns.exports().len(), 2);
    }

    #[test]
    fn a_namespace_outlives_its_first_handle() {
        // The whole reason this is an `Arc<Mutex<_>>`: two handles taken at
        // different times must see each other's writes, which is what makes
        // `import m` a live view rather than a copy.
        let mut a = Arc::new(ModuleNamespace::<i64>::empty());
        {
            let ns = Arc::get_mut(&mut a).unwrap();
            ns.store("COUNT", 0);
            ns.mark_public_mut("COUNT");
        }
        let b = a.clone();
        assert!(b.set("COUNT", 5));
        assert_eq!(a.get("COUNT"), Some(5));
    }

    #[test]
    fn is_shareable_across_threads() {
        // A module handle ends up inside `spawn` closures and GUI callbacks, so
        // the representation has to be Send + Sync.
        //
        // Asserted on the real `Value` types rather than on `i64`, which is Send
        // unconditionally and would make the assertion vacuous. This is the
        // property that lets two threads share one module's state.
        fn assert_send_sync<T: Send + Sync>(_: &T) {}
        assert_send_sync(&crate::interpreter::Value::Nil);
        assert_send_sync(&crate::value::Value::Nil);
        let interp_ns: Arc<ModuleNamespace<crate::interpreter::Value>> =
            Arc::new(ModuleNamespace::empty());
        assert_send_sync(&interp_ns);
        let vm_ns: Arc<ModuleNamespace<crate::value::Value>> = Arc::new(ModuleNamespace::empty());
        assert_send_sync(&vm_ns);
    }

    #[test]
    fn two_handles_onto_one_module_share_one_state() {
        // The property the interpreter's per-module cell buys and the VM's flat
        // globals cannot offer: two names for one module are two handles onto one
        // state, not two states. `import m` and `import m as k` depend on it.
        let mut m = Arc::new(ModuleNamespace::<i64>::empty());
        {
            let ns = Arc::get_mut(&mut m).unwrap();
            ns.store("N", 0);
            ns.mark_public_mut("N");
        }
        let other = m.clone();
        assert!(other.set("N", 7));
        assert_eq!(m.get("N"), Some(7));
        assert!(m.set("N", 8));
        assert_eq!(other.get("N"), Some(8));
    }
}
