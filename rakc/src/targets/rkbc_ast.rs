//! AST codec for the `.rkbc` container.
//!
//! A chunk's constant pool holds *baked function values*, and a function
//! value's body is a real `Vec<Stmt>` ג€” arbitrary user code. Serializing a
//! chunk therefore needs a lossless binary encoding of the whole surface of
//! [`crate::ast`] that can appear inside a function: statements, expressions,
//! patterns, types, and their sub-enums.
//!
//! Every variant is encoded with a stable tag, so the format is part of
//! `.rkbc`'s on-disk contract. Decoding is total: an unknown tag is an error
//! naming the tag and the offset, never a panic and never a guess.

use super::rkbc::{Reader, Writer};
use crate::ast::*;
use crate::value::ParamShape;

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

impl Writer {
    pub fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Hex(h) => {
                self.u8(1);
                self.u64(*h);
            }
            Expr::BinLit(b) => {
                self.u8(2);
                self.u64(*b);
            }
            Expr::OctLit(o) => {
                self.u8(3);
                self.u64(*o);
            }
            Expr::Int(i) => {
                self.u8(4);
                self.i64(*i);
            }
            Expr::Float(f) => {
                self.u8(5);
                self.f64(*f);
            }
            Expr::Float32(f) => {
                self.u8(6);
                self.f32(*f);
            }
            Expr::TypedInt(i, kind) => {
                self.u8(7);
                self.i64(*i);
                self.int_kind(*kind);
            }
            Expr::String(s) => {
                self.u8(8);
                self.str(s);
            }
            Expr::Char(c) => {
                self.u8(9);
                self.u32(*c as u32);
            }
            Expr::Interp { template, parts } => {
                self.u8(10);
                self.str(template);
                self.u32(parts.len() as u32);
                for p in parts {
                    self.expr(p)?;
                }
            }
            Expr::Bytes(b) => {
                self.u8(11);
                self.bytes(b);
            }
            Expr::Regex(pat, flags) => {
                self.u8(12);
                self.str(pat);
                self.str(flags);
            }
            Expr::Ident(s) => {
                self.u8(13);
                self.str(s);
            }
            Expr::Bool(b) => {
                self.u8(14);
                self.flag(*b);
            }
            Expr::Nil => self.u8(15),
            Expr::Tuple(items) => {
                self.u8(16);
                self.u32(items.len() as u32);
                for i in items {
                    self.expr(i)?;
                }
            }
            Expr::Array(items) => {
                self.u8(17);
                self.u32(items.len() as u32);
                for i in items {
                    self.expr(i)?;
                }
            }
            Expr::Map(pairs) => {
                self.u8(18);
                self.u32(pairs.len() as u32);
                for (k, v) in pairs {
                    self.expr(k)?;
                    self.expr(v)?;
                }
            }
            Expr::Unary(op, e) => {
                self.u8(19);
                self.un_op(*op);
                self.expr(e)?;
            }
            Expr::Binary(op, a, b) => {
                self.u8(20);
                self.bin_op(*op);
                self.expr(a)?;
                self.expr(b)?;
            }
            Expr::Assign(name, e) => {
                self.u8(21);
                self.str(name);
                self.expr(e)?;
            }
            Expr::CompoundAssign(op, name, e) => {
                self.u8(22);
                self.compound_op(*op);
                self.str(name);
                self.expr(e)?;
            }
            Expr::IndexAssign { obj, idx, value } => {
                self.u8(23);
                self.expr(obj)?;
                self.expr(idx)?;
                self.expr(value)?;
            }
            Expr::FieldAssign { obj, field, value } => {
                self.u8(24);
                self.expr(obj)?;
                self.str(field);
                self.expr(value)?;
            }
            Expr::FieldAccess(obj, field) => {
                self.u8(25);
                self.expr(obj)?;
                self.str(field);
            }
            Expr::Index(obj, idx) => {
                self.u8(26);
                self.expr(obj)?;
                self.expr(idx)?;
            }
            Expr::Range(lo, hi) => {
                self.u8(27);
                self.opt_expr(lo.as_deref())?;
                self.opt_expr(hi.as_deref())?;
            }
            Expr::Function {
                params,
                return_type,
                body,
                captures,
                is_async,
                name,
                requires,
                ensures,
            } => {
                self.u8(28);
                self.params(params)?;
                self.opt_type(return_type.as_ref())?;
                self.stmts(body)?;
                self.u32(captures.len() as u32);
                for c in captures {
                    self.str(c);
                }
                self.flag(*is_async);
                if let Some(n) = name.as_deref() {
                    self.flag(true);
                    self.str(n);
                } else {
                    self.flag(false);
                }
                self.u32(requires.len() as u32);
                for r in requires {
                    self.expr(r)?;
                }
                self.u32(ensures.len() as u32);
                for r in ensures {
                    self.expr(r)?;
                }
            }
            Expr::Call {
                callee,
                args,
                named,
            } => {
                self.u8(29);
                self.expr(callee)?;
                self.u32(args.len() as u32);
                for a in args {
                    self.expr(a)?;
                }
                self.u32(named.len() as u32);
                for (n, v) in named {
                    self.str(n);
                    self.expr(v)?;
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.u8(30);
                self.expr(cond)?;
                self.stmts(then_branch)?;
                self.opt_stmts(else_branch.as_deref())?;
            }
            Expr::Match { value, arms } => {
                self.u8(31);
                self.expr(value)?;
                self.arms(arms)?;
            }
            Expr::Block(body) => {
                self.u8(32);
                self.stmts(body)?;
            }
            Expr::Lambda {
                params,
                body,
                captures,
            } => {
                self.u8(33);
                self.params(params)?;
                self.expr(body)?;
                self.u32(captures.len() as u32);
                for c in captures {
                    self.str(c);
                }
            }
            Expr::TryExpr(e) => {
                self.u8(34);
                self.expr(e)?;
            }
            Expr::Await(e) => {
                self.u8(35);
                self.expr(e)?;
            }
            Expr::Spawn(e) => {
                self.u8(36);
                self.expr(e)?;
            }
            Expr::Raise(e) => {
                self.u8(37);
                self.expr(e)?;
            }
            Expr::Ternary { cond, then, els } => {
                self.u8(38);
                self.expr(cond)?;
                self.expr(then)?;
                self.expr(els)?;
            }
            Expr::NilCoalesce(a, b) => {
                self.u8(39);
                self.expr(a)?;
                self.expr(b)?;
            }
            Expr::OptField(obj, field) => {
                self.u8(40);
                self.expr(obj)?;
                self.str(field);
            }
            Expr::OptIndex(obj, idx) => {
                self.u8(41);
                self.expr(obj)?;
                self.expr(idx)?;
            }
            Expr::MultiAssign { targets, values } => {
                self.u8(42);
                self.u32(targets.len() as u32);
                for t in targets {
                    self.expr(t)?;
                }
                self.u32(values.len() as u32);
                for v in values {
                    self.expr(v)?;
                }
            }
            Expr::Comprehension {
                is_map,
                var,
                iterable,
                cond,
                elem,
                value,
            } => {
                self.u8(43);
                self.flag(*is_map);
                self.pattern(var)?;
                self.expr(iterable)?;
                self.opt_expr(cond.as_deref())?;
                self.expr(elem)?;
                self.opt_expr(value.as_deref())?;
            }
            Expr::Path(parts) => {
                self.u8(44);
                self.u32(parts.len() as u32);
                for p in parts {
                    self.str(p);
                }
            }
            Expr::StructLit { name, fields } => {
                self.u8(45);
                self.str(name);
                self.u32(fields.len() as u32);
                for (n, v) in fields {
                    self.str(n);
                    self.expr(v)?;
                }
            }
            Expr::As(e, t) => {
                self.u8(46);
                self.expr(e)?;
                self.type_(t)?;
            }
            Expr::MacroVar(s) => {
                self.u8(47);
                self.str(s);
            }
            Expr::MacroInvoke { name, args } => {
                self.u8(48);
                self.str(name);
                self.u32(args.len() as u32);
                for a in args {
                    self.expr(a)?;
                }
            }
            Expr::EvidenceFrom { value } => {
                self.u8(49);
                self.expr(value)?;
            }
        }
        Ok(())
    }

    fn opt_expr(&mut self, e: Option<&Expr>) -> Result<(), String> {
        match e {
            Some(x) => {
                self.flag(true);
                self.expr(x)
            }
            None => {
                self.flag(false);
                Ok(())
            }
        }
    }
}

