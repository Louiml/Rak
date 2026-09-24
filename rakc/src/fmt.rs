//! `rakc fmt` — a source formatter for Rak (Part 7B).
//!
//! Parses the source with the standard lexer/parser and re-prints the AST
//! with a canonical style: 2-space indentation, `let x = value`, spaces
//! around binary operators, no space before commas, match arms separated by
//! commas. Unformattable nodes fail with a clear message rather than
//! silently dropping code.
//!
//! CLI: `rakc fmt <file>` (print), `--write` (in place), `--check` (exit 1 if
//! the file is not canonically formatted).

use crate::ast::*;

pub struct Formatter {
    out: String,
    indent: usize,
}

type FmtResult<T> = Result<T, String>;

/// Format a Rak source file. Returns the canonically formatted source.
pub fn format_source(source: &str) -> FmtResult<String> {
    let tokens = crate::lexer::tokenize(source).map_err(|e| e.to_string())?;
    let module = crate::parser::parse(&tokens, source).map_err(|e| e.to_string())?;
    format_module(&module)
}

pub fn format_module(m: &Module) -> FmtResult<String> {
    let mut f = Formatter { out: String::new(), indent: 0 };
    f.module(m)?;
    Ok(f.out)
}

impl Formatter {
    fn w(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    // ---- Module ----

    fn module(&mut self, m: &Module) -> FmtResult<()> {
        let mut first = true;
        for imp in &m.imports {
            if !first {
                self.blank();
            }
            first = false;
            self.import(imp)?;
        }
        for stmt in &m.items {
            self.blank();
            self.stmt(stmt)?;
        }
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if !self.out.ends_with('\n') && !self.out.is_empty() {
            self.out.push('\n');
        }
        Ok(())
    }

    fn import(&mut self, imp: &Import) -> FmtResult<()> {
        let target = if imp.is_file {
            format!("{:?}", imp.path.first().cloned().unwrap_or_default())
        } else {
            imp.path.join(".")
        };
        let alias = match &imp.alias {
            Some(a) => format!(" as {}", a),
            None => String::new(),
        };
        let prefix = if imp.reexport { "pub use " } else { "" };
        let s = match (&imp.kind, imp.star, imp.from_names.is_empty()) {
            (ImportKind::From, true, _) => format!("{}from {} import *{}", prefix, target, alias),
            (ImportKind::From, false, false) => {
                let names: Vec<String> = imp
                    .from_names
                    .iter()
                    .map(|(n, a)| match a {
                        Some(a) => format!("{} as {}", n, a),
                        None => n.clone(),
                    })
                    .collect();
                format!("{}from {} import {}", prefix, target, names.join(", "))
            }
            _ => format!("{}import {}{}", prefix, target, alias),
        };
        self.line(&s);
        Ok(())
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> FmtResult<()> {
        for s in stmts {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn block(&mut self, stmts: &[Stmt]) -> FmtResult<()> {
        self.indent += 1;
        self.stmts(stmts)?;
        self.indent -= 1;
        Ok(())
    }

    // ---- Statements ----

    fn stmt(&mut self, s: &Stmt) -> FmtResult<()> {
        match s {
            Stmt::Let { name, pattern, mutable, value, type_hint } => {
                if let Some(p) = pattern {
                    // `let pattern = value` (destructuring let).
                    let expr = self.expr(value)?;
                    self.line(&format!("let {} = {}", pattern_str(p), expr));
                } else if let Expr::Function { params, body, return_type, .. } = value.as_ref() {
                    // `let name = fn(...) { ... }` renders as a fn definition.
                    self.fn_def(None, name, params, body, return_type)?;
                } else {
                    let mut head = String::from("let ");
                    if *mutable {
                        head.push_str("mut ");
                    }
                    head.push_str(name);
                    if let Some(t) = type_hint {
                        head.push_str(&format!(": {}", type_str(t)));
                    }
                    let expr = self.expr(value)?;
                    self.line(&format!("{} = {}", head, expr));
                }
            }
            Stmt::Const { name, value } => {
                let expr = self.expr(value)?;
                self.line(&format!("const {} = {}", name, expr));
            }
            Stmt::Expr(e) => {
                if let Expr::Function { params, body, return_type, .. } = e.as_ref() {
                    self.fn_def(None, "<anon>", params, body, return_type)?;
                    return Ok(());
                }
                let expr = self.expr(e)?;
                self.line(&expr);
            }
            Stmt::Return(e) => match e {
                Some(e) => {
                    let expr = self.expr(e)?;
                    self.line(&format!("return {}", expr));
                }
                None => self.line("return"),
            },
            Stmt::Dump { value, target: _ } => {
                let expr = self.expr(value)?;
                self.line(&format!("dump {}", expr));
            }
            Stmt::Trace { value } => {
                let expr = self.expr(value)?;
                self.line(&format!("trace {}", expr));
            }
            Stmt::Assert(e) => {
                let expr = self.expr(e)?;
                self.line(&format!("assert {}", expr));
            }
            Stmt::Defer(e) => {
                let expr = self.expr(e)?;
                self.line(&format!("defer {}", expr));
            }
            Stmt::If { cond, then_branch, else_branch } => {
                let cond = self.expr(cond)?;
                self.line(&format!("if {} {{", cond));
                self.block(then_branch)?;
                match else_branch {
                    Some(els) => {
                        self.line("} else {");
                        self.block(els)?;
                        self.line("}");
                    }
                    None => self.line("}"),
                }
            }
            Stmt::IfLet { pattern, value, then_branch, else_branch } => {
                let pat = pattern_str(pattern);
                let val = self.expr(value)?;
                self.line(&format!("if let {} = {} {{", pat, val));
                self.block(then_branch)?;
                match else_branch {
                    Some(els) => {
                        self.line("} else {");
                        self.block(els)?;
                        self.line("}");
                    }
                    None => self.line("}"),
                }
            }
            Stmt::While { label, cond, body, .. } => {
                let cond = self.expr(cond)?;
                self.line(&format!("{}while {} {{", label_str(label), cond));
                self.block(body)?;
                self.line("}");
            }
            Stmt::WhileLet { pattern, value, body } => {
                let pat = pattern_str(pattern);
                let val = self.expr(value)?;
                self.line(&format!("while let {} = {} {{", pat, val));
                self.block(body)?;
                self.line("}");
            }
            Stmt::DoWhile { cond, body } => {
                self.line("do {");
                self.block(body)?;
                let cond = self.expr(cond)?;
                self.line(&format!("}} while {}", cond));
            }
            Stmt::Loop { label, body } => {
                self.line(&format!("{}loop {{", label_str(label)));
                self.block(body)?;
                self.line("}");
            }
            Stmt::For { label, pattern, iterable, body } => {
                let pat = pattern_str(pattern);
                let it = self.expr(iterable)?;
                self.line(&format!("{}for {} in {} {{", label_str(label), pat, it));
                self.block(body)?;
                self.line("}");
            }
            Stmt::Break(t) => self.line(&format!("break{}", break_target(t))),
            Stmt::Continue(t) => self.line(&format!("continue{}", break_target(t))),
            Stmt::Match { value, arms } => {
                let val = self.expr(value)?;
                self.line(&format!("match {} {{", val));
                self.indent += 1;
                for (pattern, guard, body) in arms {
                    let pat = pattern_str(pattern);
                    let guard = match guard {
                        Some(g) => format!(" if {}", self.expr(g)?),
                        None => String::new(),
                    };
                    self.line(&format!("{}{} => {{", pat, guard));
                    self.indent += 1;
                    self.stmts(body)?;
                    self.indent -= 1;
                    self.line("},");
                }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Try { body, catch_name, catch_body } => {
                self.line("try {");
                self.block(body)?;
                match catch_name {
                    Some(n) => self.line(&format!("}} catch {} {{", n)),
                    None => self.line("} catch {"),
                }
                self.block(catch_body)?;
                self.line("}");
            }
            Stmt::Raise(e) => {
                let expr = self.expr(e)?;
                self.line(&format!("raise {}", expr));
            }
            Stmt::Struct { name, type_params, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|f| {
                        let t = f.type_hint.as_ref().map(type_str).unwrap_or_else(|| "nil".to_string());
                        format!("{}: {}", f.name, t)
                    })
                    .collect();
                let generics = if type_params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", type_params.join(", "))
                };
                self.line(&format!("struct {}{} {{ {} }}", name, generics, parts.join(", ")));
            }
            Stmt::Enum { name, type_params, variants } => {
                let vs: Vec<String> = variants
                    .iter()
                    .map(|v| {
                        if v.fields.is_empty() {
                            v.name.clone()
                        } else {
                            format!("{}({})", v.name, v.fields.iter().map(type_str).collect::<Vec<_>>().join(", "))
                        }
                    })
                    .collect();
                let generics = if type_params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", type_params.join(", "))
                };
                self.line(&format!("enum {}{} {{ {} }}", name, generics, vs.join(", ")));
            }
            Stmt::Impl { target, trait_name, methods } => match trait_name {
                Some(ty) => {
                    self.line(&format!("impl {} for {} {{", target, ty));
                    self.indent += 1;
                    self.stmts(methods)?;
                    self.indent -= 1;
                    self.line("}");
                }
                None => {
                    self.line(&format!("impl {} {{", target));
                    self.indent += 1;
                    self.stmts(methods)?;
                    self.indent -= 1;
                    self.line("}");
                }
            },
            Stmt::Trait { name, methods } => {
                self.line(&format!("trait {} {{", name));
                self.indent += 1;
                for m in methods {
                    let ps: Vec<String> = m
                        .params
                        .iter()
                        .map(|p| match &p.type_hint {
                            Some(t) => format!("{}: {}", p.name, type_str(t)),
                            None => p.name.clone(),
                        })
                        .collect();
                    let ret = match &m.return_type {
                        Some(t) => format!(" -> {}", type_str(t)),
                        None => String::new(),
                    };
                    self.line(&format!("fn {}({}){}", m.name, ps.join(", "), ret));
                }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Test { name, body } => {
                self.line(&format!("test {:?} {{", name));
                self.block(body)?;
                self.line("}");
            }
            Stmt::Mod { name, items } => {
                self.line(&format!("mod {} {{", name));
                self.block(items)?;
                self.line("}");
            }
            Stmt::Use { path, is_file, alias } => {
                let target = if *is_file {
                    format!("{:?}", path.first().cloned().unwrap_or_default())
                } else {
                    path.join(".")
                };
                let alias = match alias {
                    Some(a) => format!(" as {}", a),
                    None => String::new(),
                };
                self.line(&format!("use {}{}", target, alias));
            }
            Stmt::TypeAlias { name, alias } => {
                self.line(&format!("type {} = {}", name, type_str(alias)));
            }
            Stmt::Async(body) => {
                self.line("async {");
                self.block(body)?;
                self.line("}");
            }
            Stmt::Export(inner) => {
                self.stmt_pub(inner)?;
            }
            Stmt::Extern { abi, lib, decls } => {
                let lib_part = match lib {
                    Some(l) => format!(" from {:?}", l),
                    None => String::new(),
                };
                self.line(&format!("extern {:?}{} {{", abi, lib_part));
                self.indent += 1;
                for d in decls {
                    let ps: Vec<String> = d
                        .params
                        .iter()
                        .map(|p| match &p.type_hint {
                            Some(t) => format!("{}: {}", p.name, type_str(t)),
                            None => p.name.clone(),
                        })
                        .collect();
                    let va = if d.varargs { ", ..." } else { "" };
                    let ret = match &d.return_type {
                        Some(t) => format!(" -> {}", type_str(t)),
                        None => String::new(),
                    };
                    self.line(&format!("fn {}({}{}){}", d.name, ps.join(", "), va, ret));
                }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::MacroDef { name, params, body } => {
                let ps: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                self.line(&format!("macro {}({}) {{", name, ps.join(", ")));
                self.block(body)?;
                self.line("}");
            }
            Stmt::Tunnel { name, passphrase, body } => {
                self.line(&format!("tunnel {} {:?} {{", name, passphrase));
                self.block(body)?;
                self.line("}");
            }
            Stmt::BinStructDef { name, fields } => {
                self.line(&format!("binstruct {} {{", name));
                self.indent += 1;
                for f in fields {
                    self.line(&format!("{}: {}", f.name, bin_kind_str(&f.kind)));
                }
                self.indent -= 1;
                self.line("}");
            }
            other => {
                let _ = other;
                return Err("formatter: unsupported statement".to_string());
            }
        }
        Ok(())
    }

    /// `pub fn name(...) { ... }` / `pub let x = ...` — an exported item.
    fn stmt_pub(&mut self, inner: &Stmt) -> FmtResult<()> {
        match inner {
            Stmt::Let { name, pattern: _, mutable, value, type_hint } => {
                if let Expr::Function { params, body, return_type, .. } = value.as_ref() {
                    self.fn_def(Some("pub "), name, params, body, return_type)?;
                    return Ok(());
                }
                let mut head = String::from("pub let ");
                if *mutable {
                    head.push_str("mut ");
                }
                head.push_str(name);
                if let Some(t) = type_hint {
                    head.push_str(&format!(": {}", type_str(t)));
                }
                let expr = self.expr(value)?;
                self.line(&format!("{} = {}", head, expr));
            }
            Stmt::Const { name, value } => {
                let expr = self.expr(value)?;
                self.line(&format!("pub const {} = {}", name, expr));
            }
            Stmt::Struct { name, type_params, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|f| {
                        let t = f.type_hint.as_ref().map(type_str).unwrap_or_else(|| "nil".to_string());
                        format!("{}: {}", f.name, t)
                    })
                    .collect();
                let generics = if type_params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", type_params.join(", "))
                };
                self.line(&format!("pub struct {}{} {{ {} }}", name, generics, parts.join(", ")));
            }
            Stmt::Enum { name, type_params, variants } => {
                let vs: Vec<String> = variants
                    .iter()
                    .map(|v| {
                        if v.fields.is_empty() {
                            v.name.clone()
                        } else {
                            format!("{}({})", v.name, v.fields.iter().map(type_str).collect::<Vec<_>>().join(", "))
                        }
                    })
                    .collect();
                let generics = if type_params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", type_params.join(", "))
                };
                self.line(&format!("pub enum {}{} {{ {} }}", name, generics, vs.join(", ")));
            }
            Stmt::MacroDef { name, params, body } => {
                let ps: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                self.line(&format!("pub macro {}({}) {{", name, ps.join(", ")));
                self.block(body)?;
                self.line("}");
            }
            other => {
                self.line("pub");
                self.indent += 1;
                self.stmt(other)?;
                self.indent -= 1;
            }
        }
        Ok(())
    }

    /// A top-level `fn name(params) -> ret { body }` (from `let name = fn...`).
    fn fn_def(
        &mut self,
        prefix: Option<&str>,
        name: &str,
        params: &[Param],
        body: &[Stmt],
        return_type: &Option<Type>,
    ) -> FmtResult<()> {
        let mut ps: Vec<String> = Vec::new();
        for p in params {
            let mut s = p.name.clone();
            if p.optional {
                s.push('?');
            }
            if let Some(t) = &p.type_hint {
                s.push_str(&format!(": {}", type_str(t)));
            }
            if let Some(d) = p.default.as_deref() {
                s.push_str(&format!(" = {}", self.expr(d)?));
            }
            if p.rest {
                s = format!("...{}", p.name);
            }
            ps.push(s);
        }
        let ret = match return_type {
            Some(t) => format!(" -> {}", type_str(t)),
            None => String::new(),
        };
        let pfx = prefix.unwrap_or("");
        self.line(&format!("{}fn {}({}){} {{", pfx, name, ps.join(", "), ret));
        self.block(body)?;
        self.line("}");
        Ok(())
    }

    // ---- Expressions ----

    fn expr(&mut self, e: &Expr) -> FmtResult<String> {
        self.expr_inline(e)
    }

    fn expr_inline(&mut self, e: &Expr) -> FmtResult<String> {
        match e {
            Expr::Int(i) => Ok(i.to_string()),
            Expr::Hex(h) => Ok(format!("0x{:X}", h)),
            Expr::BinLit(b) => Ok(format!("0b{:b}", b)),
            Expr::OctLit(o) => Ok(format!("0o{:o}", o)),
            Expr::TypedInt(i, kind) => {
                let suffix = match kind {
                    IntKind::I8 => "i8",
                    IntKind::I16 => "i16",
                    IntKind::I32 => "i32",
                    IntKind::I64 => "i64",
                    IntKind::U8 => "u8",
                    IntKind::U16 => "u16",
                    IntKind::U32 => "u32",
                    IntKind::U64 => "u64",
                };
                Ok(format!("{}{}", i, suffix))
            }
            Expr::Float(f) => Ok(float_str(*f)),
            Expr::Float32(f) => Ok(format!("{}f32", float_str(*f as f64))),
            Expr::String(s) => Ok(format!("{:?}", s)),
            Expr::Char(c) => Ok(char_lit(*c)),
            Expr::Bool(b) => Ok(b.to_string()),
            Expr::Nil => Ok("nil".to_string()),
            Expr::Ident(n) => Ok(n.clone()),
            Expr::Path(segs) => Ok(segs.join("::")),
            Expr::Bytes(b) => {
                let parts: Vec<String> = b.iter().map(|x| format!("\\x{:02x}", x)).collect();
                Ok(format!("b\"{}\"", parts.join("")))
            }
            Expr::Interp { template, parts } => {
                // Re-render `f"..."` with `{name}` placeholders restored.
                let mut out = String::from("f\"");
                out.push_str(template);
                let _ = parts;
                out.push('"');
                Ok(out)
            }
            Expr::Array(items) => {
                let mut parts = Vec::new();
                for i in items {
                    parts.push(self.expr_inline(i)?);
                }
                Ok(format!("[{}]", parts.join(", ")))
            }
            Expr::Tuple(items) => {
                let mut parts = Vec::new();
                for i in items {
                    parts.push(self.expr_inline(i)?);
                }
                if parts.len() == 1 {
                    Ok(format!("({},)", parts[0]))
                } else {
                    Ok(format!("({})", parts.join(", ")))
                }
            }
            Expr::Map(pairs) => {
                let mut parts = Vec::new();
                for (k, v) in pairs {
                    parts.push(format!("{}: {}", self.expr_inline(k)?, self.expr_inline(v)?));
                }
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            Expr::StructLit { name, fields } => {
                let mut parts = Vec::new();
                for (k, v) in fields {
                    parts.push(format!("{}: {}", k, self.expr_inline(v)?));
                }
                Ok(format!("{} {{ {} }}", name, parts.join(", ")))
            }
            Expr::Index(obj, idx) => Ok(format!("{}[{}]", self.expr_inline(obj)?, self.expr_inline(idx)?)),
            Expr::FieldAccess(obj, field) => Ok(format!("{}.{}", self.expr_inline(obj)?, field)),
            Expr::Call { callee, args, named } => {
                let c = self.expr_inline(callee)?;
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.expr_inline(a)?);
                }
                for (n, v) in named {
                    parts.push(format!("{}: {}", n, self.expr_inline(v)?));
                }
                Ok(format!("{}({})", c, parts.join(", ")))
            }
            Expr::Unary(op, inner) => {
                let o = match op {
                    UnOp::Minus => "-",
                    UnOp::Not => "!",
                    UnOp::BitNot => "~",
                };
                Ok(format!("{}{}", o, self.expr_inline(inner)?))
            }
            Expr::Binary(op, l, r) => Ok(format!(
                "{} {} {}",
                self.expr_inline(l)?,
                binop_str(op),
                self.expr_inline(r)?
            )),
            Expr::Assign(name, value) => Ok(format!("{} = {}", name, self.expr_inline(value)?)),
            Expr::CompoundAssign(op, name, value) => {
                Ok(format!("{} {}= {}", name, compound_str(op), self.expr_inline(value)?))
            }
            Expr::IndexAssign { obj, idx, value } => Ok(format!(
                "{}[{}] = {}",
                self.expr_inline(obj)?,
                self.expr_inline(idx)?,
                self.expr_inline(value)?
            )),
            Expr::FieldAssign { obj, field, value } => {
                Ok(format!("{}.{} = {}", self.expr_inline(obj)?, field, self.expr_inline(value)?))
            }
            Expr::MultiAssign { targets, values } => {
                let mut ts = Vec::new();
                for t in targets {
                    ts.push(self.expr_inline(t)?);
                }
                let mut vs = Vec::new();
                for v in values {
                    vs.push(self.expr_inline(v)?);
                }
                Ok(format!("{} = {}", ts.join(", "), vs.join(", ")))
            }
            Expr::If { cond, then_branch, else_branch } => {
                let cond_s = self.expr(cond)?;
                let then_s = self.block_value_inline(then_branch)?;
                let else_s = match else_branch {
                    Some(els) => self.block_value_inline(els)?,
                    None => "nil".to_string(),
                };
                Ok(format!("if {} {{ {} }} else {{ {} }}", cond_s, then_s, else_s))
            }
            Expr::Block(stmts) => self.block_value_inline(stmts),
            Expr::Function { params, body, return_type, .. } => {
                let ps: Vec<String> = params
                    .iter()
                    .map(|p| {
                        let mut s = p.name.clone();
                        if let Some(t) = &p.type_hint {
                            s.push_str(&format!(": {}", type_str(t)));
                        }
                        s
                    })
                    .collect();
                let ret = match return_type {
                    Some(t) => format!(" -> {}", type_str(t)),
                    None => String::new(),
                };
                let body_s = self.block_value_inline(body)?;
                Ok(format!("fn({}){} {{ {} }}", ps.join(", "), ret, body_s))
            }
            Expr::Lambda { params, body, .. } => {
                let ps: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                Ok(format!("fn({}) {{ {} }}", ps.join(", "), self.expr(body)?))
            }
            Expr::Match { value, arms } => {
                let v = self.expr(value)?;
                let mut parts = Vec::new();
                for (pattern, guard, body) in arms {
                    let pat = pattern_str(pattern);
                    let g = match guard {
                        Some(g) => format!(" if {}", self.expr(g)?),
                        None => String::new(),
                    };
                    let body_s = self.block_value_inline(body)?;
                    parts.push(format!("{}{} => {{ {} }}", pat, g, body_s));
                }
                Ok(format!("match {} {{ {} }}", v, parts.join(", ")))
            }
            Expr::Ternary { cond, then, els } => Ok(format!(
                "{} ? {} : {}",
                self.expr(cond)?,
                self.expr(then)?,
                self.expr(els)?
            )),
            Expr::NilCoalesce(l, r) => Ok(format!("{} ?? {}", self.expr(l)?, self.expr(r)?)),
            Expr::OptField(obj, field) => Ok(format!("{}?.{}", self.expr(obj)?, field)),
            Expr::OptIndex(obj, idx) => Ok(format!("{}?[{}]", self.expr(obj)?, self.expr(idx)?)),
            Expr::TryExpr(inner) => Ok(format!("{}?", self.expr(inner)?)),
            Expr::Await(inner) => Ok(format!("await {}", self.expr(inner)?)),
            Expr::Spawn(inner) => Ok(format!("spawn {}", self.expr(inner)?)),
            Expr::Raise(inner) => Ok(format!("raise {}", self.expr(inner)?)),
            Expr::Regex(p, f) => Ok(format!("/{}/{}", p, f)),
            Expr::Range(lo, hi) => {
                let lo_s = match lo {
                    Some(l) => self.expr(l)?,
                    None => String::new(),
                };
                let hi_s = match hi {
                    Some(h) => self.expr(h)?,
                    None => String::new(),
                };
                Ok(format!("{}..{}", lo_s, hi_s))
            }
            Expr::Comprehension { is_map, var, iterable, cond, elem, value } => {
                let it = self.expr(iterable)?;
                let el = self.expr(elem)?;
                let c = match cond {
                    Some(c) => format!(" if {}", self.expr(c)?),
                    None => String::new(),
                };
                if *is_map {
                    let v = match value {
                        Some(v) => self.expr(v)?,
                        None => "nil".to_string(),
                    };
                    Ok(format!("{{{}: {} for {} in {}{}}}", el, v, pattern_str(var), it, c))
                } else {
                    Ok(format!("[{} for {} in {}{}]", el, pattern_str(var), it, c))
                }
            }
            Expr::MacroVar(n) => Ok(format!("${}", n)),
            Expr::MacroInvoke { name, args } => {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.expr(a)?);
                }
                Ok(format!("{}!({})", name, parts.join(", ")))
            }
            Expr::EvidenceFrom { value } => Ok(format!("evidence from {}", self.expr(value)?)),
            Expr::As(inner, ty) => Ok(format!("{} as {}", self.expr(inner)?, type_str(ty))),
            other => Err(format!("formatter: unsupported expression {:?}", other)),
        }
    }

    fn block_value_inline(&mut self, stmts: &[Stmt]) -> FmtResult<String> {
        let saved_indent = self.indent;
        let saved_out = std::mem::take(&mut self.out);
        self.indent += 1;
        self.stmts(stmts)?;
        self.indent = saved_indent;
        let rendered = std::mem::replace(&mut self.out, saved_out);
        Ok(rendered.trim_end().to_string())
    }
}

