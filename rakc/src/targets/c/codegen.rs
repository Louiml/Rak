//! The C codegen: Rak AST to C source.
//!
//! The unit of translation is the module's top-level statements and any
//! function bodies they contain. Every Rak value becomes a `rak_val` (a tagged
//! union in the emitted runtime), every variable a C local or global of that
//! type, every function a C function taking and returning `rak_val`. Control
//! flow maps structurally; the interpreter's dynamic semantics ג€” the numeric
//! tower, string concatenation, dynamic indexing, `dump`, `fmt` ג€” map to calls
//! into the emitted runtime so the two agree by construction.
//!
//! ## What it refuses, and why
//!
//! The C target compiles a single module with no imports and no Rak *runtime*
//! features whose implementation lives in Rust: no `spawn`/`await`, no sockets,
//! no FFI, no GUI. Those are compile errors that name the construct and the
//! target, rather than silent behavior changes. This is the same
//! written-down-limits policy the VM follows: an honest subset beats a broad
//! one that quietly miscompiles.

use super::runtime::RUNTIME_C;
use super::CodegenOptions;
use crate::ast::*;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Support check
// ---------------------------------------------------------------------------

/// What the C target can and cannot compile, checked before any code is
/// generated. A rejection names the construct and the target, so the reader
/// knows exactly what to change.
struct Support {
    structs: HashMap<String, Vec<String>>,
    enums: HashMap<String, Vec<(String, usize)>>,
}