impl Reader<'_> {
    pub fn expr(&mut self) -> Result<Expr, String> {
        let tag = self.u8()?;
        match tag {
            1 => Ok(Expr::Hex(self.u64()?)),
            2 => Ok(Expr::BinLit(self.u64()?)),
            3 => Ok(Expr::OctLit(self.u64()?)),
            4 => Ok(Expr::Int(self.i64()?)),
            5 => Ok(Expr::Float(self.f64()?)),
            6 => Ok(Expr::Float32(self.f32()?)),
            7 => {
                let i = self.i64()?;
                let kind = self.int_kind()?;
                Ok(Expr::TypedInt(i, kind))
            }
            8 => Ok(Expr::String(self.str()?)),
            9 => {
                let c = self.u32()?;
                Ok(Expr::Char(
                    char::from_u32(c).ok_or_else(|| self.bad_char(c))?,
                ))
            }
            10 => {
                let template = self.str()?;
                let n = self.count("interp parts")?;
                let mut parts = Vec::with_capacity(n);
                for _ in 0..n {
                    parts.push(self.expr()?);
                }
                Ok(Expr::Interp { template, parts })
            }
            11 => Ok(Expr::Bytes(self.bytes()?)),
            12 => {
                let pat = self.str()?;
                let flags = self.str()?;
                Ok(Expr::Regex(pat, flags))
            }
            13 => Ok(Expr::Ident(self.str()?)),
            14 => Ok(Expr::Bool(self.flag()?)),
            15 => Ok(Expr::Nil),
            16 => {
                let n = self.count("tuple items")?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.expr()?);
                }
                Ok(Expr::Tuple(v))
            }
            17 => {
                let n = self.count("array items")?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.expr()?);
                }
                Ok(Expr::Array(v))
            }
            18 => {
                let n = self.count("map pairs")?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let k = self.expr()?;
                    let val = self.expr()?;
                    v.push((k, val));
                }
                Ok(Expr::Map(v))
            }
            19 => {
                let op = self.un_op()?;
                let e = self.expr()?;
                Ok(Expr::Unary(op, Box::new(e)))
            }
            20 => {
                let op = self.bin_op()?;
                let a = self.expr()?;
                let b = self.expr()?;
                Ok(Expr::Binary(op, Box::new(a), Box::new(b)))
            }
            21 => {
                let name = self.str()?;
                let e = self.expr()?;
                Ok(Expr::Assign(name, Box::new(e)))
            }
            22 => {
                let op = self.compound_op()?;
                let name = self.str()?;
                let e = self.expr()?;
                Ok(Expr::CompoundAssign(op, name, Box::new(e)))
            }
            23 => {
                let obj = self.expr()?;
                let idx = self.expr()?;
                let value = self.expr()?;
                Ok(Expr::IndexAssign {
                    obj: Box::new(obj),
                    idx: Box::new(idx),
                    value: Box::new(value),
                })
            }
            24 => {
                let obj = self.expr()?;
                let field = self.str()?;
                let value = self.expr()?;
                Ok(Expr::FieldAssign {
                    obj: Box::new(obj),
                    field,
                    value: Box::new(value),
                })
            }
            25 => {
                let obj = self.expr()?;
                let field = self.str()?;
                Ok(Expr::FieldAccess(Box::new(obj), field))
            }
            26 => {
                let obj = self.expr()?;
                let idx = self.expr()?;
                Ok(Expr::Index(Box::new(obj), Box::new(idx)))
            }
            27 => {
                let lo = self.opt_expr()?.map(Box::new);
                let hi = self.opt_expr()?.map(Box::new);
                Ok(Expr::Range(lo, hi))
            }
            28 => {
                let params = self.params()?;
                let return_type = self.opt_type()?;
                let body = self.stmts()?;
                let n = self.count("captures")?;
                let mut captures = Vec::with_capacity(n);
                for _ in 0..n {
                    captures.push(self.str()?);
                }
                let is_async = self.flag()?;
                let name = self.opt_str_read()?;
                let rn = self.count("requires")?;
                let mut requires = Vec::with_capacity(rn);
                for _ in 0..rn {
                    requires.push(self.expr()?);
                }
                let en = self.count("ensures")?;
                let mut ensures = Vec::with_capacity(en);
                for _ in 0..en {
                    ensures.push(self.expr()?);
                }
                Ok(Expr::Function {
                    params,
                    return_type,
                    body,
                    captures,
                    is_async,
                    name,
                    requires,
                    ensures,
                })
            }
            29 => {
                let callee = self.expr()?;
                let n = self.count("call args")?;
                let mut args = Vec::with_capacity(n);
                for _ in 0..n {
                    args.push(self.expr()?);
                }
                let nn = self.count("named args")?;
                let mut named = Vec::with_capacity(nn);
                for _ in 0..nn {
                    let name = self.str()?;
                    let v = self.expr()?;
                    named.push((name, v));
                }
                Ok(Expr::Call {
                    callee: Box::new(callee),
                    args,
                    named,
                })
            }
            30 => {
                let cond = self.expr()?;
                let then_branch = self.stmts()?;
                let else_branch = self.opt_stmts()?;
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then_branch,
                    else_branch,
                })
            }
            31 => {
                let value = self.expr()?;
                let arms = self.arms()?;
                Ok(Expr::Match {
                    value: Box::new(value),
                    arms,
                })
            }
            32 => Ok(Expr::Block(self.stmts()?)),
            33 => {
                let params = self.params()?;
                let body = self.expr()?;
                let n = self.count("lambda captures")?;
                let mut captures = Vec::with_capacity(n);
                for _ in 0..n {
                    captures.push(self.str()?);
                }
                Ok(Expr::Lambda {
                    params,
                    body: Box::new(body),
                    captures,
                })
            }
            34 => Ok(Expr::TryExpr(Box::new(self.expr()?))),
            35 => Ok(Expr::Await(Box::new(self.expr()?))),
            36 => Ok(Expr::Spawn(Box::new(self.expr()?))),
            37 => Ok(Expr::Raise(Box::new(self.expr()?))),
            38 => {
                let cond = self.expr()?;
                let then = self.expr()?;
                let els = self.expr()?;
                Ok(Expr::Ternary {
                    cond: Box::new(cond),
                    then: Box::new(then),
                    els: Box::new(els),
                })
            }
            39 => {
                let a = self.expr()?;
                let b = self.expr()?;
                Ok(Expr::NilCoalesce(Box::new(a), Box::new(b)))
            }
            40 => {
                let obj = self.expr()?;
                let field = self.str()?;
                Ok(Expr::OptField(Box::new(obj), field))
            }
            41 => {
                let obj = self.expr()?;
                let idx = self.expr()?;
                Ok(Expr::OptIndex(Box::new(obj), Box::new(idx)))
            }
            42 => {
                let tn = self.count("multi-assign targets")?;
                let mut targets = Vec::with_capacity(tn);
                for _ in 0..tn {
                    targets.push(self.expr()?);
                }
                let vn = self.count("multi-assign values")?;
                let mut values = Vec::with_capacity(vn);
                for _ in 0..vn {
                    values.push(self.expr()?);
                }
                Ok(Expr::MultiAssign { targets, values })
            }
            43 => {
                let is_map = self.flag()?;
                let var = self.pattern()?;
                let iterable = self.expr()?;
                let cond = self.opt_expr()?.map(Box::new);
                let elem = self.expr()?;
                let value = self.opt_expr()?.map(Box::new);
                Ok(Expr::Comprehension {
                    is_map,
                    var,
                    iterable: Box::new(iterable),
                    cond,
                    elem: Box::new(elem),
                    value,
                })
            }
            44 => {
                let n = self.count("path segments")?;
                let mut parts = Vec::with_capacity(n);
                for _ in 0..n {
                    parts.push(self.str()?);
                }
                Ok(Expr::Path(parts))
            }
            45 => {
                let name = self.str()?;
                let n = self.count("struct fields")?;
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    let fname = self.str()?;
                    let v = self.expr()?;
                    fields.push((fname, v));
                }
                Ok(Expr::StructLit { name, fields })
            }
            46 => {
                let e = self.expr()?;
                let t = self.type_()?;
                Ok(Expr::As(Box::new(e), t))
            }
            47 => Ok(Expr::MacroVar(self.str()?)),
            48 => {
                let name = self.str()?;
                let n = self.count("macro args")?;
                let mut args = Vec::with_capacity(n);
                for _ in 0..n {
                    args.push(self.expr()?);
                }
                Ok(Expr::MacroInvoke { name, args })
            }
            49 => Ok(Expr::EvidenceFrom {
                value: Box::new(self.expr()?),
            }),
            other => Err(self.bad_tag("expression", other)),
        }
    }

    fn opt_expr(&mut self) -> Result<Option<Expr>, String> {
        if self.flag()? {
            Ok(Some(self.expr()?))
        } else {
            Ok(None)
        }
    }

    fn opt_str_read(&mut self) -> Result<Option<String>, String> {
        if self.flag()? {
            Ok(Some(self.str()?))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn bad_tag(&self, what: &str, tag: u8) -> String {
        format!(
            "{}: unknown {} tag {} (file is not a .rkbc, or is corrupt)",
            self.where_(),
            what,
            tag
        )
    }

    fn bad_char(&self, c: u32) -> String {
        format!("{}: invalid char code point {}", self.where_(), c)
    }
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

impl Writer {
    pub fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Let {
                name,
                pattern,
                mutable,
                value,
                type_hint,
            } => {
                self.u8(1);
                self.str(name);
                self.opt_pattern(pattern.as_ref())?;
                self.flag(*mutable);
                self.expr(value)?;
                self.opt_type(type_hint.as_ref())?;
            }
            Stmt::Expr(e) => {
                self.u8(2);
                self.expr(e)?;
            }
            Stmt::Return(e) => {
                self.u8(3);
                self.opt_expr(e.as_deref())?;
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.u8(4);
                self.expr(cond)?;
                self.stmts(then_branch)?;
                self.opt_stmts(else_branch.as_deref())?;
            }
            Stmt::DoWhile { cond, body } => {
                self.u8(5);
                self.expr(cond)?;
                self.stmts(body)?;
            }
            Stmt::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => {
                self.u8(6);
                self.pattern(pattern)?;
                self.expr(value)?;
                self.stmts(then_branch)?;
                self.opt_stmts(else_branch.as_deref())?;
            }
            Stmt::WhileLet {
                pattern,
                value,
                body,
            } => {
                self.u8(7);
                self.pattern(pattern)?;
                self.expr(value)?;
                self.stmts(body)?;
            }
            Stmt::Loop { label, body } => {
                self.u8(8);
                if let Some(_v) = label.as_deref() {
                    self.flag(true);
                    self.str(_v);
                } else {
                    self.flag(false);
                }
                self.stmts(body)?;
            }
            Stmt::While { label, cond, body } => {
                self.u8(9);
                if let Some(_v) = label.as_deref() {
                    self.flag(true);
                    self.str(_v);
                } else {
                    self.flag(false);
                }
                self.expr(cond)?;
                self.stmts(body)?;
            }
            Stmt::For {
                label,
                pattern,
                iterable,
                body,
            } => {
                self.u8(10);
                if let Some(_v) = label.as_deref() {
                    self.flag(true);
                    self.str(_v);
                } else {
                    self.flag(false);
                }
                self.pattern(pattern)?;
                self.expr(iterable)?;
                self.stmts(body)?;
            }
            Stmt::Scan {
                target,
                options,
                body,
            } => {
                self.u8(11);
                self.expr(target)?;
                self.str_pairs(options)?;
                self.opt_stmts(body.as_deref())?;
            }
            Stmt::Fetch {
                target,
                options,
                body,
            } => {
                self.u8(12);
                self.expr(target)?;
                self.str_pairs(options)?;
                self.opt_stmts(body.as_deref())?;
            }
            Stmt::Dump { value, target } => {
                self.u8(13);
                self.expr(value)?;
                self.opt_expr(target.as_deref())?;
            }
            Stmt::Trace { value } => {
                self.u8(14);
                self.expr(value)?;
            }
            Stmt::Break(t) => {
                self.u8(15);
                self.opt_break_target(t.as_ref())?;
            }
            Stmt::Continue(t) => {
                self.u8(16);
                self.opt_break_target(t.as_ref())?;
            }
            Stmt::Mod { name, items } => {
                self.u8(17);
                self.str(name);
                self.stmts(items)?;
            }
            Stmt::Struct {
                name,
                type_params,
                fields,
            } => {
                self.u8(18);
                self.str(name);
                self.strs(type_params)?;
                self.params(fields)?;
            }
            Stmt::Enum {
                name,
                type_params,
                variants,
            } => {
                self.u8(19);
                self.str(name);
                self.strs(type_params)?;
                self.u32(variants.len() as u32);
                for v in variants {
                    self.str(&v.name);
                    self.u32(v.fields.len() as u32);
                    for f in &v.fields {
                        self.type_(f)?;
                    }
                }
            }
            Stmt::Impl {
                target,
                trait_name,
                methods,
            } => {
                self.u8(20);
                self.str(target);
                if let Some(_v) = trait_name.as_deref() {
                    self.flag(true);
                    self.str(_v);
                } else {
                    self.flag(false);
                }
                self.stmts(methods)?;
            }
            Stmt::Trait { name, methods } => {
                self.u8(21);
                self.str(name);
                self.u32(methods.len() as u32);
                for m in methods {
                    self.str(&m.name);
                    self.params(&m.params)?;
                    self.opt_type(m.return_type.as_ref())?;
                }
            }
            Stmt::Match { value, arms } => {
                self.u8(22);
                self.expr(value)?;
                self.arms(arms)?;
            }
            Stmt::Try {
                body,
                catch_name,
                catch_body,
            } => {
                self.u8(23);
                self.stmts(body)?;
                if let Some(_v) = catch_name.as_deref() {
                    self.flag(true);
                    self.str(_v);
                } else {
                    self.flag(false);
                }
                self.stmts(catch_body)?;
            }
            Stmt::Raise(e) => {
                self.u8(24);
                self.expr(e)?;
            }
            Stmt::TypeAlias { name, alias } => {
                self.u8(25);
                self.str(name);
                self.type_(alias)?;
            }
            Stmt::Use {
                path,
                is_file,
                alias,
            } => {
                self.u8(26);
                self.strs(path)?;
                self.flag(*is_file);
                if let Some(_v) = alias.as_deref() {
                    self.flag(true);
                    self.str(_v);
                } else {
                    self.flag(false);
                }
            }
            Stmt::Async(body) => {
                self.u8(27);
                self.stmts(body)?;
            }
            Stmt::Export(inner) => {
                self.u8(28);
                self.stmt(inner)?;
            }
            Stmt::Extern { abi, lib, decls } => {
                self.u8(29);
                self.str(abi);
                if let Some(_v) = lib.as_deref() {
                    self.flag(true);
                    self.str(_v);
                } else {
                    self.flag(false);
                }
                self.u32(decls.len() as u32);
                for d in decls {
                    self.str(&d.name);
                    self.params(&d.params)?;
                    self.flag(d.varargs);
                    self.opt_type(d.return_type.as_ref())?;
                }
            }
            Stmt::MacroDef { name, params, body } => {
                self.u8(30);
                self.str(name);
                self.params(params)?;
                self.stmts(body)?;
            }
            Stmt::Const { name, value } => {
                self.u8(31);
                self.str(name);
                self.expr(value)?;
            }
            Stmt::Defer(e) => {
                self.u8(32);
                self.expr(e)?;
            }
            Stmt::Test { name, body } => {
                self.u8(33);
                self.str(name);
                self.stmts(body)?;
            }
            Stmt::Assert(e) => {
                self.u8(34);
                self.expr(e)?;
            }
            Stmt::BinStructDef { name, fields } => {
                self.u8(35);
                self.str(name);
                self.u32(fields.len() as u32);
                for f in fields {
                    self.str(&f.name);
                    self.bin_kind(&f.kind)?;
                    self.opt_expr(f.repeat.as_deref())?;
                }
            }
            Stmt::Tunnel {
                name,
                passphrase,
                body,
            } => {
                self.u8(36);
                self.str(name);
                self.str(passphrase);
                self.stmts(body)?;
            }
            Stmt::Unsafe { reason, body } => {
                self.u8(37);
                self.str(reason);
                self.stmts(body)?;
            }
        }
        Ok(())
    }

    pub fn stmts(&mut self, body: &[Stmt]) -> Result<(), String> {
        self.u32(body.len() as u32);
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn opt_stmts(&mut self, body: Option<&[Stmt]>) -> Result<(), String> {
        match body {
            Some(b) => {
                self.flag(true);
                self.stmts(b)
            }
            None => {
                self.flag(false);
                Ok(())
            }
        }
    }

    fn str_pairs(&mut self, pairs: &[(String, Expr)]) -> Result<(), String> {
        self.u32(pairs.len() as u32);
        for (k, v) in pairs {
            self.str(k);
            self.expr(v)?;
        }
        Ok(())
    }

    fn strs(&mut self, items: &[String]) -> Result<(), String> {
        self.u32(items.len() as u32);
        for i in items {
            self.str(i);
        }
        Ok(())
    }

    fn arms(&mut self, arms: &[(Pattern, Option<Expr>, Vec<Stmt>)]) -> Result<(), String> {
        self.u32(arms.len() as u32);
        for (pat, guard, body) in arms {
            self.pattern(pat)?;
            self.opt_expr(guard.as_ref())?;
            self.stmts(body)?;
        }
        Ok(())
    }

    fn opt_break_target(&mut self, t: Option<&BreakTarget>) -> Result<(), String> {
        match t {
            Some(BreakTarget::Label(s)) => {
                self.u8(1);
                self.str(s);
            }
            Some(BreakTarget::Depth(d)) => {
                self.u8(2);
                self.u32(*d);
            }
            None => self.u8(0),
        }
        Ok(())
    }

    fn opt_pattern(&mut self, p: Option<&Pattern>) -> Result<(), String> {
        match p {
            Some(x) => {
                self.flag(true);
                self.pattern(x)
            }
            None => {
                self.flag(false);
                Ok(())
            }
        }
    }
}

impl Reader<'_> {
    pub fn stmt(&mut self) -> Result<Stmt, String> {
        let tag = self.u8()?;
        match tag {
            1 => {
                let name = self.str()?;
                let pattern = if self.flag()? {
                    Some(self.pattern()?)
                } else {
                    None
                };
                let mutable = self.flag()?;
                let value = self.expr()?;
                let type_hint = self.opt_type()?;
                Ok(Stmt::Let {
                    name,
                    pattern,
                    mutable,
                    value: Box::new(value),
                    type_hint,
                })
            }
            2 => Ok(Stmt::Expr(Box::new(self.expr()?))),
            3 => Ok(Stmt::Return(self.opt_expr()?.map(Box::new))),
            4 => {
                let cond = self.expr()?;
                let then_branch = self.stmts()?;
                let else_branch = self.opt_stmts()?;
                Ok(Stmt::If {
                    cond: Box::new(cond),
                    then_branch,
                    else_branch,
                })
            }
            5 => {
                let cond = self.expr()?;
                let body = self.stmts()?;
                Ok(Stmt::DoWhile {
                    cond: Box::new(cond),
                    body,
                })
            }
            6 => {
                let pattern = self.pattern()?;
                let value = self.expr()?;
                let then_branch = self.stmts()?;
                let else_branch = self.opt_stmts()?;
                Ok(Stmt::IfLet {
                    pattern,
                    value: Box::new(value),
                    then_branch,
                    else_branch,
                })
            }
            7 => {
                let pattern = self.pattern()?;
                let value = self.expr()?;
                let body = self.stmts()?;
                Ok(Stmt::WhileLet {
                    pattern,
                    value: Box::new(value),
                    body,
                })
            }
            8 => {
                let label = self.opt_str_read()?;
                let body = self.stmts()?;
                Ok(Stmt::Loop { label, body })
            }
            9 => {
                let label = self.opt_str_read()?;
                let cond = self.expr()?;
                let body = self.stmts()?;
                Ok(Stmt::While {
                    label,
                    cond: Box::new(cond),
                    body,
                })
            }
            10 => {
                let label = self.opt_str_read()?;
                let pattern = self.pattern()?;
                let iterable = self.expr()?;
                let body = self.stmts()?;
                Ok(Stmt::For {
                    label,
                    pattern,
                    iterable: Box::new(iterable),
                    body,
                })
            }
            11 => {
                let target = self.expr()?;
                let options = self.str_pairs()?;
                let body = self.opt_stmts()?;
                Ok(Stmt::Scan {
                    target: Box::new(target),
                    options,
                    body,
                })
            }
            12 => {
                let target = self.expr()?;
                let options = self.str_pairs()?;
                let body = self.opt_stmts()?;
                Ok(Stmt::Fetch {
                    target: Box::new(target),
                    options,
                    body,
                })
            }
            13 => {
                let value = self.expr()?;
                let target = self.opt_expr()?.map(Box::new);
                Ok(Stmt::Dump {
                    value: Box::new(value),
                    target,
                })
            }
            14 => Ok(Stmt::Trace {
                value: Box::new(self.expr()?),
            }),
            15 => Ok(Stmt::Break(self.opt_break_target()?)),
            16 => Ok(Stmt::Continue(self.opt_break_target()?)),
            17 => {
                let name = self.str()?;
                let items = self.stmts()?;
                Ok(Stmt::Mod { name, items })
            }
            18 => {
                let name = self.str()?;
                let type_params = self.strs()?;
                let fields = self.params()?;
                Ok(Stmt::Struct {
                    name,
                    type_params,
                    fields,
                })
            }
            19 => {
                let name = self.str()?;
                let type_params = self.strs()?;
                let n = self.count("enum variants")?;
                let mut variants = Vec::with_capacity(n);
                for _ in 0..n {
                    let vname = self.str()?;
                    let fn_ = self.count("variant fields")?;
                    let mut fields = Vec::with_capacity(fn_);
                    for _ in 0..fn_ {
                        fields.push(self.type_()?);
                    }
                    variants.push(EnumVariant {
                        name: vname,
                        fields,
                    });
                }
                Ok(Stmt::Enum {
                    name,
                    type_params,
                    variants,
                })
            }
            20 => {
                let target = self.str()?;
                let trait_name = self.opt_str_read()?;
                let methods = self.stmts()?;
                Ok(Stmt::Impl {
                    target,
                    trait_name,
                    methods,
                })
            }
            21 => {
                let name = self.str()?;
                let n = self.count("trait methods")?;
                let mut methods = Vec::with_capacity(n);
                for _ in 0..n {
                    let mname = self.str()?;
                    let params = self.params()?;
                    let return_type = self.opt_type()?;
                    methods.push(TraitMethod {
                        name: mname,
                        params,
                        return_type,
                    });
                }
                Ok(Stmt::Trait { name, methods })
            }
            22 => {
                let value = self.expr()?;
                let arms = self.arms()?;
                Ok(Stmt::Match {
                    value: Box::new(value),
                    arms,
                })
            }
            23 => {
                let body = self.stmts()?;
                let catch_name = self.opt_str_read()?;
                let catch_body = self.stmts()?;
                Ok(Stmt::Try {
                    body,
                    catch_name,
                    catch_body,
                })
            }
            24 => Ok(Stmt::Raise(Box::new(self.expr()?))),
            25 => {
                let name = self.str()?;
                let alias = self.type_()?;
                Ok(Stmt::TypeAlias { name, alias })
            }
            26 => {
                let path = self.strs()?;
                let is_file = self.flag()?;
                let alias = self.opt_str_read()?;
                Ok(Stmt::Use {
                    path,
                    is_file,
                    alias,
                })
            }
            27 => Ok(Stmt::Async(self.stmts()?)),
            28 => Ok(Stmt::Export(Box::new(self.stmt()?))),
            29 => {
                let abi = self.str()?;
                let lib = self.opt_str_read()?;
                let n = self.count("extern decls")?;
                let mut decls = Vec::with_capacity(n);
                for _ in 0..n {
                    let name = self.str()?;
                    let params = self.params()?;
                    let varargs = self.flag()?;
                    let return_type = self.opt_type()?;
                    decls.push(ForeignFn {
                        name,
                        params,
                        varargs,
                        return_type,
                    });
                }
                Ok(Stmt::Extern { abi, lib, decls })
            }
            30 => {
                let name = self.str()?;
                let params = self.params()?;
                let body = self.stmts()?;
                Ok(Stmt::MacroDef { name, params, body })
            }
            31 => {
                let name = self.str()?;
                let value = self.expr()?;
                Ok(Stmt::Const {
                    name,
                    value: Box::new(value),
                })
            }
            32 => Ok(Stmt::Defer(Box::new(self.expr()?))),
            33 => {
                let name = self.str()?;
                let body = self.stmts()?;
                Ok(Stmt::Test { name, body })
            }
            34 => Ok(Stmt::Assert(Box::new(self.expr()?))),
            35 => {
                let name = self.str()?;
                let n = self.count("binstruct fields")?;
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    let fname = self.str()?;
                    let kind = self.bin_kind()?;
                    let repeat = self.opt_expr()?.map(Box::new);
                    fields.push(BinField {
                        name: fname,
                        kind,
                        repeat,
                    });
                }
                Ok(Stmt::BinStructDef { name, fields })
            }
            36 => {
                let name = self.str()?;
                let passphrase = self.str()?;
                let body = self.stmts()?;
                Ok(Stmt::Tunnel {
                    name,
                    passphrase,
                    body,
                })
            }
            37 => {
                let reason = self.str()?;
                let body = self.stmts()?;
                Ok(Stmt::Unsafe { reason, body })
            }
            other => Err(self.bad_tag("statement", other)),
        }
    }

    pub fn stmts(&mut self) -> Result<Vec<Stmt>, String> {
        let n = self.count("statements")?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.stmt()?);
        }
        Ok(out)
    }

    fn opt_stmts(&mut self) -> Result<Option<Vec<Stmt>>, String> {
        if self.flag()? {
            Ok(Some(self.stmts()?))
        } else {
            Ok(None)
        }
    }

    fn strs(&mut self) -> Result<Vec<String>, String> {
        let n = self.count("strings")?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.str()?);
        }
        Ok(out)
    }

    fn str_pairs(&mut self) -> Result<Vec<(String, Expr)>, String> {
        let n = self.count("pairs")?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.str()?;
            let v = self.expr()?;
            out.push((k, v));
        }
        Ok(out)
    }

    fn arms(&mut self) -> Result<Vec<(Pattern, Option<Expr>, Vec<Stmt>)>, String> {
        let n = self.count("match arms")?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let pat = self.pattern()?;
            let guard = self.opt_expr()?;
            let body = self.stmts()?;
            out.push((pat, guard, body));
        }
        Ok(out)
    }

    fn opt_break_target(&mut self) -> Result<Option<BreakTarget>, String> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(BreakTarget::Label(self.str()?))),
            2 => Ok(Some(BreakTarget::Depth(self.u32()?))),
            other => Err(self.bad_tag("break target", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

impl Writer {
    pub fn pattern(&mut self, p: &Pattern) -> Result<(), String> {
        match p {
            Pattern::Wild => self.u8(1),
            Pattern::Ident(s) => {
                self.u8(2);
                self.str(s);
            }
            Pattern::Hex(h) => {
                self.u8(3);
                self.u64(*h);
            }
            Pattern::Int(i) => {
                self.u8(4);
                self.i64(*i);
            }
            Pattern::String(s) => {
                self.u8(5);
                self.str(s);
            }
            Pattern::Bool(b) => {
                self.u8(6);
                self.flag(*b);
            }
            Pattern::Nil => self.u8(7),
            Pattern::Tuple(items) => {
                self.u8(8);
                self.u32(items.len() as u32);
                for i in items {
                    self.pattern(i)?;
                }
            }
            Pattern::Array(items) => {
                self.u8(9);
                self.u32(items.len() as u32);
                for i in items {
                    self.pattern(i)?;
                }
            }
            Pattern::Byte(b) => {
                self.u8(10);
                self.u8(*b);
            }
            Pattern::Bytes(items) => {
                self.u8(11);
                self.u32(items.len() as u32);
                for i in items {
                    match i {
                        BytesPat::Byte(b) => {
                            self.u8(1);
                            self.u8(*b);
                        }
                        BytesPat::Rest => self.u8(2),
                    }
                }
            }
            Pattern::Struct(name, fields) => {
                self.u8(12);
                self.str(name);
                self.u32(fields.len() as u32);
                for (n, p) in fields {
                    self.str(n);
                    self.pattern(p)?;
                }
            }
            Pattern::EnumVariant(enum_name, variant, subs) => {
                self.u8(13);
                self.str(enum_name);
                self.str(variant);
                self.u32(subs.len() as u32);
                for p in subs {
                    self.pattern(p)?;
                }
            }
            Pattern::Range(lo, hi) => {
                self.u8(14);
                self.pattern(lo)?;
                self.pattern(hi)?;
            }
            Pattern::Or(alts) => {
                self.u8(15);
                self.u32(alts.len() as u32);
                for p in alts {
                    self.pattern(p)?;
                }
            }
            Pattern::Some(p) => {
                self.u8(16);
                self.pattern(p)?;
            }
            Pattern::None => self.u8(17),
            Pattern::Ok(p) => {
                self.u8(18);
                self.pattern(p)?;
            }
            Pattern::Err(p) => {
                self.u8(19);
                self.pattern(p)?;
            }
        }
        Ok(())
    }
}

impl Reader<'_> {
    pub fn pattern(&mut self) -> Result<Pattern, String> {
        let tag = self.u8()?;
        match tag {
            1 => Ok(Pattern::Wild),
            2 => Ok(Pattern::Ident(self.str()?)),
            3 => Ok(Pattern::Hex(self.u64()?)),
            4 => Ok(Pattern::Int(self.i64()?)),
            5 => Ok(Pattern::String(self.str()?)),
            6 => Ok(Pattern::Bool(self.flag()?)),
            7 => Ok(Pattern::Nil),
            8 => Ok(Pattern::Tuple(self.patterns()?)),
            9 => Ok(Pattern::Array(self.patterns()?)),
            10 => Ok(Pattern::Byte(self.u8()?)),
            11 => {
                let n = self.count("byte patterns")?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    match self.u8()? {
                        1 => items.push(BytesPat::Byte(self.u8()?)),
                        2 => items.push(BytesPat::Rest),
                        other => return Err(self.bad_tag("byte pattern", other)),
                    }
                }
                Ok(Pattern::Bytes(items))
            }
            12 => {
                let name = self.str()?;
                let n = self.count("struct pattern fields")?;
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    let fname = self.str()?;
                    let p = self.pattern()?;
                    fields.push((fname, p));
                }
                Ok(Pattern::Struct(name, fields))
            }
            13 => {
                let enum_name = self.str()?;
                let variant = self.str()?;
                let subs = self.patterns()?;
                Ok(Pattern::EnumVariant(enum_name, variant, subs))
            }
            14 => {
                let lo = self.pattern()?;
                let hi = self.pattern()?;
                Ok(Pattern::Range(Box::new(lo), Box::new(hi)))
            }
            15 => Ok(Pattern::Or(self.patterns()?)),
            16 => Ok(Pattern::Some(Box::new(self.pattern()?))),
            17 => Ok(Pattern::None),
            18 => Ok(Pattern::Ok(Box::new(self.pattern()?))),
            19 => Ok(Pattern::Err(Box::new(self.pattern()?))),
            other => Err(self.bad_tag("pattern", other)),
        }
    }

    fn patterns(&mut self) -> Result<Vec<Pattern>, String> {
        let n = self.count("patterns")?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.pattern()?);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Types, params, and the small enums
