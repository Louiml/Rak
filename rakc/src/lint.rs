//! `rakc lint` — advisory static checks for Rak (Part 7B).
//!
//! Rules (each reported as `warning[<rule>]: ...`):
//! - `unused-var` — a `let` binding that is never read afterwards.
//! - `shadowed` — the same name bound twice in the same block.
//! - `unreachable` — statements after a `return` in the same block.
//! - `missing-ret-type` — an exported (`pub`) `fn` without a return type.
//! - `duplicate-import` — the same module imported more than once.
//!
//! Advisory by default (exit 0). `--deny` exits 1 when any warning fires, for
//! CI use. Names starting with `_` silence `unused-var`.

use crate::ast::*;
use std::collections::HashSet;

pub struct LintFinding {
    pub rule: &'static str,
    pub message: String,
}

pub fn lint_source(source: &str) -> Result<Vec<LintFinding>, String> {
    let tokens = crate::lexer::tokenize(source).map_err(|e| e.to_string())?;
    let module = crate::parser::parse(&tokens, source).map_err(|e| e.to_string())?;
    let mut l = Linter::new();
    l.module(&module);
    l.finish();
    Ok(l.findings)
}

struct Linter {
    findings: Vec<LintFinding>,
    /// `(name, mutable)` bindings from plain `let`/`const` statements.
    declared: Vec<(String, bool)>,
    /// All names ever bound (patterns included) — never "unused".
    defined: HashSet<String>,
    /// Names read anywhere in the program.
    reads: HashSet<String>,
    /// Names that appear on the left of `=`/`+=` (assigned, not just declared).
    assigned: HashSet<String>,
    saw_return: bool,
    imported: HashSet<String>,
}

impl Linter {
    fn new() -> Self {
        Linter {
            findings: Vec::new(),
            declared: Vec::new(),
            defined: HashSet::new(),
            reads: HashSet::new(),
            assigned: HashSet::new(),
            saw_return: false,
            imported: HashSet::new(),
        }
    }

    fn module(&mut self, m: &Module) {
        for imp in &m.imports {
            self.import(imp);
        }
        for s in &m.items {
            self.stmt(s);
        }
    }