impl Support {
    fn check(module: &Module) -> Result<Support, String> {
        let mut s = Support {
            structs: HashMap::new(),
            enums: HashMap::new(),
        };
        for item in &module.items {
            s.check_stmt(item)?;
        }
        Ok(s)
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { value, .. } => self.check_expr(value),
            Stmt::Expr(e) | Stmt::Dump { value: e, .. } | Stmt::Trace { value: e } => {
                self.check_expr(e)
            }
            Stmt::Return(Some(e)) | Stmt::Raise(e) | Stmt::Defer(e) | Stmt::Assert(e) => {
                self.check_expr(e)
            }
            Stmt::Return(None) => Ok(()),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(cond)?;
                for s in then_branch {
                    self.check_stmt(s)?;
                }
                if let Some(b) = else_branch {
                    for s in b {
                        self.check_stmt(s)?;
                    }
                }
                Ok(())
            }
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond)?;
                for s in body {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            Stmt::DoWhile { cond, body } => {
                self.check_expr(cond)?;
                for s in body {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            Stmt::Loop { body, .. } => {
                for s in body {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            Stmt::For {
                iterable, body, ..
            } => {
                self.check_expr(iterable)?;
                for s in body {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            Stmt::Match { value, arms } => {
                self.check_expr(value)?;
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.check_expr(g)?;
                    }
                    for s in body {
                        self.check_stmt(s)?;
                    }
                }
                Ok(())
            }
            Stmt::Try {
                body, catch_body, ..
            } => {
                for s in body {
                    self.check_stmt(s)?;
                }
                for s in catch_body {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            Stmt::Const { value, .. } => self.check_expr(value),
            Stmt::Struct { name, fields, .. } => {
                self.structs.insert(
                    name.clone(),
                    fields.iter().map(|f| f.name.clone()).collect(),
                );
                Ok(())
            }
            Stmt::Enum {
                name, variants, ..
            } => {
                self.enums.insert(
                    name.clone(),
                    variants
                        .iter()
                        .map(|v| (v.name.clone(), v.fields.len()))
                        .collect(),
                );
                Ok(())
            }
            Stmt::Unsafe { body, .. } => {
                for s in body {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            // Rejected constructs, each with the reason.
            Stmt::Use { .. } => Err(self.reject("import", "the C target compiles a single module; use `rakc build --emit=exe` or `--emit=bytecode` for multi-file programs")),
            Stmt::Async(_) => Err(self.reject("async", "the C target has no async runtime")),
            Stmt::Extern { .. } => Err(self.reject("extern \"C\"", "the C target has no FFI")),
            Stmt::Impl { .. } | Stmt::Trait { .. } => Err(self.reject("impl/trait", "the C target has no trait dispatch")),
            Stmt::Scan { .. } => Err(self.reject("scan", "the C target has no port scanner")),
            Stmt::Fetch { .. } => Err(self.reject("fetch", "the C target has no HTTP client")),
            Stmt::Tunnel { .. } => Err(self.reject("tunnel", "the C target has no UDP transport")),
            Stmt::Test { .. } => Err(self.reject("test", "the C target has no test runner")),
            Stmt::MacroDef { .. } => Ok(()), // expanded before codegen
            Stmt::Mod { items, .. } => {
                for s in items {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            Stmt::Export(inner) => self.check_stmt(inner),
            Stmt::TypeAlias { .. } => Ok(()),
            Stmt::BinStructDef { .. } => Err(self.reject("binstruct", "the C target has no binary wire-format support")),
            Stmt::IfLet { .. } => Err(self.reject("if let", "the C target does not support if-let patterns")),
            Stmt::WhileLet { .. } => Err(self.reject("while let", "the C target does not support while-let patterns")),
            Stmt::Break(_) | Stmt::Continue(_) => Ok(()),
        }
    }

    fn check_expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Spawn(_) => Err(self.reject("spawn", "the C target has no threads")),
            Expr::Await(_) => Err(self.reject("await", "the C target has no async runtime")),
            Expr::Regex(_, _) => Err(self.reject("regex", "the C target has no regex engine")),
            Expr::Comprehension { .. } => Err(self.reject(
                "comprehension",
                "the C target does not support comprehensions",
            )),
            Expr::EvidenceFrom { .. } => {
                Err(self.reject("evidence", "the C target has no provenance tracking"))
            }
            Expr::MacroVar(_) => {
                Err(self.reject("macro variable", "macros must be expanded before codegen"))
            }
            Expr::MacroInvoke { .. } => Ok(()), // expanded before codegen
            Expr::Call {
                callee,
                args,
                named,
            } => {
                if !named.is_empty() {
                    return Err(self.reject(
                        "named arguments",
                        "the C target does not support named call arguments",
                    ));
                }
                self.check_expr(callee)?;
                for a in args {
                    self.check_expr(a)?;
                }
                Ok(())
            }
            Expr::Function {
                body,
                requires,
                ensures,
                ..
            } => {
                for s in body {
                    self.check_stmt(s)?;
                }
                for r in requires {
                    self.check_expr(r)?;
                }
                for r in ensures {
                    self.check_expr(r)?;
                }
                Ok(())
            }
            Expr::Lambda { body, .. } => self.check_expr(body),
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(cond)?;
                for s in then_branch {
                    self.check_stmt(s)?;
                }
                if let Some(b) = else_branch {
                    for s in b {
                        self.check_stmt(s)?;
                    }
                }
                Ok(())
            }
            Expr::Match { value, arms } => {
                self.check_expr(value)?;
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.check_expr(g)?;
                    }
                    for s in body {
                        self.check_stmt(s)?;
                    }
                }
                Ok(())
            }
            Expr::Block(body) => {
                for s in body {
                    self.check_stmt(s)?;
                }
                Ok(())
            }
            Expr::TryExpr(inner) => self.check_expr(inner),
            Expr::As(inner, _) => self.check_expr(inner),
            Expr::Ternary { cond, then, els } => {
                self.check_expr(cond)?;
                self.check_expr(then)?;
                self.check_expr(els)?;
                Ok(())
            }
            Expr::NilCoalesce(a, b) => {
                self.check_expr(a)?;
                self.check_expr(b)?;
                Ok(())
            }
            Expr::OptField(a, _) | Expr::OptIndex(a, _) => self.check_expr(a),
            Expr::MultiAssign { targets, values } => {
                for t in targets {
                    self.check_expr(t)?;
                }
                for v in values {
                    self.check_expr(v)?;
                }
                Ok(())
            }
            Expr::IndexAssign { obj, idx, value } => {
                self.check_expr(obj)?;
                self.check_expr(idx)?;
                self.check_expr(value)?;
                Ok(())
            }
            Expr::FieldAssign { obj, value, .. } => {
                self.check_expr(obj)?;
                self.check_expr(value)?;
                Ok(())
            }
            Expr::Unary(_, e) | Expr::FieldAccess(e, _) | Expr::Raise(e) => self.check_expr(e),
            Expr::Binary(_, a, b) | Expr::Index(a, b) => {
                self.check_expr(a)?;
                self.check_expr(b)?;
                Ok(())
            }
            Expr::Assign(_, e) | Expr::CompoundAssign(_, _, e) => self.check_expr(e),
            Expr::Range(lo, hi) => {
                if let Some(e) = lo {
                    self.check_expr(e)?;
                }
                if let Some(e) = hi {
                    self.check_expr(e)?;
                }
                Ok(())
            }
            Expr::Interp { parts, .. } => {
                for p in parts {
                    self.check_expr(p)?;
                }
                Ok(())
            }
            Expr::Array(items) | Expr::Tuple(items) => {
                for i in items {
                    self.check_expr(i)?;
                }
                Ok(())
            }
            Expr::Map(pairs) => {
                for (k, v) in pairs {
                    self.check_expr(k)?;
                    self.check_expr(v)?;
                }
                Ok(())
            }
            Expr::Path(_)
            | Expr::Ident(_)
            | Expr::Nil
            | Expr::Bool(_)
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::Float32(_)
            | Expr::TypedInt(_, _)
            | Expr::Hex(_)
            | Expr::BinLit(_)
            | Expr::OctLit(_)
            | Expr::String(_)
            | Expr::Char(_)
            | Expr::Bytes(_) => Ok(()),
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.check_expr(v)?;
                }
                Ok(())
            }
        }
    }

    fn reject(&self, what: &str, why: &str) -> String {
        format!("error: {} is not supported by target 'c'. {}", what, why)
    }
}

// ---------------------------------------------------------------------------
// Code generator
// ---------------------------------------------------------------------------

/// The result of emitting an expression: optional prelude statements followed
/// by a C expression that yields an owned `rak_val`.
struct Emit {
    pre: Vec<String>,
    expr: String,
}

impl Emit {
    fn simple(expr: impl Into<String>) -> Emit {
        Emit {
            pre: Vec::new(),
            expr: expr.into(),
        }
    }

    /// Write the prelude statements to `out` and return the expression.
    /// Takes `&self` and writes pre lines first, then returns the expression
    /// so the caller can use it without a double borrow.
    fn flush(&self, out: &mut String) -> String {
        for line in &self.pre {
            out.push_str(line);
            out.push('\n');
        }
        self.expr.clone()
    }
}

struct LoopCtx {
    end_label: String,
    continue_label: String,
    label: Option<String>,
}

struct Gen {
    out: String,
    temp: usize,
    fn_counter: usize,
    fn_names: HashMap<String, String>,
    fn_arity: HashMap<String, i64>,
    vars: Vec<Vec<String>>,
    globals: HashSet<String>,
    loops: Vec<LoopCtx>,
    captures: Vec<String>,
    has_main: bool,
    support: Support,
    fn_defs: Vec<String>,
    global_decls: Vec<String>,
    toplevel_body: Vec<String>,
    /// `rak_val _x;` declarations for temporaries minted by `fresh`, flushed
    /// at the top of the function currently being generated.
    pending_decls: Vec<String>,
}

impl Gen {
    fn new(support: Support) -> Gen {
        Gen {
            out: String::new(),
            temp: 0,
            fn_counter: 0,
            fn_names: HashMap::new(),
            fn_arity: HashMap::new(),
            vars: Vec::new(),
            globals: HashSet::new(),
            loops: Vec::new(),
            captures: Vec::new(),
            has_main: false,
            support,
            fn_defs: Vec::new(),
            global_decls: Vec::new(),
            toplevel_body: Vec::new(),
            pending_decls: Vec::new(),
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.temp += 1;
        let name = format!("_{}{}", prefix, self.temp);
        // Every temporary a value-producing expression may assign into is
        // declared at the top of its enclosing function, not where it is
        // first used: an emitter's prelude assigns to the name in a nested
        // block, and a declaration there would be out of scope by the time
        // the value is read. `flush_decls` emits them.
        self.pending_decls.push(format!("rak_val {};", name));
        name
    }

    fn fresh_fn(&mut self) -> String {
        self.fn_counter += 1;
        format!("rak_fn_{}", self.fn_counter)
    }

    fn indent(&self) -> String {
        "    ".repeat(self.vars.len())
    }

    // ------------------------------------------------------------- scopes

    fn push_scope(&mut self) {
        self.vars.push(Vec::new());
    }

    /// Pop a scope, returning the C names its locals were bound to.
    ///
    /// The caller writes the drops: `self.out` is not necessarily the buffer
    /// this scope's body was emitted into (toplevel and function bodies emit
    /// into their own buffers), and a drop written to the wrong buffer both
    /// leaks the value and emits a reference to an undeclared name.
    fn pop_scope(&mut self) -> Vec<String> {
        match self.vars.pop() {
            Some(scope) => scope.iter().map(|n| self.c_ident(n)).collect(),
            None => Vec::new(),
        }
    }

    /// Emit `rak_drop` for every local of the innermost scope, in reverse
    /// declaration order, and pop it.
    fn drop_scope(&mut self, out: &mut String) {
        let ind = self.indent();
        for name in self.pop_scope().into_iter().rev() {
            out.push_str(&format!("{}rak_drop({});\n", ind, name));
        }
    }

    fn declare(&mut self, name: &str) {
        if let Some(scope) = self.vars.last_mut() {
            scope.push(name.to_string());
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.vars.iter().any(|s| s.iter().any(|n| n == name))
    }

    /// The index of `name` in the capture list of the closure currently being
    /// emitted, if it is captured at all.
    ///
    /// A captured name is *not* a local of the closure that captured it: it
    /// belongs to the enclosing function, and by the time the closure runs
    /// that function's C variable is gone. The closure therefore reads it out
    /// of its environment, at the index the capture list gives it.
    fn capture_index(&self, name: &str) -> Option<usize> {
        self.captures.iter().position(|c| c == name)
    }

    fn is_global(&self, name: &str) -> bool {
        self.globals.contains(name)
    }

    fn fn_c_name(&self, name: &str) -> Option<String> {
        self.fn_names.get(name).cloned()
    }

    fn fn_arity(&self, name: &str) -> i64 {
        self.fn_arity.get(name).copied().unwrap_or(-1)
    }

    // ------------------------------------------------------------- helpers

    fn c_ident(&self, name: &str) -> String {
        // Rak identifiers are [a-zA-Z_][a-zA-Z0-9_]*; C identifiers are the
        // same, so no mangling is needed beyond the prefix.
        format!("v_{}", name)
    }

    fn c_global(&self, name: &str) -> String {
        format!("g_{}", name)
    }

    fn c_str_lit(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\0' => out.push_str("\\0"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn c_bytes_lit(b: &[u8]) -> String {
        let mut out = String::with_capacity(b.len() * 4 + 2);
        out.push('{');
        for (i, byte) in b.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("0x{:02X}", byte));
        }
        out.push('}');
        out
    }

    // ------------------------------------------------------------- values

    /// Compile `e`, appending any prelude statements to `out`, and return the
    /// C expression that yields the value.
    ///
    /// This is the single correct way to consume an [`Emit`]: an expression's
    /// temporary variables live in its prelude, so dropping the prelude leaves
    /// references to names that were never declared — which is exactly the
    /// class of bug this method exists to prevent. Every statement emitter
    /// must go through here (or through `Emit::flush`) rather than reading
    /// `.expr` directly.
    fn emit_expr(&mut self, e: &Expr, out: &mut String) -> Result<String, String> {
        let emitted = self.value(e)?;
        for line in &emitted.pre {
            out.push_str(line);
            out.push('\n');
        }
        Ok(emitted.expr)
    }

    fn value(&mut self, e: &Expr) -> Result<Emit, String> {
        match e {
            Expr::Nil => Ok(Emit::simple("rak_nil()")),
            Expr::Bool(b) => Ok(Emit::simple(format!(
                "rak_bool({})",
                if *b { 1 } else { 0 }
            ))),
            Expr::Int(i) => Ok(Emit::simple(format!("rak_int({})", i))),
            Expr::Hex(h) => Ok(Emit::simple(format!("rak_hex(0x{:X})", h))),
            Expr::BinLit(b) => Ok(Emit::simple(format!("rak_hex(0b{:b})", b))),
            Expr::OctLit(o) => Ok(Emit::simple(format!("rak_hex(0o{:o})", o))),
            Expr::Float(f) => Ok(Emit::simple(format!("rak_float({})", fmt_float(*f)))),
            Expr::Float32(f) => Ok(Emit::simple(format!("rak_float({})", fmt_float(*f as f64)))),
            Expr::TypedInt(i, _) => Ok(Emit::simple(format!("rak_int({})", i))),
            Expr::String(s) => Ok(Emit::simple(format!(
                "rak_strn({}, {})",
                Self::c_str_lit(s),
                s.len()
            ))),
            Expr::Char(c) => Ok(Emit::simple(format!("rak_charv(0x{:X})", *c as u32))),
            Expr::Bytes(b) => Ok(Emit::simple(format!(
                "rak_bytesn((const unsigned char[]){}, {})",
                Self::c_bytes_lit(b),
                b.len()
            ))),
            Expr::Ident(name) => self.ident_value(name),
            Expr::Unary(op, e) => self.unary(*op, e),
            Expr::Binary(op, a, b) => self.binary(*op, a, b),
            Expr::Assign(name, e) => self.assign(name, e),
            Expr::CompoundAssign(op, name, e) => self.compound_assign(*op, name, e),
            Expr::IndexAssign { obj, idx, value } => self.index_assign(obj, idx, value),
            Expr::FieldAssign { obj, field, value } => self.field_assign(obj, field, value),
            Expr::FieldAccess(obj, field) => self.field_access(obj, field),
            Expr::Index(obj, idx) => self.index(obj, idx),
            Expr::Range(lo, hi) => self.range(lo, hi),
            Expr::Call { callee, args, .. } => self.call(callee, args),
            Expr::Function {
                params, body, name, ..
            } => self.function_value(params, body, name.as_deref()),
            Expr::Lambda { params, body, .. } => self.lambda_value(params, body),
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => self.if_expr(cond, then_branch, else_branch.as_deref()),
            Expr::Match { value, arms } => self.match_expr(value, arms),
            Expr::Block(body) => self.block_expr(body),
            Expr::TryExpr(inner) => self.try_expr(inner),
            Expr::Ternary { cond, then, els } => self.ternary(cond, then, els),
            Expr::NilCoalesce(a, b) => self.nil_coalesce(a, b),
            Expr::OptField(obj, field) => self.opt_field(obj, field),
            Expr::OptIndex(obj, idx) => self.opt_index(obj, idx),
            Expr::MultiAssign { targets, values } => self.multi_assign(targets, values),
            Expr::Path(parts) => self.path_value(parts),
            Expr::StructLit { name, fields } => self.struct_lit(name, fields),
            Expr::As(inner, ty) => self.as_cast(inner, ty),
            Expr::Interp { template, parts } => self.interp(template, parts),
            Expr::Array(items) => self.array_lit(items),
            Expr::Tuple(items) => self.tuple_lit(items),
            Expr::Map(pairs) => self.map_lit(pairs),
            Expr::MacroInvoke { .. } => Err("macro invocation was not expanded".to_string()),
            Expr::MacroVar(_) => Err("macro variable outside a macro body".to_string()),
            Expr::Spawn(_)
            | Expr::Await(_)
            | Expr::Regex(_, _)
            | Expr::Comprehension { .. }
            | Expr::EvidenceFrom { .. }
            | Expr::Raise(_) => Err("expression rejected by the support check".to_string()),
        }
    }

    fn ident_value(&mut self, name: &str) -> Result<Emit, String> {
        // A name the closure captured comes out of its environment, and that
        // check has to come first: the captured name is also a local of the
        // *enclosing* function, and `is_local` searches every scope, so it
        // would otherwise find the enclosing function's C variable -- which
        // does not exist by the time the closure runs. The generated C then
        // referenced a variable that was never declared, and the C compiler
        // rejected the program.
        if let Some(i) = self.capture_index(name) {
            return Ok(Emit::simple(format!("rak_dup(rak_env_get(env, {}))", i)));
        }
        if self.is_local(name) {
            return Ok(Emit::simple(format!("rak_dup({})", self.c_ident(name))));
        }
        if self.is_global(name) {
            return Ok(Emit::simple(format!("rak_dup({})", self.c_global(name))));
        }
        if let Some(cname) = self.fn_c_name(name) {
            let arity = self.fn_arity(name);
            return Ok(Emit::simple(format!(
                "rak_closure({}, NULL, 0, {})",
                cname, arity
            )));
        }
        if name == "None" {
            return Ok(Emit::simple("rak_none()"));
        }
        if name == "Some" || name == "Ok" || name == "Err" {
            return Ok(Emit::simple(format!(
                "rak_builtin_value({})",
                Self::c_str_lit(name)
            )));
        }
        if is_builtin_name(name) {
            return Ok(Emit::simple(format!(
                "rak_builtin_value({})",
                Self::c_str_lit(name)
            )));
        }
        Err(format!("undefined variable '{}'", name))
    }

    fn unary(&mut self, op: UnOp, e: &Expr) -> Result<Emit, String> {
        let v = self.value(e)?;
        let expr = match op {
            UnOp::Minus => format!("rak_neg({})", v.expr),
            UnOp::Not => format!("rak_bool(!rak_truthy({}))", v.expr),
            UnOp::BitNot => {
                let t = self.fresh("t");
                let pre = format!(
                    "{{ {} = {}; {} = rak_int(~rak_as_i({})); }}",
                    t, v.expr, t, t
                );
                return Ok(Emit {
                    pre: {
                        let mut p = v.pre;
                        p.push(pre);
                        p
                    },
                    expr: format!("rak_dup({})", t),
                });
            }
        };
        Ok(Emit { pre: v.pre, expr })
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr) -> Result<Emit, String> {
        match op {
            BinOp::And => self.and_or(a, b, true),
            BinOp::Or => self.and_or(a, b, false),
            BinOp::Eq => {
                let l = self.value(a)?;
                let r = self.value(b)?;
                Ok(Emit {
                    pre: {
                        let mut p = l.pre;
                        p.extend(r.pre);
                        p
                    },
                    expr: format!("rak_bool(rak_val_eq({}, {}))", l.expr, r.expr),
                })
            }
            BinOp::NotEq => {
                let l = self.value(a)?;
                let r = self.value(b)?;
                Ok(Emit {
                    pre: {
                        let mut p = l.pre;
                        p.extend(r.pre);
                        p
                    },
                    expr: format!("rak_bool(!rak_val_eq({}, {}))", l.expr, r.expr),
                })
            }
            BinOp::In => {
                let l = self.value(a)?;
                let r = self.value(b)?;
                Ok(Emit {
                    pre: {
                        let mut p = l.pre;
                        p.extend(r.pre);
                        p
                    },
                    expr: format!("rak_bool(rak_contains_member({}, {}))", r.expr, l.expr),
                })
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                let l = self.value(a)?;
                let r = self.value(b)?;
                let t = self.fresh("t");
                let u = self.fresh("u");
                let cmp = match op {
                    BinOp::Lt => format!("rak_cmp({}, {}, &_ok) < 0", t, u),
                    BinOp::Gt => format!("rak_cmp({}, {}, &_ok) > 0", t, u),
                    BinOp::LtEq => format!("rak_cmp({}, {}, &_ok) != 1", t, u),
                    _ => format!("rak_cmp({}, {}, &_ok) != -1", t, u),
                };
                let pre = format!(
                    "{{ {} = {}; rak_val {} = {}; int _ok; {} = rak_bool({}); }}",
                    t, l.expr, u, r.expr, t, cmp
                );
                Ok(Emit {
                    pre: {
                        let mut p = l.pre;
                        p.extend(r.pre);
                        p.push(pre);
                        p
                    },
                    expr: format!("rak_dup({})", t),
                })
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                let l = self.value(a)?;
                let r = self.value(b)?;
                let t = self.fresh("t");
                let u = self.fresh("u");
                let fname = match op {
                    BinOp::Add => "rak_add",
                    BinOp::Sub => "rak_sub",
                    BinOp::Mul => "rak_mul",
                    BinOp::Div => "rak_div",
                    _ => "rak_rem",
                };
                let pre = format!(
                    "{{ {} = {}; rak_val {} = {}; {} = {}({}, {}); }}",
                    t, l.expr, u, r.expr, t, fname, t, u
                );
                Ok(Emit {
                    pre: {
                        let mut p = l.pre;
                        p.extend(r.pre);
                        p.push(pre);
                        p
                    },
                    expr: format!("rak_dup({})", t),
                })
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                let l = self.value(a)?;
                let r = self.value(b)?;
                let op_char = match op {
                    BinOp::BitAnd => '&',
                    BinOp::BitOr => '|',
                    BinOp::BitXor => '^',
                    BinOp::Shl => '<',
                    _ => '>',
                };
                let t = self.fresh("t");
                let u = self.fresh("u");
                // `&`/`|`/`^` go through `rak_bitop`, which propagates the hex
                // representation the way the interpreter's int path does;
                // `<<`/`>>` go through `rak_shift`. Routing all five through
                // `rak_shift` made `0xFF & 0x0F` compute `255 >> 15` — the
                // conformance corpus caught it.
                let call = if matches!(op, BinOp::Shl | BinOp::Shr) {
                    format!(
                        "rak_int(rak_shift('{}', rak_as_i({}), rak_as_i({})))",
                        op_char, t, u
                    )
                } else {
                    format!("rak_bitop('{}', {}, {})", op_char, t, u)
                };
                let pre = format!(
                    "{{ {} = {}; {} = {}; {} = {}; }}",
                    t, l.expr, u, r.expr, t, call
                );
                Ok(Emit {
                    pre: {
                        let mut p = l.pre;
                        p.extend(r.pre);
                        p.push(pre);
                        p
                    },
                    expr: format!("rak_dup({})", t),
                })
            }
        }
    }

    fn and_or(&mut self, a: &Expr, b: &Expr, is_and: bool) -> Result<Emit, String> {
        let l = self.value(a)?;
        let r = self.value(b)?;
        let t = self.fresh("t");
        let lv = self.fresh("l");
        let rv = self.fresh("r");
        let pre = format!(
            "rak_val {};\\n{{\\n  {} = {};\n  int _t = rak_truthy({});\n  rak_drop({});\n  if ({}) {{\n    {} = {};\n    {} = rak_bool(rak_truthy({}));\n    rak_drop({});\n  }} else {{\n    {} = rak_bool({});\n  }}\n}}",
            t,
            lv, l.expr, lv, lv,
            if is_and { "_t" } else { "!_t" },
            rv, r.expr, t, rv, rv,
            t, if is_and { 0 } else { 1 }
        );
        Ok(Emit {
            pre: {
                let mut p = l.pre;
                p.extend(r.pre);
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn assign(&mut self, name: &str, e: &Expr) -> Result<Emit, String> {
        // Assigning to a captured variable is not something this backend can
        // do. The interpreter's closures share the enclosing function's
        // environment, so an assignment inside the closure is visible outside
        // it. Here the environment is an array of *copies* taken when the
        // closure was created, so the assignment would land in the copy and
        // the enclosing function would never see it -- a silent wrong answer,
        // and a different one on each backend.
        //
        // Saying so is better than generating the wrong program: the caller
        // learns the construct is unsupported here, and the interpreter still
        // runs it.
        if self.capture_index(name).is_some() {
            return Err(format!(
                "the C target cannot assign to a captured variable ('{}'): \
                 a closure's captured values are copies here and the enclosing \
                 function would not see the assignment",
                name
            ));
        }
        let v = self.value(e)?;
        let target = if self.is_local(name) {
            self.c_ident(name)
        } else if self.is_global(name) {
            self.c_global(name)
        } else {
            return Err(format!("cannot assign to undefined variable '{}'", name));
        };
        let t = self.fresh("t");
        let pre = format!(
            "{{ {} = {}; rak_assign(&{}, rak_dup({})); }}",
            t, v.expr, target, t
        );
        Ok(Emit {
            pre: {
                let mut p = v.pre;
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn compound_assign(&mut self, op: CompoundOp, name: &str, e: &Expr) -> Result<Emit, String> {
        let v = self.value(e)?;
        let target = if self.is_local(name) {
            self.c_ident(name)
        } else if self.is_global(name) {
            self.c_global(name)
        } else {
            return Err(format!("cannot assign to undefined variable '{}'", name));
        };
        let op_char = match op {
            CompoundOp::Add => '+',
            CompoundOp::Sub => '-',
            CompoundOp::Mul => '*',
            CompoundOp::Div => '/',
            CompoundOp::Rem => '%',
            CompoundOp::BitAnd => '&',
            CompoundOp::BitOr => '|',
            CompoundOp::BitXor => '^',
            CompoundOp::Shl => '<',
            CompoundOp::Shr => '>',
        };
        let t = self.fresh("t");
        let u = self.fresh("u");
        let pre = format!(
            "{{ {} = {}; {} = rak_dup({}); {} = rak_arith('{}', {}, {}); rak_assign(&{}, rak_dup({})); }}",
            t, v.expr, u, target, t, op_char, u, t, target, t
        );
        Ok(Emit {
            pre: {
                let mut p = v.pre;
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn index_assign(&mut self, obj: &Expr, idx: &Expr, value: &Expr) -> Result<Emit, String> {
        let v = self.value(value)?;
        let o = self.value(obj)?;
        let i = self.value(idx)?;
        let t = self.fresh("t");
        let iv = self.fresh("i");
        let pre = format!(
            "{{ {} = {}; {} = {}; {} = rak_index({}, {}); }}",
            t, o.expr, iv, i.expr, t, t, iv
        );
        Ok(Emit {
            pre: {
                let mut p = v.pre;
                p.extend(o.pre);
                p.extend(i.pre);
                p.push(pre);
                p
            },
            expr: "rak_nil()".to_string(),
        })
    }

    fn field_assign(&mut self, obj: &Expr, field: &str, value: &Expr) -> Result<Emit, String> {
        let v = self.value(value)?;
        let o = self.value(obj)?;
        let t = self.fresh("t");
        let ov = self.fresh("o");
        let pre = format!(
            "{{ {} = {}; {} = {}; rak_field_set({}, {}, {}); }}",
            t,
            v.expr,
            ov,
            o.expr,
            ov,
            Self::c_str_lit(field),
            t
        );
        Ok(Emit {
            pre: {
                let mut p = v.pre;
                p.extend(o.pre);
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn field_access(&mut self, obj: &Expr, field: &str) -> Result<Emit, String> {
        let o = self.value(obj)?;
        Ok(Emit {
            pre: o.pre,
            expr: format!("rak_field({}, {})", o.expr, Self::c_str_lit(field)),
        })
    }

    fn index(&mut self, obj: &Expr, idx: &Expr) -> Result<Emit, String> {
        // `xs[a..b]` is a slice, not an index.
        if let Expr::Range(lo, hi) = idx {
            let o = self.value(obj)?;
            let l = match lo {
                Some(e) => self.value(e)?,
                None => Emit::simple("rak_nil()"),
            };
            let h = match hi {
                Some(e) => self.value(e)?,
                None => Emit::simple("rak_nil()"),
            };
            let t = self.fresh("t");
            let lv = self.fresh("l");
            let hv = self.fresh("h");
            let pre = format!(
                "{{ {} = {}; {} = {}; {} = {}; {} = bi_slice((rak_val[]){{{}, {}, {}}}, 3); }}",
                t, o.expr, lv, l.expr, hv, h.expr, t, t, lv, hv
            );
            return Ok(Emit {
                pre: {
                    let mut p = o.pre;
                    p.extend(l.pre);
                    p.extend(h.pre);
                    p.push(pre);
                    p
                },
                expr: format!("rak_dup({})", t),
            });
        }
        let o = self.value(obj)?;
        let i = self.value(idx)?;
        let t = self.fresh("t");
        let iv = self.fresh("i");
        let pre = format!(
            "{{ {} = {}; {} = {}; {} = rak_index({}, {}); }}",
            t, o.expr, iv, i.expr, t, t, iv
        );
        Ok(Emit {
            pre: {
                let mut p = o.pre;
                p.extend(i.pre);
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn range(&mut self, lo: &Option<Box<Expr>>, hi: &Option<Box<Expr>>) -> Result<Emit, String> {
        let l = match lo {
            Some(e) => self.value(e)?,
            None => Emit::simple("rak_int(0)"),
        };
        let h = match hi {
            Some(e) => self.value(e)?,
            None => Emit::simple("rak_int(0)"),
        };
        let t = self.fresh("t");
        let lv = self.fresh("l");
        let hv = self.fresh("h");
        let _pre = format!(
            "{{ {} = {}; rak_val {} = {}; {} = rak_array_of((rak_val[]){{}}, 0); }}",
            t,
            l.expr,
            self.fresh("h"),
            h.expr,
            t
        );
        // A standalone range is an array of ints from lo to hi inclusive.
        let pre = format!(
            "rak_val {};\\n{{\\n  {} = {};\n  {} = {};\n  long long _s = rak_as_i({});\n  long long _e = rak_as_i({});\n  if (_e < _s) _e = _s;\n  size_t _n = (size_t)(_e - _s + 1);\n  rak_val *_items = (rak_val *)malloc((_n ? _n : 1) * sizeof(*_items));\n  for (long long _i = _s; _i <= _e; _i++) _items[_i - _s] = rak_int(_i);\n  {} = rak_array_of(_items, _n);\n}}",
            t, lv, l.expr, hv, h.expr, lv, hv, t
        );
        Ok(Emit {
            pre: {
                let mut p = l.pre;
                p.extend(h.pre);
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> Result<Emit, String> {
        // Method call: `obj.method(args)` ג†’ `bi_method((rak_val[]){obj, args...}, 1+n)`.
        if let Expr::FieldAccess(obj, method) = callee {
            if is_method_builtin(method) {
                let o = self.value(obj)?;
                let mut arg_emits = Vec::new();
                for a in args {
                    arg_emits.push(self.value(a)?);
                }
                let t = self.fresh("t");
                let mut pre = o.pre;
                let mut arg_exprs = vec![o.expr.clone()];
                for ae in arg_emits {
                    pre.extend(ae.pre);
                    arg_exprs.push(ae.expr);
                }
                let arg_list = arg_exprs.join(", ");
                let n = arg_exprs.len();
                pre.push(format!(
                    "{{ {} = bi_{}((rak_val[]){{{}}}, {}); }}",
                    t, method, arg_list, n
                ));
                return Ok(Emit {
                    pre,
                    expr: format!("rak_dup({})", t),
                });
            }
        }

        // Direct call to a known top-level function.
        if let Expr::Ident(name) = callee {
            if let Some(cname) = self.fn_c_name(name) {
                let mut arg_emits = Vec::new();
                for a in args {
                    arg_emits.push(self.value(a)?);
                }
                let t = self.fresh("t");
                let mut pre = Vec::new();
                let mut arg_exprs = Vec::new();
                for ae in arg_emits {
                    pre.extend(ae.pre);
                    arg_exprs.push(ae.expr);
                }
                let arg_list = arg_exprs.join(", ");
                let n = arg_exprs.len();
                pre.push(format!(
                    "{{ {} = {}(NULL, (rak_val[]){{{}}}, {}); }}",
                    t, cname, arg_list, n
                ));
                return Ok(Emit {
                    pre,
                    expr: format!("rak_dup({})", t),
                });
            }
            // Builtin call.
            if is_builtin_name(name) {
                let mut arg_emits = Vec::new();
                for a in args {
                    arg_emits.push(self.value(a)?);
                }
                let t = self.fresh("t");
                let mut pre = Vec::new();
                let mut arg_exprs = Vec::new();
                for ae in arg_emits {
                    pre.extend(ae.pre);
                    arg_exprs.push(ae.expr);
                }
                let arg_list = arg_exprs.join(", ");
                let n = arg_exprs.len();
                pre.push(format!(
                    "{{ {} = bi_{}((rak_val[]){{{}}}, {}); }}",
                    t, name, arg_list, n
                ));
                return Ok(Emit {
                    pre,
                    expr: format!("rak_dup({})", t),
                });
            }
            // Enum variant constructor: `Color::Red` or `Shape::Circle(3)`.
            if args.len() == 1 {
                if let Expr::Path(parts) = callee {
                    if parts.len() == 2 {
                        if let Some(variants) = self.support.enums.get(&parts[0]) {
                            if variants.iter().any(|(v, _)| v == &parts[1]) {
                                let a = self.value(&args[0])?;
                                let t = self.fresh("t");
                                let pre = format!(
                                    "{{ {} = {}; {} = rak_enum_new({}, {}, (rak_val[]){{{}}}, 1); }}",
                                    t, a.expr, t, Self::c_str_lit(&parts[0]), Self::c_str_lit(&parts[1]), t
                                );
                                return Ok(Emit {
                                    pre: {
                                        let mut p = a.pre;
                                        p.push(pre);
                                        p
                                    },
                                    expr: format!("rak_dup({})", t),
                                });
                            }
                        }
                    }
                }
            }
        }

        // General call through a value.
        let f = self.value(callee)?;
        let mut arg_emits = Vec::new();
        for a in args {
            arg_emits.push(self.value(a)?);
        }
        let t = self.fresh("t");
        let mut pre = f.pre;
        let mut arg_exprs = Vec::new();
        for ae in arg_emits {
            pre.extend(ae.pre);
            arg_exprs.push(ae.expr);
        }
        let arg_list = arg_exprs.join(", ");
        let n = arg_exprs.len();
        pre.push(format!(
            "{{ {} = {}; {} = rak_call({}, {}, (rak_val[]){{{}}}); }}",
            t, f.expr, t, t, n, arg_list
        ));
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn function_value(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        name: Option<&str>,
    ) -> Result<Emit, String> {
        // A top-level `fn` already reserved its C name in the pre-pass, so
        // that calls inside other bodies resolve to the name this definition
        // will actually use. Anything else (a nested or anonymous value) mints
        // a fresh one here.
        let cname = match name.and_then(|n| self.fn_c_name(n)) {
            Some(existing) => existing,
            None => self.fresh_fn(),
        };
        let arity = params
            .iter()
            .filter(|p| !p.rest && p.default.is_none() && !p.optional)
            .count() as i64;
        let arity = if params
            .iter()
            .any(|p| p.rest || p.default.is_some() || p.optional)
        {
            -1
        } else {
            arity
        };

        // Collect captures: free variables in the body that are not params.
        let param_names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let mut captures = Vec::new();
        self.collect_captures_stmts(body, &param_names, &mut captures);
        captures.sort();
        captures.dedup();

        let mut pre = Vec::new();
        let env_items: Vec<String> = captures
            .iter()
            .map(|c| {
                // A name the *enclosing* closure captured is read out of that
                // closure's environment, not out of a C variable: there is no
                // C variable for it, because the enclosing function that had
                // one is a frame away and gone.
                if let Some(j) = self.capture_index(c) {
                    format!("rak_env_get(env, {})", j)
                } else if self.is_local(c) {
                    format!("rak_dup({})", self.c_ident(c))
                } else if self.is_global(c) {
                    format!("rak_dup({})", self.c_global(c))
                } else {
                    "rak_nil()".to_string()
                }
            })
            .collect();
        let env_list = env_items.join(", ");
        let env_n = captures.len();

        // Emit the function body. The declarations this body mints start
        // accumulating *now*: anything already in `pending_decls` belongs to
        // the enclosing function's earlier statements, and `mem::take` here
        // would steal them into this definition — leaving the enclosing
        // function with references to names it never declared.
        let decls_start = self.pending_decls.len();
        let mut body_out = String::new();
        self.push_scope();
        for p in params {
            self.declare(&p.name);
        }
        // The body reads the captured names out of the environment, so the
        // capture list has to be in force while it is emitted.
        let saved_captures = std::mem::replace(&mut self.captures, captures.clone());
        for s in body {
            self.emit_stmt(&mut body_out, s)?;
        }
        self.captures = saved_captures;
        self.drop_scope(&mut body_out);
        // Only the declarations minted *during* this body (from `decls_start`
        // on) belong to this definition; earlier ones belong to the enclosing
        // function and stay put.
        let body_decls = if self.pending_decls.len() > decls_start {
            self.pending_decls.split_off(decls_start)
        } else {
            Vec::new()
        };

        let param_decls: Vec<String> = params
            .iter()
            .map(|p| format!("rak_val {} = rak_nil();", self.c_ident(&p.name)))
            .collect();
        let param_list = param_decls.join("\n    ");

        let prologue = self.fn_prologue(params, &cname)?;

        let fn_def = format!(
            "static rak_val {}(rak_obj *env, rak_val *args, int argc) {{\n    int _mark = rak_defer_mark();\n    {}\n    {}\n    {}\n    {}\n    return rak_nil();\n}}\n",
            cname,
            param_list,
            body_decls.join("\n    "),
            prologue,
            body_out
        );
        self.fn_defs.push(fn_def);

        let t = self.fresh("t");
        pre.push(format!(
            "{{ {} = rak_closure({}, rak_env_new((rak_val[]){{{}}}, {}), {}, {}); }}",
            t, cname, env_list, env_n, env_n, arity
        ));
        let _ = name;
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn lambda_value(&mut self, params: &[Param], body: &Expr) -> Result<Emit, String> {
        // A lambda is a function with an expression body.
        let cname = self.fresh_fn();
        let arity = params.len() as i64;

        let param_names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let mut captures = Vec::new();
        self.collect_captures_expr(body, &param_names, &mut captures);
        captures.sort();
        captures.dedup();

        let mut pre = Vec::new();
        let env_items: Vec<String> = captures
            .iter()
            .map(|c| {
                // As in `function_value`: a name the enclosing closure
                // captured comes out of that closure's environment.
                if let Some(j) = self.capture_index(c) {
                    format!("rak_env_get(env, {})", j)
                } else if self.is_local(c) {
                    format!("rak_dup({})", self.c_ident(c))
                } else if self.is_global(c) {
                    format!("rak_dup({})", self.c_global(c))
                } else {
                    "rak_nil()".to_string()
                }
            })
            .collect();
        let env_list = env_items.join(", ");
        let env_n = captures.len();

        // Bound the lambda's declarations to what its own body mints, as
        // `function_value` does: an unbounded take would steal the enclosing
        // function's accumulated temporaries.
        let decls_start = self.pending_decls.len();
        let body_val = self.value(body)?;
        let mut body_out = String::new();
        self.push_scope();
        for p in params {
            self.declare(&p.name);
        }
        // The body reads the captured names out of the environment.
        let saved_captures = std::mem::replace(&mut self.captures, captures.clone());
        for line in &body_val.pre {
            body_out.push_str("    ");
            body_out.push_str(line);
            body_out.push('\n');
        }
        body_out.push_str(&format!("    return {};\n", body_val.expr));
        self.captures = saved_captures;
        self.drop_scope(&mut body_out);
        let body_decls = if self.pending_decls.len() > decls_start {
            self.pending_decls.split_off(decls_start)
        } else {
            Vec::new()
        };

        let param_decls: Vec<String> = params
            .iter()
            .map(|p| format!("rak_val {} = rak_nil();", self.c_ident(&p.name)))
            .collect();
        let param_list = param_decls.join("\n    ");
        let prologue = self.fn_prologue(params, &cname)?;

        let fn_def = format!(
            "static rak_val {}(rak_obj *env, rak_val *args, int argc) {{\n    int _mark = rak_defer_mark();\n    {}\n    {}\n    {}\n    {}\n    return rak_nil();\n}}\n",
            cname,
            param_list,
            body_decls.join("\n    "),
            prologue,
            body_out
        );
        self.fn_defs.push(fn_def);

        let t = self.fresh("t");
        pre.push(format!(
            "{{ {} = rak_closure({}, rak_env_new((rak_val[]){{{}}}, {}), {}, {}); }}",
            t, cname, env_list, env_n, env_n, arity
        ));
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn fn_prologue(&mut self, params: &[Param], _cname: &str) -> Result<String, String> {
        let mut lines = Vec::new();
        let required = params
            .iter()
            .filter(|p| !p.rest && p.default.is_none() && !p.optional)
            .count();
        let has_rest = params.iter().any(|p| p.rest);
        let max_args = params.iter().filter(|p| !p.rest).count();

        if !has_rest {
            lines.push(format!(
                "if (argc < {}) bi_error(\"expected at least {} arguments, got %d\", argc);",
                required, required
            ));
            lines.push(format!(
                "if (argc > {}) bi_error(\"expected at most {} arguments, got %d\", argc);",
                max_args, max_args
            ));
        } else {
            lines.push(format!(
                "if (argc < {}) bi_error(\"expected at least {} arguments, got %d\", argc);",
                required, required
            ));
        }

        for (i, p) in params.iter().enumerate() {
            let idx = i as i64;
            if p.rest {
                // Collect the remainder into an array.
                let t = self.fresh("rest");
                lines.push(format!("rak_val {} = rak_array_of((rak_val[]){{}}, 0);", t));
                lines.push(format!("for (int _i = {}; _i < argc; _i++) {{", idx));
                lines.push("  rak_val _a = rak_dup(args[_i]);".to_string());
                lines.push(format!("  {} = bi_push((rak_val[]){{{}, _a}}, 2);", t, t));
                lines.push("}".to_string());
                lines.push(format!("{} = rak_dup({});", self.c_ident(&p.name), t));
            } else if let Some(default) = &p.default {
                let d = self.value(default)?;
                lines.push(format!(
                    "if (argc > {}) {{ {} = rak_dup(args[{}]); }} else {{ {} = {}; }}",
                    idx,
                    self.c_ident(&p.name),
                    idx,
                    self.c_ident(&p.name),
                    d.expr
                ));
            } else if p.optional {
                lines.push(format!(
                    "if (argc > {}) {{ {} = rak_dup(args[{}]); }} else {{ {} = rak_nil(); }}",
                    idx,
                    self.c_ident(&p.name),
                    idx,
                    self.c_ident(&p.name)
                ));
            } else {
                lines.push(format!(
                    "{} = rak_dup(args[{}]);",
                    self.c_ident(&p.name),
                    idx
                ));
            }
        }
        Ok(lines.join("\n    "))
    }

    fn if_expr(
        &mut self,
        cond: &Expr,
        then_branch: &[Stmt],
        else_branch: Option<&[Stmt]>,
    ) -> Result<Emit, String> {
        let c = self.value(cond)?;
        let t = self.fresh("t");
        let mut pre = c.pre;
        pre.push("{".to_string());
        pre.push(format!("  rak_val _c = {};", c.expr));
        pre.push("  if (rak_truthy(_c)) {".to_string());
        pre.push("    rak_drop(_c);".to_string());
        pre.push(format!("    {}", self.block_value_stmts(then_branch, &t)?));
        if let Some(else_body) = else_branch {
            pre.push("  } else {".to_string());
            pre.push(format!("    {}", self.block_value_stmts(else_body, &t)?));
            pre.push("  }".to_string());
        } else {
            // An if without an else evaluates to nil — the only branch left.
            pre.push("  } else {".to_string());
            pre.push(format!("    {} = rak_nil();", t));
            pre.push("  }".to_string());
        }
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn match_expr(
        &mut self,
        value: &Expr,
        arms: &[(Pattern, Option<Expr>, Vec<Stmt>)],
    ) -> Result<Emit, String> {
        let v = self.value(value)?;
        let t = self.fresh("t");
        // Minted before the arms: each arm mints its own temps, so reading
        // `self.temp` at the goto/label sites gave them different numbers.
        let end_label = self.fresh("match_end");
        let mut pre = v.pre;
        pre.push("{".to_string());
        pre.push(format!("  rak_val _m = {};", v.expr));
        for (i, (pat, guard, body)) in arms.iter().enumerate() {
            pre.push("  {".to_string());
            pre.push("    int _ok = 1;".to_string());
            let mut binds = Vec::new();
            // `emit_pattern` writes to a buffer; this builder collects into
            // `pre`, which is what the value-producing match assembles.
            let mut pat_buf = String::new();
            self.emit_pattern(&mut pat_buf, pat, "_m", &mut binds)?;
            for line in pat_buf.lines() {
                pre.push(format!("    {}", line.trim_end()));
            }
            if let Some(g) = guard {
                let gv = self.value(g)?;
                pre.extend(gv.pre);
                pre.push(format!(
                    "    if (_ok) {{ if (!rak_truthy({})) _ok = 0; rak_drop({}); }}",
                    gv.expr, gv.expr
                ));
            }
            pre.push("    if (_ok) {".to_string());
            pre.push(format!("      {}", self.block_value_stmts(body, &t)?));
            pre.push("      rak_drop(_m);".to_string());
            pre.push(format!("      goto {};", end_label));
            pre.push("    }".to_string());
            // Bind drops are emitted by the scope exit: `emit_pattern`
            // declared each binding, so a manual drop here would free the
            // value twice.
            pre.push("  }".to_string());
            let _ = i;
        }
        pre.push("  rak_drop(_m);".to_string());
        pre.push(format!("  {}: ;", end_label));
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn block_expr(&mut self, body: &[Stmt]) -> Result<Emit, String> {
        let t = self.fresh("t");
        let mut pre = Vec::new();
        pre.push("{".to_string());
        pre.push(format!("  {}", self.block_value_stmts(body, &t)?));
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    /// Emit statements whose value is the last expression statement's value,
    /// assigned to `result`.
    fn block_value_stmts(&mut self, body: &[Stmt], result: &str) -> Result<String, String> {
        let mut out = String::new();
        self.push_scope();
        for (i, s) in body.iter().enumerate() {
            let is_last = i == body.len() - 1;
            if is_last {
                if let Stmt::Expr(e) = s {
                    let v = self.value(e)?;
                    v.flush(&mut out);
                    out.push_str(&format!("{} = {};\n", result, v.expr));
                    continue;
                }
            }
            self.emit_stmt(&mut out, s)?;
        }
        self.drop_scope(&mut out);
        Ok(out)
    }

    fn try_expr(&mut self, inner: &Expr) -> Result<Emit, String> {
        let v = self.value(inner)?;
        let t = self.fresh("t");
        let _pre = format!(
            "{{ {} = {}; {} = rak_call({}, 1, (rak_val[]){{{}}}); }}",
            t, v.expr, t, t, t
        );
        // `expr?` unwraps a Result/Option or raises. The runtime has no
        // TryUnwrap; approximate with a call to a helper.
        let pre = format!("{{ {} = {}; {} = rak_try_unwrap({}); }}", t, v.expr, t, t);
        Ok(Emit {
            pre: {
                let mut p = v.pre;
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn ternary(&mut self, cond: &Expr, then: &Expr, els: &Expr) -> Result<Emit, String> {
        let c = self.value(cond)?;
        let t = self.fresh("t");
        let mut pre = c.pre;
        pre.push("{".to_string());
        pre.push(format!("  rak_val _c = {};", c.expr));
        pre.push("  if (rak_truthy(_c)) {".to_string());
        pre.push("    rak_drop(_c);".to_string());
        let tv = self.value(then)?;
        pre.extend(tv.pre);
        pre.push(format!("    {} = {};", t, tv.expr));
        pre.push("  } else {".to_string());
        let ev = self.value(els)?;
        pre.extend(ev.pre);
        pre.push(format!("    {} = {};", t, ev.expr));
        pre.push("  }".to_string());
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn nil_coalesce(&mut self, a: &Expr, b: &Expr) -> Result<Emit, String> {
        let av = self.value(a)?;
        let t = self.fresh("t");
        let mut pre = av.pre;
        pre.push("{".to_string());
        pre.push(format!("  rak_val _a = {};", av.expr));
        pre.push("  if (_a.t != RAK_NIL && _a.t != RAK_NONE) {".to_string());
        pre.push(format!("    {} = rak_dup(_a);", t));
        pre.push("    rak_drop(_a);".to_string());
        pre.push("  } else {".to_string());
        pre.push("    rak_drop(_a);".to_string());
        let bv = self.value(b)?;
        pre.extend(bv.pre);
        pre.push(format!("    {} = {};", t, bv.expr));
        pre.push("  }".to_string());
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn opt_field(&mut self, obj: &Expr, field: &str) -> Result<Emit, String> {
        let o = self.value(obj)?;
        let t = self.fresh("t");
        let pre = format!(
            "{{ {} = {}; {} = rak_field({}, {}); }}",
            t,
            o.expr,
            t,
            t,
            Self::c_str_lit(field)
        );
        Ok(Emit {
            pre: {
                let mut p = o.pre;
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn opt_index(&mut self, obj: &Expr, idx: &Expr) -> Result<Emit, String> {
        let o = self.value(obj)?;
        let i = self.value(idx)?;
        let t = self.fresh("t");
        let iv = self.fresh("i");
        let pre = format!(
            "{{ {} = {}; {} = {}; {} = rak_index({}, {}); }}",
            t, o.expr, iv, i.expr, t, t, iv
        );
        Ok(Emit {
            pre: {
                let mut p = o.pre;
                p.extend(i.pre);
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn multi_assign(&mut self, targets: &[Expr], values: &[Expr]) -> Result<Emit, String> {
        if targets.len() != values.len() {
            return Err("multi-assign target/value count mismatch".to_string());
        }
        let mut pre = Vec::new();
        let mut val_exprs = Vec::new();
        for v in values {
            let ve = self.value(v)?;
            pre.extend(ve.pre);
            val_exprs.push(ve.expr);
        }
        for (i, t) in targets.iter().enumerate() {
            if let Expr::Ident(name) = t {
                let target = if self.is_local(name) {
                    self.c_ident(name)
                } else if self.is_global(name) {
                    self.c_global(name)
                } else {
                    return Err(format!("cannot assign to undefined variable '{}'", name));
                };
                pre.push(format!(
                    "rak_assign(&{}, rak_dup({}));",
                    target, val_exprs[i]
                ));
            } else {
                return Err("multi-assign targets must be identifiers".to_string());
            }
        }
        Ok(Emit {
            pre,
            expr: "rak_nil()".to_string(),
        })
    }

    fn path_value(&mut self, parts: &[String]) -> Result<Emit, String> {
        if parts.len() == 2 {
            // Enum variant without data: `Color::Red`.
            if let Some(variants) = self.support.enums.get(&parts[0]) {
                if variants.iter().any(|(v, n)| v == &parts[1] && *n == 0) {
                    return Ok(Emit::simple(format!(
                        "rak_enum_new({}, {}, NULL, 0)",
                        Self::c_str_lit(&parts[0]),
                        Self::c_str_lit(&parts[1])
                    )));
                }
            }
        }
        Err(format!("undefined path '{}'", parts.join("::")))
    }

    fn struct_lit(&mut self, name: &str, fields: &[(String, Expr)]) -> Result<Emit, String> {
        let t = self.fresh("t");
        let mut pre = Vec::new();
        pre.push(format!(
            "{{ {} = rak_struct_new({});",
            t,
            Self::c_str_lit(name)
        ));
        for (fname, fval) in fields {
            let fv = self.value(fval)?;
            pre.extend(fv.pre);
            pre.push(format!(
                "  {{ rak_val _f = {}; rak_map_set({}, {}, _f); }}",
                fv.expr,
                t,
                Self::c_str_lit(fname)
            ));
        }
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn as_cast(&mut self, inner: &Expr, ty: &Type) -> Result<Emit, String> {
        let v = self.value(inner)?;
        let t = self.fresh("t");
        let cast = match ty {
            Type::Int => format!("bi_int((rak_val[]){{{}}}, 1)", v.expr),
            Type::F64 => format!("bi_float((rak_val[]){{{}}}, 1)", v.expr),
            Type::String => format!("bi_string((rak_val[]){{{}}}, 1)", v.expr),
            Type::Bool => format!("rak_bool(rak_truthy({}))", v.expr),
            Type::Char => format!("rak_charv((unsigned int)rak_as_i({}))", v.expr),
            Type::Bytes => format!("bi_bytes((rak_val[]){{{}}}, 1)", v.expr),
            _ => return Err(format!("cast to {:?} is not supported by target 'c'", ty)),
        };
        let pre = format!("{{ {} = {}; }}", t, cast);
        Ok(Emit {
            pre: {
                let mut p = v.pre;
                p.push(pre);
                p
            },
            expr: format!("rak_dup({})", t),
        })
    }

    fn interp(&mut self, template: &str, parts: &[Expr]) -> Result<Emit, String> {
        // The interpreter's f-string: literal text + Display of each part,
        // with the spec inside `{...}` dropped.
        let t = self.fresh("t");
        let mut pre = Vec::new();
        pre.push(format!(
            "{{ {} = rak_strn({}, {});",
            t,
            Self::c_str_lit(""),
            0
        ));
        let mut char_indices = template.char_indices().peekable();
        let mut part_idx = 0;
        let mut lit_start = 0usize;
        let mut lit_len = 0usize;
        let flush_lit = |pre: &mut Vec<String>, lit: &str| {
            if !lit.is_empty() {
                pre.push(format!(
                    "{{ rak_val _l = rak_strn({}, {}); {} = rak_add({}, _l); }}",
                    Self::c_str_lit(lit),
                    lit.len(),
                    t,
                    t
                ));
            }
        };
        while let Some((i, c)) = char_indices.next() {
            if c == '{' {
                if char_indices.peek().map(|(_, c)| *c) == Some('{') {
                    char_indices.next();
                    lit_len += 2;
                    continue;
                }
                // Flush the literal before the placeholder.
                let lit = &template[lit_start..lit_start + lit_len];
                flush_lit(&mut pre, lit);
                // Skip to the closing brace.
                for (_, c2) in char_indices.by_ref() {
                    if c2 == '}' {
                        break;
                    }
                }
                if part_idx < parts.len() {
                    let pv = self.value(&parts[part_idx])?;
                    pre.extend(pv.pre);
                    pre.push(format!(
                        "{{ rak_val _p = rak_display({}); {} = rak_add({}, _p); }}",
                        pv.expr, t, t
                    ));
                    part_idx += 1;
                }
                lit_start = i + 1;
                lit_len = 0;
            } else if c == '}' {
                if char_indices.peek().map(|(_, c)| *c) == Some('}') {
                    char_indices.next();
                    lit_len += 2;
                } else {
                    lit_len += 1;
                }
            } else {
                lit_len += c.len_utf8();
            }
        }
        let lit = &template[lit_start..lit_start + lit_len];
        flush_lit(&mut pre, lit);
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn array_lit(&mut self, items: &[Expr]) -> Result<Emit, String> {
        let t = self.fresh("t");
        let mut pre = Vec::new();
        let mut exprs = Vec::new();
        for i in items {
            let ie = self.value(i)?;
            pre.extend(ie.pre);
            exprs.push(ie.expr);
        }
        let list = exprs.join(", ");
        let n = exprs.len();
        pre.push(format!(
            "{{ {} = rak_array_of((rak_val[]){{{}}}, {}); }}",
            t, list, n
        ));
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn tuple_lit(&mut self, items: &[Expr]) -> Result<Emit, String> {
        let t = self.fresh("t");
        let mut pre = Vec::new();
        let mut exprs = Vec::new();
        for i in items {
            let ie = self.value(i)?;
            pre.extend(ie.pre);
            exprs.push(ie.expr);
        }
        let list = exprs.join(", ");
        let n = exprs.len();
        pre.push(format!(
            "{{ {} = rak_seq(RAK_TUPLE, (rak_val[]){{{}}}, {}); }}",
            t, list, n
        ));
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    fn map_lit(&mut self, pairs: &[(Expr, Expr)]) -> Result<Emit, String> {
        let t = self.fresh("t");
        let mut pre = Vec::new();
        pre.push(format!("{{ {} = rak_map();", t));
        for (k, v) in pairs {
            let kv = self.value(k)?;
            let vv = self.value(v)?;
            pre.extend(kv.pre);
            pre.extend(vv.pre);
            pre.push(format!(
                "  {{ rak_val _k = {}; rak_val _v = {}; rak_map_set({}, rak_cstr(_k), _v); rak_drop(_k); }}",
                kv.expr, vv.expr, t
            ));
        }
        pre.push("}".to_string());
        Ok(Emit {
            pre,
            expr: format!("rak_dup({})", t),
        })
    }

    // ------------------------------------------------------------- patterns

    fn emit_pattern(
        &mut self,
        out: &mut String,
        pat: &Pattern,
        scrut: &str,
        binds: &mut Vec<(String, String)>,
    ) -> Result<(), String> {
        match pat {
            Pattern::Wild => {}
            Pattern::Ident(name) => {
                if name != "_" {
                    // A pattern binding is a scoped local at the point of
                    // the match/loop, declared alongside its assignment:
                    // unlike a `fresh()` temporary it lives only inside the
                    // arm or iteration that introduced it.
                    let b = self.c_ident(name);
                    self.declare(name);
                    binds.push((b.clone(), name.clone()));
                    out.push_str(&format!("rak_val {} = rak_dup({});\n", b, scrut));
                }
            }
            Pattern::Int(i) => {
                let t = self.fresh("t");
                out.push_str(&format!(
                    "{{ {} = {}; if (!rak_val_eq({}, rak_int({}))) _ok = 0; rak_drop({}); }}\n",
                    t, scrut, t, i, t
                ));
            }
            Pattern::Hex(h) => {
                let t = self.fresh("t");
                out.push_str(&format!(
                    "{{ {} = {}; if (!rak_val_eq({}, rak_hex(0x{:X}))) _ok = 0; rak_drop({}); }}\n",
                    t, scrut, t, h, t
                ));
            }
            Pattern::Byte(b) => {
                let t = self.fresh("t");
                out.push_str(&format!(
                    "{{ {} = {}; if (!rak_val_eq({}, rak_int({}))) _ok = 0; rak_drop({}); }}\n",
                    t, scrut, t, b, t
                ));
            }
            Pattern::String(s) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {}; if (!rak_val_eq({}, rak_strn({}, {}))) _ok = 0; rak_drop({}); }}\n", t, scrut, t, Self::c_str_lit(s), s.len(), t));
            }
            Pattern::Bool(b) => {
                let t = self.fresh("t");
                out.push_str(&format!(
                    "{{ {} = {}; if (!rak_val_eq({}, rak_bool({}))) _ok = 0; rak_drop({}); }}\n",
                    t,
                    scrut,
                    t,
                    if *b { 1 } else { 0 },
                    t
                ));
            }
            Pattern::Nil => {
                let t = self.fresh("t");
                out.push_str(&format!(
                    "{{ {} = {}; if (!rak_val_eq({}, rak_nil())) _ok = 0; rak_drop({}); }}\n",
                    t, scrut, t, t
                ));
            }
            Pattern::Tuple(pats) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!(
                    "{}if ({}.t != RAK_TUPLE || {}.u.o->u.seq.len != {}) _ok = 0;\n",
                    self.indent(),
                    t,
                    t,
                    pats.len()
                ));
                for (i, p) in pats.iter().enumerate() {
                    let sub = self.fresh("s");
                    out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                    out.push_str(&format!(
                        "{}  rak_val {} = rak_index({}, rak_int({}));\n",
                        self.indent(),
                        sub,
                        t,
                        i
                    ));
                    self.emit_pattern(out, p, &sub, binds)?;
                    out.push_str(&format!("{}  rak_drop({});\n", self.indent(), sub));
                    out.push_str(&format!("{}}}\n", self.indent()));
                }
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
            Pattern::Array(pats) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!(
                    "{}if ({}.t != RAK_ARRAY || {}.u.o->u.seq.len != {}) _ok = 0;\n",
                    self.indent(),
                    t,
                    t,
                    pats.len()
                ));
                for (i, p) in pats.iter().enumerate() {
                    let sub = self.fresh("s");
                    out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                    out.push_str(&format!(
                        "{}  rak_val {} = rak_index({}, rak_int({}));\n",
                        self.indent(),
                        sub,
                        t,
                        i
                    ));
                    self.emit_pattern(out, p, &sub, binds)?;
                    out.push_str(&format!("{}  rak_drop({});\n", self.indent(), sub));
                    out.push_str(&format!("{}}}\n", self.indent()));
                }
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
            Pattern::Bytes(pats) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!(
                    "{}if ({}.t != RAK_BYTES || {}.u.o->u.bytes.len < {}) _ok = 0;\n",
                    self.indent(),
                    t,
                    t,
                    pats.len()
                ));
                for (i, p) in pats.iter().enumerate() {
                    match p {
                        BytesPat::Byte(b) => {
                            out.push_str(&format!("{}if (_ok) {{ if ({}.u.o->u.bytes.len <= {} || {}.u.o->u.bytes.p[{}] != 0x{:02X}) _ok = 0; }}\n", self.indent(), t, i, t, i, b));
                        }
                        BytesPat::Rest => break,
                    }
                }
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
            Pattern::Struct(name, fields) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!("{}if ({}.t != RAK_STRUCT || {}.u.o->u.name == NULL || strcmp({}.u.o->u.name, {}) != 0) _ok = 0;\n", self.indent(), t, t, t, Self::c_str_lit(name)));
                for (fname, fp) in fields {
                    let sub = self.fresh("s");
                    out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                    out.push_str(&format!(
                        "{}  rak_val {} = rak_field({}, {});\n",
                        self.indent(),
                        sub,
                        t,
                        Self::c_str_lit(fname)
                    ));
                    self.emit_pattern(out, fp, &sub, binds)?;
                    out.push_str(&format!("{}  rak_drop({});\n", self.indent(), sub));
                    out.push_str(&format!("{}}}\n", self.indent()));
                }
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
            Pattern::EnumVariant(en, var, subs) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!("{}if ({}.t != RAK_ENUM || {}.u.o->u.enm.name == NULL || strcmp({}.u.o->u.enm.name, {}) != 0 || strcmp({}.u.o->u.enm.variant, {}) != 0 || {}.u.o->u.enm.len != {}) _ok = 0;\n", self.indent(), t, t, t, Self::c_str_lit(en), t, Self::c_str_lit(var), t, subs.len()));
                for (i, sp) in subs.iter().enumerate() {
                    let sub = self.fresh("s");
                    out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                    out.push_str(&format!(
                        "{}  rak_val {} = rak_index({}, rak_int({}));\n",
                        self.indent(),
                        sub,
                        t,
                        i
                    ));
                    self.emit_pattern(out, sp, &sub, binds)?;
                    out.push_str(&format!("{}  rak_drop({});\n", self.indent(), sub));
                    out.push_str(&format!("{}}}\n", self.indent()));
                }
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
            Pattern::Range(lo, hi) => {
                let lt = self.fresh("lo");
                let ht = self.fresh("hi");
                out.push_str(&format!(
                    "{{ {} = rak_int({:?}); rak_val {} = rak_int({:?});\n",
                    lt, lo, ht, hi
                ));
                out.push_str(&format!("{}if (_ok) {{ int _c1; int _c2; int _o1; int _o2; _c1 = rak_cmp({}, {}, &_o1); _c2 = rak_cmp({}, {}, &_o2); if (!_o1 || _c1 < 0 || !_o2 || _c2 > 0) _ok = 0; }}\n", self.indent(), scrut, lt, scrut, ht));
                out.push_str(&format!(
                    "{}rak_drop({}); rak_drop({});\n",
                    self.indent(),
                    lt,
                    ht
                ));
            }
            Pattern::Or(alts) => {
                for alt in alts {
                    out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                    out.push_str(&format!("{}  int _ok2 = 1;\n", self.indent()));
                    let mut alt_binds = Vec::new();
                    self.emit_pattern_alt(out, alt, scrut, &mut alt_binds)?;
                    out.push_str(&format!("{}  if (_ok2) {{ _ok = 1; }}\n", self.indent()));
                    for (name, _) in &alt_binds {
                        out.push_str(&format!("{}  rak_drop({});\n", self.indent(), name));
                    }
                    out.push_str(&format!("{}}}\n", self.indent()));
                }
            }
            Pattern::Some(inner) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!(
                    "{}if ({}.t != RAK_SOME) _ok = 0;\n",
                    self.indent(),
                    t
                ));
                let sub = self.fresh("s");
                out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                out.push_str(&format!(
                    "{}  rak_val {} = rak_index({}, rak_int(0));\n",
                    self.indent(),
                    sub,
                    t
                ));
                self.emit_pattern(out, inner, &sub, binds)?;
                out.push_str(&format!("{}  rak_drop({});\n", self.indent(), sub));
                out.push_str(&format!("{}}}\n", self.indent()));
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
            Pattern::None => {
                let t = self.fresh("t");
                out.push_str(&format!(
                    "{{ {} = {}; if (!rak_val_eq({}, rak_none())) _ok = 0; rak_drop({}); }}\n",
                    t, scrut, t, t
                ));
            }
            Pattern::Ok(inner) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!(
                    "{}if ({}.t != RAK_OK) _ok = 0;\n",
                    self.indent(),
                    t
                ));
                let sub = self.fresh("s");
                out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                out.push_str(&format!(
                    "{}  rak_val {} = rak_index({}, rak_int(0));\n",
                    self.indent(),
                    sub,
                    t
                ));
                self.emit_pattern(out, inner, &sub, binds)?;
                out.push_str(&format!("{}  rak_drop({});\n", self.indent(), sub));
                out.push_str(&format!("{}}}\n", self.indent()));
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
            Pattern::Err(inner) => {
                let t = self.fresh("t");
                out.push_str(&format!("{{ {} = {};\n", t, scrut));
                out.push_str(&format!(
                    "{}if ({}.t != RAK_ERR) _ok = 0;\n",
                    self.indent(),
                    t
                ));
                let sub = self.fresh("s");
                out.push_str(&format!("{}if (_ok) {{\n", self.indent()));
                out.push_str(&format!(
                    "{}  rak_val {} = rak_index({}, rak_int(0));\n",
                    self.indent(),
                    sub,
                    t
                ));
                self.emit_pattern(out, inner, &sub, binds)?;
                out.push_str(&format!("{}  rak_drop({});\n", self.indent(), sub));
                out.push_str(&format!("{}}}\n", self.indent()));
                out.push_str(&format!("{}rak_drop({});\n", self.indent(), t));
            }
        }
        Ok(())
    }

    /// Emit an Or-pattern alternative with its own ok flag.
    fn emit_pattern_alt(
        &mut self,
        out: &mut String,
        pat: &Pattern,
        scrut: &str,
        binds: &mut Vec<(String, String)>,
    ) -> Result<(), String> {
        // For Or patterns, each alternative must bind the same names. We
        // emit the alternative's test into a nested block with a local ok.
        match pat {
            Pattern::Ident(name) => {
                if name != "_" {
                    let b = self.c_ident(name);
                    self.declare(name);
                    binds.push((b.clone(), name.clone()));
                    out.push_str(&format!("{} = rak_dup({});\n", b, scrut));
                }
            }
            Pattern::Wild => {}
            _ => {
                // For non-ident alternatives in an Or, emit a test without
                // binding (the interpreter requires all alternatives to bind
                // the same names, so a non-ident alternative is an error).
                return Err("or-pattern alternatives must be identifiers".to_string());
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- statements

    fn emit_stmt(&mut self, out: &mut String, stmt: &Stmt) -> Result<(), String> {
        let ind = self.indent();
        match stmt {
            Stmt::Let { name, value, .. } => {
                let v = self.emit_expr(value, out)?;
                if self.is_local(name) {
                    out.push_str(&format!(
                        "{}rak_assign(&{}, {});\n",
                        ind,
                        self.c_ident(name),
                        v
                    ));
                } else {
                    // Top-level or function-level let: declare a local.
                    //
                    // The declaration needs the type. Without it this
                    // emitted a bare `v_x = ...`, which is an assignment
                    // to a variable that was never declared -- so the C
                    // compiler rejected the program, and *any* function
                    // with a local was unbuildable through this target.
                    // The two branches below are the two halves of one
                    // binding: this one declares the variable, the other
                    // assigns to it again.
                    self.declare(name);
                    out.push_str(&format!("{}rak_val {} = {};\n", ind, self.c_ident(name), v));
                }
                Ok(())
            }
            Stmt::Expr(e) => {
                let v = self.emit_expr(e, out)?;
                out.push_str(&format!("{{ rak_val _t = {}; rak_drop(_t); }}\n", v));
                Ok(())
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    let v = self.emit_expr(e, out)?;
                    out.push_str(&format!("{}rak_run_defers(_mark);\n", ind));
                    out.push_str(&format!("{}return {};\n", ind, v));
                } else {
                    out.push_str(&format!("{}rak_run_defers(_mark);\n", ind));
                    out.push_str(&format!("{}return rak_nil();\n", ind));
                }
                Ok(())
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.emit_expr(cond, out)?;
                let else_label = self.fresh("else");
                // Minted beside the else label, not read from `self.temp` at
                // use sites: the branches in between mint their own temps, so
                // the goto and its target would otherwise disagree on the
                // number.
                let end_label = self.fresh("if_end");
                out.push_str(&format!("{{ rak_val _c = {};\n", c));
                out.push_str(&format!(
                    "{}if (!rak_truthy(_c)) goto {};\n",
                    ind, else_label
                ));
                out.push_str(&format!("{}rak_drop(_c);\n", ind));
                self.push_scope();
                for s in then_branch {
                    self.emit_stmt(out, s)?;
                }
                self.drop_scope(out);
                if let Some(else_body) = else_branch {
                    out.push_str(&format!("{}goto {};\n", ind, end_label));
                    out.push_str(&format!("{}{}: ;\n", ind, else_label));
                    self.push_scope();
                    for s in else_body {
                        self.emit_stmt(out, s)?;
                    }
                    self.drop_scope(out);
                    out.push_str(&format!("{}{}: ;\n", ind, end_label));
                } else {
                    out.push_str(&format!("{}{}: ;\n", ind, else_label));
                }
                out.push_str(&format!("{}}}\n", ind));
                Ok(())
            }
            Stmt::While { cond, body, label } => {
                let loop_id = self.fresh("loop");
                let end_label = format!("{}_end", loop_id);
                let continue_label = format!("{}_continue", loop_id);
                self.loops.push(LoopCtx {
                    end_label: end_label.clone(),
                    continue_label: continue_label.clone(),
                    label: label.clone(),
                });
                out.push_str(&format!("{}{}: ;\n", ind, continue_label));
                let c = self.emit_expr(cond, out)?;
                out.push_str(&format!("{{ rak_val _c = {};\n", c));
                out.push_str(&format!(
                    "{}if (!rak_truthy(_c)) goto {};\n",
                    ind, end_label
                ));
                out.push_str(&format!("{}rak_drop(_c);\n", ind));
                self.push_scope();
                for s in body {
                    self.emit_stmt(out, s)?;
                }
                self.drop_scope(out);
                out.push_str(&format!("{}goto {};\n", ind, continue_label));
                out.push_str(&format!("{}{}: ;\n", ind, end_label));
                // Close the block the condition's `_c` lives in. Without it
                // every `while` left an unbalanced `{` and gcc reported the
                // error at the end of the whole file.
                out.push_str(&format!("{}}}\n", ind));
                self.loops.pop();
                Ok(())
            }
            Stmt::Loop { body, label } => {
                let loop_id = self.fresh("loop");
                let end_label = format!("{}_end", loop_id);
                let continue_label = format!("{}_continue", loop_id);
                self.loops.push(LoopCtx {
                    end_label: end_label.clone(),
                    continue_label: continue_label.clone(),
                    label: label.clone(),
                });
                out.push_str(&format!("{}{}: ;\n", ind, continue_label));
                self.push_scope();
                for s in body {
                    self.emit_stmt(out, s)?;
                }
                self.drop_scope(out);
                out.push_str(&format!("{}goto {};\n", ind, continue_label));
                out.push_str(&format!("{}{}: ;\n", ind, end_label));
                self.loops.pop();
                Ok(())
            }
            Stmt::For {
                pattern,
                iterable,
                body,
                label,
            } => {
                let it = self.value(iterable)?;
                for _line in &it.pre {
                    out.push_str(_line);
                    out.push('\n');
                }
                let loop_id = self.fresh("loop");
                let end_label = format!("{}_end", loop_id);
                let continue_label = format!("{}_continue", loop_id);
                self.loops.push(LoopCtx {
                    end_label: end_label.clone(),
                    continue_label: continue_label.clone(),
                    label: label.clone(),
                });
                out.push_str(&format!("{{ rak_val _it = {};\n", it.expr));
                // Materialize the iterable into an array, then index.
                let items = self.fresh("items");
                out.push_str(&format!("{}rak_val {} = rak_nil();\n", ind, items));
                out.push_str(&format!(
                    "{}if (_it.t == RAK_ARRAY || _it.t == RAK_TUPLE) {} = rak_dup(_it);\n",
                    ind, items
                ));
                out.push_str(&format!("{}else if (_it.t == RAK_STR) {{\n", ind));
                out.push_str(&format!(
                    "{}  size_t _n = rak_utf8_len(_it.u.o->u.str.p, _it.u.o->u.str.len);\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}  rak_val *_a = (rak_val *)malloc((_n ? _n : 1) * sizeof(*_a));\n",
                    ind
                ));
                out.push_str(&format!("{}  for (size_t _i = 0; _i < _n; _i++) {{\n", ind));
                out.push_str(&format!(
                    "{}    size_t _off = rak_utf8_off(_it.u.o->u.str.p, _it.u.o->u.str.len, _i);\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}    unsigned char _ch = (unsigned char)_it.u.o->u.str.p[_off];\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}    size_t _w = _ch < 0x80 ? 1 : (_ch < 0xE0 ? 2 : (_ch < 0xF0 ? 3 : 4));\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}    _a[_i] = rak_strn(_it.u.o->u.str.p + _off, _w);\n",
                    ind
                ));
                out.push_str(&format!("{}  }}\n", ind));
                out.push_str(&format!("{}  {} = rak_array_of(_a, _n);\n", ind, items));
                out.push_str(&format!(
                    "{}  free(_a); /* the array owns its copy */\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}}} else if (_it.t == RAK_MAP || _it.t == RAK_STRUCT) {{\n",
                    ind
                ));
                out.push_str(&format!("{}  size_t _n = _it.u.o->u.map.len;\n", ind));
                out.push_str(&format!(
                    "{}  rak_val *_a = (rak_val *)malloc((_n ? _n : 1) * sizeof(*_a));\n",
                    ind
                ));
                out.push_str(&format!("{}  for (size_t _i = 0; _i < _n; _i++) {{\n", ind));
                out.push_str(&format!(
                    "{}    rak_val _k = rak_str(_it.u.o->u.map.keys[_i]);\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}    rak_val _v = rak_dup(_it.u.o->u.map.vals[_i]);\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}    _a[_i] = rak_seq(RAK_TUPLE, (rak_val[]){{_k, _v}}, 2);\n",
                    ind
                ));
                out.push_str(&format!("{}  }}\n", ind));
                out.push_str(&format!("{}  {} = rak_array_of(_a, _n);\n", ind, items));
                out.push_str(&format!(
                    "{}  free(_a); /* the array owns its copy */\n",
                    ind
                ));
                out.push_str(&format!("{}}} else if (_it.t == RAK_BYTES) {{\n", ind));
                out.push_str(&format!("{}  size_t _n = _it.u.o->u.bytes.len;\n", ind));
                out.push_str(&format!(
                    "{}  rak_val *_a = (rak_val *)malloc((_n ? _n : 1) * sizeof(*_a));\n",
                    ind
                ));
                out.push_str(&format!("{}  for (size_t _i = 0; _i < _n; _i++) _a[_i] = rak_int(_it.u.o->u.bytes.p[_i]);\n", ind));
                out.push_str(&format!("{}  {} = rak_array_of(_a, _n);\n", ind, items));
                out.push_str(&format!(
                    "{}  free(_a); /* the array owns its copy */\n",
                    ind
                ));
                out.push_str(&format!("{}}} else if (_it.t == RAK_SOME) {{\n", ind));
                out.push_str(&format!(
                    "{}  rak_val *_a = (rak_val *)malloc(sizeof(*_a));\n",
                    ind
                ));
                out.push_str(&format!(
                    "{}  _a[0] = rak_dup(_it.u.o->u.seq.items[0]);\n",
                    ind
                ));
                out.push_str(&format!("{}  {} = rak_array_of(_a, 1);\n", ind, items));
                out.push_str(&format!(
                    "{}  free(_a); /* the array owns its copy */\n",
                    ind
                ));
                out.push_str(&format!("{}}} else {{\n", ind));
                out.push_str(&format!(
                    "{}  bi_error(\"Cannot iterate over this value\");\n",
                    ind
                ));
                out.push_str(&format!("{}}}\n", ind));
                out.push_str(&format!("{}rak_drop(_it);\n", ind));
                out.push_str(&format!("{}{}: ;\n", ind, continue_label));
                out.push_str(&format!(
                    "{}for (size_t _i = 0; _i < {}.u.o->u.seq.len; _i++) {{\n",
                    ind, items
                ));
                self.push_scope();
                let elem = self.fresh("elem");
                out.push_str(&format!(
                    "{}  {} = rak_dup({}.u.o->u.seq.items[_i]);\n",
                    ind, elem, items
                ));
                let mut binds = Vec::new();
                self.emit_pattern(out, pattern, &elem, &mut binds)?;
                for s in body {
                    self.emit_stmt(out, s)?;
                }
                // No manual bind drops: the scope exit drops every name
                // `emit_pattern` declared, and dropping here as well would
                // double-free.
                out.push_str(&format!("{}  rak_drop({});\n", ind, elem));
                self.drop_scope(out);
                out.push_str(&format!("{}}}\n", ind));
                out.push_str(&format!("{}rak_drop({});\n", ind, items));
                out.push_str(&format!("{}{}: ;\n", ind, end_label));
                out.push_str(&format!("{}}}\n", ind));
                self.loops.pop();
                Ok(())
            }
            Stmt::Match { value, arms } => {
                let v = self.value(value)?;
                for _line in &v.pre {
                    out.push_str(_line);
                    out.push('\n');
                }
                let end_label = self.fresh("match_end");
                out.push_str(&format!("{{ rak_val _m = {};\n", v.expr));
                for (pat, guard, body) in arms {
                    out.push_str(&format!("{}{{\n", ind));
                    out.push_str(&format!("{}  int _ok = 1;\n", ind));
                    let mut binds = Vec::new();
                    self.emit_pattern(out, pat, "_m", &mut binds)?;
                    if let Some(g) = guard {
                        let gv = self.value(g)?;
                        for _line in &gv.pre {
                            out.push_str(_line);
                            out.push('\n');
                        }
                        out.push_str(&format!(
                            "{}  if (_ok) {{ if (!rak_truthy({})) _ok = 0; rak_drop({}); }}\n",
                            ind, gv.expr, gv.expr
                        ));
                    }
                    out.push_str(&format!("{}  if (_ok) {{\n", ind));
                    self.push_scope();
                    for s in body {
                        self.emit_stmt(out, s)?;
                    }
                    self.drop_scope(out);
                    out.push_str(&format!("{}    rak_drop(_m);\n", ind));
                    out.push_str(&format!("{}    goto {};\n", ind, end_label));
                    out.push_str(&format!("{}  }}\n", ind));
                    // Bind drops happen at the scope exit this arm pushed.
                    out.push_str(&format!("{}}}\n", ind));
                }
                out.push_str(&format!("{}rak_drop(_m);\n", ind));
                out.push_str(&format!("{}{}: ;\n", ind, end_label));
                out.push_str(&format!("{}}}\n", ind));
                Ok(())
            }
            Stmt::Try {
                body,
                catch_name,
                catch_body,
            } => {
                let end_label = self.fresh("try_end");
                /* `setjmp` inline, not through a helper: longjmp restores the
                 * frame that ran setjmp, and a helper's frame is gone by the
                 * time the body raises. On Windows x64 the helper form is a
                 * hard crash (0xC0000028). Locals declared after this point
                 * (like `_e`) are fresh on the catch pass, so the classic
                 * clobbered-locals hazard does not apply to them. */
                out.push_str(&format!(
                    "{}if (setjmp(rak_catch_bufs[rak_catch_depth]) == 0) {{\n",
                    ind
                ));
                out.push_str(&format!("{}  rak_catch_depth++;\n", ind));
                self.push_scope();
                for s in body {
                    self.emit_stmt(out, s)?;
                }
                self.drop_scope(out);
                out.push_str(&format!("{}  rak_catch_depth--;\n", ind));
                out.push_str(&format!("{}  goto {};\n", ind, end_label));
                out.push_str(&format!("{}}} else {{\n", ind));
                out.push_str(&format!("{}  rak_val _e = rak_thrown;\n", ind));
                out.push_str(&format!("{}  rak_thrown = rak_nil();\n", ind));
                self.push_scope();
                if let Some(cn) = catch_name {
                    self.declare(cn);
                    // Declared here, at the binding: it is a scoped local of
                    // the catch arm, not a `fresh()` temporary.
                    out.push_str(&format!("{}  rak_val {} = _e;\n", ind, self.c_ident(cn)));
                } else {
                    out.push_str(&format!("{}  rak_drop(_e);\n", ind));
                }
                for s in catch_body {
                    self.emit_stmt(out, s)?;
                }
                self.drop_scope(out);
                out.push_str(&format!("{}}}\n", ind));
                out.push_str(&format!("{}{}: ;\n", ind, end_label));
                Ok(())
            }
            Stmt::Raise(e) => {
                let v = self.emit_expr(e, out)?;
                out.push_str(&format!("{}rak_raise({});\n", ind, v));
                Ok(())
            }
            Stmt::Defer(e) => {
                // `defer f(x)` registers a call to run at function exit.
                if let Expr::Call { callee, args, .. } = e.as_ref() {
                    let f = self.emit_expr(callee, out)?;
                    let mut arg_exprs = Vec::new();
                    for a in args {
                        arg_exprs.push(self.emit_expr(a, out)?);
                    }
                    let n = arg_exprs.len();
                    let list = arg_exprs.join(", ");
                    out.push_str(&format!(
                        "{{ rak_val _f = {}; rak_val _a[] = {{{}}}; rak_defer_register(_f, {}, _a); }}\n",
                        f, list, n
                    ));
                } else {
                    let v = self.emit_expr(e, out)?;
                    out.push_str(&format!(
                        "{{ rak_val _f = {}; rak_defer_register(_f, 0, NULL); }}\n",
                        v
                    ));
                }
                Ok(())
            }
            Stmt::Dump { value, target } => {
                let v = self.emit_expr(value, out)?;
                let t = match target {
                    Some(t) => self.emit_expr(t, out)?,
                    None => "rak_nil()".to_string(),
                };
                out.push_str(&format!(
                    "{{ rak_val _a[] = {{{}, {}}}; rak_drop(bi_dump(_a, 2)); }}\n",
                    v, t
                ));
                Ok(())
            }
            Stmt::Trace { .. } => {
                // `trace` is a no-op on the C target.
                Ok(())
            }
            Stmt::Break(target) => {
                let label = self.break_label(target)?;
                out.push_str(&format!("{}goto {};\n", ind, label));
                Ok(())
            }
            Stmt::Continue(target) => {
                let label = self.continue_label(target)?;
                out.push_str(&format!("{}goto {};\n", ind, label));
                Ok(())
            }
            Stmt::Const { name, value } => {
                // A compile-time constant: evaluate at runtime as a let.
                let v = self.emit_expr(value, out)?;
                self.declare(name);
                out.push_str(&format!("{}{} {} = {};\n", ind, "", self.c_ident(name), v));
                Ok(())
            }
            Stmt::Assert(e) => {
                let v = self.emit_expr(e, out)?;
                out.push_str(&format!(
                    "{{ rak_val _a = {}; if (!rak_truthy(_a)) {{ fprintf(stderr, \"assertion failed\\n\"); exit(1); }} rak_drop(_a); }}\n",
                    v
                ));
                Ok(())
            }
            Stmt::Unsafe { body, .. } => {
                self.push_scope();
                for s in body {
                    self.emit_stmt(out, s)?;
                }
                self.drop_scope(out);
                Ok(())
            }
            Stmt::Struct { .. } | Stmt::Enum { .. } | Stmt::TypeAlias { .. } => {
                // Type declarations are compile-time only.
                Ok(())
            }
            _ => Err("statement rejected by the support check".to_string()),
        }
    }

    fn break_label(&self, target: &Option<BreakTarget>) -> Result<String, String> {
        match target {
            None => {
                if let Some(l) = self.loops.last() {
                    Ok(l.end_label.clone())
                } else {
                    Err("break outside a loop".to_string())
                }
            }
            Some(BreakTarget::Depth(n)) => {
                let idx = self
                    .loops
                    .len()
                    .checked_sub(*n as usize)
                    .ok_or("break depth exceeds loop nesting")?;
                Ok(self.loops[idx].end_label.clone())
            }
            Some(BreakTarget::Label(l)) => {
                for ctx in self.loops.iter().rev() {
                    if ctx.label.as_deref() == Some(l.as_str()) {
                        return Ok(ctx.end_label.clone());
                    }
                }
                Err(format!("unknown loop label '{}'", l))
            }
        }
    }

    fn continue_label(&self, target: &Option<BreakTarget>) -> Result<String, String> {
        match target {
            None => {
                if let Some(l) = self.loops.last() {
                    Ok(l.continue_label.clone())
                } else {
                    Err("continue outside a loop".to_string())
                }
            }
            Some(BreakTarget::Depth(n)) => {
                let idx = self
                    .loops
                    .len()
                    .checked_sub(*n as usize)
                    .ok_or("continue depth exceeds loop nesting")?;
                Ok(self.loops[idx].continue_label.clone())
            }
            Some(BreakTarget::Label(l)) => {
                for ctx in self.loops.iter().rev() {
                    if ctx.label.as_deref() == Some(l.as_str()) {
                        return Ok(ctx.continue_label.clone());
                    }
                }
                Err(format!("unknown loop label '{}'", l))
            }
        }
    }

    // ------------------------------------------------------------- captures

    fn collect_captures_stmts(
        &mut self,
        stmts: &[Stmt],
        params: &HashSet<&str>,
        out: &mut Vec<String>,
    ) {
        for s in stmts {
            self.collect_captures_stmt(s, params, out);
        }
    }

    fn collect_captures_stmt(
        &mut self,
        stmt: &Stmt,
        params: &HashSet<&str>,
        out: &mut Vec<String>,
    ) {
        match stmt {
            Stmt::Let { name, value, .. } => {
                self.collect_captures_expr(value, params, out);
                // The name itself is bound, not captured.
                let _ = name;
            }
            Stmt::Expr(e) | Stmt::Dump { value: e, .. } | Stmt::Trace { value: e } => {
                self.collect_captures_expr(e, params, out)
            }
            Stmt::Return(Some(e)) | Stmt::Raise(e) | Stmt::Defer(e) | Stmt::Assert(e) => {
                self.collect_captures_expr(e, params, out)
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.collect_captures_expr(cond, params, out);
                for s in then_branch {
                    self.collect_captures_stmt(s, params, out);
                }
                if let Some(b) = else_branch {
                    for s in b {
                        self.collect_captures_stmt(s, params, out);
                    }
                }
            }
            Stmt::While { cond, body, .. } => {
                self.collect_captures_expr(cond, params, out);
                for s in body {
                    self.collect_captures_stmt(s, params, out);
                }
            }
            Stmt::Loop { body, .. } => {
                for s in body {
                    self.collect_captures_stmt(s, params, out);
                }
            }
            Stmt::For { iterable, body, .. } => {
                self.collect_captures_expr(iterable, params, out);
                for s in body {
                    self.collect_captures_stmt(s, params, out);
                }
            }
            Stmt::Match { value, arms } => {
                self.collect_captures_expr(value, params, out);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.collect_captures_expr(g, params, out);
                    }
                    for s in body {
                        self.collect_captures_stmt(s, params, out);
                    }
                }
            }
            Stmt::Try {
                body, catch_body, ..
            } => {
                for s in body {
                    self.collect_captures_stmt(s, params, out);
                }
                for s in catch_body {
                    self.collect_captures_stmt(s, params, out);
                }
            }
            Stmt::Const { value, .. } => self.collect_captures_expr(value, params, out),
            Stmt::Unsafe { body, .. } => {
                for s in body {
                    self.collect_captures_stmt(s, params, out);
                }
            }
            _ => {}
        }
    }

    fn collect_captures_expr(&mut self, e: &Expr, params: &HashSet<&str>, out: &mut Vec<String>) {
        match e {
            Expr::Ident(name) => {
                // Only a *local* of the enclosing function needs capturing. A
                // global is read from its own C variable when the closure
                // runs, so capturing it would freeze a snapshot of it at
                // closure-creation time -- and a closure that read a global
                // would see a different value from the interpreter, which
                // reads the global when the closure is called.
                if !params.contains(name.as_str()) && self.is_local(name) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::Unary(_, e) | Expr::FieldAccess(e, _) | Expr::Raise(e) => {
                self.collect_captures_expr(e, params, out)
            }
            Expr::Binary(_, a, b) | Expr::Index(a, b) => {
                self.collect_captures_expr(a, params, out);
                self.collect_captures_expr(b, params, out);
            }
            Expr::Assign(_, e) | Expr::CompoundAssign(_, _, e) => {
                self.collect_captures_expr(e, params, out)
            }
            Expr::IndexAssign { obj, idx, value } => {
                self.collect_captures_expr(obj, params, out);
                self.collect_captures_expr(idx, params, out);
                self.collect_captures_expr(value, params, out);
            }
            Expr::FieldAssign { obj, value, .. } => {
                self.collect_captures_expr(obj, params, out);
                self.collect_captures_expr(value, params, out);
            }
            Expr::Call { callee, args, .. } => {
                self.collect_captures_expr(callee, params, out);
                for a in args {
                    self.collect_captures_expr(a, params, out);
                }
            }
            // A nested function's captures belong to its own env, not the
            // enclosing function's. The nested function value collects them
            // when it is emitted.
            Expr::Function { .. } | Expr::Lambda { .. } => {}
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.collect_captures_expr(cond, params, out);
                for s in then_branch {
                    self.collect_captures_stmt(s, params, out);
                }
                if let Some(b) = else_branch {
                    for s in b {
                        self.collect_captures_stmt(s, params, out);
                    }
                }
            }
            Expr::Match { value, arms } => {
                self.collect_captures_expr(value, params, out);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.collect_captures_expr(g, params, out);
                    }
                    for s in body {
                        self.collect_captures_stmt(s, params, out);
                    }
                }
            }
            Expr::Block(body) => self.collect_captures_stmts(body, params, out),
            Expr::TryExpr(inner) => self.collect_captures_expr(inner, params, out),
            Expr::As(inner, _) => self.collect_captures_expr(inner, params, out),
            Expr::Ternary { cond, then, els } => {
                self.collect_captures_expr(cond, params, out);
                self.collect_captures_expr(then, params, out);
                self.collect_captures_expr(els, params, out);
            }
            Expr::NilCoalesce(a, b) => {
                self.collect_captures_expr(a, params, out);
                self.collect_captures_expr(b, params, out);
            }
            Expr::OptField(a, _) | Expr::OptIndex(a, _) => {
                self.collect_captures_expr(a, params, out)
            }
            Expr::MultiAssign { targets, values } => {
                for t in targets {
                    self.collect_captures_expr(t, params, out);
                }
                for v in values {
                    self.collect_captures_expr(v, params, out);
                }
            }
            Expr::Range(lo, hi) => {
                if let Some(e) = lo {
                    self.collect_captures_expr(e, params, out);
                }
                if let Some(e) = hi {
                    self.collect_captures_expr(e, params, out);
                }
            }
            Expr::Interp { parts, .. } => {
                for p in parts {
                    self.collect_captures_expr(p, params, out);
                }
            }
            Expr::Array(items) | Expr::Tuple(items) => {
                for i in items {
                    self.collect_captures_expr(i, params, out);
                }
            }
            Expr::Map(pairs) => {
                for (k, v) in pairs {
                    self.collect_captures_expr(k, params, out);
                    self.collect_captures_expr(v, params, out);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.collect_captures_expr(v, params, out);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn is_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "len"
            | "string"
            | "int"
            | "float"
            | "bytes"
            | "upper"
            | "lower"
            | "trim"
            | "trim_start"
            | "trim_end"
            | "replace"
            | "find"
            | "starts_with"
            | "ends_with"
            | "split"
            | "join"
            | "contains"
            | "slice"
            | "push"
            | "keys"
            | "values"
            | "has"
            | "get"
            | "set"
            | "ord"
            | "chr"
            | "hex_encode"
            | "hex_decode"
            | "to_hex"
            | "from_hex"
            | "abs"
            | "sqrt"
            | "pow"
            | "min"
            | "max"
            | "sum"
            | "clamp"
            | "sort"
            | "sort_by"
            | "reverse"
            | "fmt"
            | "is_nil"
            | "type_of"
            | "print"
            | "dump"
    )
}

fn is_method_builtin(name: &str) -> bool {
    matches!(
        name,
        "len"
            | "string"
            | "int"
            | "float"
            | "upper"
            | "lower"
            | "trim"
            | "trim_start"
            | "trim_end"
            | "replace"
            | "find"
            | "starts_with"
            | "ends_with"
            | "split"
            | "join"
            | "contains"
            | "slice"
            | "push"
            | "keys"
            | "values"
            | "has"
            | "get"
            | "set"
            | "ord"
            | "chr"
            | "hex_encode"
            | "hex_decode"
            | "to_hex"
            | "from_hex"
            | "abs"
            | "sqrt"
            | "pow"
            | "min"
            | "max"
            | "sum"
            | "clamp"
            | "sort"
            | "sort_by"
            | "reverse"
            | "is_nil"
            | "type_of"
    )
}

fn fmt_float(f: f64) -> String {
    if f.is_nan() {
        return "0.0/0.0".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "1.0/0.0".to_string()
        } else {
            "-1.0/0.0".to_string()
        };
    }
    if f == f.trunc() && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    // Try increasing precision with scientific notation, checking round-trip.
    for prec in 1..=17 {
        let s = format!("{:.*e}", prec, f);
        if s.parse::<f64>() == Ok(f) {
            return s;
        }
    }
    format!("{:?}", f)
}

/// Translate a module to a complete C11 translation unit.
pub fn translate(module: &Module, opts: &CodegenOptions) -> Result<String, String> {
    let support = Support::check(module)?;
    let mut gen = Gen::new(support);

    // First pass: collect top-level function declarations.
    for item in &module.items {
        if let Stmt::Let { name, value, .. } = item {
            if let Expr::Function { params, .. } = value.as_ref() {
                let cname = gen.fresh_fn();
                let arity = params
                    .iter()
                    .filter(|p| !p.rest && p.default.is_none() && !p.optional)
                    .count() as i64;
                let arity = if params
                    .iter()
                    .any(|p| p.rest || p.default.is_some() || p.optional)
                {
                    -1
                } else {
                    arity
                };
                gen.fn_names.insert(name.clone(), cname);
                gen.fn_arity.insert(name.clone(), arity);
                if name == "main" {
                    gen.has_main = true;
                }
            }
        }
    }

    // Second pass: emit top-level items.
    for item in &module.items {
        match item {
            Stmt::Let { name, value, .. } => {
                // `let f = fn(...)` binds a function value to a name, and
                // calls through that name resolve to the C name the pre-pass
                // reserved for it. The fn expression itself is anonymous, so
                // thread the let's name into it before emitting: otherwise
                // the definition minted its own fresh name and every call
                // site referenced a function that was never defined.
                let value_owned: Box<Expr> = if let Expr::Function { .. } = value.as_ref() {
                    let mut named = (**value).clone();
                    if let Expr::Function { name: fn_name, .. } = &mut named {
                        if fn_name.is_none() {
                            *fn_name = Some(name.clone());
                        }
                    }
                    Box::new(named)
                } else {
                    value.clone()
                };
                let value = &value_owned;
                // The value is emitted into the toplevel buffer so its
                // prelude is part of the toplevel, then assigned to the
                // global. A function value also emits its definition.
                let mut scratch = std::mem::take(&mut gen.out);
                let v = gen.emit_expr(value, &mut scratch)?;
                if !scratch.is_empty() {
                    gen.toplevel_body.push(scratch.trim().to_string());
                }
                gen.toplevel_body
                    .push(format!("    rak_assign(&{}, {});", gen.c_global(name), v));
                gen.globals.insert(name.clone());
                gen.global_decls
                    .push(format!("static rak_val {};", gen.c_global(name)));
            }
            _ => {
                // Emit into a scratch buffer: `emit_stmt` needs `&mut gen`
                // while writing to the output, which is a field of `gen`.
                let mut scratch = std::mem::take(&mut gen.out);
                gen.emit_stmt(&mut scratch, item)?;
                if !scratch.trim().is_empty() {
                    gen.toplevel_body.push(scratch.trim().to_string());
                }
            }
        }
    }

    // Build the final C file.
    let mut c = String::new();
    c.push_str("/* Generated by rakc — do not edit. */\n\n");
    // The panic strategy is a property of the *source*, not of the command
    // line that compiles it. The runtime honours it at the raise site: with
    // `panic = abort` a `raise` with no catch exits immediately instead of
    // unwinding to `main`.
    //
    // It used to be a `-DRAK_PANIC_ABORT` flag on the compiler's command
    // line, which meant the C source was only correct for the one command
    // that produced it: the conformance harness compiles the source directly
    // and silently got the other behaviour, and the wasm build did too.
    if matches!(opts.panic, super::super::PanicStrategy::Abort) {
        c.push_str("#define RAK_PANIC_ABORT 1\n");
    }
    c.push_str(RUNTIME_C);
    c.push_str("\n\n");

    // Global declarations.
    for decl in &gen.global_decls {
        c.push_str(decl);
        c.push('\n');
    }
    if !gen.global_decls.is_empty() {
        c.push('\n');
    }

    // Function definitions.
    for def in &gen.fn_defs {
        c.push_str(def);
        c.push('\n');
    }

    // Toplevel function.
    c.push_str("static rak_val rak_toplevel(void) {\n");
    c.push_str("    int _mark = rak_defer_mark();\n");
    // The temporaries the toplevel minted, declared before the code that
    // assigns them.
    for d in &gen.pending_decls {
        c.push_str("    ");
        c.push_str(d);
        c.push('\n');
    }
    for line in &gen.toplevel_body {
        c.push_str("    ");
        c.push_str(line);
        c.push('\n');
    }
    c.push_str("    rak_run_defers(_mark);\n");
    c.push_str("    return rak_nil();\n");
    c.push_str("}\n\n");

    // Main.
    if gen.has_main {
        c.push_str("int main(int argc, char **argv) {\n");
        c.push_str("    rak_startup(rak_toplevel, argc, argv, ");
        c.push_str(&gen.c_global("main"));
        c.push_str(", 1);\n");
        c.push_str("    return 0;\n");
        c.push_str("}\n");
    } else {
        c.push_str("int main(int argc, char **argv) {\n");
        c.push_str("    rak_startup(rak_toplevel, argc, argv, rak_nil(), 0);\n");
        c.push_str("    return 0;\n");
        c.push_str("}\n");
    }

    Ok(c)
}