// ---------------------------------------------------------------------------

impl Writer {
    pub fn type_(&mut self, t: &Type) -> Result<(), String> {
        match t {
            Type::Hex(bits) => {
                self.u8(1);
                self.u64(*bits as u64);
            }
            Type::Int => self.u8(2),
            Type::I8 => self.u8(3),
            Type::I16 => self.u8(4),
            Type::I32 => self.u8(5),
            Type::I64 => self.u8(6),
            Type::U8 => self.u8(7),
            Type::U16 => self.u8(8),
            Type::U32 => self.u8(9),
            Type::U64 => self.u8(10),
            Type::F32 => self.u8(11),
            Type::F64 => self.u8(12),
            Type::String => self.u8(13),
            Type::Char => self.u8(14),
            Type::Bytes => self.u8(15),
            Type::Bool => self.u8(16),
            Type::Nil => self.u8(17),
            Type::Array(inner) => {
                self.u8(18);
                self.type_(inner)?;
            }
            Type::Tuple(items) => {
                self.u8(19);
                self.u32(items.len() as u32);
                for i in items {
                    self.type_(i)?;
                }
            }
            Type::Map(k, v) => {
                self.u8(20);
                self.type_(k)?;
                self.type_(v)?;
            }
            Type::Function(params, ret) => {
                self.u8(21);
                self.u32(params.len() as u32);
                for p in params {
                    self.type_(p)?;
                }
                self.type_(ret)?;
            }
            Type::Custom(s) => {
                self.u8(22);
                self.str(s);
            }
            Type::Generic(s) => {
                self.u8(23);
                self.str(s);
            }
            Type::Option(inner) => {
                self.u8(24);
                self.type_(inner)?;
            }
            Type::Result(ok, err) => {
                self.u8(25);
                self.type_(ok)?;
                self.type_(err)?;
            }
            Type::Ptr(inner) => {
                self.u8(26);
                self.type_(inner)?;
            }
            Type::Void => self.u8(27),
            Type::Evidence(inner) => {
                self.u8(28);
                self.type_(inner)?;
            }
        }
        Ok(())
    }

