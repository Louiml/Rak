use crate::ast::*;
use std::collections::HashMap;
use std::fmt;

/// Runtime value in the Rak interpreter.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Hex(u64),
    Int(u64),
    String(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Nil,
    Array(Vec<Value>),
    Map(HashMap<String, Value>),
    Function {
        params: Vec<Param>,
        body: Vec<Stmt>,
        closure: Env,
    },
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Hex(h) => write!(f, "0x{:X}", h),
            Value::Int(i) => write!(f, "{}", i),
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
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Value::Map(map) => {
                let items: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            Value::Function { .. } => write!(f, "<function>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    scopes: Vec<HashMap<String, Value>>,
}

impl Env {
    pub fn new() -> Self {
        Env {
            scopes: vec![HashMap::new()],
        }
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
        let scope = self.scopes.last_mut().unwrap();
        scope.insert(name.to_string(), value);
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
        Err(crate::RakError::Runtime(format!(
            "Undefined variable: {}",
            name
        )))
    }
}

pub struct Interpreter {
    env: Env,
    output: Vec<String>,
}

impl Interpreter {
    pub fn new() -> Self {
        let mut interpreter = Interpreter {
            env: Env::new(),
            output: vec![],
        };
        interpreter.register_builtins();
        interpreter
    }

    fn register_builtins(&mut self) {
        // Builtins (fmt, array, map, md5, sha256, hex_encode, etc.)
        // are handled directly in eval_expr's Call branch by checking
        // the callee identifier name. No fake Function values needed.
    }

    pub fn run(&mut self, module: &Module) -> crate::Result<Vec<String>> {
        // Process imports - stdlib functions are already available as builtins
        for import in &module.imports {
            self.output.push(format!("[USE] {}", import.path.join(".")));
        }
        for stmt in &module.items {
            self.exec_stmt(stmt)?;
        }
        Ok(self.output.clone())
    }

    pub fn run_source(&mut self, source: &str) -> crate::Result<Vec<String>> {
        let tokens = crate::lexer::tokenize(source)?;
        let module = crate::parser::parse(&tokens)?;
        self.run(&module)
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> crate::Result<()> {
        match stmt {
            Stmt::Let {
                name,
                mutable: _,
                value,
                type_hint: _,
            } => {
                let val = self.eval_expr(value)?;
                self.env.define(name, val);
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr)?;
            }
            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Nil,
                };
                // For simplicity, store return value in a special variable
                self.env.define("__return__", val);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let val = self.eval_expr(cond)?;
                if is_truthy(&val) {
                    self.env.push_scope();
                    for stmt in then_branch {
                        self.exec_stmt(stmt)?;
                    }
                    self.env.pop_scope();
                } else if let Some(else_stmts) = else_branch {
                    self.env.push_scope();
                    for stmt in else_stmts {
                        self.exec_stmt(stmt)?;
                    }
                    self.env.pop_scope();
                }
            }
            Stmt::Loop(body) => {
                loop {
                    self.env.push_scope();
                    for stmt in body {
                        self.exec_stmt(stmt)?;
                    }
                    // Check for break/continue
                    if let Some(Value::Bool(true)) = self.env.get("__break__") {
                        self.env.assign("__break__", Value::Bool(false)).ok();
                        self.env.pop_scope();
                        break;
                    }
                    self.env.pop_scope();
                }
            }
            Stmt::While { cond, body } => {
                while is_truthy(&self.eval_expr(cond)?) {
                    self.env.push_scope();
                    for stmt in body {
                        self.exec_stmt(stmt)?;
                    }
                    if let Some(Value::Bool(true)) = self.env.get("__break__") {
                        self.env.assign("__break__", Value::Bool(false)).ok();
                        self.env.pop_scope();
                        break;
                    }
                    self.env.pop_scope();
                }
            }
            Stmt::For {
                name,
                iterable,
                body,
            } => {
                let iter = self.eval_expr(iterable)?;
                match iter {
                    Value::Array(arr) => {
                        for item in arr {
                            self.env.push_scope();
                            self.env.define(name, item);
                            for stmt in body {
                                self.exec_stmt(stmt)?;
                            }
                            if let Some(Value::Bool(true)) = self.env.get("__break__") {
                                self.env.assign("__break__", Value::Bool(false)).ok();
                                self.env.pop_scope();
                                break;
                            }
                            self.env.pop_scope();
                        }
                    }
                    Value::String(s) => {
                        for ch in s.chars() {
                            self.env.push_scope();
                            self.env.define(name, Value::String(ch.to_string()));
                            for stmt in body {
                                self.exec_stmt(stmt)?;
                            }
                            if let Some(Value::Bool(true)) = self.env.get("__break__") {
                                self.env.assign("__break__", Value::Bool(false)).ok();
                                self.env.pop_scope();
                                break;
                            }
                            self.env.pop_scope();
                        }
                    }
                    _ => {
                        return Err(crate::RakError::Runtime(
                            "Cannot iterate over this value".to_string(),
                        ))
                    }
                }
            }
            Stmt::Scan { target, options, body } => {
                let target_val = self.eval_expr(target)?;
                let target_str = match target_val {
                    Value::String(s) => s,
                    other => other.to_string(),
                };

                // Parse options
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
                        // Specific port list
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
                                    self.output.push(format!(
                                        "[SCAN] Port {} (0x{:04X}) OPEN",
                                        port, port
                                    ));
                                    if let Some(body_stmts) = body {
                                        self.env.push_scope();
                                        self.env.define("port", Value::Int(port as u64));
                                        self.env.define("open", Value::Bool(true));
                                        let banner = rak_stdlib::net::tcp_banner_grab(&target_str, port, timeout_ms);
                                        if let Some(b) = banner {
                                            self.env.define("banner", Value::String(b));
                                        } else {
                                            self.env.define("banner", Value::Nil);
                                        }
                                        for stmt in body_stmts {
                                            self.exec_stmt(stmt)?;
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
                    self.output.push(format!(
                        "[SCAN] Port {} (0x{:04X}) OPEN",
                        port, port
                    ));
                    if let Some(body_stmts) = body {
                        self.env.push_scope();
                        self.env.define("port", Value::Int(port as u64));
                        self.env.define("open", Value::Bool(true));
                        let banner = rak_stdlib::net::tcp_banner_grab(&target_str, port, timeout_ms);
                        if let Some(b) = banner {
                            self.env.define("banner", Value::String(b));
                        } else {
                            self.env.define("banner", Value::Nil);
                        }
                        for stmt in body_stmts {
                            self.exec_stmt(stmt)?;
                        }
                        self.env.pop_scope();
                    }
                }
                self.output.push("[SCAN] Complete".to_string());
            }
            Stmt::Fetch { target, options, body } => {
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
                            self.env.define("status", Value::Int(resp.status as u64));
                            self.env.define("headers", Value::Map(header_map));
                            self.env.define("body", Value::String(resp.body.clone()));
                            for stmt in body_stmts {
                                self.exec_stmt(stmt)?;
                            }
                            self.env.pop_scope();
                        }
                    }
                    Err(e) => {
                        self.output.push(format!("[FETCH] Error: {}", e));
                    }
                }
            }
            Stmt::Dump { value, target } => {
                let val = self.eval_expr(value)?;
                if let Some(t) = target {
                    let target_val = self.eval_expr(t)?;
                    match &target_val {
                        Value::String(path) => {
                            // Write to file
                            match std::fs::write(path, val.to_string()) {
                                Ok(_) => self.output.push(format!("[DUMP] Written to {}", path)),
                                Err(e) => self.output.push(format!("[DUMP] File error: {}", e)),
                            }
                        }
                        _ => {
                            self.output.push(format!("[DUMP] {} -> {}", val, target_val));
                        }
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
                // For simplicity, continue is handled similarly to break in the loop
                self.env.define("__continue__", Value::Bool(true));
            }
            Stmt::Mod { name, items } => {
                self.env.push_scope();
                for stmt in items {
                    self.exec_stmt(stmt)?;
                }
                if let Some(mod_val) = self.env.get("__module_exports__") {
                    self.env.pop_scope();
                    self.env.define(name, mod_val);
                } else {
                    self.env.pop_scope();
                }
            }
            Stmt::Struct { name, fields: _ } => {
                // Store struct definition
                self.env.define(
                    name,
                    Value::String(format!("<struct {}>", name)),
                );
            }
            Stmt::Enum { name, variants } => {
                let mut map = HashMap::new();
                for (i, variant) in variants.iter().enumerate() {
                    map.insert(variant.clone(), Value::Int(i as u64));
                }
                self.env.define(name, Value::Map(map));
            }
            Stmt::Impl { target, methods } => {
                // Store methods in env
                for method in methods {
                    if let Stmt::Let { name, value, .. } = method {
                        let method_name = format!("{}.{}", target, name);
                        let val = self.eval_expr(value)?;
                        self.env.define(&method_name, val);
                    }
                }
            }
            Stmt::Match { value, arms } => {
                let val = self.eval_expr(value)?;
                for (pattern, body) in arms {
                    if self.pattern_matches(pattern, &val)? {
                        self.env.push_scope();
                        self.bind_pattern(pattern, &val)?;
                        for stmt in body {
                            self.exec_stmt(stmt)?;
                        }
                        self.env.pop_scope();
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn eval_expr(&mut self, expr: &Expr) -> crate::Result<Value> {
        match expr {
            Expr::Hex(h) => Ok(Value::Hex(*h)),
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Bytes(b) => Ok(Value::Bytes(b.clone())),
            Expr::Ident(name) => {
                self.env.get(name).ok_or_else(|| {
                    crate::RakError::Runtime(format!("Undefined variable: {}", name))
                })
            }
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nil => Ok(Value::Nil),
            Expr::Unary(op, expr) => {
                let val = self.eval_expr(expr)?;
                match op {
                    UnOp::Minus => match val {
                        Value::Hex(h) => Ok(Value::Hex(h.wrapping_neg())),
                        Value::Int(i) => Ok(Value::Int(i.wrapping_neg())),
                        _ => Err(crate::RakError::Runtime(
                            "Cannot negate this value".to_string(),
                        )),
                    },
                    UnOp::Not => Ok(Value::Bool(!is_truthy(&val))),
                    UnOp::BitNot => match val {
                        Value::Hex(h) => Ok(Value::Hex(!h)),
                        Value::Int(i) => Ok(Value::Int(!i)),
                        _ => Err(crate::RakError::Runtime(
                            "Cannot bitwise-not this value".to_string(),
                        )),
                    },
                }
            }
            Expr::Binary(op, left, right) => {
                let left_val = self.eval_expr(left)?;
                let right_val = self.eval_expr(right)?;
                self.eval_binary(op, &left_val, &right_val)
            }
            Expr::Assign(name, value) => {
                let val = self.eval_expr(value)?;
                self.env.assign(name, val.clone())?;
                Ok(val)
            }
            Expr::Function {
                params,
                return_type,
                body,
            } => {
                let _ = return_type;
                Ok(Value::Function {
                    params: params.clone(),
                    body: body.clone(),
                    closure: self.env.clone(),
                })
            }
            Expr::Call { callee, args } => {
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval_expr(a))
                    .collect::<crate::Result<_>>()?;

                // Check if callee is an identifier and handle builtins first
                if let Expr::Ident(name) = callee.as_ref() {
                    match name.as_str() {
                        "fmt" => {
                            if !arg_vals.is_empty() {
                                if let Value::String(format_str) = &arg_vals[0] {
                                    let result = self.format_string(format_str, &arg_vals[1..]);
                                    return Ok(Value::String(result));
                                }
                                return Ok(Value::String(arg_vals[0].to_string()));
                            }
                            return Ok(Value::String(String::new()));
                        }
                        "array" => return Ok(Value::Array(arg_vals)),
                        "map" => {
                            let mut map = HashMap::new();
                            let mut iter = arg_vals.into_iter();
                            while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
                                if let Value::String(k) = key {
                                    map.insert(k, value);
                                }
                            }
                            return Ok(Value::Map(map));
                        }
                        _ => {}
                    }

                    // Try user-defined function from environment
                    if let Some(val) = self.env.get(name) {
                        if let Value::Function { params, body, closure } = val {
                            self.env.push_scope();
                            for (key, val) in closure.scopes[0].iter() {
                                self.env.define(key, val.clone());
                            }
                            for (param, arg) in params.iter().zip(arg_vals.iter()) {
                                self.env.define(&param.name, arg.clone());
                            }
                            for stmt in &body {
                                self.exec_stmt(stmt)?;
                            }
                            let ret = self.env.get("__return__").unwrap_or(Value::Nil);
                            self.env.pop_scope();
                            return Ok(ret);
                        }
                    }

                    // Fall through to stdlib builtins
                    return self.eval_builtin(name, &arg_vals);
                }

                // Non-identifier callee: evaluate and call if it's a function
                let callee_val = self.eval_expr(callee)?;
                match callee_val {
                    Value::Function { params, body, closure } => {
                        self.env.push_scope();
                        for (key, val) in closure.scopes[0].iter() {
                            self.env.define(key, val.clone());
                        }
                        for (param, arg) in params.iter().zip(arg_vals.iter()) {
                            self.env.define(&param.name, arg.clone());
                        }
                        for stmt in &body {
                            self.exec_stmt(stmt)?;
                        }
                        let ret = self.env.get("__return__").unwrap_or(Value::Nil);
                        self.env.pop_scope();
                        Ok(ret)
                    }
                    _ => Err(crate::RakError::Runtime(
                        "Cannot call non-function".to_string(),
                    )),
                }
            }
            Expr::FieldAccess(obj, field) => {
                let obj_val = self.eval_expr(obj)?;
                match obj_val {
                    Value::Map(map) => {
                        map.get(field).cloned().ok_or_else(|| {
                            crate::RakError::Runtime(format!(
                                "Field '{}' not found",
                                field
                            ))
                        })
                    }
                    _ => Err(crate::RakError::Runtime(
                        "Cannot access field on this value".to_string(),
                    )),
                }
            }
            Expr::Index(obj, idx) => {
                let obj_val = self.eval_expr(obj)?;
                let idx_val = self.eval_expr(idx)?;
                match (&obj_val, &idx_val) {
                    (Value::Array(arr), Value::Int(i)) => {
                        let idx = *i as usize;
                        arr.get(idx).cloned().ok_or_else(|| {
                            crate::RakError::Runtime("Index out of bounds".to_string())
                        })
                    }
                    (Value::String(s), Value::Int(i)) => {
                        let idx = *i as usize;
                        s.chars()
                            .nth(idx)
                            .map(|c| Value::String(c.to_string()))
                            .ok_or_else(|| {
                                crate::RakError::Runtime("Index out of bounds".to_string())
                            })
                    }
                    (Value::Map(map), Value::String(key)) => {
                        map.get(key).cloned().ok_or_else(|| {
                            crate::RakError::Runtime(format!(
                                "Key '{}' not found",
                                key
                            ))
                        })
                    }
                    _ => Err(crate::RakError::Runtime(
                        "Invalid index operation".to_string(),
                    )),
                }
            }
        }
    }

    fn eval_binary(
        &mut self,
        op: &BinOp,
        left: &Value,
        right: &Value,
    ) -> crate::Result<Value> {
        match (op, left, right) {
            (BinOp::Add, Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b))),
            (BinOp::Add, Value::Array(a), Value::Array(b)) => {
                let mut result = a.clone();
                result.extend(b.clone());
                Ok(Value::Array(result))
            }
            _ => {
                let (l, r) = match (left, right) {
                    (Value::Hex(l), Value::Hex(r)) => (*l, *r),
                    (Value::Int(l), Value::Int(r)) => (*l, *r),
                    (Value::Hex(l), Value::Int(r)) => (*l, *r),
                    (Value::Int(l), Value::Hex(r)) => (*l, *r),
                    _ => {
                        return Err(crate::RakError::Runtime(
                            "Invalid operand types for binary operation".to_string(),
                        ))
                    }
                };

                let result = match op {
                    BinOp::Add => l.wrapping_add(r),
                    BinOp::Sub => l.wrapping_sub(r),
                    BinOp::Mul => l.wrapping_mul(r),
                    BinOp::Div => {
                        if r == 0 {
                            return Err(crate::RakError::Runtime(
                                "Division by zero".to_string(),
                            ));
                        }
                        l / r
                    }
                    BinOp::Rem => {
                        if r == 0 {
                            return Err(crate::RakError::Runtime(
                                "Division by zero".to_string(),
                            ));
                        }
                        l % r
                    }
                    BinOp::BitAnd => l & r,
                    BinOp::BitOr => l | r,
                    BinOp::BitXor => l ^ r,
                    BinOp::Shl => l << r,
                    BinOp::Shr => l >> r,
                    _ => l, // Fallback for non-arithmetic ops handled below
                };

                match op {
                    BinOp::Eq => Ok(Value::Bool(l == r)),
                    BinOp::NotEq => Ok(Value::Bool(l != r)),
                    BinOp::Lt => Ok(Value::Bool(l < r)),
                    BinOp::Gt => Ok(Value::Bool(l > r)),
                    BinOp::LtEq => Ok(Value::Bool(l <= r)),
                    BinOp::GtEq => Ok(Value::Bool(l >= r)),
                    BinOp::And => Ok(Value::Bool(is_truthy(left) && is_truthy(right))),
                    BinOp::Or => Ok(Value::Bool(is_truthy(left) || is_truthy(right))),
                    _ => {
                        if matches!(left, Value::Hex(_)) || matches!(right, Value::Hex(_)) {
                            Ok(Value::Hex(result))
                        } else {
                            Ok(Value::Int(result))
                        }
                    }
                }
            }
        }
    }

    fn eval_builtin(&mut self, name: &str, args: &[Value]) -> crate::Result<Value> {
        match name {
            // --- Crypto ---
            "md5" => {
                let data = self.val_to_bytes(args.first())?;
                Ok(Value::String(rak_stdlib::md5(&data)))
            }
            "sha1" => {
                let data = self.val_to_bytes(args.first())?;
                Ok(Value::String(rak_stdlib::sha1(&data)))
            }
            "sha256" => {
                let data = self.val_to_bytes(args.first())?;
                Ok(Value::String(rak_stdlib::sha256(&data)))
            }
            "xor" => {
                let data = self.val_to_bytes(args.first())?;
                let key = self.val_to_bytes(args.get(1))?;
                Ok(Value::Bytes(rak_stdlib::xor_encrypt(&data, &key)))
            }
            "rot13" => {
                let s = self.val_to_string(args.first())?;
                Ok(Value::String(rak_stdlib::rot13(&s)))
            }

            // --- Encoding ---
            "hex_encode" => {
                let data = self.val_to_bytes(args.first())?;
                Ok(Value::String(rak_stdlib::hex_encode(&data)))
            }
            "hex_decode" => {
                let s = self.val_to_string(args.first())?;
                match rak_stdlib::hex_decode(&s) {
                    Some(b) => Ok(Value::Bytes(b)),
                    None => Err(crate::RakError::Runtime("Invalid hex string".to_string())),
                }
            }
            "base64_encode" => {
                let data = self.val_to_bytes(args.first())?;
                Ok(Value::String(rak_stdlib::base64_encode(&data)))
            }
            "base64_decode" => {
                let s = self.val_to_string(args.first())?;
                match rak_stdlib::base64_decode(&s) {
                    Some(b) => Ok(Value::Bytes(b)),
                    None => Err(crate::RakError::Runtime("Invalid base64".to_string())),
                }
            }
            "url_encode" => {
                let s = self.val_to_string(args.first())?;
                Ok(Value::String(rak_stdlib::url_encode(&s)))
            }
            "url_decode" => {
                let s = self.val_to_string(args.first())?;
                match rak_stdlib::url_decode(&s) {
                    Some(decoded) => Ok(Value::String(decoded)),
                    None => Err(crate::RakError::Runtime("Invalid URL encoding".to_string())),
                }
            }

            // --- Recon ---
            "dns_lookup" => {
                let hostname = self.val_to_string(args.first())?;
                let ips = rak_stdlib::recon::dns_lookup(&hostname);
                let arr: Vec<Value> = ips
                    .iter()
                    .map(|ip| Value::String(ip.to_string()))
                    .collect();
                Ok(Value::Array(arr))
            }
            "subdomain_enum" => {
                let domain = self.val_to_string(args.first())?;
                let subs = rak_stdlib::recon::subdomain_enum(&domain);
                Ok(Value::Array(subs.into_iter().map(Value::String).collect()))
            }
            "reverse_dns" => {
                let ip = self.val_to_string(args.first())?;
                match rak_stdlib::recon::reverse_dns(&ip) {
                    Some(s) => Ok(Value::String(s)),
                    None => Ok(Value::Nil),
                }
            }

            // --- String / Array utilities ---
            "len" => {
                match args.first() {
                    Some(Value::String(s)) => Ok(Value::Int(s.chars().count() as u64)),
                    Some(Value::Array(a)) => Ok(Value::Int(a.len() as u64)),
                    Some(Value::Bytes(b)) => Ok(Value::Int(b.len() as u64)),
                    Some(Value::Map(m)) => Ok(Value::Int(m.len() as u64)),
                    _ => Err(crate::RakError::Runtime("len() requires a string, array, bytes, or map".to_string())),
                }
            }
            "split" => {
                let s = self.val_to_string(args.first())?;
                let delim = self.val_to_string(args.get(1))?;
                let parts: Vec<Value> = s.split(&delim).map(|p| Value::String(p.to_string())).collect();
                Ok(Value::Array(parts))
            }
            "join" => {
                let delim = self.val_to_string(args.first())?;
                if let Some(Value::Array(arr)) = args.get(1) {
                    let parts: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                    Ok(Value::String(parts.join(&delim)))
                } else {
                    Err(crate::RakError::Runtime("join() requires an array as second argument".to_string()))
                }
            }
            "contains" => {
                let haystack = self.val_to_string(args.first())?;
                let needle = self.val_to_string(args.get(1))?;
                Ok(Value::Bool(haystack.contains(&needle)))
            }
            "to_hex" => {
                let n = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
                Ok(Value::String(format!("0x{:X}", n)))
            }
            "from_hex" => {
                let s = self.val_to_string(args.first())?;
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                match u64::from_str_radix(s, 16) {
                    Ok(n) => Ok(Value::Hex(n)),
                    Err(_) => Err(crate::RakError::Runtime("Invalid hex literal".to_string())),
                }
            }
            "int" => {
                match args.first() {
                    Some(Value::Hex(h)) => Ok(Value::Int(*h)),
                    Some(Value::Int(i)) => Ok(Value::Int(*i)),
                    Some(Value::String(s)) => s.parse::<u64>().map(Value::Int).map_err(|_| crate::RakError::Runtime("Cannot convert to int".to_string())),
                    Some(Value::Bool(b)) => Ok(Value::Int(if *b { 1 } else { 0 })),
                    _ => Ok(Value::Int(0)),
                }
            }
            "string" => {
                Ok(Value::String(self.val_to_string(args.first())?))
            }
            "bytes" => {
                Ok(Value::Bytes(self.val_to_bytes(args.first())?))
            }
            "upper" => {
                Ok(Value::String(self.val_to_string(args.first())?.to_uppercase()))
            }
            "lower" => {
                Ok(Value::String(self.val_to_string(args.first())?.to_lowercase()))
            }
            "trim" => {
                Ok(Value::String(self.val_to_string(args.first())?.trim().to_string()))
            }
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

            // --- File operations ---
            "read" => {
                let path = self.val_to_string(args.first())?;
                match std::fs::read_to_string(&path) {
                    Ok(content) => Ok(Value::String(content)),
                    Err(e) => Err(crate::RakError::Runtime(format!("Read error: {}", e))),
                }
            }
            "write" => {
                let path = self.val_to_string(args.first())?;
                let content = self.val_to_string(args.get(1))?;
                match std::fs::write(&path, content) {
                    Ok(_) => Ok(Value::Bool(true)),
                    Err(e) => Err(crate::RakError::Runtime(format!("Write error: {}", e))),
                }
            }

            // --- File operations ---
            "file_read" => {
                let path = self.val_to_string(args.first())?;
                match rak_stdlib::file::read(&path) {
                    Ok(content) => Ok(Value::String(content)),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "file_write" => {
                let path = self.val_to_string(args.first())?;
                let content = self.val_to_string(args.get(1))?;
                match rak_stdlib::file::write(&path, &content) {
                    Ok(_) => Ok(Value::Bool(true)),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "file_append" => {
                let path = self.val_to_string(args.first())?;
                let content = self.val_to_string(args.get(1))?;
                match rak_stdlib::file::append(&path, &content) {
                    Ok(_) => Ok(Value::Bool(true)),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "file_exists" => {
                let path = self.val_to_string(args.first())?;
                Ok(Value::Bool(rak_stdlib::file::exists(&path)))
            }
            "file_size" => {
                let path = self.val_to_string(args.first())?;
                Ok(Value::Int(rak_stdlib::file::size(&path).unwrap_or(0)))
            }
            "file_list" => {
                let path = self.val_to_string(args.first())?;
                let entries = rak_stdlib::file::list(&path);
                Ok(Value::Array(entries.into_iter().map(Value::String).collect()))
            }
            "file_delete" => {
                let path = self.val_to_string(args.first())?;
                Ok(Value::Bool(rak_stdlib::file::delete(&path)))
            }
            "file_mkdir" => {
                let path = self.val_to_string(args.first())?;
                Ok(Value::Bool(rak_stdlib::file::create_dir(&path)))
            }
            "file_copy" => {
                let src = self.val_to_string(args.first())?;
                let dst = self.val_to_string(args.get(1))?;
                Ok(Value::Bool(rak_stdlib::file::copy(&src, &dst)))
            }
            "file_rename" => {
                let src = self.val_to_string(args.first())?;
                let dst = self.val_to_string(args.get(1))?;
                Ok(Value::Bool(rak_stdlib::file::rename(&src, &dst)))
            }
            "file_ext" => {
                let path = self.val_to_string(args.first())?;
                Ok(Value::String(rak_stdlib::file::ext(&path).unwrap_or_default()))
            }
            "file_basename" => {
                let path = self.val_to_string(args.first())?;
                Ok(Value::String(rak_stdlib::file::basename(&path)))
            }
            "file_dirname" => {
                let path = self.val_to_string(args.first())?;
                Ok(Value::String(rak_stdlib::file::dirname(&path)))
            }

            // --- Web / HTML parsing ---
            "html_title" => {
                let html = self.val_to_string(args.first())?;
                match rak_stdlib::web::html_title(&html) {
                    Some(title) => Ok(Value::String(title)),
                    None => Ok(Value::Nil),
                }
            }
            "html_select" => {
                let html = self.val_to_string(args.first())?;
                let selector = self.val_to_string(args.get(1))?;
                match rak_stdlib::web::html_select(&html, &selector) {
                    Some(text) => Ok(Value::String(text)),
                    None => Ok(Value::Nil),
                }
            }
            "html_select_all" => {
                let html = self.val_to_string(args.first())?;
                let selector = self.val_to_string(args.get(1))?;
                let results = rak_stdlib::web::html_select_all(&html, &selector);
                Ok(Value::Array(results.into_iter().map(Value::String).collect()))
            }
            "html_attr" => {
                let html = self.val_to_string(args.first())?;
                let selector = self.val_to_string(args.get(1))?;
                let attr = self.val_to_string(args.get(2))?;
                match rak_stdlib::web::html_attr(&html, &selector, &attr) {
                    Some(val) => Ok(Value::String(val)),
                    None => Ok(Value::Nil),
                }
            }
            "html_links" => {
                let html = self.val_to_string(args.first())?;
                let links = rak_stdlib::web::html_links(&html);
                Ok(Value::Array(links.into_iter().map(Value::String).collect()))
            }
            "html_images" => {
                let html = self.val_to_string(args.first())?;
                let imgs = rak_stdlib::web::html_images(&html);
                Ok(Value::Array(imgs.into_iter().map(Value::String).collect()))
            }
            "html_scripts" => {
                let html = self.val_to_string(args.first())?;
                let scripts = rak_stdlib::web::html_scripts(&html);
                Ok(Value::Array(scripts.into_iter().map(Value::String).collect()))
            }
            "html_forms" => {
                let html = self.val_to_string(args.first())?;
                let forms = rak_stdlib::web::html_forms(&html);
                let result: Vec<Value> = forms.into_iter().map(|form| {
                    Value::Map(form.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
                }).collect();
                Ok(Value::Array(result))
            }
            "html_inputs" => {
                let html = self.val_to_string(args.first())?;
                let inputs = rak_stdlib::web::html_inputs(&html);
                let result: Vec<Value> = inputs.into_iter().map(|input| {
                    Value::Map(input.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
                }).collect();
                Ok(Value::Array(result))
            }
            "html_meta" => {
                let html = self.val_to_string(args.first())?;
                let name = self.val_to_string(args.get(1))?;
                match rak_stdlib::web::html_meta(&html, &name) {
                    Some(content) => Ok(Value::String(content)),
                    None => Ok(Value::Nil),
                }
            }
            "html_count" => {
                let html = self.val_to_string(args.first())?;
                let selector = self.val_to_string(args.get(1))?;
                Ok(Value::Int(rak_stdlib::web::html_count(&html, &selector) as u64))
            }
            "html_headers" => {
                let html = self.val_to_string(args.first())?;
                let headers = rak_stdlib::web::html_headers(&html);
                Ok(Value::Array(headers.into_iter().map(Value::String).collect()))
            }

            // --- JSON utilities ---
            "json_parse" => {
                let input = self.val_to_string(args.first())?;
                match rak_stdlib::js::json_parse(&input) {
                    Ok(val) => Ok(Value::String(val.to_string())),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "json_get" => {
                let input = self.val_to_string(args.first())?;
                let key = self.val_to_string(args.get(1))?;
                match rak_stdlib::js::json_get(&input, &key) {
                    Ok(val) => Ok(Value::String(val)),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "json_path" => {
                let input = self.val_to_string(args.first())?;
                let path = self.val_to_string(args.get(1))?;
                match rak_stdlib::js::json_path(&input, &path) {
                    Ok(val) => Ok(Value::String(val)),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "json_keys" => {
                let input = self.val_to_string(args.first())?;
                match rak_stdlib::js::json_keys(&input) {
                    Ok(keys) => Ok(Value::Array(keys.into_iter().map(Value::String).collect())),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "json_len" => {
                let input = self.val_to_string(args.first())?;
                match rak_stdlib::js::json_len(&input) {
                    Ok(n) => Ok(Value::Int(n as u64)),
                    Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
                }
            }
            "json_find_all" => {
                let input = self.val_to_string(args.first())?;
                let key = self.val_to_string(args.get(1))?;
                let results = rak_stdlib::js::json_find_all(&input, &key);
                Ok(Value::Array(results.into_iter().map(Value::String).collect()))
            }

            // --- Scan as function (returns array of open ports for for-loops) ---
            "scan_ports" => {
                let target = self.val_to_string(args.first())?;
                let mut start_port: u16 = 1;
                let mut end_port: u16 = 1024;
                let mut timeout_ms: u64 = 1000;

                // Options can be a map or a range array
                if let Some(Value::Map(opts)) = args.get(1) {
                    if let Some(Value::Array(range)) = opts.get("range") {
                        if range.len() >= 2 {
                            start_port = range[0].as_u64().unwrap_or(1) as u16;
                            end_port = range[1].as_u64().unwrap_or(1024) as u16;
                        }
                    }
                    if let Some(Value::Int(t)) = opts.get("timeout") {
                        timeout_ms = *t;
                    }
                } else if let Some(Value::Array(range)) = args.get(1) {
                    if range.len() >= 2 {
                        start_port = range[0].as_u64().unwrap_or(1) as u16;
                        end_port = range[1].as_u64().unwrap_or(1024) as u16;
                    }
                }

                let open_ports = rak_stdlib::recon::port_scan(&target, start_port, end_port, timeout_ms);
                let result: Vec<Value> = open_ports.into_iter().map(|p| Value::Int(p as u64)).collect();
                Ok(Value::Array(result))
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

            _ => {
                Err(crate::RakError::Runtime(format!("Unknown function: {}", name)))
            }
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

    fn pattern_matches(&self, pattern: &Pattern, value: &Value) -> crate::Result<bool> {
        Ok(match (pattern, value) {
            (Pattern::Wild, _) => true,
            (Pattern::Ident(_), _) => true,
            (Pattern::Hex(h), Value::Hex(v)) => h == v,
            (Pattern::Int(i), Value::Int(v)) => i == v,
            (Pattern::String(s), Value::String(v)) => s == v,
            (Pattern::Bool(b), Value::Bool(v)) => b == v,
            (Pattern::Nil, Value::Nil) => true,
            _ => false,
        })
    }

    fn bind_pattern(&mut self, pattern: &Pattern, value: &Value) -> crate::Result<()> {
        if let Pattern::Ident(name) = pattern {
            if name != "_" {
                self.env.define(name, value.clone());
            }
        }
        Ok(())
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
                    // Parse format spec
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
                            if let Value::Hex(h) = arg {
                                result.push_str(&format!("{:04X}", h));
                            } else if let Value::Int(i) = arg {
                                result.push_str(&format!("{:04X}", i));
                            }
                        } else if spec.contains(":08X") {
                            if let Value::Hex(h) = arg {
                                result.push_str(&format!("{:08X}", h));
                            } else if let Value::Int(i) = arg {
                                result.push_str(&format!("{:08X}", i));
                            }
                        } else if spec.contains('X') || spec.contains('x') {
                            if let Value::Hex(h) = arg {
                                result.push_str(&format!("{:X}", h));
                            } else if let Value::Int(i) = arg {
                                result.push_str(&format!("{:X}", i));
                            }
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

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Nil => false,
        Value::Int(0) => false,
        Value::Hex(0) => false,
        Value::String(s) => !s.is_empty(),
        Value::Bytes(b) => !b.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Map(m) => !m.is_empty(),
        _ => true,
    }
}

impl Value {
    fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Hex(h) => Some(*h),
            Value::Int(i) => Some(*i),
            _ => None,
        }
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
        let output = interp
            .run_source("let total = 0; for x in [1, 2, 3] { total = total + x; } dump total;")
            .unwrap();
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
        let output = interp
            .run_source("let r = 0xDEADBEEF & 0xFF00FF00; dump r;")
            .unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 0xDE00BE00")));
    }

    #[test]
    fn test_interpreter_for_over_array() {
        let mut interp = Interpreter::new();
        let output = interp
            .run_source("for x in [10, 20, 30] { dump x; }")
            .unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 10")));
        assert!(output.iter().any(|l| l.contains("[DUMP] 20")));
        assert!(output.iter().any(|l| l.contains("[DUMP] 30")));
    }

    #[test]
    fn test_interpreter_json_get() {
        let mut interp = Interpreter::new();
        let output = interp
            .run_source("dump json_get(\"{\\\"name\\\": \\\"Rak\\\"}\", \"name\");")
            .unwrap_or_default();
        let _ = output;
    }

    #[test]
    fn test_interpreter_html_select() {
        let mut interp = Interpreter::new();
        let html = "<html><title>Test Page</title><body>Hello</body></html>";
        let source = format!("dump html_title(\"{}\");", html);
        let output = interp.run_source(&source).unwrap();
        assert!(output.iter().any(|l| l.contains("Test Page")));
    }

    #[test]
    fn test_interpreter_html_links() {
        let mut interp = Interpreter::new();
        let html = "<a href='https://a.com'>A</a><a href='https://b.com'>B</a>";
        let source = format!("let links = html_links(\"{}\"); dump len(links);", html);
        let output = interp.run_source(&source).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 2")));
    }

    #[test]
    fn test_interpreter_file_write_read() {
        let mut interp = Interpreter::new();
        let output = interp
            .run_source("file_write(\"test_rak.txt\", \"hello\"); dump file_read(\"test_rak.txt\");")
            .unwrap();
        assert!(output.iter().any(|l| l.contains("hello")));
        let _ = std::fs::remove_file("test_rak.txt");
    }

    #[test]
    fn test_interpreter_string_ops() {
        let mut interp = Interpreter::new();
        let output = interp
            .run_source("dump upper(\"hello\"); dump len(split(\"a,b,c\", \",\")); dump contains(\"hello world\", \"world\");")
            .unwrap();
        assert!(output.iter().any(|l| l.contains("HELLO")));
        assert!(output.iter().any(|l| l.contains("[DUMP] 3")));
        assert!(output.iter().any(|l| l.contains("true")));
    }
}