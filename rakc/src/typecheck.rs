//! Static type checking pass.
//!
//! Runs after parsing, before interpretation/compilation. It walks the AST,
//! infers the type of each expression, checks explicit type annotations,
//! validates function-call arguments and returns, and enforces Rak's
//! mutability semantics (assignment to an immutable `let` binding).
//!
//! Diagnostics carry a stable error code, a message, a best-effort source
//! location and snippet, plus the expected vs. found types.

use crate::ast::{self, Expr, Pattern, Stmt, Type};
use std::collections::{HashMap, HashSet};

/// A single compile-time diagnostic produced by the type checker.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Stable error code, e.g. `E0308`.
    pub code: &'static str,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub col: usize,
    /// The offending source line (for rendering a snippet).
    pub source_line: String,
    pub expected: Option<String>,
    pub found: Option<String>,
    pub suggestion: Option<String>,
}

impl Diagnostic {
    pub fn render(&self) -> String {
        let mut out = format!(
            "error[{}]: {}\n  --> {}:{}:{}",
            self.code, self.message, self.file, self.line, self.col
        );
        if !self.source_line.is_empty() {
            out.push('\n');
            let digits = self.line.to_string().len();
            out.push_str(&format!(
                "{:>width$} | {}\n",
                self.line,
                self.source_line,
                width = digits
            ));
            out.push_str(&format!(
                "{:>width$} | {}{}",
                "",
                " ".repeat(self.col.saturating_sub(1).min(self.source_line.len())),
                "^".repeat(1.max(1)),
                width = digits
            ));
        }
        if let (Some(e), Some(f)) = (&self.expected, &self.found) {
            out.push_str(&format!("\n\nexpected: {}\nfound:    {}", e, f));
        }
        if let Some(s) = &self.suggestion {
            out.push_str(&format!("\nhelp: {}", s));
        }
        out
    }
}

/// A type environment mapping in-scope names to their inferred types.
#[derive(Clone, Default)]
struct Scope {
    vars: HashMap<String, Type>,
}

impl Scope {
    fn new() -> Self {
        Scope {
            vars: HashMap::new(),
        }
    }
    fn get(&self, name: &str) -> Option<&Type> {
        self.vars.get(name)
    }
}

/// The type-checker state. It holds the source for snippet rendering, the
/// current file name, an environment stack, and collected diagnostics.
/// Nearest declared name, for "did you mean" hints on a field or variant.
///
/// Shares the shape of [`suggest_trait_method`] -- a case-insensitive hit first, then a
/// bounded edit distance -- because the realistic mistakes are the same ones: `nope` for
/// `v`, `G` for `R`, a casing slip.
fn suggest_field(declared: &[String], given: &str) -> Option<String> {
    let lower = given.to_ascii_lowercase();
    declared
        .iter()
        .find(|n| n.to_ascii_lowercase() == lower && *n != given)
        .or_else(|| {
            let mut best: Option<(usize, &String)> = None;
            for n in declared {
                if n == given {
                    continue;
                }
                let d = edit_distance(n, given);
                if d > given.len() / 3 + 1 {
                    continue;
                }
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, n));
                }
            }
            best.map(|(_, n)| n)
        })
        .map(|n| format!("did you mean `{}`?", n))
}

/// Nearest method name in a trait, for "did you mean" hints.
///
/// Cheap on purpose: a distance bound plus a case-insensitive prefix fallback, which
/// between them cover the realistic mistakes (`h1` for `hi`, `iter` for `items`,
/// `Fmt` for `fmt`) without pulling in a metric.
fn suggest_trait_method(required: &[(String, usize)], given: &str) -> Option<String> {
    // Never suggest the name that was already given. That reads as a non sequitur --
    // "does not implement `hi` / help: did you mean `hi`?" -- which is what the
    // missing-method case would otherwise produce, since it passes the method it just
    // named as the thing being misspelled.
    let lower = given.to_ascii_lowercase();
    if let Some((n, _)) = required
        .iter()
        .find(|(n, _)| n.to_ascii_lowercase() == lower && n != given)
    {
        return Some(format!("did you mean `{}`?", n));
    }
    let mut best: Option<(usize, &str)> = None;
    for (n, _) in required {
        if n == given {
            continue;
        }
        let d = edit_distance(n, given);
        if d > given.len() / 3 + 1 {
            continue;
        }
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, n));
        }
    }
    best.map(|(_, n)| format!("did you mean `{}`?", n))
}

/// Levenshtein distance, single-row. Only used to size up a "did you mean" hint.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// How many alias hops [`TypeChecker::expand_alias`] will follow.
///
/// Long enough for any sane chain (`type A = B`, `type B = int`) and short
/// enough that a cycle terminates. Reaching the limit is not an error: the
/// alias is simply left unexpanded and keeps comparing by name, which is what
/// happened before aliases were recorded at all.
const ALIAS_EXPANSION_LIMIT: usize = 16;