    fn import(&mut self, imp: &Import) {
        let target = if imp.is_file {
            imp.path.first().cloned().unwrap_or_default()
        } else {
            imp.path.join(".")
        };
        if !self.imported.insert(target.clone()) {
            self.findings.push(LintFinding {
                rule: "duplicate-import",
                message: format!("module '{}' is imported more than once", target),
            });
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { name, value, pattern, .. } => {
                match pattern {
                    Some(p) => self.binds_from_pattern(p),
                    None => {
                        self.declared.push((name.clone(), matches!(s, Stmt::Let { mutable: true, .. })));
                        self.defined.insert(name.clone());
                    }
                }
                self.expr(value);
            }
            Stmt::Const { name, value } => {
                self.declared.push((name.clone(), false));
                self.defined.insert(name.clone());
                self.expr(value);
            }
            Stmt::Expr(e) => {
                if let Expr::Assign(name, _) = e.as_ref() {
                    self.assigned.insert(name.clone());
                }
                if let Expr::IndexAssign { obj, .. } = e.as_ref() {
                    if let Expr::Ident(n) = obj.as_ref() {
                        self.assigned.insert(n.clone());
                    }
                }
                if let Expr::FieldAssign { obj, .. } = e.as_ref() {
                    if let Expr::Ident(n) = obj.as_ref() {
                        self.assigned.insert(n.clone());
                    }
                }
                if let Expr::MultiAssign { targets, .. } = e.as_ref() {
                    for t in targets {
                        if let Expr::Ident(n) = t {
                            self.assigned.insert(n.clone());
                        }
                    }
                }
                self.expr(e)
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
                self.saw_return = true;
            }
            Stmt::Dump { value, .. } => self.expr(value),
            Stmt::Trace { value } => self.expr(value),
            Stmt::Assert(e) => self.expr(e),
            Stmt::Defer(e) => self.expr(e),
            Stmt::If { cond, then_branch, else_branch } => {
                self.expr(cond);
                self.block(then_branch);
                if let Some(els) = else_branch {
                    self.block(els);
                }
            }
            Stmt::IfLet { pattern, value, then_branch, else_branch } => {
                self.binds_from_pattern(pattern);
                self.expr(value);
                self.block(then_branch);
                if let Some(els) = else_branch {
                    self.block(els);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::WhileLet { pattern, value, body } => {
                self.binds_from_pattern(pattern);
                self.expr(value);
                self.block(body);
            }
            Stmt::DoWhile { cond, body } => {
                self.block(body);
                self.expr(cond);
            }
            Stmt::Loop { body, .. } => self.block(body),
            Stmt::For { pattern, iterable, body, .. } => {
                self.binds_from_pattern(pattern);
                self.expr(iterable);
                self.block(body);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Scan { target, options, body } => {
                self.expr(target);
                for (_, e) in options {
                    self.expr(e);
                }
                if let Some(b) = body {
                    self.block(b);
                }
            }
            Stmt::Fetch { target, options, body } => {
                self.expr(target);
                for (_, e) in options {
                    self.expr(e);
                }
                if let Some(b) = body {
                    self.block(b);
                }
            }
            Stmt::Match { value, arms } => {
                self.expr(value);
                for (pattern, guard, body) in arms {
                    self.binds_from_pattern(pattern);
                    if let Some(g) = guard {
                        self.expr(g);
                    }
                    self.block(body);
                }
            }
            Stmt::Try { body, catch_name, catch_body } => {
                self.block(body);
                if let Some(n) = catch_name {
                    self.defined.insert(n.clone());
                }
                self.block(catch_body);
            }
            Stmt::Raise(e) => self.expr(e),
            Stmt::Struct { fields, .. } => {
                for f in fields {
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                }
            }
            Stmt::Enum { .. } => {}
            Stmt::Impl { methods, .. } => {
                for m in methods {
                    self.stmt(m);
                }
            }
            Stmt::Trait { .. } => {}
            Stmt::Test { body, .. } => self.block(body),
            Stmt::Mod { items, .. } => self.block(items),
            Stmt::Use { .. } | Stmt::TypeAlias { .. } => {}
            Stmt::Async(body) => self.block(body),
            Stmt::Export(inner) => {
                if let Stmt::Let { name, value, .. } = inner.as_ref() {
                    if let Expr::Function { return_type: None, .. } = value.as_ref() {
                        self.findings.push(LintFinding {
                            rule: "missing-ret-type",
                            message: format!("exported fn '{}' has no return type annotation", name),
                        });
                    }
                }
                self.stmt(inner);
            }
            Stmt::Extern { .. } => {}
            Stmt::MacroDef { body, .. } => self.block(body),
            Stmt::Tunnel { body, .. } => self.block(body),
            Stmt::BinStructDef { .. } => {}
        }
    }

    fn block(&mut self, stmts: &[Stmt]) {
        let mut seen: HashSet<String> = HashSet::new();
        self.saw_return = false;
        for s in stmts {
            if self.saw_return && !matches!(s, Stmt::Return(_) | Stmt::Test { .. }) {
                self.findings.push(LintFinding {
                    rule: "unreachable",
                    message: "statement is unreachable (a `return` precedes it in this block)".to_string(),
                });
                self.saw_return = false;
            }
            if let Stmt::Let { name, pattern: None, .. } = s {
                if !seen.insert(name.clone()) {
                    self.findings.push(LintFinding {
                        rule: "shadowed",
                        message: format!("'{}' is bound again in the same block", name),
                    });
                }
            }
            self.stmt(s);
        }
        self.saw_return = false;
    }

    /// Record pattern-bound names as definitions (never "unused").
    fn binds_from_pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Wild => {}
            Pattern::Ident(n) => {
                self.defined.insert(n.clone());
            }
            Pattern::Tuple(ps) | Pattern::Array(ps) | Pattern::Or(ps) => {
                for x in ps {
                    self.binds_from_pattern(x);
                }
            }
            Pattern::Struct(_, fields) => {
                for (_, sub) in fields {
                    self.binds_from_pattern(sub);
                }
            }
            Pattern::EnumVariant(_, _, subs) => {
                for x in subs {
                    self.binds_from_pattern(x);
                }
            }
            Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => self.binds_from_pattern(i),
            Pattern::Range(lo, hi) => {
                self.binds_from_pattern(lo);
                self.binds_from_pattern(hi);
            }
            _ => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Ident(n) => {
                self.reads.insert(n.clone());
            }
            Expr::Binary(_, l, r) => {
                self.expr(l);
                self.expr(r);
            }
            Expr::Assign(name, value) => {
                self.assigned.insert(name.clone());
                self.expr(value);
            }
            Expr::CompoundAssign(_, name, value) => {
                self.assigned.insert(name.clone());
                self.expr(value);
            }
            Expr::Ternary { cond, then, els } => {
                self.expr(cond);
                self.expr(then);
                self.expr(els);
            }
            Expr::Unary(_, i)
            | Expr::TryExpr(i)
            | Expr::Await(i)
            | Expr::Spawn(i)
            | Expr::Raise(i)
            | Expr::As(i, _) => self.expr(i),
            Expr::NilCoalesce(l, r) => {
                self.expr(l);
                self.expr(r);
            }
            Expr::Index(o, i) | Expr::OptIndex(o, i) => {
                self.expr(o);
                self.expr(i);
            }
            Expr::OptField(o, _) | Expr::FieldAccess(o, _) => self.expr(o),
            Expr::FieldAssign { obj, value, .. } => {
                self.expr(obj);
                self.expr(value);
            }
            Expr::IndexAssign { obj, idx, value } => {
                self.expr(obj);
                self.expr(idx);
                self.expr(value);
            }
            Expr::MultiAssign { targets, values } => {
                for t in targets {
                    if let Expr::Ident(n) = t {
                        self.assigned.insert(n.clone());
                    }
                    self.expr(t);
                }
                for v in values {
                    self.expr(v);
                }
            }
            Expr::Call { callee, args, named } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
                for (_, v) in named {
                    self.expr(v);
                }
            }
            Expr::Array(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::Tuple(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::Map(pairs) => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            Expr::Function { params, body, .. } => {
                for p in params {
                    if let Some(d) = &p.default {
                        self.expr(d);
                    }
                }
                self.block(body);
            }
            Expr::Lambda { params, body, .. } => {
                for p in params {
                    if let Some(d) = &p.default {
                        self.expr(d);
                    }
                }
                self.expr(body);
            }
            Expr::If { cond, then_branch, else_branch } => {
                self.expr(cond);
                self.block(then_branch);
                if let Some(els) = else_branch {
                    self.block(els);
                }
            }
            Expr::Match { value, arms } => {
                self.expr(value);
                for (pattern, guard, body) in arms {
                    self.binds_from_pattern(pattern);
                    if let Some(g) = guard {
                        self.expr(g);
                    }
                    self.block(body);
                }
            }
            Expr::Block(stmts) => self.block(stmts),
            Expr::Comprehension { iterable, cond, elem, value, .. } => {
                self.expr(iterable);
                self.expr(elem);
                if let Some(c) = cond {
                    self.expr(c);
                }
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            Expr::MacroInvoke { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            Expr::EvidenceFrom { value } => self.expr(value),
            _ => {}
        }
    }

    /// Report `let` bindings that were never read.
    fn finish(&mut self) {
        let declared = std::mem::take(&mut self.declared);
        for (name, _mutable) in declared {
            if name.starts_with('_') {
                continue; // convention: `_name` silences the lint
            }
            if !self.reads.contains(&name) {
                self.findings.push(LintFinding {
                    rule: "unused-var",
                    message: format!("variable '{}' is never read", name),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_flags_unused_variable() {
        let findings = lint_source("let x = 10\ndump 1").unwrap();
        assert!(findings.iter().any(|f| f.rule == "unused-var" && f.message.contains("'x'")));
    }

    #[test]
    fn lint_accepts_read_variable() {
        let findings = lint_source("let x = 10\ndump x").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "unused-var"));
    }

    #[test]
    fn lint_underscore_silences() {
        let findings = lint_source("let _ignored = 10\ndump 1").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "unused-var"));
    }

    #[test]
    fn lint_flags_unreachable_after_return() {
        let findings = lint_source("fn f() {\n    return 1\n    dump 2\n}\ndump f()").unwrap();
        assert!(findings.iter().any(|f| f.rule == "unreachable"));
    }

    #[test]
    fn lint_flags_shadowed_binding() {
        let findings = lint_source("fn f() {\n    let a = 1\n    let a = 2\n    dump a\n}\ndump f()").unwrap();
        assert!(findings.iter().any(|f| f.rule == "shadowed" && f.message.contains("'a'")));
    }

    #[test]
    fn lint_flags_missing_pub_ret_type() {
        let findings = lint_source("pub fn go() {\n    return 1\n}\ndump go()").unwrap();
        assert!(findings.iter().any(|f| f.rule == "missing-ret-type"));
    }
}