    fn opt_type(&mut self, t: Option<&Type>) -> Result<(), String> {
        match t {
            Some(x) => {
                self.flag(true);
                self.type_(x)
            }
            None => {
                self.flag(false);
                Ok(())
            }
        }
    }

    pub fn params(&mut self, params: &[Param]) -> Result<(), String> {
        self.u32(params.len() as u32);
        for p in params {
            self.str(&p.name);
            self.opt_type(p.type_hint.as_ref())?;
            self.opt_expr(p.default.as_deref())?;
            self.flag(p.rest);
            self.flag(p.optional);
        }
        Ok(())
    }

    /// The call-time parameter shape the VM records beside a closure body.
    pub fn param_shape(&mut self, p: &ParamShape) -> Result<(), String> {
        self.str(&p.name);
        self.flag(p.rest);
        self.flag(p.optional);
        self.flag(p.has_default);
        Ok(())
    }

    pub fn param_shapes(&mut self, params: &[ParamShape]) -> Result<(), String> {
        self.u32(params.len() as u32);
        for p in params {
            self.param_shape(p)?;
        }
        Ok(())
    }

    fn int_kind(&mut self, k: IntKind) {
        self.u8(match k {
            IntKind::I8 => 1,
            IntKind::I16 => 2,
            IntKind::I32 => 3,
            IntKind::I64 => 4,
            IntKind::U8 => 5,
            IntKind::U16 => 6,
            IntKind::U32 => 7,
            IntKind::U64 => 8,
        });
    }