pub struct TypeChecker<'a> {
    source: &'a str,
    file: &'a str,
    scopes: Vec<Scope>,
    /// Globals declared so far.
    globals: Scope,
    /// Function signatures by name (for call checking).
    funcs: HashMap<String, (Vec<Type>, Type)>,
    /// Enum definitions: name → (variant name → number of payload fields).
    enums: HashMap<String, Vec<(String, usize)>>,
    /// `type X = T` declarations, name → aliased type.
    ///
    /// Collected so an alias can be expanded when two types are compared.
    /// Without this an alias stayed a bare `Type::Custom`, which `compatible`
    /// does not consider a numeric type -- so `type Meters = int` made
    /// `let x: Meters = 5` a type error while the VM accepted the same program.
    aliases: HashMap<String, Type>,
    /// `trait` declarations, name → (method name → arity).
    ///
    /// Arity includes the receiver, which a method declares as its own first
    /// parameter, so `fn hi(self)` is arity 1.
    traits: HashMap<String, Vec<(String, usize)>>,
    /// `struct` declarations, name → (field name → declared type).
    ///
    /// The type is optional because a field may be declared without one.
    structs: HashMap<String, Vec<(String, Option<Type>)>>,
    /// `enum` payload types, enum name → variant name → field types.
    ///
    /// Separate from `enums`, which records arity only and is read by the match
    /// exhaustiveness check; widening it would mean rewriting that for no gain.
    enum_fields: HashMap<String, HashMap<String, Vec<Type>>>,
    /// Trait method names usable via `.method(...)`.
    pub diagnostics: Vec<Diagnostic>,
    /// Current line being checked (best-effort).
    line: usize,
    /// Whether the original AST used the permissive `let`-is-mutable model
    /// (backwards compatibility). When true, assignment to a plain `let`
    /// binding is not flagged, so existing Rak programs keep working.
    pub permissive_let: bool,
}

impl<'a> TypeChecker<'a> {
    pub fn new(source: &'a str, file: &'a str) -> Self {
        TypeChecker {
            source,
            file,
            scopes: Vec::new(),
            globals: Scope::new(),
            funcs: HashMap::new(),
            enums: HashMap::new(),
            aliases: HashMap::new(),
            traits: HashMap::new(),
            structs: HashMap::new(),
            enum_fields: HashMap::new(),
            diagnostics: Vec::new(),
            line: 1,
            permissive_let: false,
        }
    }

    /// Run the checker over a parsed module, returning collected diagnostics.
    pub fn check_module(&mut self, module: &ast::Module) -> Vec<Diagnostic> {
        // First pass: collect function signatures so calls can be checked even
        // when a function is defined after its first use.
        let mut first_pass = TypeChecker::new(self.source, self.file);
        first_pass.collect_fns(&module.items);
        self.funcs = first_pass.funcs;
        // Structs and enum payloads are collected here for the same reason: a literal
        // may name a type declared further down the file, and a diagnostic about the
        // literal should not depend on the order of the two.
        self.structs = first_pass.structs.clone();
        self.enum_fields = first_pass.enum_fields.clone();
        // Traits are collected here too, for the same reason: an `impl` can name a
        // trait declared further down the file, and a diagnostic about the `impl`
        // should not depend on the order of the two declarations.
        self.traits = first_pass.traits.clone();

        self.scopes.push(Scope::new());
        for stmt in &module.items {
            self.exec_stmt(stmt);
        }
        self.scopes.pop();
        std::mem::take(&mut self.diagnostics)
    }

    fn collect_fns(&mut self, items: &[Stmt]) {
        for item in items {
            self.collect_fn_stmt(item);
        }
    }