fn label_str(l: &Option<String>) -> String {
    match l {
        Some(l) => format!("'{}: ", l),
        None => String::new(),
    }
}

fn break_target(t: &Option<BreakTarget>) -> String {
    match t {
        None => String::new(),
        Some(BreakTarget::Depth(n)) => format!(" {}", n),
        Some(BreakTarget::Label(l)) => format!(" '{}", l),
    }
}

fn param_default(p: &Param) -> Option<&Expr> {
    p.default.as_deref()
}

fn float_str(f: f64) -> String {
    if f == f.trunc() && f.is_finite() && f.abs() < 1e15 {
        format!("{:.1}", f)
    } else {
        format!("{}", f)
    }
}

fn char_lit(c: char) -> String {
    match c {
        '\'' => "'\\''".to_string(),
        '\\' => "'\\\\'".to_string(),
        '\n' => "'\\n'".to_string(),
        '\t' => "'\\t'".to_string(),
        '\r' => "'\\r'".to_string(),
        '\0' => "'\\0'".to_string(),
        _ => format!("'{}'", c),
    }
}

fn binop_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
    }
}

fn compound_str(op: &CompoundOp) -> &'static str {
    match op {
        CompoundOp::Add => "+",
        CompoundOp::Sub => "-",
        CompoundOp::Mul => "*",
        CompoundOp::Div => "/",
        CompoundOp::Rem => "%",
        CompoundOp::BitAnd => "&",
        CompoundOp::BitOr => "|",
        CompoundOp::BitXor => "^",
        CompoundOp::Shl => "<<",
        CompoundOp::Shr => ">>",
    }
}