    fn un_op(&mut self, op: UnOp) {
        self.u8(match op {
            UnOp::Minus => 1,
            UnOp::Not => 2,
            UnOp::BitNot => 3,
        });
    }

    fn bin_op(&mut self, op: BinOp) {
        self.u8(match op {
            BinOp::Add => 1,
            BinOp::Sub => 2,
            BinOp::In => 3,
            BinOp::Mul => 4,
            BinOp::Div => 5,
            BinOp::Rem => 6,
            BinOp::And => 7,
            BinOp::Or => 8,
            BinOp::BitAnd => 9,
            BinOp::BitOr => 10,
            BinOp::BitXor => 11,
            BinOp::Shl => 12,
            BinOp::Shr => 13,
            BinOp::Eq => 14,
            BinOp::NotEq => 15,
            BinOp::Lt => 16,
            BinOp::Gt => 17,
            BinOp::LtEq => 18,
            BinOp::GtEq => 19,
        });
    }

    fn compound_op(&mut self, op: CompoundOp) {
        self.u8(match op {
            CompoundOp::Add => 1,
            CompoundOp::Sub => 2,
            CompoundOp::Mul => 3,
            CompoundOp::Div => 4,
            CompoundOp::Rem => 5,
            CompoundOp::BitAnd => 6,
            CompoundOp::BitOr => 7,
            CompoundOp::BitXor => 8,
            CompoundOp::Shl => 9,
            CompoundOp::Shr => 10,
        });
    }