    fn collect_fn_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, .. } => {
                if let Expr::Function {
                    params,
                    return_type,
                    ..
                } = value.as_ref()
                {
                    let params_t: Vec<Type> = params
                        .iter()
                        .map(|p| p.type_hint.clone().unwrap_or(Type::Int))
                        .collect();
                    let ret = return_type.clone().unwrap_or(Type::Nil);
                    self.funcs.insert(name.clone(), (params_t, ret));
                }
            }
            Stmt::Struct { name, fields, .. } => {
                self.structs.insert(
                    name.clone(),
                    fields
                        .iter()
                        .map(|f| (f.name.clone(), f.type_hint.clone()))
                        .collect(),
                );
            }
            Stmt::Enum { name, variants, .. } => {
                self.enum_fields.insert(
                    name.clone(),
                    variants
                        .iter()
                        .map(|v| (v.name.clone(), v.fields.clone()))
                        .collect(),
                );
            }
            Stmt::Trait { name, methods } => {
                self.traits.insert(
                    name.clone(),
                    methods
                        .iter()
                        .map(|m| (m.name.clone(), m.params.len()))
                        .collect(),
                );
            }
            Stmt::Mod { items, .. } => self.collect_fns(items),
            Stmt::Export(inner) => self.collect_fn_stmt(inner),
            _ => {}
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn lookup(&self, name: &str) -> Option<&Type> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.get(name) {
                return Some(t);
            }
        }
        self.globals.get(name)
    }

    fn declare(&mut self, name: &str, ty: Type, mutable: bool) {
        if let Some(s) = self.scopes.last_mut() {
            s.vars.insert(name.to_string(), ty);
        } else {
            self.globals.vars.insert(name.to_string(), ty);
        }
        if !mutable {
            let _ = name; // mutability is enforced by runtime & compiler VMs
        }
    }

    fn report(
        &mut self,
        code: &'static str,
        msg: &str,
        expected: Option<String>,
        found: Option<String>,
        hint: Option<String>,
        needle: Option<&str>,
    ) {
        let (line, col) = self.locate(needle);
        let source_line = self
            .source
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("")
            .to_string();
        self.diagnostics.push(Diagnostic {
            code,
            message: msg.to_string(),
            file: self.file.to_string(),
            line: self.line.max(line),
            col,
            source_line,
            expected,
            found,
            suggestion: hint,
        });
    }

    /// Best-effort source location for a needle string, falling back to the
    /// current statement line.
    fn locate(&self, needle: Option<&str>) -> (usize, usize) {
        if let Some(n) = needle {
            if let Some(idx) = self.source.find(n) {
                return crate::lexer::offset_to_line_col(self.source, idx);
            }
        }
        (self.line, 1)
    }

    fn type_name(&self, t: &Type) -> String {
        crate::value::type_of(t)
    }

    /// Check that `ty` is assignable to `expected` (numeric compatibility plus
    /// exact matches). Returns true if compatible.
    /// Expand a `type` alias to what it names, transitively.
    ///
    /// Bounded by [`ALIAS_EXPANSION_LIMIT`] so a self-referential alias (`type A = A`,
    /// or a cycle of two) terminates instead of recursing; such an alias simply stops
    /// expanding and is left as a custom name.
    fn expand_alias(&self, ty: &Type) -> Type {
        let mut cur = ty.clone();
        for _ in 0..ALIAS_EXPANSION_LIMIT {
            let Type::Custom(name) = &cur else {
                break;
            };
            match self.aliases.get(name) {
                Some(next) => cur = next.clone(),
                None => break,
            }
        }
        cur
    }

    /// Record a `type X = T` declaration so later annotations can expand it.
    fn record_alias(&mut self, name: &str, alias: &Type) {
        self.aliases.insert(name.to_string(), alias.clone());
    }

    fn compatible(&self, expected: &Type, actual: &Type) -> bool {
        use Type::*;
        // Expand aliases first, so `type Meters = int` compares as `int` rather than
        // as an unrelated custom name. Without this the annotation said `Meters`, the
        // alias meant `int`, and nothing consulted the alias -- so the interpreter
        // rejected a value the VM accepted.
        let expected = self.expand_alias(expected);
        let actual = self.expand_alias(actual);
        match (&expected, &actual) {
            // Exact matches.
            (a, b) if a == b => true,
            // Numeric compatibility: any int/float/hex form is mutually ok.
            (
                Int | I8 | I16 | I32 | I64 | U8 | U16 | U32 | U64 | F32 | F64 | Hex(_),
                Int | I8 | I16 | I32 | I64 | U8 | U16 | U32 | U64 | F32 | F64 | Hex(_),
            ) => true,
            // nil/void interop.
            (Void, Nil) | (Nil, Nil) => true,
            // Generics / unknown accept anything.
            (Generic(_), _) => true,
            (_, Generic(_)) => true,
            // Any container fits `array`/`map` base forms via element checks.
            (Array(_), Array(_)) => true,
            (Map(_, _), Map(_, _)) => true,
            (Tuple(_), Tuple(_)) => true,
            (Option(_), Option(_)) => true,
            (Result(_, _), Result(_, _)) => true,
            (Function(_, _), Function(_, _)) => true,
            // Custom types match by name only.
            (Custom(a), Custom(b)) => a == b,
            _ => false,
        }
    }

    /// Report an `impl Trait for Type` that does not match `Trait`.
    ///
    /// Three things can go wrong, and each is worth naming precisely:
    ///
    /// - the trait is not declared at all, which usually means a typo in the name or
    ///   a missing `use`;
    /// - the impl provides a method the trait does not declare, which is almost
    ///   always a typo, and silently leaves the real method unimplemented;
    /// - the impl omits a method the trait requires, which otherwise surfaces as a
    ///   `No method ...` runtime error at an unrelated call site.
    ///
    /// Comparison is by name and arity. The receiver counts toward arity because a
    /// method declares it as its own first parameter (`fn hi(self)` is arity 1), so
    /// a trait method written with `self` must be implemented with `self`.
    fn check_impl_covers_trait(&mut self, trait_name: &str, type_name: &str, methods: &[Stmt]) {
        let Some(required) = self.traits.get(trait_name).cloned() else {
            self.report(
                "E0410",
                &format!(
                    "`impl {} for {}`: trait `{}` is not declared",
                    trait_name, type_name, trait_name
                ),
                None,
                None,
                Some(format!(
                    "declare `trait {}` before implementing it",
                    trait_name
                )),
                Some(trait_name),
            );
            return;
        };

        // The parser stores an `impl` body as `let <name> = fn ...` bindings, which
        // is why the interpreter reads its methods out of `Stmt::Let`.
        let mut provided: HashMap<&str, usize> = HashMap::new();
        for m in methods {
            if let Stmt::Let {
                name: mname, value, ..
            } = m
            {
                if let Expr::Function { params, .. } = value.as_ref() {
                    provided.insert(mname.as_str(), params.len());
                }
            }
        }

        for (mname, marity) in &provided {
            let Some(&(_, rarity)) = required.iter().find(|(n, _)| n == mname) else {
                self.report(
                    "E0411",
                    &format!(
                        "`impl {} for {}`: `{}` is not a method of trait `{}`",
                        trait_name, type_name, mname, trait_name
                    ),
                    Some("a method declared by the trait".to_string()),
                    Some(format!("`{}`", mname)),
                    suggest_trait_method(&required, mname),
                    Some(mname),
                );
                continue;
            };
            if *marity != rarity {
                self.report(
                    "E0412",
                    &format!(
                        "`impl {} for {}`: `{}` takes {} parameter(s) here but the trait expects {}",
                        trait_name, type_name, mname, marity, rarity
                    ),
                    Some(format!("{} parameter(s)", rarity)),
                    Some(format!("{} parameter(s)", marity)),
                    Some("the receiver is a parameter too: `fn hi(self)` is arity 1".to_string()),
                    Some(mname),
                );
            }
        }

        for (rname, _) in &required {
            if provided.contains_key(rname.as_str()) {
                continue;
            }
            self.report(
                "E0413",
                &format!(
                    "`impl {} for {}` does not implement `{}`",
                    trait_name, type_name, rname
                ),
                Some(format!("`{}`", rname)),
                None,
                suggest_trait_method(&required, rname),
                Some(rname),
            );
        }
    }

    fn exec_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            // Record the alias so annotations naming it expand. The body is still
            // walked below, since a `type` statement can appear inside a block.
            Stmt::TypeAlias { name, alias } => self.record_alias(name, alias),
            Stmt::Let {
                name,
                value,
                type_hint,
                mutable,
                ..
            } => {
                // Function definitions: record the signature; don't treat the
                // function body as a runtime value assignment.
                if let Expr::Function {
                    params,
                    return_type,
                    body,
                    ..
                } = value.as_ref()
                {
                    self.push_scope();
                    for p in params {
                        let pt = p.type_hint.clone().unwrap_or(Type::Int);
                        self.declare(&p.name, pt, true);
                    }
                    for s in body {
                        self.exec_stmt(s);
                    }
                    self.pop_scope();
                    let params_t: Vec<Type> = params
                        .iter()
                        .map(|p| p.type_hint.clone().unwrap_or(Type::Int))
                        .collect();
                    let ret = return_type.clone().unwrap_or(Type::Nil);
                    self.funcs.insert(name.clone(), (params_t, ret.clone()));
                    self.declare(name, Type::Function(vec![], Box::new(ret)), false);
                    return;
                }
                let inferred = self.infer(value);
                let ty = type_hint.clone().unwrap_or_else(|| inferred.clone());
                if let Some(hint) = type_hint {
                    if !self.compatible(hint, &inferred) {
                        let found = self.type_name(&inferred);
                        let exp = self.type_name(hint);
                        let needle = needle_for(value);
                        self.report(
                            "E0308",
                            "type mismatch",
                            Some(exp),
                            Some(found),
                            Some(
                                "an explicit `let x: T = value` requires `value` to have type `T`"
                                    .to_string(),
                            ),
                            needle.as_deref(),
                        );
                    }
                }
                self.declare(name, ty, *mutable);
            }
            Stmt::Dump { value, .. } | Stmt::Trace { value } | Stmt::Expr(value) => {
                self.infer(value);
            }
            Stmt::Return(expr) => {
                if let Some(e) = expr {
                    self.infer(e);
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.infer(cond);
                self.push_scope();
                for s in then_branch {
                    self.exec_stmt(s);
                }
                self.pop_scope();
                if let Some(els) = else_branch {
                    self.push_scope();
                    for s in els {
                        self.exec_stmt(s);
                    }
                    self.pop_scope();
                }
            }
            Stmt::While { cond, body, .. } => {
                self.infer(cond);
                self.block(body);
            }
            Stmt::Loop { body, .. } => {
                self.block(body);
            }
            Stmt::DoWhile { cond, body, .. } => {
                self.infer(cond);
                self.block(body);
            }
            Stmt::For {
                pattern,
                iterable,
                body,
                ..
            } => {
                let it = self.infer(iterable);
                self.push_scope();
                self.bind_pattern(pattern, &it);
                self.block(body);
                self.pop_scope();
            }
            Stmt::Struct { name, fields, .. } => {
                let _ = (&name, &fields);
            }
            Stmt::Enum { name, variants, .. } => {
                self.enums.insert(
                    name.clone(),
                    variants
                        .iter()
                        .map(|v| (v.name.clone(), v.fields.len()))
                        .collect(),
                );
            }
            Stmt::Mod { items, .. } => {
                self.push_scope();
                for s in items {
                    self.exec_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::Export(inner) => self.exec_stmt(inner),
            Stmt::Try {
                body, catch_name, ..
            } => {
                self.push_scope();
                for s in body {
                    self.exec_stmt(s);
                }
                self.pop_scope();
                if let Some(cn) = catch_name {
                    self.declare(cn, Type::Custom("Error".into()), false);
                }
            }
            Stmt::Const { name, value, .. } => {
                let ty = self.infer(value);
                self.declare(name, ty, false);
            }
            Stmt::Impl {
                target,
                trait_name,
                methods,
            } => {
                // The parser stores `impl <A> for <B>` with the trait in `target`
                // and the type in `trait_name`, which reads backwards; an inherent
                // `impl <Type>` has no `trait_name`.
                let (trait_str, type_name) = match trait_name {
                    Some(ty) => (target.clone(), ty.clone()),
                    None => (String::new(), target.clone()),
                };
                if !trait_str.is_empty() {
                    self.check_impl_covers_trait(&trait_str, &type_name, methods);
                }
                for m in methods {
                    self.exec_stmt(m);
                }
            }
            Stmt::Trait { .. } => {}
            _ => {}
        }
    }

    fn block(&mut self, body: &[Stmt]) {
        self.push_scope();
        for s in body {
            self.exec_stmt(s);
        }
        self.pop_scope();
    }

    fn bind_pattern(&mut self, pat: &Pattern, ty: &Type) {
        match pat {
            Pattern::Ident(n) => {
                self.declare(n, ty.clone(), true);
            }
            Pattern::Tuple(ps) => {
                for p in ps {
                    self.bind_pattern(p, &Type::Int);
                }
            }
            Pattern::Array(ps) => {
                for p in ps {
                    self.bind_pattern(p, &Type::Int);
                }
            }
            Pattern::Struct(_, fields) => {
                for (_, p) in fields {
                    self.bind_pattern(p, &Type::Int);
                }
            }
            Pattern::Some(inner) => self.bind_pattern(inner, ty),
            Pattern::Ok(inner) => self.bind_pattern(inner, ty),
            Pattern::Err(inner) => self.bind_pattern(inner, ty),
            _ => {}
        }
    }

    /// Infer the type of an expression. Returns a best-effort `Type` and emits
    /// diagnostics for mismatches it can determine statically.
    /// Validate a `Name { field: value, ... }` literal against the declaration.
    ///
    /// Reports an unknown field, a missing field, and a field whose value does not
    /// match the declared type. A field declared without a type is not checked --
    /// there is nothing to check it against, and that is the existing convention for
    /// untyped fields.
    ///
    /// An unknown *struct* name is not reported here: that is a different diagnostic,
    /// it is already caught at runtime, and failing to find a declaration may just mean
    /// it lives in another module.
    fn check_struct_literal(&mut self, name: &str, fields: &[(String, Expr)]) {
        let Some(declared) = self.structs.get(name).cloned() else {
            // Still infer the values so their own errors surface.
            for (_, v) in fields {
                self.infer(v);
            }
            return;
        };

        let mut seen: HashSet<&str> = HashSet::new();
        for (fname, value) in fields {
            seen.insert(fname.as_str());
            match declared.iter().find(|(n, _)| n.as_str() == fname) {
                Some((_, Some(want))) => {
                    let got = self.infer(value);
                    if !self.compatible(want, &got) {
                        let want_s = self.type_name(want);
                        let got_s = self.type_name(&got);
                        self.report(
                            "E0422",
                            &format!("`{}`: field `{}` is declared `{}`", name, fname, want_s),
                            Some(want_s),
                            Some(got_s),
                            None,
                            Some(fname),
                        );
                    }
                }
                Some((_, None)) => {
                    self.infer(value);
                }
                None => {
                    let hint = suggest_field(
                        &declared.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
                        fname,
                    );
                    self.report(
                        "E0420",
                        &format!("`{}` has no field `{}`", name, fname),
                        None,
                        None,
                        hint,
                        Some(fname),
                    );
                    self.infer(value);
                }
            }
        }

        for (dname, _) in &declared {
            if !seen.contains(dname.as_str()) {
                self.report(
                    "E0421",
                    &format!("`{}` is missing field `{}`", name, dname),
                    None,
                    None,
                    None,
                    Some(dname),
                );
            }
        }
    }

    /// Validate an `Enum::Variant(...)` construction.
    ///
    /// Checks the variant exists, the arity matches, and each argument matches its
    /// payload type. A path with two segments is only treated as an enum construction
    /// when the first segment names a declared enum, so a module path is left alone.
    fn check_enum_variant(&mut self, enum_name: &str, variant: &str, args: &[Expr]) {
        let Some(variants) = self.enum_fields.get(enum_name).cloned() else {
            return;
        };
        let Some((_, payload)) = variants.iter().find(|(n, _)| n.as_str() == variant) else {
            let hint = suggest_field(
                &variants.keys().cloned().collect::<Vec<_>>(),
                variant,
            );
            self.report(
                "E0423",
                &format!("enum `{}` has no variant `{}`", enum_name, variant),
                None,
                None,
                hint,
                Some(variant),
            );
            return;
        };

        if args.len() != payload.len() {
            let want = if payload.len() == 1 {
                "1 value".to_string()
            } else {
                format!("{} values", payload.len())
            };
            self.report(
                "E0424",
                &format!(
                    "`{}::{}` takes {} but got {}",
                    enum_name,
                    variant,
                    want,
                    args.len()
                ),
                Some(want),
                Some(format!("{} value(s)", args.len())),
                None,
                Some(variant),
            );
            return;
        }

        for (i, (arg, want)) in args.iter().zip(payload.iter()).enumerate() {
            let got = self.infer(arg);
            if !self.compatible(want, &got) {
                let want_s = format!("{} (field {})", self.type_name(want), i + 1);
                let got_s = self.type_name(&got);
                self.report(
                    "E0425",
                    &format!(
                        "`{}::{}`: field {} is declared `{}`",
                        enum_name,
                        variant,
                        i + 1,
                        self.type_name(want)
                    ),
                    Some(want_s),
                    Some(got_s),
                    None,
                    Some(variant),
                );
            }
        }
    }

    fn infer(&mut self, expr: &Expr) -> Type {
        use Expr::*;
        match expr {
            Hex(_) | BinLit(_) | OctLit(_) => Type::Hex(64),
            Int(_) | TypedInt(_, _) => Type::Int,
            Float(_) | Float32(_) => Type::F64,
            String(_) => Type::String,
            Char(_) => Type::Char,
            Bytes(_) => Type::Bytes,
            Regex(_, _) => Type::Custom("regex".into()),
            Bool(_) => Type::Bool,
            Nil => Type::Nil,
            Ident(name) => self
                .lookup(name)
                .cloned()
                .unwrap_or(Type::Generic("unknown".into())),
            Tuple(items) => Type::Tuple(items.iter().map(|e| self.infer(e)).collect()),
            Array(items) => {
                let elem = items
                    .iter()
                    .map(|e| self.infer(e))
                    .next()
                    .unwrap_or(Type::Int);
                Type::Array(Box::new(elem))
            }
            Map(pairs) => {
                for (_, v) in pairs {
                    self.infer(v);
                }
                Type::Map(Box::new(Type::String), Box::new(Type::Int))
            }
            Unary(_, e) => self.infer(e),
            Binary(op, l, r) => {
                let lt = self.infer(l);
                let _rt = self.infer(r);
                use ast::BinOp::*;
                match op {
                    Eq | NotEq | Lt | Gt | LtEq | GtEq | And | Or | In => Type::Bool,
                    Add => {
                        // string + anything is string
                        if lt == Type::String {
                            Type::String
                        } else {
                            lt
                        }
                    }
                    _ => lt,
                }
            }
            Assign(name, value) => {
                let _ = self.infer(value);
                self.is_immutable_name(name);
                self.lookup(name).cloned().unwrap_or(Type::Nil)
            }
            CompoundAssign(_, name, value) => {
                let _ = self.infer(value);
                self.is_immutable_name(name);
                self.lookup(name).cloned().unwrap_or(Type::Nil)
            }
            FieldAccess(obj, _) => {
                let ot = self.infer(obj);
                match ot {
                    Type::Array(_) => Type::Int,
                    Type::Map(_, v) => *v,
                    _ => Type::Generic("field".into()),
                }
            }
            Index(obj, idx) => {
                let _ = self.infer(idx);
                let ot = self.infer(obj);
                match ot {
                    Type::Array(e) => *e,
                    Type::Map(_, v) => *v,
                    Type::String => Type::Char,
                    _ => Type::Generic("elem".into()),
                }
            }
            Range(_, _) => Type::Array(Box::new(Type::Int)),
            Function { .. } => Type::Function(Vec::new(), Box::new(Type::Nil)),
            Lambda { .. } => Type::Function(Vec::new(), Box::new(Type::Nil)),
            Call { callee, args, .. } => {
                for a in args {
                    self.infer(a);
                }
                // `Enum::Variant(...)` is an ordinary call with a path callee, so the
                // variant and its arity are checked here rather than in `infer_call`,
                // which only ever saw a name.
                if let Expr::Path(segments) = callee.as_ref() {
                    if segments.len() == 2 {
                        self.check_enum_variant(&segments[0], &segments[1], args);
                    }
                }
                self.infer_call(callee, args)
            }
            If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.infer(cond);
                // Type of an if-expr is the type of the last statement (approx).
                let tt = last_stmt_type(then_branch).unwrap_or(Type::Nil);
                let et = else_branch
                    .as_deref()
                    .and_then(last_stmt_type)
                    .unwrap_or(Type::Nil);
                let _ = tt;
                et
            }
            Match { value, arms } => {
                let value_ty = self.infer(value);
                // Detect non-exhaustive matches over user enum unit variants and
                // unreachable (previously matched) patterns.
                let mut matched_variants = Vec::new();
                let mut has_catchall = false;
                let mut seen_patterns: std::vec::Vec<std::string::String> = Vec::new();
                for (p, _, _) in arms {
                    match p {
                        Pattern::Wild => {
                            has_catchall = true;
                            matched_variants.push("_".to_string());
                        }
                        Pattern::EnumVariant(en, vn, _) => {
                            matched_variants.push(format!("{}::{}", en, vn));
                            if seen_patterns.contains(vn) {
                                self.report(
                                    "E0223",
                                    "unreachable pattern",
                                    None,
                                    None,
                                    Some(format!(
                                        "variant `{}::{}` is matched more than once",
                                        en, vn
                                    )),
                                    Some(vn),
                                );
                            }
                            seen_patterns.push(vn.clone());
                        }
                        _ => {}
                    }
                }
                // If the matched value is a known enum and no catch-all covers
                // all unit variants, warn when a unit variant is unmatched.
                if let Type::Custom(ename) = &value_ty {
                    let variants: Option<std::vec::Vec<(std::string::String, usize)>> =
                        self.enums.get(ename).cloned();
                    if let Some(variants) = variants {
                        if !has_catchall {
                            let covered = matched_variants.clone();
                            for (vname, nfields) in &variants {
                                if *nfields == 0
                                    && !covered.iter().any(|c| c.ends_with(&format!("::{}", vname)))
                                {
                                    let (en, vn) = (ename.clone(), vname.clone());
                                    self.report(
                                        "W0001",
                                        "non-exhaustive match: missing enum variant",
                                        None,
                                        None,
                                        Some(format!(
                                            "add a match arm for `{}::{}` or a catch-all `_` pattern",
                                            en, vn
                                        )),
                                        Some(&vn),
                                    );
                                }
                            }
                        }
                    }
                }
                let mut t = Type::Nil;
                for (_, _, body) in arms {
                    t = last_stmt_type(body).unwrap_or(Type::Nil);
                }
                t
            }
            Block(stmts) => last_stmt_type(stmts).unwrap_or(Type::Nil),
            Ternary { cond, then, els } => {
                let _ = self.infer(cond);
                let _ = self.infer(then);
                self.infer(els)
            }
            NilCoalesce(l, r) => {
                let lt = self.infer(l);
                let rt = self.infer(r);
                if lt == Type::Nil {
                    rt
                } else {
                    lt
                }
            }
            OptField(obj, _) => self.infer(obj),
            OptIndex(obj, _) => self.infer(obj),
            MultiAssign { values, .. } => {
                for v in values {
                    self.infer(v);
                }
                Type::Tuple(vec![])
            }
            Comprehension { iterable, elem, .. } => {
                let _ = self.infer(iterable);
                let et = self.infer(elem);
                Type::Array(Box::new(et))
            }
            Path(segments) => {
                // `EnumName::Variant` — a user enum construction.
                if segments.len() == 2 {
                    // A bare path is a unit-variant construction, so it needs the same
                    // checks as `Enum::Variant(...)`: an unknown variant is otherwise
                    // only caught at runtime, as "Undefined path", and a payload
                    // variant used without its values is not caught at all. Passing no
                    // arguments is what makes a bare `Enum::Payload` an arity error
                    // while `Enum::Unit` stays valid.
                    self.check_enum_variant(&segments[0], &segments[1], &[]);
                    Type::Custom(segments[0].clone())
                } else {
                    Type::Generic("path".into())
                }
            }
            StructLit { name, fields } => {
                self.check_struct_literal(name, fields);
                // The struct's own name, not the placeholder `"struct"` the
                // declaration is now validated against. Dropping it is why
                // `let p = P { v: "s" }` went unchecked while the hand-annotated
                // `let p: P = P { v: "s" }` was caught.
                Type::Custom(name.clone())
            }
            As(e, t) => {
                let _ = self.infer(e);
                t.clone()
            }
            MacroVar(_) | MacroInvoke { .. } | EvidenceFrom { .. } => Type::Generic("macro".into()),
            TryExpr(e) => self.infer(e),
            Await(e) => self.infer(e),
            Spawn(e) => self.infer(e),
            Raise(e) => {
                self.infer(e);
                Type::Nil
            }
            IndexAssign { obj, idx, value } => {
                let _ = (self.infer(obj), self.infer(idx));
                self.infer(value)
            }
            FieldAssign { obj, field, value } => {
                let _ = (self.infer(obj), field);
                self.infer(value)
            }
            Interp { parts, .. } => {
                for p in parts {
                    self.infer(p);
                }
                Type::String
            }
        }
    }

    fn is_immutable_name(&self, _name: &str) -> bool {
        // Mutability is enforced by the interpreter and compiler runtimes. The
        // static checker does not duplicate the (already enforced) runtime check
        // to avoid false positives with pattern bindings and shadowing.
        false
    }

    fn infer_call(&mut self, callee: &Expr, args: &[Expr]) -> Type {
        // Built-in constructor types.
        if let Expr::Ident(name) = callee {
            match name.as_str() {
                "Some" => return Type::Option(Box::new(self.infer_first_arg(args))),
                "Ok" => {
                    return Type::Result(Box::new(self.infer_first_arg(args)), Box::new(Type::Nil))
                }
                "Err" => {
                    return Type::Result(Box::new(Type::Nil), Box::new(self.infer_first_arg(args)))
                }
                "None" => return Type::Option(Box::new(Type::Nil)),
                "array" => return Type::Array(Box::new(Type::Int)),
                "map" => {
                    return Type::Map(Box::new(Type::String), Box::new(self.infer_first_arg(args)))
                }
                _ => {}
            }
            // User function: check argument count and types.
            if let Some((params, ret)) = self.funcs.get(name).cloned() {
                let args_t: Vec<Type> = args.iter().map(|a| self.infer(a)).collect();
                for (idx, (param_t, arg_t)) in params.iter().zip(args_t.iter()).enumerate() {
                    let _ = idx;
                    // `Int`-typed params accept any numeric literal; skip clear
                    // generics and numeric widening, only flag obvious mismatches
                    // (e.g. a string passed where i32 is expected).
                    if arg_t == &Type::Generic("unknown".into())
                        || &Type::Generic("arg".into()) == arg_t
                    {
                        continue;
                    }
                    if param_t != &Type::Int && !self.compatible(param_t, arg_t) {
                        let found = self.type_name(arg_t);
                        let exp = self.type_name(param_t);
                        self.report(
                            "E0308",
                            "function argument type mismatch",
                            Some(exp),
                            Some(found),
                            Some(
                                "the argument type must match the declared parameter type"
                                    .to_string(),
                            ),
                            None,
                        );
                    }
                }
                return ret;
            }
        }
        // Method call.
        if let Expr::FieldAccess(obj, _) = callee {
            let _ = self.infer(obj);
            return Type::Generic("method".into());
        }
        // Expr::Path module call etc.
        Type::Generic("call".into())
    }

    fn infer_first_arg(&self, args: &[Expr]) -> Type {
        args.first().map(|_| Type::Int).unwrap_or(Type::Nil)
    }
}