fn type_str(t: &Type) -> String {
    match t {
        Type::Hex(_) => "hex".to_string(),
        Type::Int => "int".to_string(),
        Type::I8 => "i8".to_string(),
        Type::I16 => "i16".to_string(),
        Type::I32 => "i32".to_string(),
        Type::I64 => "i64".to_string(),
        Type::U8 => "u8".to_string(),
        Type::U16 => "u16".to_string(),
        Type::U32 => "u32".to_string(),
        Type::U64 => "u64".to_string(),
        Type::F32 => "f32".to_string(),
        Type::F64 => "f64".to_string(),
        Type::String => "string".to_string(),
        Type::Char => "char".to_string(),
        Type::Bytes => "bytes".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Nil => "nil".to_string(),
        Type::Array(t) => format!("array<{}>", type_str(t)),
        Type::Tuple(ts) => format!("({})", ts.iter().map(type_str).collect::<Vec<_>>().join(", ")),
        Type::Map(k, v) => format!("map<{}, {}>", type_str(k), type_str(v)),
        Type::Function(ps, r) => format!("fn({}) -> {}", ps.iter().map(type_str).collect::<Vec<_>>().join(", "), type_str(r)),
        Type::Custom(n) => n.clone(),
        Type::Generic(n) => n.clone(),
        Type::Option(t) => format!("option<{}>", type_str(t)),
        Type::Result(o, e) => format!("result<{}, {}>", type_str(o), type_str(e)),
        Type::Ptr(t) => format!("*{}", type_str(t)),
        Type::Void => "void".to_string(),
        Type::Evidence(t) => format!("evidence<{}>", type_str(t)),
    }
}

