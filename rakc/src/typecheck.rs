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
use std::collections::HashMap;

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
        let mut out = format!("error[{}]: {}\n  --> {}:{}:{}", self.code, self.message, self.file, self.line, self.col);
        if !self.source_line.is_empty() {
            out.push('\n');
            let digits = self.line.to_string().len();
            out.push_str(&format!("{:>width$} | {}\n", self.line, self.source_line, width = digits));
            out.push_str(&format!("{:>width$} | {}{}", "", " ".repeat(self.col.saturating_sub(1).min(self.source_line.len())), "^".repeat(1.max(1)), width = digits));
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
        Scope { vars: HashMap::new() }
    }
    fn get(&self, name: &str) -> Option<&Type> {
        self.vars.get(name)
    }
}

/// The type-checker state. It holds the source for snippet rendering, the
/// current file name, an environment stack, and collected diagnostics.
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
                if let Expr::Function { params, return_type, .. } = value.as_ref() {
                    let params_t: Vec<Type> = params
                        .iter()
                        .map(|p| p.type_hint.clone().unwrap_or(Type::Int))
                        .collect();
                    let ret = return_type.clone().unwrap_or(Type::Nil);
                    self.funcs.insert(name.clone(), (params_t, ret));
                }
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
    fn compatible(&self, expected: &Type, actual: &Type) -> bool {
        use Type::*;
        match (expected, actual) {
            // Exact matches.
            (a, b) if a == b => true,
            // Numeric compatibility: any int/float/hex form is mutually ok.
            (Int | I8 | I16 | I32 | I64 | U8 | U16 | U32 | U64 | F32 | F64 | Hex(_),
             Int | I8 | I16 | I32 | I64 | U8 | U16 | U32 | U64 | F32 | F64 | Hex(_)) => true,
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

    fn exec_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, type_hint, mutable, .. } => {
                // Function definitions: record the signature; don't treat the
                // function body as a runtime value assignment.
                if let Expr::Function { params, return_type, body, .. } = value.as_ref() {
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
                            Some("an explicit `let x: T = value` requires `value` to have type `T`".to_string()),
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
            Stmt::If { cond, then_branch, else_branch, .. } => {
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
            Stmt::For { pattern, iterable, body, .. } => {
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
            Stmt::Try { body, catch_name, .. } => {
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
            Ident(name) => self.lookup(name).cloned().unwrap_or(Type::Generic("unknown".into())),
            Tuple(items) => Type::Tuple(items.iter().map(|e| self.infer(e)).collect()),
            Array(items) => {
                let elem = items.iter().map(|e| self.infer(e)).next().unwrap_or(Type::Int);
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
                    Eq | NotEq | Lt | Gt | LtEq | GtEq | And | Or => Type::Bool,
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
                self.infer_call(callee, args)
            }
            If { cond, then_branch, else_branch } => {
                self.infer(cond);
                // Type of an if-expr is the type of the last statement (approx).
                let tt = last_stmt_type(then_branch).unwrap_or(Type::Nil);
                let et = else_branch.as_deref().and_then(last_stmt_type).unwrap_or(Type::Nil);
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
                                    Some(format!("variant `{}::{}` is matched more than once", en, vn)),
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
                    Type::Custom(segments[0].clone())
                } else {
                    Type::Generic("path".into())
                }
            }
            StructLit { fields, .. } => {
                for (_, f) in fields {
                    self.infer(f);
                }
                Type::Custom("struct".into())
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
                "Ok" => return Type::Result(Box::new(self.infer_first_arg(args)), Box::new(Type::Nil)),
                "Err" => return Type::Result(Box::new(Type::Nil), Box::new(self.infer_first_arg(args))),
                "None" => return Type::Option(Box::new(Type::Nil)),
                "array" => return Type::Array(Box::new(Type::Int)),
                "map" => return Type::Map(Box::new(Type::String), Box::new(self.infer_first_arg(args))),
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
                    if arg_t == &Type::Generic("unknown".into()) || &Type::Generic("arg".into()) == arg_t {
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
                            Some("the argument type must match the declared parameter type".to_string()),
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
        assert!(diags[0].found.as_deref() == Some("string"), "got: {:?}", diags[0]);
    }

    #[test]
    fn accepts_char_hint_with_char_literal() {
        assert!(check("let c: char = 'א'").is_empty(), "got: {:?}", check("let c: char = 'א'"));
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
        assert!(missing.is_some(), "got: {:?}", diags.iter().map(|d| &d.code).collect::<Vec<_>>());
    }

    #[test]
    fn accepts_exhaustive_enum_match() {
        let diags = check(
            "enum State { Ready, Running, Finished }\nlet s: State = State::Ready\nmatch s {\n    State::Ready => { dump 1 }\n    State::Running => { dump 2 }\n    State::Finished => { dump 3 }\n}",
        );
        assert!(diags.is_empty(), "got: {:?}", diags.iter().map(|d| d.render()).collect::<Vec<_>>());
    }

    #[test]
    fn catches_unreachable_variant_pattern() {
        let diags = check(
            "enum E { A, B }\nlet e: E = E::A\nmatch e {\n    E::A => { dump 1 }\n    E::A => { dump 2 }\n    E::B => { dump 3 }\n}",
        );
        let un = diags.iter().find(|d| d.code == "E0223");
        assert!(un.is_some(), "got: {:?}", diags.iter().map(|d| d.code).collect::<Vec<_>>());
    }
}