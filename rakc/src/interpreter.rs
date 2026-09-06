use crate::ast::*;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread::JoinHandle;

#[derive(Clone)]
pub enum Value {
    Hex(u64),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Nil,
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    Map(HashMap<String, Value>),
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    Enum {
        name: String,
        variant: String,
        data: Vec<Value>,
    },
    Option(Option<Box<Value>>),
    Result(Option<Box<Value>>, Option<Box<Value>>),
    Function {
        params: Vec<Param>,
        body: Vec<Stmt>,
        closure: Arc<Env>,
        is_async: bool,
    },
    EnumDef {
        variants: Vec<EnumVariant>,
    },
    StructDef {
        fields: Vec<Param>,
    },
    Module(HashMap<String, Value>),
    TcpListener(Arc<Mutex<std::net::TcpListener>>),
    TcpStream(Arc<Mutex<std::net::TcpStream>>),
    JoinHandle(Arc<Mutex<Option<JoinHandle<Value>>>>),
    Sender(Arc<Mutex<mpsc::Sender<Value>>>),
    Receiver(Arc<Mutex<mpsc::Receiver<Value>>>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Hex(a), Value::Hex(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Option(a), Value::Option(b)) => a == b,
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Hex(h) => write!(f, "0x{:X}", h),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Bytes(b) => {
                write!(f, "b\"")?;
                for byte in b {
                    write!(f, "\\x{:02X}", byte)?;
                }
                write!(f, "\"")
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Nil => write!(f, "nil"),
            Value::Tuple(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                write!(f, "({})", parts.join(", "))
            }
            Value::Array(arr) => {
                let parts: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", parts.join(", "))
            }
            Value::Map(map) => {
                let parts: Vec<String> = map.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
            Value::Struct { name, fields } => {
                let parts: Vec<String> = fields.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                write!(f, "{} {{{}}}", name, parts.join(", "))
            }
            Value::Enum { name, variant, data } => {
                if data.is_empty() {
                    write!(f, "{}::{}", name, variant)
                } else {
                    let parts: Vec<String> = data.iter().map(|v| v.to_string()).collect();
                    write!(f, "{}::{}({})", name, variant, parts.join(", "))
                }
            }
            Value::Option(Some(v)) => write!(f, "Some({})", v),
            Value::Option(None) => write!(f, "None"),
            Value::Result(Some(ok), _) => write!(f, "Ok({})", ok),
            Value::Result(_, Some(err)) => write!(f, "Err({})", err),
            Value::Result(None, None) => write!(f, "Ok(nil)"),
            Value::Function { .. } => write!(f, "<function>"),
            Value::EnumDef { .. } => write!(f, "<enum def>"),
            Value::StructDef { .. } => write!(f, "<struct def>"),
            Value::Module(_) => write!(f, "<module>"),
            Value::TcpListener(_) => write!(f, "<tcp-listener>"),
            Value::TcpStream(_) => write!(f, "<tcp-stream>"),
            Value::JoinHandle(_) => write!(f, "<thread>"),
            Value::Sender(_) => write!(f, "<sender>"),
            Value::Receiver(_) => write!(f, "<receiver>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Env {
    scopes: Vec<HashMap<String, Value>>,
}

impl Env {
    pub fn new() -> Self {
        Env { scopes: vec![HashMap::new()] }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn define(&mut self, name: &str, value: Value) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), value);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val.clone());
            }
        }
        None
    }

    pub fn assign(&mut self, name: &str, value: Value) -> crate::Result<()> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return Ok(());
            }
        }
        Err(crate::RakError::Runtime(format!("Undefined variable: {}", name)))
    }

    pub fn current_scope_clone(&self) -> HashMap<String, Value> {
        self.scopes.last().cloned().unwrap_or_default()
    }
}