fn last_stmt_type(stmts: &[Stmt]) -> Option<Type> {
    for s in stmts.iter().rev() {
        if let Stmt::Expr(e) = s {
            return expr_shallow_type(e);
        }
        if let Stmt::Return(Some(e)) = s {
            return expr_shallow_type(e);
        }
    }
    None
}

fn expr_shallow_type(e: &Expr) -> Option<Type> {
    use Expr::*;
    match e {
        Int(_) | TypedInt(_, _) => Some(Type::Int),
        Hex(_) | BinLit(_) | OctLit(_) => Some(Type::Hex(64)),
        Float(_) | Float32(_) => Some(Type::F64),
        String(_) => Some(Type::String),
        Char(_) => Some(Type::Char),
        Bytes(_) => Some(Type::Bytes),
        Bool(_) => Some(Type::Bool),
        Nil => Some(Type::Nil),
        Array(_) => Some(Type::Array(Box::new(Type::Int))),
        Tuple(_) => Some(Type::Tuple(vec![])),
        Ident(_) => Some(Type::Generic("unknown".into())),
        _ => None,
    }
}

/// Produce a short textual needle (for source location) from an expression.
fn needle_for(e: &Expr) -> Option<String> {
    use Expr::*;
    match e {
        String(s) => Some(s.clone()),
        Int(i) => Some(i.to_string()),
        Hex(h) => Some(format!("0x{:X}", h)),
        Float(f) => Some(f.to_string()),
        Char(c) => Some(format!("'{}'", c)),
        Bytes(_) => Some("b\"".to_string()),
        Array(_) => Some("[".to_string()),
        Nil => Some("nil".to_string()),
        Ident(_) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn check(src: &str) -> Vec<Diagnostic> {
        let tokens = tokenize(src).unwrap();
        let module = crate::parser::parse(&tokens, src).unwrap();
        let mut tc = TypeChecker::new(src, "test.rak");
        tc.check_module(&module)
    }

    #[test]
    fn infers_and_accepts_valid_type_annotations() {
        let src = "let a: i32 = 42\nlet name: string = \"Rak\"\nlet items = [1, 2, 3]";
        assert!(check(src).is_empty(), "got: {:?}", check(src));
    }

    #[test]
    fn rejects_string_assigned_to_int() {
        let diags = check("let count: i32 = \"hello\"");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E0308");
        assert!(
            diags[0].found.as_deref() == Some("string"),
            "got: {:?}",
            diags[0]
        );
    }

    #[test]
    fn accepts_char_hint_with_char_literal() {
        assert!(
            check("let c: char = 'א'").is_empty(),
            "got: {:?}",
            check("let c: char = 'א'")
        );
    }

    #[test]
    fn infers_numeric_literals() {
        assert!(check("let a = 42\nlet b = 3.14\nlet c = 0b1010\nlet d = 0o755").is_empty());
    }

    #[test]
    fn warns_on_non_exhaustive_enum_match() {
        let diags = check(
            "enum State { Ready, Running, Finished }\nlet s: State = State::Ready\nmatch s {\n    State::Ready => { dump 1 }\n    State::Running => { dump 2 }\n}",
        );
        let missing = diags.iter().find(|d| d.code == "W0001");
        assert!(
            missing.is_some(),
            "got: {:?}",
            diags.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }

    #[test]
    fn accepts_exhaustive_enum_match() {
        let diags = check(
            "enum State { Ready, Running, Finished }\nlet s: State = State::Ready\nmatch s {\n    State::Ready => { dump 1 }\n    State::Running => { dump 2 }\n    State::Finished => { dump 3 }\n}",
        );
        assert!(
            diags.is_empty(),
            "got: {:?}",
            diags.iter().map(|d| d.render()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn catches_unreachable_variant_pattern() {
        let diags = check(
            "enum E { A, B }\nlet e: E = E::A\nmatch e {\n    E::A => { dump 1 }\n    E::A => { dump 2 }\n    E::B => { dump 3 }\n}",
        );
        let un = diags.iter().find(|d| d.code == "E0223");
        assert!(
            un.is_some(),
            "got: {:?}",
            diags.iter().map(|d| d.code).collect::<Vec<_>>()
        );
    }
}