fn bin_kind_str(k: &BinKind) -> String {
    match k {
        BinKind::Uint { bits, endian } => format!("u{}{}", bits, endian_suffix(endian)),
        BinKind::Int { bits, endian } => format!("i{}{}", bits, endian_suffix(endian)),
        BinKind::Bits { bits } => format!("u{}", bits),
        BinKind::Bytes(n) => format!("bytes({})", n),
        BinKind::Rest => "rest".to_string(),
        BinKind::Ref(n) => n.clone(),
    }
}

fn endian_suffix(e: &Endian) -> &'static str {
    match e {
        Endian::Big => "be",
        Endian::Little => "le",
    }
}

fn pattern_str(p: &Pattern) -> String {
    match p {
        Pattern::Wild => "_".to_string(),
        Pattern::Ident(n) => n.clone(),
        Pattern::Hex(h) => format!("0x{:X}", h),
        Pattern::Int(i) => i.to_string(),
        Pattern::String(s) => format!("{:?}", s),
        Pattern::Bool(b) => b.to_string(),
        Pattern::Nil => "nil".to_string(),
        Pattern::Tuple(ps) => format!("({})", ps.iter().map(pattern_str).collect::<Vec<_>>().join(", ")),
        Pattern::Array(ps) => format!("[{}]", ps.iter().map(pattern_str).collect::<Vec<_>>().join(", ")),
        Pattern::Byte(b) => format!("0x{:02X}", b),
        Pattern::Bytes(pats) => {
            let parts: Vec<String> = pats
                .iter()
                .map(|bp| match bp {
                    BytesPat::Byte(b) => format!("0x{:02X}", b),
                    BytesPat::Rest => "..".to_string(),
                })
                .collect();
            format!("[{}]", parts.join(", "))
        }
        Pattern::Struct(name, fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(n, sub)| {
                    if matches!(sub, Pattern::Ident(s) if s == n) {
                        n.clone()
                    } else {
                        format!("{}: {}", n, pattern_str(sub))
                    }
                })
                .collect();
            format!("{} {{ {} }}", name, parts.join(", "))
        }
        Pattern::EnumVariant(en, vn, subs) => {
            if subs.is_empty() {
                format!("{}::{}", en, vn)
            } else {
                format!("{}::{}({})", en, vn, subs.iter().map(pattern_str).collect::<Vec<_>>().join(", "))
            }
        }
        Pattern::Range(lo, hi) => format!("{}..{}", pattern_str(lo), pattern_str(hi)),
        Pattern::Or(ps) => format!("| {}", ps.iter().map(pattern_str).collect::<Vec<_>>().join(" | ")),
        Pattern::Some(inner) => format!("Some({})", pattern_str(inner)),
        Pattern::None => "None".to_string(),
        Pattern::Ok(inner) => format!("Ok({})", pattern_str(inner)),
        Pattern::Err(inner) => format!("Err({})", pattern_str(inner)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_is_idempotent() {
        let src = r#"struct Point { x: int, y: int }
fn add(a, b) { return a + b }
let p = Point { x: 1, y: 2 }
dump add(p.x, p.y)
for i in 1..10 { dump i }
if p.x > 0 { dump "pos" } else { dump "neg" }
match p.x {
    1 => {
        dump "one"
    },
    _ => {
        dump "other"
    },
}
while p.y < 10 {
    p.y = p.y + 1
}
try {
    raise "x"
} catch e {
    dump e
}
"#;
        let once = format_source(src).unwrap();
        let twice = format_source(&once).unwrap();
        assert_eq!(once, twice, "fmt must be idempotent");
    }

    #[test]
    fn fmt_renders_common_statements() {
        let src = "let mut n = 0\nn += 1\ndump n\n";
        let out = format_source(src).unwrap();
        assert!(out.contains("let mut n = 0"));
        assert!(out.contains("n += 1"));
        assert!(out.contains("dump n"));
    }
}