    fn bin_kind(&mut self, k: &BinKind) -> Result<(), String> {
        match k {
            BinKind::Uint { bits, endian } => {
                self.u8(1);
                self.u8(*bits);
                self.endian(*endian);
            }
            BinKind::Int { bits, endian } => {
                self.u8(2);
                self.u8(*bits);
                self.endian(*endian);
            }
            BinKind::Bits { bits } => {
                self.u8(3);
                self.u8(*bits);
            }
            BinKind::Bytes(n) => {
                self.u8(4);
                self.u64(*n as u64);
            }
            BinKind::Rest => self.u8(5),
            BinKind::Ref(name) => {
                self.u8(6);
                self.str(name);
            }
        }
        Ok(())
    }

    fn endian(&mut self, e: Endian) {
        self.u8(match e {
            Endian::Big => 1,
            Endian::Little => 2,
        });
    }
}

impl Reader<'_> {
    pub fn type_(&mut self) -> Result<Type, String> {
        let tag = self.u8()?;
        match tag {
            1 => Ok(Type::Hex(self.u64()? as usize)),
            2 => Ok(Type::Int),
            3 => Ok(Type::I8),
            4 => Ok(Type::I16),
            5 => Ok(Type::I32),
            6 => Ok(Type::I64),
            7 => Ok(Type::U8),
            8 => Ok(Type::U16),
            9 => Ok(Type::U32),
            10 => Ok(Type::U64),
            11 => Ok(Type::F32),
            12 => Ok(Type::F64),
            13 => Ok(Type::String),
            14 => Ok(Type::Char),
            15 => Ok(Type::Bytes),
            16 => Ok(Type::Bool),
            17 => Ok(Type::Nil),
            18 => Ok(Type::Array(Box::new(self.type_()?))),
            19 => {
                let n = self.count("tuple types")?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.type_()?);
                }
                Ok(Type::Tuple(items))
            }
            20 => {
                let k = self.type_()?;
                let v = self.type_()?;
                Ok(Type::Map(Box::new(k), Box::new(v)))
            }
            21 => {
                let n = self.count("fn param types")?;
                let mut params = Vec::with_capacity(n);
                for _ in 0..n {
                    params.push(self.type_()?);
                }
                let ret = self.type_()?;
                Ok(Type::Function(params, Box::new(ret)))
            }
            22 => Ok(Type::Custom(self.str()?)),
            23 => Ok(Type::Generic(self.str()?)),
            24 => Ok(Type::Option(Box::new(self.type_()?))),
            25 => {
                let ok = self.type_()?;
                let err = self.type_()?;
                Ok(Type::Result(Box::new(ok), Box::new(err)))
            }
            26 => Ok(Type::Ptr(Box::new(self.type_()?))),
            27 => Ok(Type::Void),
            28 => Ok(Type::Evidence(Box::new(self.type_()?))),
            other => Err(self.bad_tag("type", other)),
        }
    }

    fn opt_type(&mut self) -> Result<Option<Type>, String> {
        if self.flag()? {
            Ok(Some(self.type_()?))
        } else {
            Ok(None)
        }
    }

    pub fn params(&mut self) -> Result<Vec<Param>, String> {
        let n = self.count("params")?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let name = self.str()?;
            let type_hint = self.opt_type()?;
            let default = self.opt_expr()?.map(Box::new);
            let rest = self.flag()?;
            let optional = self.flag()?;
            out.push(Param {
                name,
                type_hint,
                default,
                rest,
                optional,
            });
        }
        Ok(out)
    }

    pub fn param_shapes(&mut self) -> Result<Vec<ParamShape>, String> {
        let n = self.count("param shapes")?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let name = self.str()?;
            let rest = self.flag()?;
            let optional = self.flag()?;
            let has_default = self.flag()?;
            out.push(ParamShape {
                name: std::sync::Arc::from(name.as_str()),
                rest,
                optional,
                has_default,
            });
        }
        Ok(out)
    }

    fn int_kind(&mut self) -> Result<IntKind, String> {
        Ok(match self.u8()? {
            1 => IntKind::I8,
            2 => IntKind::I16,
            3 => IntKind::I32,
            4 => IntKind::I64,
            5 => IntKind::U8,
            6 => IntKind::U16,
            7 => IntKind::U32,
            8 => IntKind::U64,
            other => return Err(self.bad_tag("integer kind", other)),
        })
    }

    fn un_op(&mut self) -> Result<UnOp, String> {
        Ok(match self.u8()? {
            1 => UnOp::Minus,
            2 => UnOp::Not,
            3 => UnOp::BitNot,
            other => return Err(self.bad_tag("unary op", other)),
        })
    }

    fn bin_op(&mut self) -> Result<BinOp, String> {
        Ok(match self.u8()? {
            1 => BinOp::Add,
            2 => BinOp::Sub,
            3 => BinOp::In,
            4 => BinOp::Mul,
            5 => BinOp::Div,
            6 => BinOp::Rem,
            7 => BinOp::And,
            8 => BinOp::Or,
            9 => BinOp::BitAnd,
            10 => BinOp::BitOr,
            11 => BinOp::BitXor,
            12 => BinOp::Shl,
            13 => BinOp::Shr,
            14 => BinOp::Eq,
            15 => BinOp::NotEq,
            16 => BinOp::Lt,
            17 => BinOp::Gt,
            18 => BinOp::LtEq,
            19 => BinOp::GtEq,
            other => return Err(self.bad_tag("binary op", other)),
        })
    }

    fn compound_op(&mut self) -> Result<CompoundOp, String> {
        Ok(match self.u8()? {
            1 => CompoundOp::Add,
            2 => CompoundOp::Sub,
            3 => CompoundOp::Mul,
            4 => CompoundOp::Div,
            5 => CompoundOp::Rem,
            6 => CompoundOp::BitAnd,
            7 => CompoundOp::BitOr,
            8 => CompoundOp::BitXor,
            9 => CompoundOp::Shl,
            10 => CompoundOp::Shr,
            other => return Err(self.bad_tag("compound op", other)),
        })
    }

    fn bin_kind(&mut self) -> Result<BinKind, String> {
        Ok(match self.u8()? {
            1 => BinKind::Uint {
                bits: self.u8()?,
                endian: self.endian()?,
            },
            2 => BinKind::Int {
                bits: self.u8()?,
                endian: self.endian()?,
            },
            3 => BinKind::Bits { bits: self.u8()? },
            4 => BinKind::Bytes(self.u64()? as usize),
            5 => BinKind::Rest,
            6 => BinKind::Ref(self.str()?),
            other => return Err(self.bad_tag("binstruct field kind", other)),
        })
    }

    fn endian(&mut self) -> Result<Endian, String> {
        Ok(match self.u8()? {
            1 => Endian::Big,
            2 => Endian::Little,
            other => return Err(self.bad_tag("endian", other)),
        })
    }
}

/// Public wrappers the container and tests use.
pub fn write_params(w: &mut Writer, params: &[Param]) -> Result<(), String> {
    w.params(params)
}
pub fn read_params(r: &mut Reader) -> Result<Vec<Param>, String> {
    r.params()
}
pub fn write_stmts(w: &mut Writer, body: &[Stmt]) -> Result<(), String> {
    w.stmts(body)
}
pub fn read_stmts(r: &mut Reader) -> Result<Vec<Stmt>, String> {
    r.stmts()
}
pub fn write_param_shapes(w: &mut Writer, params: &[ParamShape]) -> Result<(), String> {
    w.param_shapes(params)
}
pub fn read_param_shapes(r: &mut Reader) -> Result<Vec<ParamShape>, String> {
    r.param_shapes()
}
pub fn write_type(w: &mut Writer, t: &Type) -> Result<(), String> {
    w.type_(t)
}
pub fn read_type(r: &mut Reader) -> Result<Type, String> {
    r.type_()
}