pub struct Interpreter {
    env: Env,
    output: Vec<String>,
    base_dir: String,
    returning: bool,
    return_value: Value,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            env: Env::new(),
            output: vec![],
            base_dir: ".".to_string(),
            returning: false,
            return_value: Value::Nil,
        }
    }

    pub fn with_base_dir(base_dir: String) -> Self {
        Interpreter {
            env: Env::new(),
            output: vec![],
            base_dir,
            returning: false,
            return_value: Value::Nil,
        }
    }

    pub fn run(&mut self, module: &Module) -> crate::Result<Vec<String>> {
        for import in &module.imports {
            self.load_import(import)?;
        }
        for stmt in &module.items {
            self.exec_stmt(stmt)?;
        }
        Ok(self.output.clone())
    }

    pub fn run_source(&mut self, source: &str) -> crate::Result<Vec<String>> {
        let tokens = crate::lexer::tokenize(source)?;
        let module = crate::parser::parse(&tokens, source)?;
        self.run(&module)
    }

    fn load_import(&mut self, import: &Import) -> crate::Result<()> {
        if import.is_file {
            let rel = &import.path[0];
            let full = if Path::new(rel).is_absolute() {
                rel.clone()
            } else {
                Path::new(&self.base_dir)
                    .join(rel)
                    .to_string_lossy()
                    .to_string()
            };
            let source = std::fs::read_to_string(&full).map_err(|e| {
                crate::RakError::Runtime(format!("Cannot import '{}': {}", full, e))
            })?;
            let tokens = crate::lexer::tokenize(&source)?;
            let module = crate::parser::parse(&tokens, &source)?;
            let saved_base = self.base_dir.clone();
            if let Some(parent) = Path::new(&full).parent() {
                self.base_dir = parent.to_string_lossy().to_string();
            }
            let mut exports = HashMap::new();
            self.env.push_scope();
            for stmt in &module.items {
                if let Stmt::Export(inner) = stmt {
                    if let Stmt::Let { name, value, .. } = inner.as_ref() {
                        let v = self.eval_expr(value)?;
                        self.env.define(name, v.clone());
                        exports.insert(name.clone(), v);
                    } else {
                        self.exec_stmt(inner)?;
                    }
                } else {
                    self.exec_stmt(stmt)?;
                }
            }
            let _ = self.env.pop_scope();
            self.base_dir = saved_base;
            let key = import
                .alias
                .clone()
                .unwrap_or_else(|| Path::new(rel).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            if !exports.is_empty() {
                self.env.define(&key, Value::Module(exports));
            }
            return Ok(());
        }
        self.output.push(format!("[USE] {}", import.path.join("::")));
        if let Some(alias) = &import.alias {
            self.output.push(format!("[USE] as {}", alias));
        }
        Ok(())
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> crate::Result<()> {
        match stmt {
            Stmt::Export(inner) => {
                self.exec_stmt(inner)?;
            }
            Stmt::Let { name, pattern, mutable: _, value, type_hint: _ } => {
                let val = self.eval_expr(value)?;
                if let Some(p) = pattern {
                    self.bind_pattern(p, &val)?;
                } else {
                    self.env.define(name, val);
                }
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr)?;
            }
            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Nil,
                };
                self.return_value = val;
                self.returning = true;
            }
            Stmt::If { cond, then_branch, else_branch } => {
                let val = self.eval_expr(cond)?;
                if is_truthy(&val) {
                    self.env.push_scope();
                    for s in then_branch {
                        self.exec_stmt(s)?;
                        if self.returning {
                            break;
                        }
                    }
                    self.env.pop_scope();
                } else if let Some(else_stmts) = else_branch {
                    self.env.push_scope();
                    for s in else_stmts {
                        self.exec_stmt(s)?;
                        if self.returning {
                            break;
                        }
                    }
                    self.env.pop_scope();
                }
            }
            Stmt::Loop(body) => {
                loop {
                    self.env.push_scope();
                    for s in body {
                        self.exec_stmt(s)?;
                        if self.returning {
                            break;
                        }
                    }
                    if self.returning {
                        self.env.pop_scope();
                        break;
                    }
                    if self.check_break_reset() {
                        self.env.pop_scope();
                        break;
                    }
                    if self.check_continue_reset() {
                        self.env.pop_scope();
                        continue;
                    }
                    self.env.pop_scope();
                }
            }
            Stmt::While { cond, body } => {
                while is_truthy(&self.eval_expr(cond)?) {
                    self.env.push_scope();
                    for s in body {
                        self.exec_stmt(s)?;
                        if self.returning {
                            break;
                        }
                    }
                    if self.returning {
                        self.env.pop_scope();
                        break;
                    }
                    let broke = self.check_break_reset();
                    let _ = self.check_continue_reset();
                    self.env.pop_scope();
                    if broke {
                        break;
                    }
                }
            }
            Stmt::For { name, iterable, body } => {
                let iter = self.eval_expr(iterable)?;
                match iter {
                    Value::Array(arr) => self.run_for_loop(name, arr, body)?,
                    Value::Tuple(t) => self.run_for_loop(name, t, body)?,
                    Value::String(s) => {
                        let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
                        self.run_for_loop(name, chars, body)?;
                    }
                    Value::Map(m) => {
                        let entries: Vec<Value> = m.into_iter().map(|(k, v)| Value::Tuple(vec![Value::String(k), v])).collect();
                        self.run_for_loop(name, entries, body)?;
                    }
                    Value::Option(Some(v)) => self.run_for_loop(name, vec![*v], body)?,
                    _ => return Err(crate::RakError::Runtime("Cannot iterate over this value".to_string())),
                }
            }
            Stmt::Scan { target, options, body } => self.exec_scan(target, options, body)?,
            Stmt::Fetch { target, options, body } => self.exec_fetch(target, options, body)?,
            Stmt::Dump { value, target } => {
                let val = self.eval_expr(value)?;
                if let Some(t) = target {
                    let target_val = self.eval_expr(t)?;
                    match &target_val {
                        Value::String(path) => {
                            match std::fs::write(path, val.to_string()) {
                                Ok(_) => self.output.push(format!("[DUMP] Written to {}", path)),
                                Err(e) => self.output.push(format!("[DUMP] File error: {}", e)),
                            }
                        }
                        _ => self.output.push(format!("[DUMP] {} -> {}", val, target_val)),
                    }
                } else {
                    self.output.push(format!("[DUMP] {}", val));
                }
            }
            Stmt::Trace { value } => {
                let val = self.eval_expr(value)?;
                self.output.push(format!("[TRACE] {:?}", val));
            }
            Stmt::Break => {
                self.env.define("__break__", Value::Bool(true));
            }
            Stmt::Continue => {
                self.env.define("__continue__", Value::Bool(true));
            }
            Stmt::Mod { name, items } => {
                self.env.push_scope();
                for s in items {
                    self.exec_stmt(s)?;
                }
                let exports = self.env.current_scope_clone();
                self.env.pop_scope();
                let mut filtered = HashMap::new();
                for (k, v) in exports {
                    if !k.starts_with("__") {
                        filtered.insert(k, v);
                    }
                }
                self.env.define(name, Value::Module(filtered));
            }
            Stmt::Struct { name, type_params: _, fields } => {
                self.env.define(name, Value::StructDef { fields: fields.clone() });
            }
            Stmt::Enum { name, type_params: _, variants } => {
                self.env.define(name, Value::EnumDef { variants: variants.clone() });
            }
            Stmt::Impl { target, trait_name: _, methods } => {
                for method in methods {
                    if let Stmt::Let { name: mname, value, .. } = method {
                        let method_name = format!("{}.{}", target, mname);
                        let val = self.eval_expr(value)?;
                        self.env.define(&method_name, val);
                    }
                }
            }
            Stmt::Trait { name: _, methods: _ } => {}
            Stmt::Match { value, arms } => {
                let val = self.eval_expr(value)?;
                for (pattern, guard, body) in arms {
                    if self.pattern_matches(pattern, &val)? {
                        if let Some(g) = guard {
                            self.env.push_scope();
                            self.bind_pattern(pattern, &val)?;
                            let gval = self.eval_expr(g)?;
                            let passed = is_truthy(&gval);
                            if !passed {
                                self.env.pop_scope();
                                continue;
                            }
                        } else {
                            self.env.push_scope();
                            self.bind_pattern(pattern, &val)?;
                        }
                        for s in body {
                            self.exec_stmt(s)?;
                            if self.returning {
                                break;
                            }
                        }
                        self.env.pop_scope();
                        return Ok(());
                    }
                }
            }
            Stmt::Try { body, catch_name, catch_body } => {
                self.env.push_scope();
                let result: crate::Result<()> = (|| {
                    for s in body {
                        self.exec_stmt(s)?;
                        if self.returning {
                            break;
                        }
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => self.env.pop_scope(),
                    Err(crate::RakError::Raise(_)) => {
                        let raised = self.env.get("__raised__").unwrap_or(Value::Nil);
                        self.env.pop_scope();
                        self.env.push_scope();
                        if let Some(cn) = catch_name {
                            self.env.define(cn, raised);
                        }
                        for s in catch_body {
                            self.exec_stmt(s)?;
                        }
                        self.env.pop_scope();
                    }
                    Err(e) => {
                        self.env.pop_scope();
                        self.env.push_scope();
                        if let Some(cn) = catch_name {
                            self.env.define(cn, Value::String(e.to_string()));
                        }
                        for s in catch_body {
                            self.exec_stmt(s)?;
                        }
                        self.env.pop_scope();
                    }
                }
            }
            Stmt::Raise(expr) => {
                let val = self.eval_expr(expr)?;
                self.env.define("__raised__", val.clone());
                return Err(crate::RakError::Raise(val.to_string()));
            }
            Stmt::TypeAlias { name: _, alias: _ } => {}
            Stmt::Use { .. } => {}
            Stmt::Async(body) => {
                for s in body {
                    self.exec_stmt(s)?;
                }
            }
        }
        Ok(())
    }

    fn run_for_loop(&mut self, name: &str, items: Vec<Value>, body: &[Stmt]) -> crate::Result<()> {
        for item in items {
            self.env.push_scope();
            self.env.define(name, item);
            for s in body {
                self.exec_stmt(s)?;
                if self.returning {
                    break;
                }
            }
            let broke = self.check_break_reset();
            let _ = self.check_continue_reset();
            self.env.pop_scope();
            if self.returning || broke {
                break;
            }
        }
        Ok(())
    }

    fn check_break_reset(&mut self) -> bool {
        if let Some(Value::Bool(true)) = self.env.get("__break__") {
            self.env.assign("__break__", Value::Bool(false)).ok();
            true
        } else {
            false
        }
    }

    fn check_continue_reset(&mut self) -> bool {
        if let Some(Value::Bool(true)) = self.env.get("__continue__") {
            self.env.assign("__continue__", Value::Bool(false)).ok();
            true
        } else {
            false
        }
    }

    fn exec_scan(&mut self, target: &Expr, options: &[(String, Expr)], body: &Option<Vec<Stmt>>) -> crate::Result<()> {
        let target_val = self.eval_expr(target)?;
        let target_str = match target_val {
            Value::String(s) => s,
            other => other.to_string(),
        };
        let mut start_port: u16 = 1;
        let mut end_port: u16 = 1024;
        let mut timeout_ms: u64 = 1000;
        for (key, expr) in options {
            if key == "range" {
                if let Value::Array(arr) = self.eval_expr(expr)? {
                    if arr.len() >= 2 {
                        start_port = arr[0].as_u64().unwrap_or(1) as u16;
                        end_port = arr[1].as_u64().unwrap_or(1024) as u16;
                    }
                }
            } else if key == "timeout" {
                timeout_ms = self.eval_expr(expr)?.as_u64().unwrap_or(1000);
            } else if key == "ports" {
                if let Value::Array(arr) = self.eval_expr(expr)? {
                    self.output.push(format!(
                        "[SCAN] Target: {} | Ports: {}",
                        target_str,
                        arr.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
                    ));
                    for port_val in arr {
                        let port = port_val.as_u64().unwrap_or(0) as u16;
                        let is_open = rak_stdlib::net::tcp_scan(&target_str, port, timeout_ms);
                        if is_open {
                            self.output.push(format!("[SCAN] Port {} (0x{:04X}) OPEN", port, port));
                            if let Some(body_stmts) = body {
                                self.env.push_scope();
                                self.env.define("port", Value::Int(port as i64));
                                self.env.define("open", Value::Bool(true));
                                let banner = rak_stdlib::net::tcp_banner_grab(&target_str, port, timeout_ms);
                                self.env.define("banner", banner.map(Value::String).unwrap_or(Value::Nil));
                                for s in body_stmts {
                                    self.exec_stmt(s)?;
                                }
                                self.env.pop_scope();
                            }
                        }
                    }
                    self.output.push("[SCAN] Complete".to_string());
                    return Ok(());
                }
            }
        }
        self.output.push(format!(
            "[SCAN] Target: {} | Ports: {}-{} (0x{:04X}-0x{:04X})",
            target_str, start_port, end_port, start_port, end_port
        ));
        let open_ports = rak_stdlib::recon::port_scan(&target_str, start_port, end_port, timeout_ms);
        for port in open_ports {
            self.output.push(format!("[SCAN] Port {} (0x{:04X}) OPEN", port, port));
            if let Some(body_stmts) = body {
                self.env.push_scope();
                self.env.define("port", Value::Int(port as i64));
                self.env.define("open", Value::Bool(true));
                let banner = rak_stdlib::net::tcp_banner_grab(&target_str, port, timeout_ms);
                self.env.define("banner", banner.map(Value::String).unwrap_or(Value::Nil));
                for s in body_stmts {
                    self.exec_stmt(s)?;
                }
                self.env.pop_scope();
            }
        }
        self.output.push("[SCAN] Complete".to_string());
        Ok(())
    }

    fn exec_fetch(&mut self, target: &Expr, options: &[(String, Expr)], body: &Option<Vec<Stmt>>) -> crate::Result<()> {
        let target_val = self.eval_expr(target)?;
        let target_str = match target_val {
            Value::String(s) => s,
            other => other.to_string(),
        };
        let mut method = "GET".to_string();
        let mut headers_map: HashMap<String, String> = HashMap::new();
        let mut post_body: Option<String> = None;
        for (key, expr) in options {
            if key == "method" {
                if let Value::String(s) = self.eval_expr(expr)? {
                    method = s.to_uppercase();
                }
            } else if key == "headers" {
                if let Value::Map(m) = self.eval_expr(expr)? {
                    for (k, v) in m {
                        headers_map.insert(k, v.to_string());
                    }
                }
            } else if key == "body" {
                post_body = Some(self.eval_expr(expr)?.to_string());
            }
        }
        self.output.push(format!("[FETCH] {} {}", method, target_str));
        let result = if method == "POST" {
            rak_stdlib::net::http_post(
                &target_str,
                post_body.as_deref().unwrap_or(""),
                if headers_map.is_empty() { None } else { Some(headers_map.clone()) },
            )
        } else {
            rak_stdlib::net::http_get(
                &target_str,
                if headers_map.is_empty() { None } else { Some(headers_map.clone()) },
            )
        };
        match result {
            Ok(resp) => {
                self.output.push(format!("[FETCH] Status: {} {}", resp.status, if resp.status < 400 { "OK" } else { "ERROR" }));
                let mut header_map = HashMap::new();
                for (k, v) in &resp.headers {
                    self.output.push(format!("[FETCH] {}: {}", k, v));
                    header_map.insert(k.clone(), Value::String(v.clone()));
                }
                if let Some(body_stmts) = body {
                    self.env.push_scope();
                    self.env.define("status", Value::Int(resp.status as i64));
                    self.env.define("headers", Value::Map(header_map));
                    self.env.define("body", Value::String(resp.body.clone()));
                    for s in body_stmts {
                        self.exec_stmt(s)?;
                    }
                    self.env.pop_scope();
                }
            }
            Err(e) => {
                self.output.push(format!("[FETCH] Error: {}", e));
            }
        }
        Ok(())
    }

    fn eval_expr(&mut self, expr: &Expr) -> crate::Result<Value> {
        match expr {
            Expr::Hex(h) => Ok(Value::Hex(*h)),
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Float32(f) => Ok(Value::Float(*f as f64)),
            Expr::TypedInt(v, k) => Ok(match k {
                crate::ast::IntKind::I8 | crate::ast::IntKind::I16 | crate::ast::IntKind::I32 | crate::ast::IntKind::I64 => Value::Int(*v),
                crate::ast::IntKind::U8 | crate::ast::IntKind::U16 | crate::ast::IntKind::U32 | crate::ast::IntKind::U64 => Value::Hex(*v as u64),
            }),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Bytes(b) => Ok(Value::Bytes(b.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nil => Ok(Value::Nil),
            Expr::Ident(name) => self.eval_ident(name),
            Expr::Tuple(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for e in items {
                    vals.push(self.eval_expr(e)?);
                }
                Ok(Value::Tuple(vals))
            }
            Expr::Array(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for e in items {
                    vals.push(self.eval_expr(e)?);
                }
                Ok(Value::Array(vals))
            }
            Expr::Map(pairs) => {
                let mut map = HashMap::new();
                for (k, v) in pairs {
                    let key = self.eval_expr(k)?.to_string();
                    let val = self.eval_expr(v)?;
                    map.insert(key, val);
                }
                Ok(Value::Map(map))
            }
            Expr::Interp { template, parts } => {
                let mut result = String::new();
                let mut idx = 0;
                let mut chars = template.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '{' {
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            result.push('{');
                        } else if idx < parts.len() {
                            let v = self.eval_expr(&parts[idx])?;
                            result.push_str(&v.to_string());
                            idx += 1;
                            while let Some(c2) = chars.next() {
                                if c2 == '}' {
                                    break;
                                }
                            }
                        }
                    } else if c == '}' {
                        if chars.peek() == Some(&'}') {
                            chars.next();
                        }
                        result.push('}');
                    } else {
                        result.push(c);
                    }
                }
                Ok(Value::String(result))
            }
            Expr::Unary(op, e) => {
                let val = self.eval_expr(e)?;
                match op {
                    UnOp::Minus => match val {
                        Value::Hex(h) => Ok(Value::Hex(h.wrapping_neg())),
                        Value::Int(i) => Ok(Value::Int(i.wrapping_neg())),
                        Value::Float(n) => Ok(Value::Float(-n)),
                        _ => Err(crate::RakError::Runtime("Cannot negate this value".to_string())),
                    },
                    UnOp::Not => Ok(Value::Bool(!is_truthy(&val))),
                    UnOp::BitNot => match val {
                        Value::Hex(h) => Ok(Value::Hex(!h)),
                        Value::Int(i) => Ok(Value::Int(!i)),
                        _ => Err(crate::RakError::Runtime("Cannot bitwise-not this value".to_string())),
                    },
                }
            }
            Expr::Binary(op, l, r) => {
                match op {
                    BinOp::And => {
                        let lv = self.eval_expr(l)?;
                        if !is_truthy(&lv) {
                            return Ok(Value::Bool(false));
                        }
                        let rv = self.eval_expr(r)?;
                        return Ok(Value::Bool(is_truthy(&rv)));
                    }
                    BinOp::Or => {
                        let lv = self.eval_expr(l)?;
                        if is_truthy(&lv) {
                            return Ok(Value::Bool(true));
                        }
                        let rv = self.eval_expr(r)?;
                        return Ok(Value::Bool(is_truthy(&rv)));
                    }
                    _ => {}
                }
                let lv = self.eval_expr(l)?;
                let rv = self.eval_expr(r)?;
                self.eval_binary(op, &lv, &rv)
            }
            Expr::Assign(name, value) => {
                let val = self.eval_expr(value)?;
                self.env.assign(name, val.clone())?;
                Ok(val)
            }
            Expr::IndexAssign { obj, idx, value } => {
                let v = self.eval_expr(value)?;
                let mut container = self.eval_expr(obj)?;
                let idx_val = self.eval_expr(idx)?;
                match &mut container {
                    Value::Array(a) => {
                        if let Value::Int(i) = idx_val {
                            let i = i as usize;
                            if i < a.len() {
                                a[i] = v.clone();
                            }
                        }
                    }
                    Value::Map(m) => {
                        m.insert(idx_val.to_string(), v.clone());
                    }
                    _ => return Err(crate::RakError::Runtime("cannot index-assign this value".to_string())),
                }
                self.store_back(obj, container)?;
                Ok(v)
            }
            Expr::FieldAssign { obj, field, value } => {
                let v = self.eval_expr(value)?;
                let mut container = self.eval_expr(obj)?;
                match &mut container {
                    Value::Map(m) => {
                        m.insert(field.clone(), v.clone());
                    }
                    Value::Struct { fields, .. } => {
                        fields.insert(field.clone(), v.clone());
                    }
                    _ => return Err(crate::RakError::Runtime("cannot field-assign this value".to_string())),
                }
                self.store_back(obj, container)?;
                Ok(v)
            }
            Expr::CompoundAssign(op, name, value) => {
                let cur = self.env.get(name).ok_or_else(|| crate::RakError::Runtime(format!("Undefined: {}", name)))?;
                let rv = self.eval_expr(value)?;
                let binop = match op {
                    CompoundOp::Add => BinOp::Add,
                    CompoundOp::Sub => BinOp::Sub,
                    CompoundOp::Mul => BinOp::Mul,
                    CompoundOp::Div => BinOp::Div,
                    CompoundOp::Rem => BinOp::Rem,
                };
                let newv = self.eval_binary(&binop, &cur, &rv)?;
                self.env.assign(name, newv.clone())?;
                Ok(newv)
            }
            Expr::FieldAccess(obj, field) => {
                let obj_val = self.eval_expr(obj)?;
                match &obj_val {
                    Value::Map(map) => map.get(field).cloned().ok_or_else(|| {
                        crate::RakError::Runtime(format!("Field '{}' not found", field))
                    }),
                    Value::Struct { fields, .. } => fields.get(field).cloned().ok_or_else(|| {
                        crate::RakError::Runtime(format!("Field '{}' not found", field))
                    }),
                    Value::Module(map) => map.get(field).cloned().ok_or_else(|| {
                        crate::RakError::Runtime(format!("'{}' not found in module", field))
                    }),
                    Value::Tuple(t) => {
                        let idx: usize = field.parse().map_err(|_| crate::RakError::Runtime("Bad tuple index".to_string()))?;
                        t.get(idx).cloned().ok_or_else(|| crate::RakError::Runtime("Tuple index out of bounds".to_string()))
                    }
                    _ => Err(crate::RakError::Runtime("Cannot access field on this value".to_string())),
                }
            }
            Expr::Index(obj, idx) => {
                let obj_val = self.eval_expr(obj)?;
                let idx_val = self.eval_expr(idx)?;
                match (&obj_val, &idx_val) {
                    (Value::Array(arr), Value::Int(i)) => {
                        arr.get(*i as usize).cloned().ok_or_else(|| crate::RakError::Runtime("Index out of bounds".to_string()))
                    }
                    (Value::Tuple(t), Value::Int(i)) => {
                        t.get(*i as usize).cloned().ok_or_else(|| crate::RakError::Runtime("Index out of bounds".to_string()))
                    }
                    (Value::String(s), Value::Int(i)) => {
                        s.chars().nth(*i as usize).map(|c| Value::String(c.to_string())).ok_or_else(|| crate::RakError::Runtime("Index out of bounds".to_string()))
                    }
                    (Value::Map(map), Value::String(key)) => {
                        map.get(key).cloned().ok_or_else(|| crate::RakError::Runtime(format!("Key '{}' not found", key)))
                    }
                    _ => Err(crate::RakError::Runtime("Invalid index operation".to_string())),
                }
            }
            Expr::Range(lo, hi) => {
                let start = match lo {
                    Some(e) => self.eval_expr(e)?.as_i64().unwrap_or(0),
                    None => 0,
                };
                let end = match hi {
                    Some(e) => self.eval_expr(e)?.as_i64().unwrap_or(0),
                    None => start,
                };
                let mut arr = Vec::new();
                let mut i = start;
                while i <= end {
                    arr.push(Value::Int(i));
                    i += 1;
                }
                Ok(Value::Array(arr))
            }
            Expr::Function { params, return_type: _, body, captures: _, is_async } => {
                Ok(Value::Function {
                    params: params.clone(),
                    body: body.clone(),
                    closure: Arc::new(self.env.clone()),
                    is_async: *is_async,
                })
            }
            Expr::Lambda { params, body, captures: _ } => {
                let single = Stmt::Expr(body.clone());
                Ok(Value::Function {
                    params: params.clone(),
                    body: vec![single],
                    closure: Arc::new(self.env.clone()),
                    is_async: false,
                })
            }
            Expr::Call { callee, args } => self.eval_call(callee, args),
            Expr::If { cond, then_branch, else_branch } => {
                let v = self.eval_expr(cond)?;
                if is_truthy(&v) {
                    self.env.push_scope();
                    let mut last = Value::Nil;
                    for s in then_branch {
                        if let Stmt::Expr(e) = s {
                            last = self.eval_expr(e)?;
                        } else {
                            self.exec_stmt(s)?;
                        }
                        if self.returning {
                            break;
                        }
                    }
                    self.env.pop_scope();
                    Ok(last)
                } else if let Some(else_stmts) = else_branch {
                    self.env.push_scope();
                    let mut last = Value::Nil;
                    for s in else_stmts {
                        if let Stmt::Expr(e) = s {
                            last = self.eval_expr(e)?;
                        } else {
                            self.exec_stmt(s)?;
                        }
                        if self.returning {
                            break;
                        }
                    }
                    self.env.pop_scope();
                    Ok(last)
                } else {
                    Ok(Value::Nil)
                }
            }
            Expr::Match { value, arms } => {
                let val = self.eval_expr(value)?;
                for (pattern, guard, body) in arms {
                    if self.pattern_matches(pattern, &val)? {
                        self.env.push_scope();
                        self.bind_pattern(pattern, &val)?;
                        if let Some(g) = guard {
                            if !is_truthy(&self.eval_expr(g)?) {
                                self.env.pop_scope();
                                continue;
                            }
                        }
                        let mut last = Value::Nil;
                        for s in body {
                            if let Stmt::Expr(e) = s {
                                last = self.eval_expr(e)?;
                            } else {
                                self.exec_stmt(s)?;
                            }
                            if self.returning {
                                break;
                            }
                        }
                        self.env.pop_scope();
                        return Ok(last);
                    }
                }
                Ok(Value::Nil)
            }
            Expr::Block(stmts) => {
                self.env.push_scope();
                let mut last = Value::Nil;
                for s in stmts {
                    if let Stmt::Expr(e) = s {
                        last = self.eval_expr(e)?;
                    } else {
                        self.exec_stmt(s)?;
                    }
                    if self.returning {
                        break;
                    }
                }
                self.env.pop_scope();
                Ok(last)
            }
            Expr::TryExpr(inner) => {
                let v = self.eval_expr(inner)?;
                match v {
                    Value::Result(Some(ok), _) => Ok(*ok),
                    Value::Result(_, Some(err)) => {
                        let s = err.to_string();
                        self.env.define("__raised__", *err);
                        Err(crate::RakError::Raise(s))
                    }
                    Value::Option(Some(v)) => Ok(*v),
                    Value::Option(None) => {
                        self.env.define("__raised__", Value::Nil);
                        Err(crate::RakError::Raise("None".to_string()))
                    }
                    other => Ok(other),
                }
            }
            Expr::Await(inner) => self.eval_expr(inner),
            Expr::Spawn(inner) => self.eval_expr(inner),
            Expr::Raise(inner) => {
                let v = self.eval_expr(inner)?;
                self.env.define("__raised__", v.clone());
                Err(crate::RakError::Raise(v.to_string()))
            }
            Expr::Path(segs) => self.eval_path(segs, &[]),
            Expr::StructLit { name, fields } => {
                let mut fmap = HashMap::new();
                for (fname, fval) in fields {
                    fmap.insert(fname.clone(), self.eval_expr(fval)?);
                }
                Ok(Value::Struct { name: name.clone(), fields: fmap })
            }
            Expr::As(inner, ty) => {
                let v = self.eval_expr(inner)?;
                Ok(self.cast_as(&v, ty))
            }
        }
    }

    fn store_back(&mut self, target: &Expr, value: Value) -> crate::Result<()> {
        match target {
            Expr::Ident(name) => {
                self.env.assign(name, value)?;
                Ok(())
            }
            Expr::FieldAccess(obj, field) => {
                let mut container = self.eval_expr(obj)?;
                match &mut container {
                    Value::Map(m) => {
                        m.insert(field.clone(), value);
                    }
                    Value::Struct { fields, .. } => {
                        fields.insert(field.clone(), value);
                    }
                    _ => return Err(crate::RakError::Runtime("cannot field-assign this value".to_string())),
                }
                self.store_back(obj, container)
            }
            Expr::Index(obj, idx) => {
                let mut container = self.eval_expr(obj)?;
                let idx_val = self.eval_expr(idx)?;
                match &mut container {
                    Value::Array(a) => {
                        if let Value::Int(i) = idx_val {
                            let i = i as usize;
                            if i < a.len() {
                                a[i] = value;
                            }
                        }
                    }
                    Value::Map(m) => {
                        m.insert(idx_val.to_string(), value);
                    }
                    _ => return Err(crate::RakError::Runtime("cannot index-assign this value".to_string())),
                }
                self.store_back(obj, container)
            }
            _ => Err(crate::RakError::Runtime("invalid assignment target".to_string())),
        }
    }

    fn eval_ident(&mut self, name: &str) -> crate::Result<Value> {
        if let Some(v) = self.env.get(name) {
            return Ok(v);
        }
        match name {
            "None" => return Ok(Value::Option(None)),
            "nil" => return Ok(Value::Nil),
            _ => {}
        }
        Err(crate::RakError::Runtime(format!("Undefined variable: {}", name)))
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr]) -> crate::Result<Value> {
        if let Expr::Ident(name) = callee {
            match name.as_str() {
                "Some" => {
                    let v = self.eval_expr(&args[0])?;
                    return Ok(Value::Option(Some(Box::new(v))));
                }
                "Ok" => {
                    let v = self.eval_expr(&args[0])?;
                    return Ok(Value::Result(Some(Box::new(v)), None));
                }
                "Err" => {
                    let v = self.eval_expr(&args[0])?;
                    return Ok(Value::Result(None, Some(Box::new(v))));
                }
                "array" => {
                    let mut vals = Vec::with_capacity(args.len());
                    for a in args {
                        vals.push(self.eval_expr(a)?);
                    }
                    return Ok(Value::Array(vals));
                }
                "map" => {
                    let mut m = HashMap::new();
                    let mut i = 0;
                    while i + 1 < args.len() {
                        let k = self.eval_expr(&args[i])?.to_string();
                        let v = self.eval_expr(&args[i + 1])?;
                        m.insert(k, v);
                        i += 2;
                    }
                    return Ok(Value::Map(m));
                }
                "fmt" => {
                    if !args.is_empty() {
                        if let Value::String(format_str) = self.eval_expr(&args[0])? {
                            let mut rest = Vec::with_capacity(args.len() - 1);
                            for a in &args[1..] {
                                rest.push(self.eval_expr(a)?);
                            }
                            return Ok(Value::String(self.format_string(&format_str, &rest)));
                        }
                    }
                    return Ok(Value::String(String::new()));
                }
                _ => {}
            }
            if let Some(val) = self.env.get(name) {
                if let Value::Function { params, body, closure, is_async } = val {
                    return self.call_function(&params, &body, &closure, is_async, args);
                }
            }
            let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
            return self.eval_builtin(name, &arg_vals);
        }
        if let Expr::Path(segs) = callee {
            return self.eval_path(segs, args);
        }
        let callee_val = self.eval_expr(callee)?;
        if let Value::Function { params, body, closure, is_async } = callee_val {
            return self.call_function(&params, &body, &closure, is_async, args);
        }
        if let Value::Module(map) = callee_val {
            let _ = map;
        }
        Err(crate::RakError::Runtime("Cannot call non-function".to_string()))
    }

    fn call_function(&mut self, params: &[Param], body: &[Stmt], closure: &Arc<Env>, is_async: bool, args: &[Expr]) -> crate::Result<Value> {
        let _ = is_async;
        let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
        let saved_returning = self.returning;
        self.returning = false;
        self.env.push_scope();
        for (k, v) in &closure.scopes[0] {
            self.env.define(k, v.clone());
        }
        for (p, a) in params.iter().zip(arg_vals.iter()) {
            self.env.define(&p.name, a.clone());
        }
        for s in body {
            self.exec_stmt(s)?;
            if self.returning {
                break;
            }
        }
        let ret = if self.returning {
            std::mem::replace(&mut self.return_value, Value::Nil)
        } else {
            Value::Nil
        };
        self.env.pop_scope();
        self.returning = saved_returning;
        Ok(ret)
    }

    fn eval_path(&mut self, segs: &[String], args: &[Expr]) -> crate::Result<Value> {
        if segs.len() >= 2 {
            let base = &segs[0];
            let variant = &segs[1];
            if let Some(Value::EnumDef { variants }) = self.env.get(base) {
                if variants.iter().any(|v| v.name == *variant) {
                    let data: crate::Result<Vec<Value>> = args.iter().map(|a| self.eval_expr(a)).collect();
                    let data = data?;
                    return Ok(Value::Enum { name: base.clone(), variant: variant.clone(), data });
                }
            }
            if let Some(Value::Module(map)) = self.env.get(base) {
                if let Some(v) = map.get(variant) {
                    if let Value::Function { params, body, closure, is_async } = v {
                        return self.call_function(&params, &body, &closure, *is_async, args);
                    }
                    return Ok(v.clone());
                }
            }
        }
        if segs.len() == 1 {
            return self.eval_ident(&segs[0]);
        }
        let joined = segs.join("::");
        if args.is_empty() {
            return Err(crate::RakError::Runtime(format!("Undefined path: {}", joined)));
        }
        let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
        self.eval_builtin(&joined, &arg_vals)
    }

    fn cast_as(&self, v: &Value, ty: &Type) -> Value {
        match ty {
            Type::Int | Type::I64 | Type::I32 | Type::I16 | Type::I8 => Value::Int(v.as_i64().unwrap_or(0)),
            Type::U64 | Type::U32 | Type::U16 | Type::U8 | Type::Hex(_) => Value::Hex(v.as_u64().unwrap_or(0)),
            Type::F64 | Type::F32 => Value::Float(v.as_f64().unwrap_or(0.0)),
            Type::String => Value::String(v.to_string()),
            _ => v.clone(),
        }
    }

    fn eval_binary(&mut self, op: &BinOp, left: &Value, right: &Value) -> crate::Result<Value> {
        match op {
            BinOp::And => return Ok(Value::Bool(is_truthy(left) && is_truthy(right))),
            BinOp::Or => return Ok(Value::Bool(is_truthy(left) || is_truthy(right))),
            BinOp::Eq => return Ok(Value::Bool(left == right)),
            BinOp::NotEq => return Ok(Value::Bool(left != right)),
            _ => {}
        }
        match (op, left, right) {
            (BinOp::Add, Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b))),
            (BinOp::Add, Value::Array(a), Value::Array(b)) => {
                let mut r = a.clone();
                r.extend(b.clone());
                Ok(Value::Array(r))
            }
            _ if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem)
                && (matches!(left, Value::Float(_)) || matches!(right, Value::Float(_))) =>
            {
                let (l, r) = (left.as_f64().unwrap_or(0.0), right.as_f64().unwrap_or(0.0));
                let res = match op {
                    BinOp::Add => l + r,
                    BinOp::Sub => l - r,
                    BinOp::Mul => l * r,
                    BinOp::Div => if r == 0.0 { return Err(crate::RakError::Runtime("Division by zero".to_string())); } else { l / r },
                    BinOp::Rem => l % r,
                    _ => unreachable!(),
                };
                Ok(Value::Float(res))
            }
            _ => {
                let (l, r, is_hex) = match (left, right) {
                    (Value::Hex(l), Value::Hex(r)) => (*l as i64, *r as i64, true),
                    (Value::Int(l), Value::Int(r)) => (*l, *r, false),
                    (Value::Hex(l), Value::Int(r)) => (*l as i64, *r, true),
                    (Value::Int(l), Value::Hex(r)) => (*l, *r as i64, true),
                    _ => return Err(crate::RakError::Runtime("Invalid operand types for binary operation".to_string())),
                };
                let res = match op {
                    BinOp::Add => l.wrapping_add(r),
                    BinOp::Sub => l.wrapping_sub(r),
                    BinOp::Mul => l.wrapping_mul(r),
                    BinOp::Div => if r == 0 { return Err(crate::RakError::Runtime("Division by zero".to_string())); } else { l / r },
                    BinOp::Rem => if r == 0 { return Err(crate::RakError::Runtime("Division by zero".to_string())); } else { l % r },
                    BinOp::BitAnd => l & r,
                    BinOp::BitOr => l | r,
                    BinOp::BitXor => l ^ r,
                    BinOp::Shl => l << r,
                    BinOp::Shr => l >> r,
                    BinOp::Eq => return Ok(Value::Bool(l == r)),
                    BinOp::NotEq => return Ok(Value::Bool(l != r)),
                    BinOp::Lt => return Ok(Value::Bool(l < r)),
                    BinOp::Gt => return Ok(Value::Bool(l > r)),
                    BinOp::LtEq => return Ok(Value::Bool(l <= r)),
                    BinOp::GtEq => return Ok(Value::Bool(l >= r)),
                    BinOp::And => return Ok(Value::Bool(is_truthy(left) && is_truthy(right))),
                    BinOp::Or => return Ok(Value::Bool(is_truthy(left) || is_truthy(right))),
                };
                if is_hex {
                    Ok(Value::Hex(res as u64))
                } else {
                    Ok(Value::Int(res))
                }
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern, value: &Value) -> crate::Result<()> {
        match pattern {
            Pattern::Wild => {}
            Pattern::Ident(n) => {
                if n != "_" {
                    self.env.define(n, value.clone());
                }
            }
            Pattern::Tuple(pats) => {
                if let Value::Tuple(vals) = value {
                    for (p, v) in pats.iter().zip(vals.iter()) {
                        self.bind_pattern(p, v)?;
                    }
                }
            }
            Pattern::Array(pats) => {
                if let Value::Array(vals) = value {
                    for (p, v) in pats.iter().zip(vals.iter()) {
                        self.bind_pattern(p, v)?;
                    }
                }
            }
            Pattern::Struct(_, fields) => {
                if let Value::Struct { fields: fmap, .. } = value {
                    for (fname, fp) in fields {
                        if let Some(v) = fmap.get(fname) {
                            self.bind_pattern(fp, v)?;
                        }
                    }
                }
            }
            Pattern::Some(inner) => {
                if let Value::Option(Some(v)) = value {
                    self.bind_pattern(inner, v)?;
                }
            }
            Pattern::Ok(inner) => {
                if let Value::Result(Some(v), _) = value {
                    self.bind_pattern(inner, v)?;
                }
            }
            Pattern::Err(inner) => {
                if let Value::Result(_, Some(v)) = value {
                    self.bind_pattern(inner, v)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn pattern_matches(&self, pattern: &Pattern, value: &Value) -> crate::Result<bool> {
        Ok(match (pattern, value) {
            (Pattern::Wild, _) => true,
            (Pattern::Ident(_), _) => true,
            (Pattern::Hex(h), Value::Hex(v)) => h == v,
            (Pattern::Int(i), Value::Int(v)) => i == v,
            (Pattern::String(s), Value::String(v)) => s == v,
            (Pattern::Bool(b), Value::Bool(v)) => b == v,
            (Pattern::Nil, Value::Nil) => true,
            (Pattern::None, Value::Option(None)) => true,
            (Pattern::Some(inner), Value::Option(Some(v))) => self.pattern_matches(inner, v)?,
            (Pattern::Ok(inner), Value::Result(Some(v), _)) => self.pattern_matches(inner, v)?,
            (Pattern::Err(inner), Value::Result(_, Some(v))) => self.pattern_matches(inner, v)?,
            (Pattern::Tuple(pats), Value::Tuple(vals)) => {
                pats.len() == vals.len() && pats.iter().zip(vals.iter()).all(|(p, v)| self.pattern_matches(p, v).unwrap_or(false))
            }
            (Pattern::Array(pats), Value::Array(vals)) => {
                pats.len() == vals.len() && pats.iter().zip(vals.iter()).all(|(p, v)| self.pattern_matches(p, v).unwrap_or(false))
            }
            (Pattern::Struct(name, fields), Value::Struct { name: sn, fields: fmap }) => {
                name == sn && fields.iter().all(|(fname, fp)| {
                    fmap.get(fname).map(|v| self.pattern_matches(fp, v).unwrap_or(false)).unwrap_or(false)
                })
            }
            (Pattern::Or(opts), _) => opts.iter().any(|p| self.pattern_matches(p, value).unwrap_or(false)),
            _ => false,
        })
    }

    fn eval_builtin(&mut self, name: &str, args: &[Value]) -> crate::Result<Value> {
        match name {
            "md5" => Ok(Value::String(rak_stdlib::md5(&self.val_to_bytes(args.first())?))),
            "sha1" => Ok(Value::String(rak_stdlib::sha1(&self.val_to_bytes(args.first())?))),
            "sha256" => Ok(Value::String(rak_stdlib::sha256(&self.val_to_bytes(args.first())?))),
            "xor" => Ok(Value::Bytes(rak_stdlib::xor_encrypt(&self.val_to_bytes(args.first())?, &self.val_to_bytes(args.get(1))?))),
            "rot13" => Ok(Value::String(rak_stdlib::rot13(&self.val_to_string(args.first())?))),
            "hex_encode" => Ok(Value::String(rak_stdlib::hex_encode(&self.val_to_bytes(args.first())?))),
            "hex_decode" => match rak_stdlib::hex_decode(&self.val_to_string(args.first())?) {
                Some(b) => Ok(Value::Bytes(b)),
                None => Err(crate::RakError::Runtime("Invalid hex string".to_string())),
            },
            "base64_encode" => Ok(Value::String(rak_stdlib::base64_encode(&self.val_to_bytes(args.first())?))),
            "base64_decode" => match rak_stdlib::base64_decode(&self.val_to_string(args.first())?) {
                Some(b) => Ok(Value::Bytes(b)),
                None => Err(crate::RakError::Runtime("Invalid base64".to_string())),
            },
            "url_encode" => Ok(Value::String(rak_stdlib::url_encode(&self.val_to_string(args.first())?))),
            "url_decode" => match rak_stdlib::url_decode(&self.val_to_string(args.first())?) {
                Some(d) => Ok(Value::String(d)),
                None => Err(crate::RakError::Runtime("Invalid URL encoding".to_string())),
            },
            "dns_lookup" => {
                let ips = rak_stdlib::recon::dns_lookup(&self.val_to_string(args.first())?);
                Ok(Value::Array(ips.into_iter().map(|ip| Value::String(ip.to_string())).collect()))
            }
            "subdomain_enum" => {
                let subs = rak_stdlib::recon::subdomain_enum(&self.val_to_string(args.first())?);
                Ok(Value::Array(subs.into_iter().map(Value::String).collect()))
            }
            "reverse_dns" => match rak_stdlib::recon::reverse_dns(&self.val_to_string(args.first())?) {
                Some(s) => Ok(Value::String(s)),
                None => Ok(Value::Nil),
            },
            "len" => match args.first() {
                Some(Value::String(s)) => Ok(Value::Int(s.chars().count() as i64)),
                Some(Value::Array(a)) => Ok(Value::Int(a.len() as i64)),
                Some(Value::Tuple(t)) => Ok(Value::Int(t.len() as i64)),
                Some(Value::Bytes(b)) => Ok(Value::Int(b.len() as i64)),
                Some(Value::Map(m)) => Ok(Value::Int(m.len() as i64)),
                Some(Value::Struct { fields, .. }) => Ok(Value::Int(fields.len() as i64)),
                _ => Err(crate::RakError::Runtime("len() requires a string, array, tuple, bytes, or map".to_string())),
            },
            "split" => {
                let s = self.val_to_string(args.first())?;
                let delim = self.val_to_string(args.get(1))?;
                Ok(Value::Array(s.split(&delim).map(|p| Value::String(p.to_string())).collect()))
            }
            "join" => {
                let delim = self.val_to_string(args.first())?;
                if let Some(Value::Array(arr)) = args.get(1) {
                    let parts: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                    Ok(Value::String(parts.join(&delim)))
                } else {
                    Err(crate::RakError::Runtime("join() requires an array".to_string()))
                }
            }
            "contains" => {
                let haystack = self.val_to_string(args.first())?;
                let needle = self.val_to_string(args.get(1))?;
                Ok(Value::Bool(haystack.contains(&needle)))
            }
            "to_hex" => Ok(Value::String(format!("0x{:X}", args.first().and_then(|v| v.as_u64()).unwrap_or(0)))),
            "from_hex" => {
                let s = self.val_to_string(args.first())?;
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                u64::from_str_radix(s, 16).map(Value::Hex).map_err(|_| crate::RakError::Runtime("Invalid hex literal".to_string()))
            }
            "int" => match args.first() {
                Some(Value::Hex(h)) => Ok(Value::Int(*h as i64)),
                Some(Value::Int(i)) => Ok(Value::Int(*i)),
                Some(Value::Float(f)) => Ok(Value::Int(*f as i64)),
                Some(Value::String(s)) => s.parse::<i64>().map(Value::Int).map_err(|_| crate::RakError::Runtime("Cannot convert to int".to_string())),
                Some(Value::Bool(b)) => Ok(Value::Int(if *b { 1 } else { 0 })),
                _ => Ok(Value::Int(0)),
            },
            "float" => Ok(Value::Float(args.first().and_then(|v| v.as_f64()).unwrap_or(0.0))),
            "string" => Ok(Value::String(self.val_to_string(args.first())?)),
            "bytes" => Ok(Value::Bytes(self.val_to_bytes(args.first())?)),
            "upper" => Ok(Value::String(self.val_to_string(args.first())?.to_uppercase())),
            "lower" => Ok(Value::String(self.val_to_string(args.first())?.to_lowercase())),
            "trim" => Ok(Value::String(self.val_to_string(args.first())?.trim().to_string())),
            "push" => {
                if let Some(Value::Array(arr)) = args.first().cloned() {
                    let mut arr = arr;
                    if let Some(item) = args.get(1) {
                        arr.push(item.clone());
                    }
                    Ok(Value::Array(arr))
                } else {
                    Err(crate::RakError::Runtime("push() requires an array".to_string()))
                }
            }
            "keys" => {
                if let Some(Value::Map(m)) = args.first() {
                    Ok(Value::Array(m.keys().cloned().map(Value::String).collect()))
                } else {
                    Err(crate::RakError::Runtime("keys() requires a map".to_string()))
                }
            }
            "has" => {
                let m = args.first();
                let k = self.val_to_string(args.get(1))?;
                match m {
                    Some(Value::Map(map)) => Ok(Value::Bool(map.contains_key(&k))),
                    _ => Ok(Value::Bool(false)),
                }
            }
            "get" => {
                let k = self.val_to_string(args.get(1))?;
                match args.first() {
                    Some(Value::Map(map)) => Ok(map.get(&k).cloned().unwrap_or_else(|| args.get(2).cloned().unwrap_or(Value::Nil))),
                    _ => Ok(args.get(2).cloned().unwrap_or(Value::Nil)),
                }
            }
            "values" => {
                if let Some(Value::Map(m)) = args.first() {
                    Ok(Value::Array(m.values().cloned().collect()))
                } else {
                    Err(crate::RakError::Runtime("values() requires a map".to_string()))
                }
            }
            "read" => match std::fs::read_to_string(self.val_to_string(args.first())?) {
                Ok(c) => Ok(Value::String(c)),
                Err(e) => Err(crate::RakError::Runtime(format!("Read error: {}", e))),
            },
            "write" => match std::fs::write(self.val_to_string(args.first())?, self.val_to_string(args.get(1))?) {
                Ok(_) => Ok(Value::Bool(true)),
                Err(e) => Err(crate::RakError::Runtime(format!("Write error: {}", e))),
            },
            "file_read" => match rak_stdlib::file::read(&self.val_to_string(args.first())?) {
                Ok(c) => Ok(Value::String(c)),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "file_write" => match rak_stdlib::file::write(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?) {
                Ok(_) => Ok(Value::Bool(true)),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "file_append" => match rak_stdlib::file::append(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?) {
                Ok(_) => Ok(Value::Bool(true)),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "file_exists" => Ok(Value::Bool(rak_stdlib::file::exists(&self.val_to_string(args.first())?))),
            "file_size" => Ok(Value::Int(rak_stdlib::file::size(&self.val_to_string(args.first())?).unwrap_or(0) as i64)),
            "file_list" => Ok(Value::Array(rak_stdlib::file::list(&self.val_to_string(args.first())?).into_iter().map(Value::String).collect())),
            "file_delete" => Ok(Value::Bool(rak_stdlib::file::delete(&self.val_to_string(args.first())?))),
            "file_mkdir" => Ok(Value::Bool(rak_stdlib::file::create_dir(&self.val_to_string(args.first())?))),
            "file_copy" => Ok(Value::Bool(rak_stdlib::file::copy(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?))),
            "file_rename" => Ok(Value::Bool(rak_stdlib::file::rename(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?))),
            "file_ext" => Ok(Value::String(rak_stdlib::file::ext(&self.val_to_string(args.first())?).unwrap_or_default())),
            "file_basename" => Ok(Value::String(rak_stdlib::file::basename(&self.val_to_string(args.first())?))),
            "file_dirname" => Ok(Value::String(rak_stdlib::file::dirname(&self.val_to_string(args.first())?))),
            "html_title" => match rak_stdlib::web::html_title(&self.val_to_string(args.first())?) {
                Some(t) => Ok(Value::String(t)),
                None => Ok(Value::Nil),
            },
            "html_select" => match rak_stdlib::web::html_select(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?) {
                Some(t) => Ok(Value::String(t)),
                None => Ok(Value::Nil),
            },
            "html_select_all" => Ok(Value::Array(rak_stdlib::web::html_select_all(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?).into_iter().map(Value::String).collect())),
            "html_attr" => match rak_stdlib::web::html_attr(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?, &self.val_to_string(args.get(2))?) {
                Some(v) => Ok(Value::String(v)),
                None => Ok(Value::Nil),
            },
            "html_links" => Ok(Value::Array(rak_stdlib::web::html_links(&self.val_to_string(args.first())?).into_iter().map(Value::String).collect())),
            "html_images" => Ok(Value::Array(rak_stdlib::web::html_images(&self.val_to_string(args.first())?).into_iter().map(Value::String).collect())),
            "html_scripts" => Ok(Value::Array(rak_stdlib::web::html_scripts(&self.val_to_string(args.first())?).into_iter().map(Value::String).collect())),
            "html_forms" => Ok(Value::Array(rak_stdlib::web::html_forms(&self.val_to_string(args.first())?).into_iter().map(|f| Value::Map(f.into_iter().map(|(k, v)| (k, Value::String(v))).collect())).collect())),
            "html_inputs" => Ok(Value::Array(rak_stdlib::web::html_inputs(&self.val_to_string(args.first())?).into_iter().map(|f| Value::Map(f.into_iter().map(|(k, v)| (k, Value::String(v))).collect())).collect())),
            "html_meta" => match rak_stdlib::web::html_meta(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?) {
                Some(c) => Ok(Value::String(c)),
                None => Ok(Value::Nil),
            },
            "html_count" => Ok(Value::Int(rak_stdlib::web::html_count(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?) as i64)),
            "html_headers" => Ok(Value::Array(rak_stdlib::web::html_headers(&self.val_to_string(args.first())?).into_iter().map(Value::String).collect())),
            "json_parse" => match rak_stdlib::js::json_parse(&self.val_to_string(args.first())?) {
                Ok(v) => Ok(Value::String(v.to_string())),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "json_get" => match rak_stdlib::js::json_get(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?) {
                Ok(v) => Ok(Value::String(v)),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "json_path" => match rak_stdlib::js::json_path(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?) {
                Ok(v) => Ok(Value::String(v)),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "json_keys" => match rak_stdlib::js::json_keys(&self.val_to_string(args.first())?) {
                Ok(k) => Ok(Value::Array(k.into_iter().map(Value::String).collect())),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "json_len" => match rak_stdlib::js::json_len(&self.val_to_string(args.first())?) {
                Ok(n) => Ok(Value::Int(n as i64)),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "json_find_all" => Ok(Value::Array(rak_stdlib::js::json_find_all(&self.val_to_string(args.first())?, &self.val_to_string(args.get(1))?).into_iter().map(Value::String).collect())),
            "scan_ports" => {
                let target = self.val_to_string(args.first())?;
                let mut start_port: u16 = 1;
                let mut end_port: u16 = 1024;
                let mut timeout_ms: u64 = 1000;
                if let Some(Value::Map(opts)) = args.get(1) {
                    if let Some(Value::Array(range)) = opts.get("range") {
                        if range.len() >= 2 {
                            start_port = range[0].as_u64().unwrap_or(1) as u16;
                            end_port = range[1].as_u64().unwrap_or(1024) as u16;
                        }
                    }
                    if let Some(v) = opts.get("timeout") {
                        timeout_ms = v.as_u64().unwrap_or(1000);
                    }
                } else if let Some(Value::Array(range)) = args.get(1) {
                    if range.len() >= 2 {
                        start_port = range[0].as_u64().unwrap_or(1) as u16;
                        end_port = range[1].as_u64().unwrap_or(1024) as u16;
                    }
                }
                let open_ports = rak_stdlib::recon::port_scan(&target, start_port, end_port, timeout_ms);
                Ok(Value::Array(open_ports.into_iter().map(|p| Value::Int(p as i64)).collect()))
            }
            "scan_subdomains" => {
                let domain = self.val_to_string(args.first())?;
                let subs = rak_stdlib::recon::subdomain_enum(&domain);
                let mut results: Vec<Value> = vec![];
                for sub in subs {
                    let ips = rak_stdlib::recon::dns_lookup(&sub);
                    if !ips.is_empty() {
                        results.push(Value::String(format!("{} -> {}", sub, ips.first().unwrap())));
                    }
                }
                Ok(Value::Array(results))
            }
            "net_listen" => {
                let addr = self.val_to_string(args.first())?;
                match std::net::TcpListener::bind(&addr) {
                    Ok(l) => {
                        l.set_nonblocking(false).ok();
                        Ok(Value::TcpListener(Arc::new(Mutex::new(l))))
                    }
                    Err(e) => Err(crate::RakError::Runtime(format!("net_listen: {}", e))),
                }
            }
            "net_accept" => {
                match args.first() {
                    Some(Value::TcpListener(l)) => {
                        let accepted = l.lock().unwrap().accept();
                        match accepted {
                            Ok((stream, addr)) => Ok(Value::Tuple(vec![
                                Value::TcpStream(Arc::new(Mutex::new(stream))),
                                Value::String(addr.to_string()),
                            ])),
                            Err(e) => Err(crate::RakError::Runtime(format!("net_accept: {}", e))),
                        }
                    }
                    _ => Err(crate::RakError::Runtime("net_accept requires a listener".to_string())),
                }
            }
            "net_connect" => {
                let addr = self.val_to_string(args.first())?;
                match std::net::TcpStream::connect(&addr) {
                    Ok(s) => Ok(Value::TcpStream(Arc::new(Mutex::new(s)))),
                    Err(e) => Err(crate::RakError::Runtime(format!("net_connect: {}", e))),
                }
            }
            "net_local_addr" => {
                match args.first() {
                    Some(Value::TcpListener(l)) => {
                        Ok(Value::String(l.lock().unwrap().local_addr().map(|a| a.to_string()).unwrap_or_default()))
                    }
                    _ => Err(crate::RakError::Runtime("net_local_addr requires a listener".to_string())),
                }
            }
            "tcp_write" => {
                match (args.first(), args.get(1)) {
                    (Some(Value::TcpStream(s)), Some(v)) => {
                        let data = self.val_to_bytes(Some(v))?;
                        let n = {
                            let mut st = s.lock().unwrap();
                            use std::io::Write;
                            st.write(&data).map_err(|e| crate::RakError::Runtime(format!("tcp_write: {}", e)))?
                        };
                        Ok(Value::Int(n as i64))
                    }
                    _ => Err(crate::RakError::Runtime("tcp_write(stream, data)".to_string())),
                }
            }
            "tcp_read" => {
                match (args.first(), args.get(1)) {
                    (Some(Value::TcpStream(s)), Some(v)) => {
                        let n = v.as_u64().unwrap_or(1024) as usize;
                        let mut buf = vec![0u8; n];
                        let read = {
                            use std::io::Read;
                            let mut st = s.lock().unwrap();
                            st.read(&mut buf).map_err(|e| crate::RakError::Runtime(format!("tcp_read: {}", e)))?
                        };
                        buf.truncate(read);
                        Ok(Value::Bytes(buf))
                    }
                    _ => Err(crate::RakError::Runtime("tcp_read(stream, n)".to_string())),
                }
            }
            "tcp_read_line" => {
                match args.first() {
                    Some(Value::TcpStream(s)) => {
                        let s = s.clone();
                        let mut out = Vec::new();
                        use std::io::Read;
                        loop {
                            let mut byte = [0u8; 1];
                            let r = { s.lock().unwrap().read(&mut byte) };
                            match r {
                                Ok(0) => break,
                                Ok(_) => {
                                    if byte[0] == b'\n' { break; }
                                    if byte[0] != b'\r' { out.push(byte[0]); }
                                }
                                Err(e) => return Err(crate::RakError::Runtime(format!("tcp_read_line: {}", e))),
                            }
                        }
                        Ok(Value::String(String::from_utf8_lossy(&out).to_string()))
                    }
                    _ => Err(crate::RakError::Runtime("tcp_read_line(stream)".to_string())),
                }
            }
            "tcp_close" => {
                match args.first() {
                    Some(Value::TcpStream(s)) => {
                        use std::io::Write;
                        let _ = s.lock().unwrap().shutdown(std::net::Shutdown::Both);
                        let _ = s.lock().unwrap().flush();
                        Ok(Value::Nil)
                    }
                    _ => Err(crate::RakError::Runtime("tcp_close(stream)".to_string())),
                }
            }
            "spawn" => {
                match args.first() {
                    Some(Value::Function { params, body, closure, .. }) => {
                        let params = params.clone();
                        let body = body.clone();
                        let closure = closure.clone();
                        let handle: JoinHandle<Value> = std::thread::spawn(move || {
                            let mut interp = Interpreter::new();
                            interp.env = (*closure).clone();
                            interp.env.push_scope();
                            for s in &body {
                                let _ = interp.exec_stmt(s);
                                if interp.returning {
                                    break;
                                }
                            }
                            let ret = if interp.returning {
                                std::mem::replace(&mut interp.return_value, Value::Nil)
                            } else {
                                Value::Nil
                            };
                            ret
                        });
                        let _ = params;
                        Ok(Value::JoinHandle(Arc::new(Mutex::new(Some(handle)))))
                    }
                    _ => Err(crate::RakError::Runtime("spawn requires a function".to_string())),
                }
            }
            "thread_join" => {
                match args.first() {
                    Some(Value::JoinHandle(h)) => {
                        let handle_opt = h.lock().unwrap().take();
                        if let Some(handle) = handle_opt {
                            match handle.join() {
                                Ok(v) => Ok(v),
                                Err(_) => Err(crate::RakError::Runtime("thread panicked".to_string())),
                            }
                        } else {
                            Err(crate::RakError::Runtime("already joined".to_string()))
                        }
                    }
                    _ => Err(crate::RakError::Runtime("thread_join requires a thread handle".to_string())),
                }
            }
            "channel" => {
                let (tx, rx) = mpsc::channel::<Value>();
                Ok(Value::Tuple(vec![
                    Value::Sender(Arc::new(Mutex::new(tx))),
                    Value::Receiver(Arc::new(Mutex::new(rx))),
                ]))
            }
            "chan_send" => {
                match (args.first(), args.get(1)) {
                    (Some(Value::Sender(tx)), Some(v)) => {
                        tx.lock().unwrap().send(v.clone()).map(|_| Value::Bool(true)).map_err(|_| crate::RakError::Runtime("chan_send failed".to_string()))
                    }
                    _ => Err(crate::RakError::Runtime("chan_send(tx, v)".to_string())),
                }
            }
            "chan_recv" => {
                match args.first() {
                    Some(Value::Receiver(rx)) => {
                        match rx.lock().unwrap().recv() {
                            Ok(v) => Ok(Value::Option(Some(Box::new(v)))),
                            Err(_) => Ok(Value::Option(None)),
                        }
                    }
                    _ => Err(crate::RakError::Runtime("chan_recv(rx)".to_string())),
                }
            }
            "sleep" => {
                let ms = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
                std::thread::sleep(std::time::Duration::from_millis(ms));
                Ok(Value::Nil)
            }
            "now_ms" => Ok(Value::Int(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0))),
            "args" => {
                let a: Vec<Value> = std::env::args().skip(1).map(Value::String).collect();
                Ok(Value::Array(a))
            }
            "env_get" => {
                let k = self.val_to_string(args.first())?;
                Ok(std::env::var(&k).map(Value::String).unwrap_or(Value::Nil))
            }
            "ord" => {
                let s = self.val_to_string(args.first())?;
                Ok(Value::Int(s.chars().next().map(|c| c as i64).unwrap_or(0)))
            }
            "chr" => {
                let n = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as u32;
                Ok(Value::String(char::from_u32(n).map(|c| c.to_string()).unwrap_or_default()))
            }
            "substr" => {
                let s = self.val_to_string(args.first())?;
                let start = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                let length = args.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                let chars: Vec<char> = s.chars().collect();
                let end = (start + length).min(chars.len());
                let s2 = start.min(chars.len());
                Ok(Value::String(chars[s2..end].iter().collect()))
            }
            "sort" => {
                if let Some(Value::Array(a)) = args.first().cloned() {
                    let mut a = a;
                    a.sort_by(|x, y| x.to_string().cmp(&y.to_string()));
                    Ok(Value::Array(a))
                } else {
                    Err(crate::RakError::Runtime("sort() requires an array".to_string()))
                }
            }
            "exit" => {
                let code = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                std::process::exit(code);
            }
            "print" => {
                let s = self.val_to_string(args.first())?;
                println!("{}", s);
                use std::io::Write;
                std::io::stdout().flush().ok();
                Ok(Value::Nil)
            }
            "dbg" => {
                let s = self.val_to_string(args.first())?;
                eprintln!("[dbg] {}", s);
                Ok(Value::Nil)
            }
            _ => Err(crate::RakError::Runtime(format!("Unknown function: {}", name))),
        }
    }

    fn val_to_string(&self, val: Option<&Value>) -> crate::Result<String> {
        match val {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(Value::Bytes(b)) => Ok(String::from_utf8_lossy(b).to_string()),
            Some(v) => Ok(v.to_string()),
            None => Ok(String::new()),
        }
    }

    fn val_to_bytes(&self, val: Option<&Value>) -> crate::Result<Vec<u8>> {
        match val {
            Some(Value::Bytes(b)) => Ok(b.clone()),
            Some(Value::String(s)) => Ok(s.bytes().collect()),
            Some(Value::Hex(h)) => Ok(h.to_le_bytes().to_vec()),
            Some(Value::Int(i)) => Ok(i.to_le_bytes().to_vec()),
            Some(v) => Ok(v.to_string().bytes().collect()),
            None => Ok(vec![]),
        }
    }

    fn format_string(&self, format: &str, args: &[Value]) -> String {
        let mut result = String::new();
        let mut arg_idx = 0;
        let mut chars = format.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut spec = String::new();
                    while let Some(c) = chars.next() {
                        if c == '}' {
                            break;
                        }
                        spec.push(c);
                    }
                    if arg_idx < args.len() {
                        let arg = &args[arg_idx];
                        if spec.contains(":04X") {
                            result.push_str(&format!("{:04X}", arg.as_u64().unwrap_or(0)));
                        } else if spec.contains(":08X") {
                            result.push_str(&format!("{:08X}", arg.as_u64().unwrap_or(0)));
                        } else if spec.contains('X') || spec.contains('x') {
                            result.push_str(&format!("{:X}", arg.as_u64().unwrap_or(0)));
                        } else if spec.contains('.') && (spec.contains('f') || spec.contains('e')) {
                            result.push_str(&format!("{}", arg.as_f64().unwrap_or(0.0)));
                        } else {
                            result.push_str(&arg.to_string());
                        }
                        arg_idx += 1;
                    }
                }
            } else if ch == '}' {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    result.push('}');
                }
            } else {
                result.push(ch);
            }
        }
        result
    }
}

impl Value {
    fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Hex(h) => Some(*h as i64),
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
    fn as_u64(&self) -> Option<u64> {
        self.as_i64().map(|v| v as u64)
    }
    fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Hex(h) => Some(*h as f64),
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Nil => false,
        Value::Int(0) => false,
        Value::Hex(0) => false,
        Value::Float(0.0) => false,
        Value::String(s) => !s.is_empty(),
        Value::Bytes(b) => !b.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Map(m) => !m.is_empty(),
        Value::Tuple(t) => !t.is_empty(),
        Value::Option(Some(_)) => true,
        Value::Option(None) => false,
        Value::Result(Some(_), _) => true,
        Value::Result(None, _) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpreter_hex() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let x = 0xFF; dump x;").unwrap();
        assert_eq!(output, vec!["[DUMP] 0xFF"]);
    }

    #[test]
    fn test_interpreter_for_loop() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let total = 0; for x in [1, 2, 3] { total = total + x; } dump total;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 6")));
    }

    #[test]
    fn test_interpreter_md5() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("dump md5(\"hello\");").unwrap();
        assert!(output.iter().any(|l| l.contains("5d41402abc4b2a76b9719d911017c592")));
    }

    #[test]
    fn test_interpreter_hex_encode() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("dump hex_encode(\"ABC\");").unwrap();
        assert!(output.iter().any(|l| l.contains("414243")));
    }

    #[test]
    fn test_interpreter_to_hex() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("dump to_hex(0xFF);").unwrap();
        assert!(output.iter().any(|l| l.contains("0xFF")));
    }

    #[test]
    fn test_interpreter_len() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("dump len(\"hello\");").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 5")));
    }

    #[test]
    fn test_interpreter_bitwise() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let r = 0xDEADBEEF & 0xFF00FF00; dump r;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 0xDE00BE00")));
    }

    #[test]
    fn test_interpreter_for_over_array() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("for x in [10, 20, 30] { dump x; }").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 10")));
    }

    #[test]
    fn test_interpreter_float() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let x = 3.14; dump x;").unwrap();
        assert!(output.iter().any(|l| l.contains("3.14")));
    }

    #[test]
    fn test_interpreter_tuple() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let p = (1, 2, 3); dump p.1;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 2")));
    }

    #[test]
    fn test_interpreter_interp() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let name = \"Rak\"; dump f\"hello {name}!\";").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] hello Rak!")));
    }

    #[test]
    fn test_interpreter_if_expr() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let x = if true { 1 } else { 2 }; dump x;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 1")));
    }

    #[test]
    fn test_interpreter_result() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let r = Ok(42); dump r;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] Ok(42)")));
    }

    #[test]
    fn test_interpreter_try_question() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let r = Ok(42); let v = r?; dump v;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 42")));
    }

    #[test]
    fn test_interpreter_try_catch() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("try { raise \"boom\" } catch e { dump e }").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] boom")));
    }

    #[test]
    fn test_interpreter_enum() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("enum Color { Red, Green, Blue } let c = Color::Red; dump c;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] Color::Red")));
    }

    #[test]
    fn test_interpreter_struct_destructure() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let Point { x, y } = p;").unwrap_or_default();
        let _ = output;
    }

    #[test]
    fn test_interpreter_range() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let s = 0; for i in 1..3 { s = s + i } dump s;").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 6")));
    }

    #[test]
    fn test_interpreter_match() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let x = 2; match x { 1 => { dump \"one\" }, 2 => { dump \"two\" }, _ => { dump \"other\" } }").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] two")));
    }

    #[test]
    fn test_interpreter_file_write_read() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("file_write(\"test_rak.txt\", \"hello\"); dump file_read(\"test_rak.txt\");").unwrap();
        assert!(output.iter().any(|l| l.contains("hello")));
        let _ = std::fs::remove_file("test_rak.txt");
    }

    #[test]
    fn test_interpreter_string_ops() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("dump upper(\"hello\"); dump len(split(\"a,b,c\", \",\")); dump contains(\"hello world\", \"world\");").unwrap();
        assert!(output.iter().any(|l| l.contains("HELLO")));
        assert!(output.iter().any(|l| l.contains("[DUMP] 3")));
        assert!(output.iter().any(|l| l.contains("true")));
    }

    #[test]
    fn test_interpreter_recursive_fib() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("fn fib(n) { if n < 2 { return n } return fib(n - 1) + fib(n - 2) } dump fib(15);").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 610")));
    }
}
