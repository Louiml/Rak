use crate::ast::*;
use crate::bytecode::{Chunk, Op};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Compiler {
    chunk: Chunk,
    locals: Vec<(String, usize)>,
    scope_depth: usize,
    func_names: std::collections::HashSet<String>,
    func_closures: HashMap<String, Value>,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
            func_names: std::collections::HashSet::new(),
            func_closures: HashMap::new(),
        }
    }

    pub fn compile(&mut self, module: &Module) -> Result<Chunk, String> {
        for stmt in &module.items {
            if let Stmt::Let { name, value, .. } = stmt {
                if let Expr::Function { params, body, .. } = value.as_ref() {
                    self.func_names.insert(name.clone());
                    let closure = self.compile_function(name, params, body)?;
                    self.func_closures.insert(name.clone(), closure);
                }
            }
        }
        let closures: Vec<(String, Value)> = self.func_closures.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (name, closure) in closures {
            self.load_const(closure);
            let ci = self.const_str(&name);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        for stmt in &module.items {
            if let Stmt::Let { value, .. } = stmt {
                if matches!(value.as_ref(), Expr::Function { .. }) {
                    continue;
                }
            }
            self.compile_stmt(stmt)?;
        }
        self.emit_op(Op::Nil);
        self.emit_op(Op::Return);
        Ok(std::mem::replace(&mut self.chunk, Chunk::new()))
    }

    fn compile_function(&self, name: &str, params: &[Param], body: &[Stmt]) -> Result<Value, String> {
        let mut sub = Compiler::new();
        sub.scope_depth = 1;
        for p in params {
            sub.add_local(p.name.clone());
        }
        sub.func_names = self.func_names.clone();
        sub.func_closures = self.func_closures.clone();
        for s in body {
            sub.compile_stmt(s)?;
        }
        sub.emit_op(Op::Nil);
        sub.emit_op(Op::Return);
        let sub_chunk = std::mem::replace(&mut sub.chunk, Chunk::new());
        Ok(Value::Closure {
            code: Arc::from(sub_chunk),
            nparams: params.len(),
            name: Arc::from(name),
        })
    }

    fn line(&self) -> u32 {
        0
    }

    fn emit_op(&mut self, op: Op) {
        self.chunk.write_op(op, self.line());
    }

    fn emit_byte(&mut self, b: u8) {
        self.chunk.write_byte(b, self.line());
    }

    fn emit_u16(&mut self, v: u16) {
        self.chunk.write_u16(v, self.line());
    }

    fn emit_const(&mut self, value: Value) -> u16 {
        self.chunk.add_const(value)
    }

    fn load_const(&mut self, value: Value) {
        let ci = self.chunk.add_const(value);
        self.emit_op(Op::LoadConst);
        self.emit_u16(ci);
    }

    fn emit_jump(&mut self, op: Op) -> usize {
        self.emit_op(op);
        let pos = self.chunk.code.len();
        self.emit_byte(0);
        self.emit_byte(0);
        pos
    }

    fn patch_jump(&mut self, pos: usize) {
        let target = self.chunk.code.len() as u16;
        self.chunk.code[pos] = (target >> 8) as u8;
        self.chunk.code[pos + 1] = (target & 0xFF) as u8;
    }

    fn add_local(&mut self, name: String) -> u8 {
        let slot = self.locals.len() as u8;
        self.locals.push((name, self.scope_depth));
        slot
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        self.locals.iter().rposition(|(n, _)| n == name).map(|i| i as u8)
    }

    fn const_str(&mut self, s: &str) -> u16 {
        self.emit_const(Value::String(Arc::from(s)))
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                self.compile_expr(value)?;
                if self.scope_depth == 0 {
                    let ci = self.const_str(name);
                    self.emit_op(Op::StoreGlobal);
                    self.emit_u16(ci);
                } else {
                    let slot = self.add_local(name.clone());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                }
            }
            Stmt::Expr(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::Pop);
            }
            Stmt::Dump { value, target: _ } => {
                self.compile_expr(value)?;
                self.emit_op(Op::Print);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.compile_expr(e)?;
                } else {
                    self.emit_op(Op::Nil);
                }
                self.emit_op(Op::Return);
            }
            Stmt::If { cond, then_branch, else_branch } => {
                self.compile_expr(cond)?;
                let jfalse = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.begin_scope();
                for s in then_branch {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                let jend = self.emit_jump(Op::Jump);
                self.patch_jump(jfalse);
                self.emit_op(Op::Pop);
                if let Some(else_stmts) = else_branch {
                    self.begin_scope();
                    for s in else_stmts {
                        self.compile_stmt(s)?;
                    }
                    self.end_scope();
                }
                self.patch_jump(jend);
            }
            Stmt::While { cond, body } => {
                let loop_start = self.chunk.code.len();
                self.compile_expr(cond)?;
                let jexit = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_jump_back(loop_start);
                self.patch_jump(jexit);
                self.emit_op(Op::Pop);
            }
            Stmt::Loop(body) => {
                let loop_start = self.chunk.code.len();
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_jump_back(loop_start);
            }
            Stmt::For { name, iterable, body } => self.compile_for(name, iterable, body)?,
            other => {
                return Err(format!("VM does not support statement: {:?}", other));
            }
        }
        Ok(())
    }

    fn emit_jump_back(&mut self, target: usize) {
        self.emit_op(Op::Jump);
        let t = target as u16;
        self.emit_byte((t >> 8) as u8);
        self.emit_byte((t & 0xFF) as u8);
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        if self.scope_depth > 0 {
            self.scope_depth -= 1;
        }
        while let Some((_, d)) = self.locals.last() {
            if *d > self.scope_depth {
                self.locals.pop();
                self.emit_op(Op::Pop);
            } else {
                break;
            }
        }
    }

    fn compile_for(&mut self, name: &str, iterable: &Expr, body: &[Stmt]) -> Result<(), String> {
        match iterable {
            Expr::Range(lo, hi) => {
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    self.compile_expr(lo)?;
                    let lo_slot = self.add_local("__for_lo".to_string());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(lo_slot);
                    self.compile_expr(hi)?;
                    let hi_slot = self.add_local("__for_hi".to_string());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(hi_slot);
                    let loop_start = self.chunk.code.len();
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(lo_slot);
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(hi_slot);
                    self.emit_op(Op::LtEq);
                    let jexit = self.emit_jump(Op::JumpIfFalse);
                    self.emit_op(Op::Pop);
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(lo_slot);
                    let item_slot = self.add_local(name.to_string());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(item_slot);
                    self.begin_scope();
                    for s in body {
                        self.compile_stmt(s)?;
                    }
                    self.end_scope();
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(lo_slot);
                    self.emit_op(Op::Dup);
                    self.load_const(Value::I64(1));
                    self.emit_op(Op::AddI);
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(lo_slot);
                    self.emit_jump_back(loop_start);
                    self.patch_jump(jexit);
                    self.emit_op(Op::Pop);
                    self.emit_op(Op::Pop);
                    self.emit_op(Op::Pop);
                }
                Ok(())
            }
            _ => {
                self.compile_expr(iterable)?;
                let arr_slot = self.add_local("__for_arr".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(arr_slot);
                self.load_const(Value::I64(0));
                let idx_slot = self.add_local("__for_idx".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(idx_slot);
                let loop_start = self.chunk.code.len();
                self.emit_op(Op::LoadLocal);
                self.emit_byte(idx_slot);
                self.emit_op(Op::LoadLocal);
                self.emit_byte(arr_slot);
                self.emit_op(Op::IndexGet);
                let jexit = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                let item_slot = self.add_local(name.to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(item_slot);
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_op(Op::LoadLocal);
                self.emit_byte(idx_slot);
                self.emit_op(Op::Dup);
                self.load_const(Value::I64(1));
                self.emit_op(Op::AddI);
                self.emit_op(Op::StoreLocal);
                self.emit_byte(idx_slot);
                self.emit_jump_back(loop_start);
                self.patch_jump(jexit);
                self.emit_op(Op::Pop);
                self.emit_op(Op::Pop);
                self.emit_op(Op::Pop);
                Ok(())
            }
        }
    }

    fn compile_block_value(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        if stmts.is_empty() {
            self.emit_op(Op::Nil);
            return Ok(());
        }
        let (last, rest) = stmts.split_last().unwrap();
        for s in rest {
            self.compile_stmt(s)?;
        }
        match last {
            Stmt::Expr(e) => self.compile_expr(e)?,
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.compile_expr(e)?;
                } else {
                    self.emit_op(Op::Nil);
                }
                self.emit_op(Op::Return);
            }
            other => {
                self.compile_stmt(other)?;
                self.emit_op(Op::Nil);
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Int(i) => {
                let ci = self.emit_const(Value::I64(*i));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Hex(h) => {
                let ci = self.emit_const(Value::Hex(*h, 64));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Float(f) => {
                let ci = self.emit_const(Value::F64(*f));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Float32(f) => {
                let ci = self.emit_const(Value::F32(*f));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::String(s) => {
                let ci = self.const_str(s);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Bool(b) => {
                self.emit_op(if *b { Op::True } else { Op::False });
            }
            Expr::Nil => {
                self.emit_op(Op::Nil);
            }
            Expr::Ident(name) => {
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(slot);
                } else {
                    let ci = self.const_str(name);
                    self.emit_op(Op::LoadGlobal);
                    self.emit_u16(ci);
                }
            }
            Expr::Unary(op, e) => {
                self.compile_expr(e)?;
                match op {
                    UnOp::Minus => self.emit_op(Op::NegI),
                    UnOp::Not => self.emit_op(Op::Not),
                    UnOp::BitNot => self.emit_op(Op::BitNot),
                }
            }
            Expr::Binary(op, l, r) => {
                self.compile_expr(l)?;
                self.compile_expr(r)?;
                self.emit_op(match op {
                    BinOp::Add => Op::AddI,
                    BinOp::Sub => Op::SubI,
                    BinOp::Mul => Op::MulI,
                    BinOp::Div => Op::DivI,
                    BinOp::Rem => Op::RemI,
                    BinOp::BitAnd => Op::BitAnd,
                    BinOp::BitOr => Op::BitOr,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::Shl => Op::Shl,
                    BinOp::Shr => Op::Shr,
                    BinOp::Eq => Op::Eq,
                    BinOp::NotEq => Op::NotEq,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::LtEq => Op::LtEq,
                    BinOp::GtEq => Op::GtEq,
                    BinOp::And => Op::Nop,
                    BinOp::Or => Op::Nop,
                });
            }
            Expr::Assign(name, value) => {
                self.compile_expr(value)?;
                self.emit_op(Op::Dup);
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                } else {
                    let ci = self.const_str(name);
                    self.emit_op(Op::StoreGlobal);
                    self.emit_u16(ci);
                }
            }
            Expr::Array(items) => {
                for e in items {
                    self.compile_expr(e)?;
                }
                let ci = self.emit_const(Value::I64(items.len() as i64));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::NewArray);
            }
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    if self.func_names.contains(name) {
                        let ci = self.const_str(name);
                        self.emit_op(Op::LoadGlobal);
                        self.emit_u16(ci);
                        for a in args {
                            self.compile_expr(a)?;
                        }
                        self.emit_op(Op::Call);
                        self.emit_byte(args.len() as u8);
                        return Ok(());
                    }
                }
                self.compile_expr(callee)?;
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(Op::Call);
                self.emit_byte(args.len() as u8);
            }
            Expr::If { cond, then_branch, else_branch } => {
                self.compile_expr(cond)?;
                let jfalse = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.compile_block_value(then_branch)?;
                let jend = self.emit_jump(Op::Jump);
                self.patch_jump(jfalse);
                self.emit_op(Op::Pop);
                if let Some(else_stmts) = else_branch {
                    self.compile_block_value(else_stmts)?;
                } else {
                    self.emit_op(Op::Nil);
                }
                self.patch_jump(jend);
            }
            Expr::Block(stmts) => {
                self.compile_block_value(stmts)?;
            }
            Expr::Tuple(items) => {
                for e in items {
                    self.compile_expr(e)?;
                }
                self.load_const(Value::I64(items.len() as i64));
                self.emit_op(Op::NewTuple);
            }
            Expr::Map(pairs) => {
                for (k, v) in pairs {
                    self.compile_expr(k)?;
                    self.compile_expr(v)?;
                }
                self.load_const(Value::I64(pairs.len() as i64));
                self.emit_op(Op::NewMap);
            }
            Expr::Index(obj, idx) => {
                self.compile_expr(obj)?;
                self.compile_expr(idx)?;
                self.emit_op(Op::IndexGet);
            }
            Expr::FieldAccess(obj, field) => {
                self.compile_expr(obj)?;
                let ci = self.const_str(field);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::FieldGet);
            }
            Expr::Interp { template, parts } => {
                let fmt_str = interp_to_fmt(template);
                let fmt_ci = self.const_str("fmt");
                let tpl_ci = self.const_str(&fmt_str);
                self.emit_op(Op::LoadGlobal);
                self.emit_u16(fmt_ci);
                self.emit_op(Op::LoadConst);
                self.emit_u16(tpl_ci);
                for p in parts {
                    self.compile_expr(p)?;
                }
                self.emit_op(Op::Call);
                self.emit_byte((1 + parts.len()) as u8);
            }
            Expr::Match { value, arms } => {
                self.compile_match(value, arms)?;
            }
            Expr::Regex(pattern, flags) => {
                let v = make_regex_value(pattern, flags)?;
                let ci = self.emit_const(v);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            other => {
                return Err(format!("VM does not support expression: {:?}", other));
            }
        }
        Ok(())
    }

    fn compile_match(&mut self, value: &Expr, arms: &[(Pattern, Option<Expr>, Vec<Stmt>)]) -> Result<(), String> {
        self.compile_expr(value)?;
        let v_slot = self.add_local("__match_v".to_string());
        self.emit_op(Op::StoreLocal);
        self.emit_byte(v_slot);
        let mut end_jumps: Vec<usize> = Vec::new();
        for (pattern, guard, body) in arms {
            self.emit_op(Op::LoadLocal);
            self.emit_byte(v_slot);
            let bind = self.compile_pattern(pattern)?;
            let jnext = self.emit_jump(Op::JumpIfFalse);
            self.emit_op(Op::Pop);
            if let Some(name) = bind {
                self.begin_scope();
                let slot = self.add_local(name);
                self.emit_op(Op::LoadLocal);
                self.emit_byte(v_slot);
                self.emit_op(Op::StoreLocal);
                self.emit_byte(slot);
            } else {
                self.begin_scope();
            }
            if let Some(g) = guard {
                self.compile_expr(g)?;
                let jg = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.compile_block_value(body)?;
                self.end_scope_for_match();
                let jend = self.emit_jump(Op::Jump);
                end_jumps.push(jend);
                self.patch_jump(jg);
                self.emit_op(Op::Pop);
                self.patch_jump(jnext);
                let _ = jg;
            } else {
                self.compile_block_value(body)?;
                self.end_scope_for_match();
                let jend = self.emit_jump(Op::Jump);
                end_jumps.push(jend);
                self.patch_jump(jnext);
            }
        }
        self.emit_op(Op::Nil);
        for j in end_jumps {
            self.patch_jump(j);
        }
        Ok(())
    }

    fn end_scope_for_match(&mut self) {
        while let Some((_, d)) = self.locals.last() {
            if *d > self.scope_depth {
                self.locals.pop();
            } else {
                break;
            }
        }
    }

    fn compile_pattern(&mut self, pattern: &Pattern) -> Result<Option<String>, String> {
        match pattern {
            Pattern::Wild => {
                self.emit_op(Op::Pop);
                self.emit_op(Op::True);
                Ok(None)
            }
            Pattern::Ident(n) => {
                self.emit_op(Op::Pop);
                self.emit_op(Op::True);
                Ok(Some(n.clone()))
            }
            Pattern::Int(i) => {
                self.load_const(Value::I64(*i));
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::Hex(h) => {
                self.load_const(Value::Hex(*h, 64));
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::String(s) => {
                let ci = self.const_str(s);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::Bool(b) => {
                self.emit_op(if *b { Op::True } else { Op::False });
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::Nil => {
                self.emit_op(Op::Nil);
                self.emit_op(Op::Eq);
                Ok(None)
            }
            other => Err(format!("VM match does not support pattern: {:?}", other)),
        }
    }
}

fn interp_to_fmt(template: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                out.push_str("{{");
            } else {
                out.push_str("{}");
                let mut depth = 1;
                while let Some(c2) = chars.next() {
                    if c2 == '{' {
                        depth += 1;
                    } else if c2 == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
            }
        } else if c == '}' {
            if chars.peek() == Some(&'}') {
                chars.next();
                out.push_str("}}");
            } else {
                out.push('}');
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn compile_module(module: &Module) -> Result<Chunk, String> {
    let mut c = Compiler::new();
    c.compile(module)
}

/// Build a `Value::Regex` constant, compiling the pattern at compile time so
/// the VM never pays for regex construction at runtime.
fn make_regex_value(pattern: &str, flags: &str) -> Result<Value, String> {
    let mut b = regex::RegexBuilder::new(pattern);
    for f in flags.chars() {
        match f {
            'i' | 'I' => b.case_insensitive(true),
            'm' | 'M' => b.multi_line(true),
            's' | 'S' => b.dot_matches_new_line(true),
            'x' | 'X' => b.ignore_whitespace(true),
            'g' | 'G' => continue,
            _ => return Err(format!("unknown regex flag '{}'", f)),
        };
    }
    let re = b.build().map_err(|e| format!("invalid regex /{}/{}: {}", pattern, flags, e))?;
    Ok(Value::Regex(Arc::new(crate::value::RegexValue {
        pattern: pattern.to_string(),
        flags: flags.to_string(),
        re,
    })))
}
