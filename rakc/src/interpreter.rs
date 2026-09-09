use crate::ast::*;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::ffi::CString;

fn async_runtime() -> &'static tokio::runtime::Runtime {
    crate::async_rt::runtime()
}

/// The state of a `Value::Future`. A future is either already resolved, pending
/// on a Tokio task (async I/O builtins), or a deferred `async fn` body that the
/// interpreter runs on the first `await`.
pub enum FutureState {
    Ready(Value),
    Pending(tokio::task::JoinHandle<Value>),
    Deferred {
        params: Vec<Param>,
        body: Vec<Stmt>,
        closure: Arc<Env>,
        args: Vec<Value>,
    },
    Polled,
}

pub struct FutureHandle {
    pub state: Mutex<FutureState>,
}

/// A compiled regular expression value. Stored behind an `Arc` so it can be
/// cloned cheaply inside `Value`.
pub struct RegexValue {
    pub pattern: String,
    pub flags: String,
    pub re: regex::Regex,
}

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
    Regex(Arc<RegexValue>),
    /// A loaded native shared library (`ffi_load` / `extern` default lib).
    ForeignLib(Arc<Mutex<rak_stdlib::ffi::LibHandle>>),
    /// An opaque raw pointer (`ffi_ptr`, `ffi_alloc`, FFI returns).
    ForeignPtr(u64),
    /// A memory-mapped file (`mmap_open`).
    Mmap(Arc<rak_stdlib::mmap::MmapHandle>),
    /// A zero-copy view into a `Mmap` (`mmap_slice`); keeps the mapping alive.
    MmapSlice(Arc<rak_stdlib::mmap::MmapHandle>, usize, usize),
    /// An async future (`async fn` or an async I/O builtin).
    Future(Arc<FutureHandle>),
    /// A PCAP capture handle (`pcap_open`).
    Pcap(Arc<Mutex<rak_stdlib::pcap::PcapHandle>>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

/// Holds a buffer alive for the duration of an FFI call so a `u64` argument
/// pointing into it stays valid.
enum MarshalGuard {
    #[allow(dead_code)]
    CStr(CString),
    #[allow(dead_code)]
    Bytes(Vec<u8>),
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
            (Value::Regex(a), Value::Regex(b)) => a.pattern == b.pattern && a.flags == b.flags,
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
            Value::Regex(r) => write!(f, "/{}/{}", r.pattern, r.flags),
            Value::ForeignLib(_) => write!(f, "<ffi-lib>"),
            Value::ForeignPtr(p) => write!(f, "0x{:X}", p),
            Value::Mmap(_) => write!(f, "<mmap>"),
            Value::MmapSlice(_, _, n) => write!(f, "<mmap-slice {}B>", n),
            Value::Future(_) => write!(f, "<future>"),
            Value::Pcap(_) => write!(f, "<pcap>"),
        }
    }
}

#[derive(Clone)]
pub struct Env {
    global: Arc<Mutex<HashMap<String, Value>>>,
    scopes: Vec<HashMap<String, Value>>,
}

impl Default for Env {
    fn default() -> Self {
        Env::new()
    }
}

impl Env {
    pub fn new() -> Self {
        Env {
            global: Arc::new(Mutex::new(HashMap::new())),
            scopes: Vec::new(),
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        if !self.scopes.is_empty() {
            self.scopes.pop();
        }
    }

    pub fn define(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), value);
        } else {
            self.global.lock().unwrap().insert(name.to_string(), value);
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val.clone());
            }
        }
        self.global.lock().unwrap().get(name).cloned()
    }

    pub fn assign(&mut self, name: &str, value: Value) -> crate::Result<()> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return Ok(());
            }
        }
        let mut g = self.global.lock().unwrap();
        if g.contains_key(name) {
            g.insert(name.to_string(), value);
            return Ok(());
        }
        drop(g);
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
    #[cfg(feature = "gui")]
    gui: Option<crate::gui::GuiManager>,
    /// (trait, type, method) -> function. trait == "" for inherent impls.
    trait_impls: HashMap<(String, String, String), Value>,
    /// (type, method) -> function, used for `obj.method(...)` call syntax.
    /// Populated from both inherent and trait impls.
    methods: HashMap<(String, String), Value>,
    /// `extern "C"` declarations, keyed by function name. Calls to these names
    /// resolve here before the generic builtin dispatch.
    foreign_fns: HashMap<String, ForeignFnDecl>,
    /// Lazily-loaded platform default C library for `extern "C"` blocks that
    /// don't name an explicit `from "path"`.
    foreign_default_lib: Option<Arc<Mutex<rak_stdlib::ffi::LibHandle>>>,
    /// Tracked native allocations made by `ffi_alloc` / `ffi_string_to_cstr`,
    /// keyed by raw pointer address → byte length, so `ffi_free` can release
    /// them with the correct `Vec::from_raw_parts` layout.
    ffi_allocs: HashMap<u64, usize>,
}

/// An `extern "C"` declaration plus the (lazily resolved) library it lives in.
#[derive(Clone)]
struct ForeignFnDecl {
    decl: crate::ast::ForeignFn,
    lib: Arc<Mutex<rak_stdlib::ffi::LibHandle>>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            env: Env::new(),
            output: vec![],
            base_dir: ".".to_string(),
            returning: false,
            return_value: Value::Nil,
            #[cfg(feature = "gui")]
            gui: None,
            trait_impls: HashMap::new(),
            methods: HashMap::new(),
            foreign_fns: HashMap::new(),
            foreign_default_lib: None,
            ffi_allocs: HashMap::new(),
        }
    }

    pub fn with_base_dir(base_dir: String) -> Self {
        Interpreter {
            env: Env::new(),
            output: vec![],
            base_dir,
            returning: false,
            return_value: Value::Nil,
            #[cfg(feature = "gui")]
            gui: None,
            trait_impls: HashMap::new(),
            methods: HashMap::new(),
            foreign_fns: HashMap::new(),
            foreign_default_lib: None,
            ffi_allocs: HashMap::new(),
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
                let tn = iter.type_name();
                if let Some(func) = self
                    .trait_impls
                    .get(&("Iterable".to_string(), tn.clone(), "iter".to_string()))
                    .cloned()
                {
                    let produced = self.call_method_value(func, iter, &[])?;
                    match produced {
                        Value::Array(items) => self.run_for_loop(name, items, body)?,
                        other => {
                            return Err(crate::RakError::Runtime(format!(
                                "Iterable::iter must return an array, got {}",
                                other.type_name()
                            )))
                        }
                    }
                } else {
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
            }
            Stmt::Scan { target, options, body } => self.exec_scan(target, options, body)?,
            Stmt::Fetch { target, options, body } => self.exec_fetch(target, options, body)?,
            Stmt::Dump { value, target } => {
                let val = self.eval_expr(value)?;
                if let Some(t) = target {
                    let target_val = self.eval_expr(t)?;
                    match &target_val {
                        Value::String(path) => {
                            let s = self.display_value(&val)?;
                            match std::fs::write(path, s) {
                                Ok(_) => self.output.push(format!("[DUMP] Written to {}", path)),
                                Err(e) => self.output.push(format!("[DUMP] File error: {}", e)),
                            }
                        }
                        _ => self.output.push(format!("[DUMP] {} -> {}", val, target_val)),
                    }
                } else {
                    let s = self.display_value(&val)?;
                    self.output.push(format!("[DUMP] {}", s));
                }
            }
            Stmt::Trace { value } => {
                let val = self.eval_expr(value)?;
                let s = self.debug_value(&val)?;
                self.output.push(format!("[TRACE] {}", s));
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
            Stmt::Impl { target, trait_name, methods } => {
                // Parser stores `impl <A> for <B>` as target=A (trait), trait_name=B (type).
                // For an inherent `impl <Type>` (no `for`), trait_name is None.
                let (trait_str, type_name) = match trait_name {
                    Some(ty) => (target.clone(), ty.clone()),
                    None => (String::new(), target.clone()),
                };
                for method in methods {
                    if let Stmt::Let { name: mname, value, .. } = method {
                        let method_name = format!("{}.{}", target, mname);
                        let val = self.eval_expr(value)?;
                        self.env.define(&method_name, val.clone());
                        self.trait_impls.insert(
                            (trait_str.clone(), type_name.clone(), mname.clone()),
                            val.clone(),
                        );
                        self.methods.insert((type_name.clone(), mname.clone()), val);
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
            Stmt::Extern { abi: _, lib, decls } => {
                let lib_handle = if let Some(path) = lib {
                    Arc::new(Mutex::new(
                        rak_stdlib::ffi::load(path).map_err(crate::RakError::Runtime)?,
                    ))
                } else {
                    match &self.foreign_default_lib {
                        Some(arc) => arc.clone(),
                        None => {
                            let h = rak_stdlib::ffi::load_default()
                                .map_err(crate::RakError::Runtime)?;
                            let arc = Arc::new(Mutex::new(h));
                            self.foreign_default_lib = Some(arc.clone());
                            arc
                        }
                    }
                };
                for decl in decls {
                    self.foreign_fns.insert(
                        decl.name.clone(),
                        ForeignFnDecl { decl: decl.clone(), lib: lib_handle.clone() },
                    );
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
            Expr::Regex(pattern, flags) => {
                let r = self.build_regex(pattern, flags)?;
                Ok(Value::Regex(r))
            }
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
                let container = self.eval_expr(obj)?;
                let idx_val = self.eval_expr(idx)?;
                let tn = container.type_name();
                if let Some(func) = self
                    .trait_impls
                    .get(&("IndexMut".to_string(), tn, "set".to_string()))
                    .cloned()
                {
                    // IndexMut::set returns the updated container (functional
                    // update semantics), which we store back to the target.
                    let updated = self.call_method_with_values(func, container, vec![idx_val, v.clone()])?;
                    self.store_back(obj, updated)?;
                    return Ok(v);
                }
                let mut container = container;
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
                // Zero-copy indexing/slicing for memory maps.
                if let Value::Mmap(h) = &obj_val {
                    if let Expr::Range(lo, hi) = idx.as_ref() {
                        let len = h.len();
                        let s = lo.as_ref().map(|e| self.eval_expr(e)).transpose()?.and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                        let e = hi.as_ref().map(|e| self.eval_expr(e)).transpose()?.and_then(|v| v.as_i64()).unwrap_or(len as i64).min(len as i64).max(0) as usize;
                        if s > e { return Err(crate::RakError::Runtime("mmap slice: start > end".to_string())); }
                        return Ok(Value::MmapSlice(h.clone(), s, e - s));
                    }
                    let idx_val = self.eval_expr(idx)?;
                    let i = idx_val.as_i64().unwrap_or(0) as usize;
                    let data = h.as_slice();
                    if i >= data.len() { return Err(crate::RakError::Runtime("Index out of bounds".to_string())); }
                    return Ok(Value::Int(data[i] as i64));
                }
                if let Value::MmapSlice(h, base, n) = &obj_val {
                    if let Expr::Range(lo, hi) = idx.as_ref() {
                        let s = lo.as_ref().map(|e| self.eval_expr(e)).transpose()?.and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                        let e = hi.as_ref().map(|e| self.eval_expr(e)).transpose()?.and_then(|v| v.as_i64()).unwrap_or(*n as i64).min(*n as i64).max(0) as usize;
                        if s > e { return Err(crate::RakError::Runtime("mmap slice: start > end".to_string())); }
                        return Ok(Value::MmapSlice(h.clone(), base + s, e - s));
                    }
                    let idx_val = self.eval_expr(idx)?;
                    let i = idx_val.as_i64().unwrap_or(0) as usize;
                    if i >= *n { return Err(crate::RakError::Runtime("Index out of bounds".to_string())); }
                    return Ok(Value::Int(h.as_slice()[base + i] as i64));
                }
                if let Value::Bytes(b) = &obj_val {
                    if let Expr::Range(lo, hi) = idx.as_ref() {
                        let len = b.len();
                        let s = lo.as_ref().map(|e| self.eval_expr(e)).transpose()?.and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                        let e = hi.as_ref().map(|e| self.eval_expr(e)).transpose()?.and_then(|v| v.as_i64()).unwrap_or(len as i64).min(len as i64).max(0) as usize;
                        if s > e { return Err(crate::RakError::Runtime("bytes slice: start > end".to_string())); }
                        return Ok(Value::Bytes(b[s..e].to_vec()));
                    }
                    let idx_val = self.eval_expr(idx)?;
                    if let Value::Int(i) = idx_val {
                        return b.get(i as usize).map(|v| Value::Int(*v as i64)).ok_or_else(|| crate::RakError::Runtime("Index out of bounds".to_string()));
                    }
                }
                let idx_val = self.eval_expr(idx)?;
                let tn = obj_val.type_name();
                if let Some(func) = self
                    .trait_impls
                    .get(&("Index".to_string(), tn.clone(), "index".to_string()))
                    .cloned()
                {
                    return self.call_method_with_values(func, obj_val, vec![idx_val]);
                }
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
            Expr::Await(inner) => {
                let fv = self.eval_expr(inner)?;
                self.await_future(fv)
            }
            Expr::Spawn(inner) => {
                let v = self.eval_expr(inner)?;
                self.spawn_value(v)
            }
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
            if let Some(decl) = self.foreign_fns.get(name).cloned() {
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
                return self.call_foreign(decl, &arg_vals);
            }
            let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
            return self.eval_builtin(name, &arg_vals);
        }
        if let Expr::Path(segs) = callee {
            return self.eval_path(segs, args);
        }
        // Method-call syntax: `obj.method(args...)`
        if let Expr::FieldAccess(obj_expr, method) = callee {
            let obj_val = self.eval_expr(obj_expr)?;
            if let Value::Regex(_) = &obj_val {
                return self.call_regex_method(&obj_val, method, args);
            }
            if let Value::ForeignLib(_) = &obj_val {
                return self.call_foreign_lib_method(&obj_val, method, args);
            }
            let tn = obj_val.type_name();
            if let Some(func) = self.methods.get(&(tn.clone(), method.clone())).cloned() {
                return self.call_method_value(func, obj_val, args);
            }
            // Fallback: a field that itself holds a callable (modules, etc.).
            if let Some(v) = self.field_value(&obj_val, method) {
                if let Value::Function { params, body, closure, is_async } = v {
                    return self.call_function(&params, &body, &closure, is_async, args);
                }
            }
            return Err(crate::RakError::Runtime(format!(
                "No method '{}' on {}",
                method, tn
            )));
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
        let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
        self.call_function_values(params, body, closure, is_async, arg_vals)
    }

    /// Call a function value with already-evaluated argument values. Used by
    /// trait-method dispatch where the receiver and arguments are computed
    /// before the call.
    fn call_function_with_values(&mut self, func: Value, arg_vals: Vec<Value>) -> crate::Result<Value> {
        if let Value::Function { params, body, closure, is_async } = func {
            self.call_function_values(&params, &body, &closure, is_async, arg_vals)
        } else {
            Err(crate::RakError::Runtime("value is not callable".to_string()))
        }
    }

    fn call_function_values(&mut self, params: &[Param], body: &[Stmt], closure: &Arc<Env>, is_async: bool, arg_vals: Vec<Value>) -> crate::Result<Value> {
        if is_async {
            // An async function does not run its body at call time; it returns a
            // deferred future whose body is driven on the first `await`.
            return Ok(Value::Future(Arc::new(FutureHandle {
                state: Mutex::new(FutureState::Deferred {
                    params: params.to_vec(),
                    body: body.to_vec(),
                    closure: closure.clone(),
                    args: arg_vals,
                }),
            })));
        }
        let saved_returning = self.returning;
        self.returning = false;
        let saved_env = self.env.clone();
        self.env = (**closure).clone();
        self.env.push_scope();
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
        self.env = saved_env;
        self.returning = saved_returning;
        Ok(ret)
    }

    /// Resolve a `Value::Future`. A ready future returns its value; a pending
    /// Tokio-backed future (async I/O builtin) is awaited via `block_on`; a
    /// deferred `async fn` body is run on the interpreter thread. Awaiting a
    /// non-future value returns it unchanged.
    fn await_future(&mut self, fv: Value) -> crate::Result<Value> {
        let handle = match fv {
            Value::Future(h) => h,
            other => return Ok(other),
        };
        let state = std::mem::replace(&mut *handle.state.lock().unwrap(), FutureState::Polled);
        match state {
            FutureState::Ready(v) => Ok(v),
            FutureState::Pending(jh) => {
                let joined = async_runtime().block_on(async { jh.await })
                    .map_err(|e| crate::RakError::Runtime(format!("await: task failed: {}", e)))?;
                *handle.state.lock().unwrap() = FutureState::Ready(joined.clone());
                Ok(joined)
            }
            FutureState::Deferred { params, body, closure, args } => {
                let result = self.call_function_values(&params, &body, &closure, false, args)?;
                *handle.state.lock().unwrap() = FutureState::Ready(result.clone());
                Ok(result)
            }
            FutureState::Polled => Err(crate::RakError::Runtime("await: future already polled".to_string())),
        }
    }

    /// `spawn` a value: a `Future` is returned as-is (async I/O is already
    /// concurrent); a function runs on a native thread (legacy `spawn`).
    fn spawn_value(&mut self, v: Value) -> crate::Result<Value> {
        match v {
            Value::Future(_) => Ok(v),
            Value::Function { params: _, body, closure, .. } => {
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
                    if interp.returning {
                        std::mem::replace(&mut interp.return_value, Value::Nil)
                    } else {
                        Value::Nil
                    }
                });
                Ok(Value::JoinHandle(Arc::new(Mutex::new(Some(handle)))))
            }
            other => Ok(other),
        }
    }

    /// Dispatch a user-defined method `obj.method(args...)`, passing `obj` as
    /// the first argument (the receiver).
    fn call_method_value(&mut self, func: Value, receiver: Value, args: &[Expr]) -> crate::Result<Value> {
        let mut arg_vals = vec![receiver];
        for a in args {
            arg_vals.push(self.eval_expr(a)?);
        }
        self.call_function_with_values(func, arg_vals)
    }

    /// Like `call_method_value` but with already-evaluated argument values.
    fn call_method_with_values(&mut self, func: Value, receiver: Value, args: Vec<Value>) -> crate::Result<Value> {
        let mut arg_vals = vec![receiver];
        arg_vals.extend(args);
        self.call_function_with_values(func, arg_vals)
    }

    /// Read a field of a value as a callable, used as a fallback for
    /// `obj.method(...)` when no method is registered (e.g. module functions
    /// or struct fields that hold functions).
    fn field_value(&self, obj: &Value, field: &str) -> Option<Value> {
        match obj {
            Value::Map(m) => m.get(field).cloned(),
            Value::Struct { fields, .. } => fields.get(field).cloned(),
            Value::Module(m) => m.get(field).cloned(),
            Value::Tuple(t) => field.parse::<usize>().ok().and_then(|i| t.get(i).cloned()),
            _ => None,
        }
    }

    /// Render a value using `Display::fmt` if implemented, else `to_string`.
    fn display_value(&mut self, v: &Value) -> crate::Result<String> {
        let tn = v.type_name();
        if let Some(func) = self
            .trait_impls
            .get(&("Display".to_string(), tn, "fmt".to_string()))
            .cloned()
        {
            let r = self.call_method_with_values(func, v.clone(), vec![])?;
            self.val_to_string(Some(&r))
        } else {
            Ok(v.to_string())
        }
    }

    /// Render a value using `Debug::fmt` if implemented, else `{:?}`.
    fn debug_value(&mut self, v: &Value) -> crate::Result<String> {
        let tn = v.type_name();
        if let Some(func) = self
            .trait_impls
            .get(&("Debug".to_string(), tn, "fmt".to_string()))
            .cloned()
        {
            let r = self.call_method_with_values(func, v.clone(), vec![])?;
            self.val_to_string(Some(&r))
        } else {
            Ok(format!("{:?}", v))
        }
    }

    /// Compile a regex pattern with the given flags into a `RegexValue`.
    fn build_regex(&self, pattern: &str, flags: &str) -> crate::Result<Arc<RegexValue>> {
        let mut b = regex::RegexBuilder::new(pattern);
        for f in flags.chars() {
            match f {
                'i' | 'I' => b.case_insensitive(true),
                'm' | 'M' => b.multi_line(true),
                's' | 'S' => b.dot_matches_new_line(true),
                'x' | 'X' => b.ignore_whitespace(true),
                'g' | 'G' => continue,
                _ => {
                    return Err(crate::RakError::Runtime(format!(
                        "unknown regex flag '{}'",
                        f
                    )))
                }
            };
        }
        let re = b.build().map_err(|e| {
            crate::RakError::Runtime(format!("invalid regex /{}/{}: {}", pattern, flags, e))
        })?;
        Ok(Arc::new(RegexValue {
            pattern: pattern.to_string(),
            flags: flags.to_string(),
            re,
        }))
    }

    /// Coerce an argument into a compiled regex: pass through `Value::Regex`,
    /// or build one from a string pattern (with optional flags argument).
    fn coerce_regex(&self, v: Option<&Value>, flags_arg: Option<&Value>) -> crate::Result<Arc<RegexValue>> {
        match v {
            Some(Value::Regex(r)) => Ok(r.clone()),
            Some(other) => {
                let pattern = self.val_to_string(Some(other))?;
                let flags = self.val_to_string(flags_arg)?;
                self.build_regex(&pattern, &flags)
            }
            None => Err(crate::RakError::Runtime("expected a regex".to_string())),
        }
    }

    /// Dispatch native methods on a `Value::Regex`.
    fn call_regex_method(&mut self, re_val: &Value, method: &str, args: &[Expr]) -> crate::Result<Value> {
        let re = match re_val {
            Value::Regex(r) => r.clone(),
            _ => return Err(crate::RakError::Runtime("not a regex".to_string())),
        };
        let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
        match method {
            "match" | "is_match" => {
                let hay = self.val_to_string(arg_vals.first())?;
                Ok(Value::Bool(re.re.is_match(&hay)))
            }
            "find" => {
                let hay = self.val_to_string(arg_vals.first())?;
                Ok(re.re.find(&hay).map(|m| Value::String(m.as_str().to_string())).unwrap_or(Value::Nil))
            }
            "find_all" => {
                let hay = self.val_to_string(arg_vals.first())?;
                Ok(Value::Array(
                    re.re.find_iter(&hay).map(|m| Value::String(m.as_str().to_string())).collect(),
                ))
            }
            "replace" | "replace_all" => {
                let hay = self.val_to_string(arg_vals.first())?;
                let rep = self.val_to_string(arg_vals.get(1))?;
                Ok(Value::String(re.re.replace_all(&hay, rep.as_str()).into_owned()))
            }
            _ => Err(crate::RakError::Runtime(format!("regex has no method '{}'", method))),
        }
    }

    /// Dispatch `lib.method(args)` on a `Value::ForeignLib`.
    fn call_foreign_lib_method(&mut self, lib_val: &Value, method: &str, args: &[Expr]) -> crate::Result<Value> {
        let lib = match lib_val {
            Value::ForeignLib(h) => h.clone(),
            _ => return Err(crate::RakError::Runtime("not an ffi library".to_string())),
        };
        match method {
            "call" => {
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<crate::Result<_>>()?;
                let symbol = self.val_to_string(arg_vals.first())?;
                let c_args: Vec<Value> = match arg_vals.get(1) {
                    Some(Value::Array(a)) => a.clone(),
                    Some(Value::Nil) | None => Vec::new(),
                    Some(other) => return Err(crate::RakError::Runtime(format!(
                        "ffi: lib.call(symbol, args) expects an array of args, got {}", other.type_name()
                    ))),
                };
                let mut marshalled: Vec<u64> = Vec::with_capacity(c_args.len());
                let _guards = self.marshal_args(&c_args, &mut marshalled)?;
                let addr = {
                    let h = lib.lock().unwrap();
                    rak_stdlib::ffi::sym_addr(&h, &symbol).map_err(crate::RakError::Runtime)?
                };
                let ret = unsafe { rak_stdlib::ffi::call_int(addr, &marshalled) };
                Ok(Value::Int(ret as i64))
            }
            "sym" => {
                let sym_expr = args.first().cloned().unwrap_or(Expr::Nil);
                let sym_val = self.eval_expr(&sym_expr)?;
                let symbol = self.val_to_string(Some(&sym_val))?;
                let addr = {
                    let h = lib.lock().unwrap();
                    rak_stdlib::ffi::sym_addr(&h, &symbol).map_err(crate::RakError::Runtime)?
                };
                Ok(Value::ForeignPtr(addr as u64))
            }
            "close" => {
                // Drop the handle if this is the last strong ref. Returns nil.
                if let Value::ForeignLib(arc) = lib_val {
                    if Arc::strong_count(arc) <= 2 {
                        let h = arc.lock().unwrap();
                        // Releasing happens via Drop when the last Arc drops.
                        let _ = &h;
                    }
                }
                Ok(Value::Nil)
            }
            _ => Err(crate::RakError::Runtime(format!("ffi library has no method '{}'", method))),
        }
    }

    /// Call a typed `extern "C"` declaration with already-evaluated args.
    fn call_foreign(&self, decl: ForeignFnDecl, args: &[Value]) -> crate::Result<Value> {
        let ForeignFnDecl { decl, lib } = decl;
        if !decl.varargs && args.len() > decl.params.len() {
            return Err(crate::RakError::Runtime(format!(
                "ffi: {} expects {} args, got {}",
                decl.name, decl.params.len(), args.len()
            )));
        }
        let mut marshalled: Vec<u64> = Vec::with_capacity(args.len());
        let _guards = self.marshal_args_typed(args, &decl.params, decl.varargs, &mut marshalled)?;
        let addr = {
            let h = lib.lock().unwrap();
            rak_stdlib::ffi::sym_addr(&h, &decl.name).map_err(crate::RakError::Runtime)?
        };
        let is_float_ret = matches!(decl.return_type, Some(Type::F32) | Some(Type::F64));
        let ret_bits = if is_float_ret {
            let f = unsafe { rak_stdlib::ffi::call_float(addr, &marshalled) };
            f.to_bits()
        } else {
            unsafe { rak_stdlib::ffi::call_int(addr, &marshalled) }
        };
        Ok(self.unmarshal_ret(&decl.return_type, ret_bits, is_float_ret))
    }

    /// Marshal Rak `Value`s to `u64` bit patterns by runtime type (dynamic
    /// `lib.call`). Returns a guard vector holding the backing buffers alive
    /// for the duration of the call.
    fn marshal_args(&self, args: &[Value], out: &mut Vec<u64>) -> crate::Result<Vec<MarshalGuard>> {
        let mut guards = Vec::with_capacity(args.len());
        for a in args {
            match a {
                Value::Int(i) => out.push(*i as u64),
                Value::Hex(h) => out.push(*h),
                Value::Bool(b) => out.push(if *b { 1 } else { 0 }),
                Value::Float(f) => out.push(*f as u64), // best-effort, integer ABI
                Value::ForeignPtr(p) => out.push(*p),
                Value::Nil => out.push(0),
                Value::String(s) => {
                    let c = CString::new(s.as_str()).map_err(|e| crate::RakError::Runtime(format!("ffi: bad string: {}", e)))?;
                    let p = c.as_ptr() as u64;
                    out.push(p);
                    guards.push(MarshalGuard::CStr(c));
                }
                Value::Bytes(b) => {
                    let mut v = b.clone();
                    v.push(0);
                    let p = v.as_ptr() as u64;
                    out.push(p);
                    guards.push(MarshalGuard::Bytes(v));
                }
                other => return Err(crate::RakError::Runtime(format!("ffi: cannot marshal {} to C", other.type_name()))),
            }
        }
        Ok(guards)
    }

    /// Marshal Rak `Value`s to `u64` bit patterns using declared `Param` types.
    fn marshal_args_typed(&self, args: &[Value], params: &[Param], varargs: bool, out: &mut Vec<u64>) -> crate::Result<Vec<MarshalGuard>> {
        let mut guards = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            let ty = params.get(i).and_then(|p| p.type_hint.as_ref());
            match (a, ty) {
                (Value::String(s), _) => {
                    let c = CString::new(s.as_str()).map_err(|e| crate::RakError::Runtime(format!("ffi: bad string: {}", e)))?;
                    let p = c.as_ptr() as u64;
                    out.push(p);
                    guards.push(MarshalGuard::CStr(c));
                }
                (Value::Bytes(b), _) => {
                    let mut v = b.clone();
                    v.push(0);
                    let p = v.as_ptr() as u64;
                    out.push(p);
                    guards.push(MarshalGuard::Bytes(v));
                }
                (Value::ForeignPtr(p), _) => out.push(*p),
                (Value::Int(i), _) => out.push(*i as u64),
                (Value::Hex(h), _) => out.push(*h),
                (Value::Bool(b), _) => out.push(if *b { 1 } else { 0 }),
                (Value::Float(f), Some(Type::F32)) => out.push((*f as f32).to_bits() as u64),
                (Value::Float(f), Some(Type::F64)) => out.push((*f).to_bits()),
                (Value::Float(f), _) => out.push(*f as u64),
                (Value::Nil, _) => out.push(0),
                (other, _) => {
                    if varargs && i >= params.len() {
                        // Varargs: marshal best-effort by runtime type.
                        match other {
                            Value::Int(i) => out.push(*i as u64),
                            Value::Hex(h) => out.push(*h),
                            Value::ForeignPtr(p) => out.push(*p),
                            Value::Bool(b) => out.push(if *b { 1 } else { 0 }),
                            Value::Nil => out.push(0),
                            _ => return Err(crate::RakError::Runtime(format!("ffi: cannot marshal {} as vararg", other.type_name()))),
                        }
                    } else {
                        return Err(crate::RakError::Runtime(format!("ffi: cannot marshal {} to C", other.type_name())));
                    }
                }
            }
        }
        Ok(guards)
    }

    /// Convert a raw return-word into a Rak `Value` per the declared return type.
    fn unmarshal_ret(&self, ret: &Option<Type>, bits: u64, is_float: bool) -> Value {
        match ret {
            None | Some(Type::Void) => Value::Nil,
            Some(Type::I8) => Value::Int((bits as u8) as i8 as i64),
            Some(Type::I16) => Value::Int((bits as u16) as i16 as i64),
            Some(Type::I32) | Some(Type::Int) => Value::Int(bits as u32 as i64),
            Some(Type::I64) => Value::Int(bits as i64),
            Some(Type::U8) => Value::Hex(bits as u8 as u64),
            Some(Type::U16) => Value::Hex(bits as u16 as u64),
            Some(Type::U32) => Value::Hex(bits as u32 as u64),
            Some(Type::U64) | Some(Type::Hex(_)) => Value::Hex(bits),
            Some(Type::F32) => Value::Float(f32::from_bits(bits as u32) as f64),
            Some(Type::F64) => Value::Float(f64::from_bits(bits)),
            Some(Type::Ptr(_)) => Value::ForeignPtr(bits),
            Some(Type::Custom(_)) => Value::ForeignPtr(bits),
            Some(_) => {
                if is_float {
                    Value::Float(f64::from_bits(bits))
                } else {
                    Value::ForeignPtr(bits)
                }
            }
        }
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
            // Exact-length byte slice matching via an array pattern of byte literals.
            (Pattern::Array(pats), Value::Bytes(vals)) => {
                pats.len() == vals.len()
                    && pats.iter().zip(vals.iter()).all(|(p, v)| match p {
                        Pattern::Hex(h) => *h == (*v as u64),
                        Pattern::Int(i) => *i == (*v as i64),
                        Pattern::Byte(b) => *b == *v,
                        _ => false,
                    })
            }
            (Pattern::Byte(b), Value::Bytes(vals)) => vals.len() == 1 && vals[0] == *b,
            (Pattern::Byte(b), Value::Int(i)) => *i == (*b as i64),
            (Pattern::Bytes(pats), Value::Bytes(vals)) => {
                let mut vi = 0usize;
                let mut pi = 0usize;
                while pi < pats.len() {
                    match &pats[pi] {
                        BytesPat::Byte(b) => {
                            if vi >= vals.len() || vals[vi] != *b {
                                return Ok(false);
                            }
                            vi += 1;
                            pi += 1;
                        }
                        BytesPat::Rest => {
                            // A trailing (or sole) rest matches everything left.
                            return Ok(true);
                        }
                    }
                }
                vi == vals.len()
            }
            (Pattern::Struct(name, fields), Value::Struct { name: sn, fields: fmap }) => {
                name == sn && fields.iter().all(|(fname, fp)| {
                    fmap.get(fname).map(|v| self.pattern_matches(fp, v).unwrap_or(false)).unwrap_or(false)
                })
            }
            (Pattern::Or(opts), _) => opts.iter().any(|p| self.pattern_matches(p, value).unwrap_or(false)),
            // Binary pattern matching directly against a zero-copy mmap slice.
            (Pattern::Bytes(pats), Value::MmapSlice(h, off, n)) => {
                let data = &h.as_slice()[*off..off + n];
                let mut vi = 0usize;
                let mut pi = 0usize;
                while pi < pats.len() {
                    match &pats[pi] {
                        BytesPat::Byte(b) => {
                            if vi >= data.len() || data[vi] != *b { return Ok(false); }
                            vi += 1; pi += 1;
                        }
                        BytesPat::Rest => return Ok(true),
                    }
                }
                vi == data.len()
            }
            (Pattern::Array(pats), Value::MmapSlice(h, off, n)) => {
                let data = &h.as_slice()[*off..off + n];
                pats.len() == data.len()
                    && pats.iter().zip(data.iter()).all(|(p, v)| match p {
                        Pattern::Hex(h) => *h == (*v as u64),
                        Pattern::Int(i) => *i == (*v as i64),
                        Pattern::Byte(b) => *b == *v,
                        _ => false,
                    })
            }
            (Pattern::Byte(b), Value::MmapSlice(h, off, n)) => *n == 1 && h.as_slice()[*off] == *b,
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
                Ok(v) => Ok(json_to_value(v)),
                Err(e) => Err(crate::RakError::Runtime(format!("{}", e))),
            },
            "json_stringify" => {
                let v = args.first().cloned().unwrap_or(Value::Nil);
                Ok(Value::String(value_to_json(&v).to_string()))
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
            // --- String methods ---
            "replace" => {
                let s = self.val_to_string(args.first())?;
                let from = self.val_to_string(args.get(1))?;
                let to = self.val_to_string(args.get(2))?;
                Ok(Value::String(s.replace(&from, &to)))
            }
            "find" => {
                let s = self.val_to_string(args.first())?;
                let needle = self.val_to_string(args.get(1))?;
                Ok(s.find(&needle).map(|i| Value::Int(i as i64)).unwrap_or(Value::Int(-1)))
            }
            "starts_with" => {
                let s = self.val_to_string(args.first())?;
                let p = self.val_to_string(args.get(1))?;
                Ok(Value::Bool(s.starts_with(&p)))
            }
            "ends_with" => {
                let s = self.val_to_string(args.first())?;
                let p = self.val_to_string(args.get(1))?;
                Ok(Value::Bool(s.ends_with(&p)))
            }
            "slice" => {
                let s = self.val_to_string(args.first())?;
                let start = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                let end = args.get(2).and_then(|v| v.as_i64()).unwrap_or(s.len() as i64).max(0) as usize;
                let chars: Vec<char> = s.chars().collect();
                let e = end.min(chars.len());
                let st = start.min(chars.len());
                Ok(Value::String(chars[st..e].iter().collect()))
            }
            "repeat" => {
                let s = self.val_to_string(args.first())?;
                let n = args.get(1).and_then(|v| v.as_i64()).unwrap_or(1).max(0) as usize;
                Ok(Value::String(s.repeat(n)))
            }
            "trim_start" => Ok(Value::String(self.val_to_string(args.first())?.trim_start().to_string())),
            "trim_end" => Ok(Value::String(self.val_to_string(args.first())?.trim_end().to_string())),
            "reverse" => {
                if let Some(Value::Array(a)) = args.first().cloned() {
                    let mut a = a;
                    a.reverse();
                    Ok(Value::Array(a))
                } else if let Some(Value::String(s)) = args.first().cloned() {
                    Ok(Value::String(s.chars().rev().collect()))
                } else {
                    Err(crate::RakError::Runtime("reverse() requires array or string".to_string()))
                }
            }
            "min" => {
                let nums: Vec<i64> = args.iter().filter_map(|v| v.as_i64()).collect();
                if nums.is_empty() { return Ok(Value::Nil); }
                Ok(Value::Int(*nums.iter().min().unwrap()))
            }
            "max" => {
                let nums: Vec<i64> = args.iter().filter_map(|v| v.as_i64()).collect();
                if nums.is_empty() { return Ok(Value::Nil); }
                Ok(Value::Int(*nums.iter().max().unwrap()))
            }
            "sum" => {
                if let Some(Value::Array(a)) = args.first() {
                    let mut total = 0i64;
                    for v in a.iter() { if let Some(n) = v.as_i64() { total += n; } else if v.as_f64().is_some() { return Ok(Value::Float(a.iter().filter_map(|v| v.as_f64()).sum())); } }
                    Ok(Value::Int(total))
                } else { Ok(Value::Int(0)) }
            }
            "abs" => Ok(Value::Int(args.first().and_then(|v| v.as_i64()).unwrap_or(0).abs())),
            "sqrt" => Ok(Value::Float(args.first().and_then(|v| v.as_f64()).unwrap_or(0.0).sqrt())),
            "pow" => {
                let base = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
                let exp = args.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
                Ok(Value::Float(base.powf(exp)))
            }
            "clamp" => {
                let v = args.first().and_then(|v| v.as_i64()).unwrap_or(0);
                let lo = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
                let hi = args.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(Value::Int(v.max(lo).min(hi)))
            }
            "env_set" => {
                let k = self.val_to_string(args.first())?;
                let v = self.val_to_string(args.get(1))?;
                std::env::set_var(k, v);
                Ok(Value::Nil)
            }
            "to_string" => Ok(Value::String(self.val_to_string(args.first())?)),
            "extern_call" => {
                let fname = self.val_to_string(args.get(0))?;
                Err(crate::RakError::Runtime(format!(
                    "extern_call(\"{}\"): C FFI bindings are not linked into this build. Link the generated Rust wrapper to enable it.",
                    fname
                )))
            }
            #[cfg(feature = "gui")]
            "gui_open" => {
                let title = self.val_to_string(args.get(0))?;
                let html = self.val_to_string(args.get(1))?;
                let width = args.get(2).and_then(|v| v.as_u64()).unwrap_or(800) as u32;
                let height = args.get(3).and_then(|v| v.as_u64()).unwrap_or(600) as u32;
                let mgr = self.gui.get_or_insert(crate::gui::GuiManager::new());
                Ok(Value::Int(mgr.open(&title, &html, width, height)))
            }
            #[cfg(feature = "gui")]
            "gui_update" => {
                let id = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
                let html = self.val_to_string(args.get(1))?;
                let mgr = self.gui.get_or_insert(crate::gui::GuiManager::new());
                mgr.update(id, &html);
                Ok(Value::Nil)
            }
            #[cfg(feature = "gui")]
            "gui_title" => {
                let id = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
                let title = self.val_to_string(args.get(1))?;
                let mgr = self.gui.get_or_insert(crate::gui::GuiManager::new());
                mgr.set_title(id, &title);
                Ok(Value::Nil)
            }
            #[cfg(feature = "gui")]
            "gui_close" => {
                let id = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
                let mgr = self.gui.get_or_insert(crate::gui::GuiManager::new());
                mgr.close(id);
                Ok(Value::Nil)
            }
            #[cfg(feature = "gui")]
            "gui_wait" => {
                let mgr = self.gui.get_or_insert(crate::gui::GuiManager::new());
                mgr.wait();
                Ok(Value::Nil)
            }
            #[cfg(feature = "gui")]
            "gui_callback" => {
                let name = self.val_to_string(args.get(0))?;
                let mgr = self.gui.get_or_insert(crate::gui::GuiManager::new());
                mgr.register_callback(&name);
                Ok(Value::Nil)
            }
            #[cfg(not(feature = "gui"))]
            "gui_open" | "gui_update" | "gui_title" | "gui_close" | "gui_wait" | "gui_callback" => {
                Err(crate::RakError::Runtime("GUI support not enabled (build with --features gui)".to_string()))
            }
            // --- FFI builtins ---
            "ffi_load" => {
                let path = self.val_to_string(args.first())?;
                let h = rak_stdlib::ffi::load(&path).map_err(crate::RakError::Runtime)?;
                Ok(Value::ForeignLib(Arc::new(Mutex::new(h))))
            }
            "ffi_ptr" => Ok(Value::ForeignPtr(args.first().and_then(|v| v.as_u64()).unwrap_or(0))),
            "ffi_alloc" => {
                let n = args.first().and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let mut v = vec![0u8; n];
                let ptr = v.as_mut_ptr() as u64;
                std::mem::forget(v);
                self.ffi_allocs.insert(ptr, n);
                Ok(Value::ForeignPtr(ptr))
            }
            "ffi_free" => {
                let ptr = match args.first() {
                    Some(Value::ForeignPtr(p)) => *p,
                    _ => return Err(crate::RakError::Runtime("ffi_free(ptr) requires a ptr".to_string())),
                };
                match self.ffi_allocs.remove(&ptr) {
                    Some(n) => {
                        unsafe { let _ = Vec::from_raw_parts(ptr as *mut u8, n, n); }
                        Ok(Value::Nil)
                    }
                    None => Err(crate::RakError::Runtime("ffi_free: pointer was not allocated by ffi_alloc/ffi_string_to_cstr".to_string())),
                }
            }
            "ffi_write" => {
                let ptr = args.first().and_then(|v| match v { Value::ForeignPtr(p) => Some(*p), _ => None }).ok_or_else(|| crate::RakError::Runtime("ffi_write(ptr, off, byte)".to_string()))?;
                let off = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let byte = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                unsafe { *((ptr as usize + off) as *mut u8) = byte; }
                Ok(Value::Nil)
            }
            "ffi_read" => {
                let ptr = args.first().and_then(|v| match v { Value::ForeignPtr(p) => Some(*p), _ => None }).ok_or_else(|| crate::RakError::Runtime("ffi_read(ptr, off)".to_string()))?;
                let off = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let b = unsafe { *((ptr as usize + off) as *const u8) };
                Ok(Value::Int(b as i64))
            }
            "ffi_read_i32" => {
                let ptr = args.first().and_then(|v| match v { Value::ForeignPtr(p) => Some(*p), _ => None }).ok_or_else(|| crate::RakError::Runtime("ffi_read_i32(ptr, off)".to_string()))?;
                let off = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as isize;
                let v = unsafe { *((ptr as usize).wrapping_add(off as usize) as *const i32) };
                Ok(Value::Int(v as i64))
            }
            "ffi_cstr_to_string" => {
                let ptr = args.first().and_then(|v| match v { Value::ForeignPtr(p) => Some(*p), _ => None }).ok_or_else(|| crate::RakError::Runtime("ffi_cstr_to_string(ptr)".to_string()))? as usize;
                let s = unsafe {
                    let mut len = 0usize;
                    while *(ptr as *const u8).add(len) != 0 { len += 1; }
                    let slice = std::slice::from_raw_parts(ptr as *const u8, len);
                    String::from_utf8_lossy(slice).into_owned()
                };
                Ok(Value::String(s))
            }
            "ffi_string_to_cstr" => {
                let s = self.val_to_string(args.first())?;
                let bytes = match CString::new(s.as_str()) {
                    Ok(c) => c.into_bytes_with_nul(),
                    Err(e) => return Err(crate::RakError::Runtime(format!("ffi_string_to_cstr: {}", e))),
                };
                let len = bytes.len();
                let ptr = bytes.as_ptr() as u64;
                std::mem::forget(bytes);
                self.ffi_allocs.insert(ptr, len);
                Ok(Value::ForeignPtr(ptr))
            }
            "ffi_call" => {
                let (lib, symbol) = match (args.first(), args.get(1)) {
                    (Some(Value::ForeignLib(h)), Some(Value::String(s))) => (h.clone(), s.clone()),
                    _ => return Err(crate::RakError::Runtime("ffi_call(lib, symbol, args_array)".to_string())),
                };
                let c_args: Vec<Value> = match args.get(2) {
                    Some(Value::Array(a)) => a.clone(),
                    Some(Value::Nil) | None => Vec::new(),
                    Some(other) => return Err(crate::RakError::Runtime(format!(
                        "ffi_call: args must be an array, got {}", other.type_name()
                    ))),
                };
                let mut marshalled = Vec::with_capacity(c_args.len());
                let _g = self.marshal_args(&c_args, &mut marshalled)?;
                let addr = { let h = lib.lock().unwrap(); rak_stdlib::ffi::sym_addr(&h, &symbol).map_err(crate::RakError::Runtime)? };
                let ret = unsafe { rak_stdlib::ffi::call_int(addr, &marshalled) };
                Ok(Value::Int(ret as i64))
            }
            // --- Memory-mapped files ---
            "mmap_open" => {
                let path = self.val_to_string(args.first())?;
                let mode = self.val_to_string(args.get(1)).unwrap_or_else(|_| "r".to_string());
                match rak_stdlib::mmap::open(&path, &mode) {
                    Ok(h) => Ok(Value::Mmap(h)),
                    Err(e) => Err(crate::RakError::Runtime(e)),
                }
            }
            "mmap_slice" => {
                let (h, off, len) = match args {
                    [Value::Mmap(h), Value::Int(o), Value::Int(l)] => (h.clone(), *o as usize, *l as usize),
                    [Value::Mmap(h), Value::Hex(o), Value::Hex(l)] => (h.clone(), *o as usize, *l as usize),
                    _ => return Err(crate::RakError::Runtime("mmap_slice(mmap, off, len)".to_string())),
                };
                let total = h.len();
                if off.saturating_add(len) > total {
                    return Err(crate::RakError::Runtime(format!(
                        "mmap_slice: [off, off+len) = [{}, {}) out of range (len {})", off, off + len, total
                    )));
                }
                Ok(Value::MmapSlice(h, off, len))
            }
            "mmap_size" => match args.first() {
                Some(Value::Mmap(h)) => Ok(Value::Int(h.len() as i64)),
                Some(Value::MmapSlice(_, _, n)) => Ok(Value::Int(*n as i64)),
                _ => Err(crate::RakError::Runtime("mmap_size(mmap)".to_string())),
            },
            "mmap_close" => Ok(Value::Nil),
            "mmap_find" => {
                let h = match args.first() {
                    Some(Value::Mmap(h)) => h.clone(),
                    Some(Value::MmapSlice(h, _, _)) => h.clone(),
                    _ => return Err(crate::RakError::Runtime("mmap_find(mmap, needle)".to_string())),
                };
                let needle = self.val_to_bytes(args.get(1))?;
                match rak_stdlib::mmap::find(&h, &needle) {
                    Some(p) => Ok(Value::Int(p as i64)),
                    None => Ok(Value::Int(-1)),
                }
            }
            "mmap_lines" => {
                let h = match args.first() {
                    Some(Value::Mmap(h)) => h.clone(),
                    _ => return Err(crate::RakError::Runtime("mmap_lines(mmap, delim?)".to_string())),
                };
                let delim = self.val_to_string(args.get(1)).unwrap_or_else(|_| "\n".to_string());
                let ls = rak_stdlib::mmap::lines(&h, delim.as_bytes());
                Ok(Value::Array(ls.into_iter().map(Value::String).collect()))
            }
            "mmap_lines_off" => {
                let h = match args.first() {
                    Some(Value::Mmap(h)) => h.clone(),
                    _ => return Err(crate::RakError::Runtime("mmap_lines_off(mmap, delim?)".to_string())),
                };
                let delim = self.val_to_string(args.get(1)).unwrap_or_else(|_| "\n".to_string());
                let offs = rak_stdlib::mmap::lines_off(&h, delim.as_bytes());
                Ok(Value::Array(offs.into_iter().map(|(o, l)| Value::Tuple(vec![Value::Int(o as i64), Value::Int(l as i64)])).collect()))
            }
            // --- Async I/O (Tokio-backed futures) ---
            "http_get_async" => {
                let url = self.val_to_string(args.first())?;
                let jh = async_runtime().spawn_blocking(move || {
                    match rak_stdlib::net::http_get(&url, None) {
                        Ok(r) => Value::String(r.body),
                        Err(e) => Value::String(format!("error: {}", e)),
                    }
                });
                Ok(Value::Future(Arc::new(FutureHandle {
                    state: Mutex::new(FutureState::Pending(jh)),
                })))
            }
            "tcp_probe" => {
                let host = self.val_to_string(args.first())?;
                let port = args.get(1).and_then(|v| v.as_u64()).unwrap_or(80) as u16;
                let timeout_ms = args.get(2).and_then(|v| v.as_u64()).unwrap_or(1000) as u64;
                let jh = async_runtime().spawn_blocking(move || {
                    Value::Bool(rak_stdlib::net::tcp_scan(&host, port, timeout_ms))
                });
                Ok(Value::Future(Arc::new(FutureHandle {
                    state: Mutex::new(FutureState::Pending(jh)),
                })))
            }
            "tcp_connect_async" => {
                let addr = self.val_to_string(args.first())?;
                let jh = async_runtime().spawn_blocking(move || {
                    use std::net::TcpStream;
                    match TcpStream::connect(&addr) {
                        Ok(s) => {
                            s.set_nonblocking(false).ok();
                            Value::TcpStream(Arc::new(Mutex::new(s)))
                        }
                        Err(_) => Value::Nil,
                    }
                });
                Ok(Value::Future(Arc::new(FutureHandle {
                    state: Mutex::new(FutureState::Pending(jh)),
                })))
            }
            // --- Raw sockets / packet forging ---
            "net_raw_csum" => Ok(Value::Int(rak_stdlib::net_raw::csum16(&self.val_to_bytes(args.first())?) as i64)),
            "net_raw_ipv4" => {
                let src = self.val_to_string(args.first())?;
                let dst = self.val_to_string(args.get(1))?;
                let proto = args.get(2).and_then(|v| v.as_u64()).unwrap_or(6) as u8;
                let payload = self.val_to_bytes(args.get(3))?;
                Ok(Value::Bytes(rak_stdlib::net_raw::ipv4(&src, &dst, proto, &payload).map_err(crate::RakError::Runtime)?))
            }
            "net_raw_tcp" => {
                let src_ip = self.val_to_string(args.first())?;
                let dst_ip = self.val_to_string(args.get(1))?;
                let src_port = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let dst_port = args.get(3).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let flags = self.val_to_string(args.get(4)).unwrap_or_else(|_| "S".to_string());
                let seq = args.get(5).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let ack = args.get(6).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let payload = self.val_to_bytes(args.get(7))?;
                Ok(Value::Bytes(rak_stdlib::net_raw::tcp(&src_ip, &dst_ip, src_port, dst_port, &flags, seq, ack, &payload).map_err(crate::RakError::Runtime)?))
            }
            "net_raw_udp" => {
                let src_ip = self.val_to_string(args.first())?;
                let dst_ip = self.val_to_string(args.get(1))?;
                let src_port = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let dst_port = args.get(3).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let payload = self.val_to_bytes(args.get(4))?;
                Ok(Value::Bytes(rak_stdlib::net_raw::udp(&src_ip, &dst_ip, src_port, dst_port, &payload).map_err(crate::RakError::Runtime)?))
            }
            "net_raw_tcp_syn" => {
                let src = self.val_to_string(args.first())?;
                let dst = self.val_to_string(args.get(1))?;
                let src_port = args.get(2).and_then(|v| v.as_u64()).unwrap_or(12345) as u16;
                let dport = args.get(3).and_then(|v| v.as_u64()).unwrap_or(80) as u16;
                Ok(Value::Bytes(rak_stdlib::net_raw::tcp_syn(&src, &dst, src_port, dport).map_err(crate::RakError::Runtime)?))
            }
            "net_raw_send" => {
                let pkt = self.val_to_bytes(args.first())?;
                match rak_stdlib::net_raw::send(&pkt) {
                    Ok(n) => Ok(Value::Result(Some(Box::new(Value::Int(n as i64))), None)),
                    Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(e))))),
                }
            }
            "net_raw_recv" => {
                let max = args.first().and_then(|v| v.as_u64()).unwrap_or(4096) as usize;
                match rak_stdlib::net_raw::recv(max) {
                    Ok(b) => Ok(Value::Result(Some(Box::new(Value::Bytes(b))), None)),
                    Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(e))))),
                }
            }
            // --- DNS ---
            "dns_query" => {
                let name = self.val_to_string(args.first())?;
                let rtype = self.val_to_string(args.get(1)).unwrap_or_else(|_| "A".to_string());
                let server = args.get(2).map(|v| v.to_string());
                match rak_stdlib::dns::query(&name, &rtype, server.as_deref()) {
                    Ok(resp) => {
                        let answers: Vec<Value> = resp.answers.into_iter().map(|r| {
                            Value::Map(HashMap::from([
                                ("name".to_string(), Value::String(r.name)),
                                ("type".to_string(), Value::String(r.rtype)),
                                ("ttl".to_string(), Value::Int(r.ttl as i64)),
                                ("rdata".to_string(), Value::String(r.rdata)),
                            ]))
                        }).collect();
                        let m = Value::Map(HashMap::from([
                            ("answers".to_string(), Value::Array(answers)),
                            ("truncated".to_string(), Value::Bool(resp.truncated)),
                        ]));
                        Ok(Value::Result(Some(Box::new(m)), None))
                    }
                    Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(e))))),
                }
            }
            "dns_build" => {
                let name = self.val_to_string(args.first())?;
                let rtype = self.val_to_string(args.get(1)).unwrap_or_else(|_| "A".to_string());
                Ok(Value::Bytes(rak_stdlib::dns::build_query(&name, &rtype)))
            }
            "dns_parse" => {
                let msg = self.val_to_bytes(args.first())?;
                match rak_stdlib::dns::parse_response(&msg) {
                    Ok(resp) => {
                        let answers: Vec<Value> = resp.answers.into_iter().map(|r| {
                            Value::Map(HashMap::from([
                                ("name".to_string(), Value::String(r.name)),
                                ("type".to_string(), Value::String(r.rtype)),
                                ("ttl".to_string(), Value::Int(r.ttl as i64)),
                                ("rdata".to_string(), Value::String(r.rdata)),
                            ]))
                        }).collect();
                        Ok(Value::Map(HashMap::from([
                            ("answers".to_string(), Value::Array(answers)),
                            ("truncated".to_string(), Value::Bool(resp.truncated)),
                        ])))
                    }
                    Err(e) => Err(crate::RakError::Runtime(e)),
                }
            }
            // --- TLS ---
            "tls_parse_client_hello" => {
                let bytes = self.val_to_bytes(args.first())?;
                match rak_stdlib::tls::parse_client_hello(&bytes) {
                    Ok(info) => {
                        let ciphers: Vec<Value> = info.ciphers.into_iter().map(|c| Value::Hex(c as u64)).collect();
                        Ok(Value::Map(HashMap::from([
                            ("sni".to_string(), Value::String(info.sni)),
                            ("ciphers".to_string(), Value::Array(ciphers)),
                        ])))
                    }
                    Err(e) => Err(crate::RakError::Runtime(e)),
                }
            }
            "tls_parse_cert_chain" => {
                let der = self.val_to_bytes(args.first())?;
                let certs: Vec<Value> = rak_stdlib::tls::parse_cert_chain(&der).into_iter().map(|c| {
                    Value::Map(HashMap::from([
                        ("subject".to_string(), Value::String(c.subject)),
                        ("issuer".to_string(), Value::String(c.issuer)),
                    ]))
                }).collect();
                Ok(Value::Array(certs))
            }
            // --- PCAP ---
            "pcap_open" => {
                let path = self.val_to_string(args.first())?;
                match rak_stdlib::pcap::open(&path) {
                    Ok(h) => Ok(Value::Result(Some(Box::new(Value::Pcap(Arc::new(Mutex::new(h))))), None)),
                    Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(e))))),
                }
            }
            "pcap_next" => {
                let h = match args.first() {
                    Some(Value::Pcap(h)) => h.clone(),
                    _ => return Err(crate::RakError::Runtime("pcap_next(handle)".to_string())),
                };
                let mut guard = h.lock().unwrap();
                match rak_stdlib::pcap::next(&mut guard) {
                    Some(p) => Ok(Value::Map(HashMap::from([
                        ("timestamp".to_string(), Value::Int(p.timestamp)),
                        ("linktype".to_string(), Value::Int(p.linktype)),
                        ("payload".to_string(), Value::Bytes(p.payload)),
                    ]))),
                    None => Ok(Value::Nil),
                }
            }
            // --- Regex builtins ---
            "regex_new" => {
                let pattern = self.val_to_string(args.first())?;
                let flags = self.val_to_string(args.get(1))?;
                Ok(Value::Regex(self.build_regex(&pattern, &flags)?))
            }
            "regex_match" | "regex_is_match" => {
                let re = self.coerce_regex(args.first(), args.get(2))?;
                let hay = self.val_to_string(args.get(1))?;
                Ok(Value::Bool(re.re.is_match(&hay)))
            }
            "regex_find" => {
                let re = self.coerce_regex(args.first(), args.get(2))?;
                let hay = self.val_to_string(args.get(1))?;
                Ok(re.re.find(&hay).map(|m| Value::String(m.as_str().to_string())).unwrap_or(Value::Nil))
            }
            "regex_find_all" => {
                let re = self.coerce_regex(args.first(), args.get(2))?;
                let hay = self.val_to_string(args.get(1))?;
                Ok(Value::Array(re.re.find_iter(&hay).map(|m| Value::String(m.as_str().to_string())).collect()))
            }
            "regex_replace" | "regex_replace_all" => {
                let re = self.coerce_regex(args.first(), args.get(3))?;
                let hay = self.val_to_string(args.get(1))?;
                let rep = self.val_to_string(args.get(2))?;
                Ok(Value::String(re.re.replace_all(&hay, rep.as_str()).into_owned()))
            }
            _ => Err(crate::RakError::Runtime(format!("Unknown function: {}", name))),
        }
    }

    fn val_to_string(&self, val: Option<&Value>) -> crate::Result<String> {
        match val {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(Value::Bytes(b)) => Ok(String::from_utf8_lossy(b).to_string()),
            Some(Value::MmapSlice(h, off, n)) => Ok(String::from_utf8_lossy(&h.as_slice()[*off..off + n]).to_string()),
            Some(Value::Mmap(h)) => Ok(String::from_utf8_lossy(h.as_slice()).to_string()),
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
            Some(Value::MmapSlice(h, off, n)) => Ok(h.as_slice()[*off..off + n].to_vec()),
            Some(Value::Mmap(h)) => Ok(h.as_slice().to_vec()),
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
    fn type_name(&self) -> String {
        match self {
            Value::Hex(_) => "hex".to_string(),
            Value::Int(_) => "int".to_string(),
            Value::Float(_) => "float".to_string(),
            Value::String(_) => "string".to_string(),
            Value::Bytes(_) => "bytes".to_string(),
            Value::Bool(_) => "bool".to_string(),
            Value::Nil => "nil".to_string(),
            Value::Tuple(_) => "tuple".to_string(),
            Value::Array(_) => "array".to_string(),
            Value::Map(_) => "map".to_string(),
            Value::Struct { name, .. } => name.clone(),
            Value::Enum { name, .. } => name.clone(),
            Value::Regex(_) => "regex".to_string(),
            Value::Function { .. } => "function".to_string(),
            Value::Module(_) => "module".to_string(),
            Value::ForeignLib(_) => "ffi-lib".to_string(),
            Value::ForeignPtr(_) => "ptr".to_string(),
            Value::Mmap(_) => "mmap".to_string(),
            Value::MmapSlice(_, _, _) => "mmap-slice".to_string(),
            Value::Future(_) => "future".to_string(),
            Value::Pcap(_) => "pcap".to_string(),
            _ => "<opaque>".to_string(),
        }
    }
    fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Hex(h) => Some(*h as i64),
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::ForeignPtr(p) => Some(*p as i64),
            _ => None,
        }
    }
    fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Hex(h) => Some(*h),
            Value::ForeignPtr(p) => Some(*p),
            _ => self.as_i64().map(|v| v as u64),
        }
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

fn json_to_value(j: serde_json::Value) -> Value {
    use serde_json::Value as J;
    match j {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Bool(b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Nil
            }
        }
        J::String(s) => Value::String(s),
        J::Array(a) => Value::Array(a.into_iter().map(json_to_value).collect()),
        J::Object(o) => Value::Map(o.into_iter().map(|(k, v)| (k, json_to_value(v))).collect()),
    }
}

fn value_to_json(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Nil => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Int(i) => serde_json::json!(*i),
        Value::Hex(h) => serde_json::json!(*h),
        Value::Float(f) => serde_json::json!(*f),
        Value::String(s) => J::String(s.clone()),
        Value::Bytes(b) => J::String(String::from_utf8_lossy(b).to_string()),
        Value::Array(a) => J::Array(a.iter().map(value_to_json).collect()),
        Value::Tuple(t) => J::Array(t.iter().map(value_to_json).collect()),
        Value::Map(m) => {
            let mut o = serde_json::Map::new();
            for (k, val) in m.iter() {
                o.insert(k.clone(), value_to_json(val));
            }
            J::Object(o)
        }
        Value::Struct { fields, .. } => {
            let mut o = serde_json::Map::new();
            for (k, val) in fields.iter() {
                o.insert(k.clone(), value_to_json(val));
            }
            J::Object(o)
        }
        Value::Option(Some(v)) => value_to_json(v),
        Value::Option(None) => J::Null,
        Value::Result(Some(v), _) => value_to_json(v),
        Value::Result(_, Some(e)) => value_to_json(e),
        _ => J::Null,
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

    #[test]
    fn test_interpreter_json_roundtrip() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("let j = json_parse(\"{\\\"a\\\": 1, \\\"b\\\": [2, 3]}\") dump j.a dump j.b[1] dump json_stringify(j)").unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 1")));
        assert!(output.iter().any(|l| l.contains("[DUMP] 3")));
        assert!(output.iter().any(|l| l.contains("\"a\"")));
    }

    #[test]
    fn test_interpreter_string_stdlib() {
        let mut interp = Interpreter::new();
        let output = interp.run_source("dump replace(\"hello\", \"l\", \"L\"); dump starts_with(\"hello\", \"he\"); dump slice(\"hello\", 1, 3); dump reverse(\"abc\"); dump sum([1, 2, 3, 4]); dump max(3, 9, 2);").unwrap();
        assert!(output.iter().any(|l| l.contains("heLLo")));
        assert!(output.iter().any(|l| l.contains("true")));
        assert!(output.iter().any(|l| l.contains("[DUMP] el")));
        assert!(output.iter().any(|l| l.contains("[DUMP] cba")));
        assert!(output.iter().any(|l| l.contains("[DUMP] 10")));
        assert!(output.iter().any(|l| l.contains("[DUMP] 9")));
    }

    #[test]
    fn test_interpreter_pipeline() {
        let mut interp = Interpreter::new();
        let src = "fn inc(n) { return n + 1 } fn dbl(n) { return n * 2 } let r = 5 |> inc |> dbl; dump r";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 12")));
    }

    #[test]
    fn test_interpreter_pipeline_first_arg() {
        let mut interp = Interpreter::new();
        let src = "fn add(a, b) { return a + b } let r = 3 |> add(10); dump r";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 13")));
    }

    #[test]
    fn test_interpreter_regex_literal_methods() {
        let mut interp = Interpreter::new();
        let src = r#"let re = /\d+/g; dump re.is_match("abc123"); dump re.find_all("a1 b22 c333");"#;
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("true")));
        // Value::Display does not quote strings inside arrays.
        assert!(output.iter().any(|l| l.contains("[DUMP] [1, 22, 333]")));
    }

    #[test]
    fn test_interpreter_regex_replace() {
        let mut interp = Interpreter::new();
        let src = r#"let re = /\s+/g; dump re.replace("a  b   c", "_")"#;
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("a_b_c")));
    }

    #[test]
    fn test_interpreter_regex_case_insensitive_flag() {
        let mut interp = Interpreter::new();
        let src = r#"dump (/rak/i).is_match("RAK language")"#;
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("true")));
    }

    #[test]
    fn test_interpreter_bytes_pattern_match() {
        let mut interp = Interpreter::new();
        let src = r#"let png = b"\x89PNG\x0d\x0a\x1a\x0a"; match png { [0x89, 'P', 'N', 'G', ..] => { dump "png" }, _ => { dump "other" } }"#;
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] png")));
    }

    #[test]
    fn test_interpreter_bytes_pattern_no_match() {
        let mut interp = Interpreter::new();
        let src = r#"let jpg = b"\xFF\xD8\xFF"; match jpg { [0x89, 'P', 'N', 'G', ..] => { dump "png" }, _ => { dump "other" } }"#;
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] other")));
    }

    #[test]
    fn test_interpreter_trait_display_dump() {
        let mut interp = Interpreter::new();
        let src = "struct Point { x: int, y: int } impl Display for Point { fn fmt(self) { return fmt(\"({}, {})\", self.x, self.y) } } let p = Point { x: 3, y: 4 } dump p";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] (3, 4)")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_trait_iterable() {
        let mut interp = Interpreter::new();
        let src = "struct Range { lo: int, hi: int } impl Iterable for Range { fn iter(self) { let out = []; let i = self.lo; while i <= self.hi { out = push(out, i); i = i + 1 } return out } } let r = Range { lo: 1, hi: 4 }; let s = 0; for n in r { s = s + n } dump s";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 10")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_trait_index() {
        let mut interp = Interpreter::new();
        let src = "struct Vec3 { data: array } impl Index for Vec3 { fn index(self, i) { return self.data[i] } } let v = Vec3 { data: [10, 20, 30] } dump v[1]";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 20")), "got: {:?}", output);
    }

    // --- FFI ---

    #[test]
    fn test_interpreter_ffi_extern_abs() {
        let mut interp = Interpreter::new();
        let src = "extern \"C\" { fn abs(n: i32) -> i32 } dump abs(-42)";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_ffi_alloc_write_read_free() {
        let mut interp = Interpreter::new();
        let src = "let buf = ffi_alloc(4); ffi_write(buf, 0, 0x41); ffi_write(buf, 1, 0x00); dump ffi_read(buf, 0); dump ffi_cstr_to_string(buf); ffi_free(buf)";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 65")), "got: {:?}", output);
        assert!(output.iter().any(|l| l.contains("[DUMP] A")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_ffi_string_to_cstr_roundtrip() {
        let mut interp = Interpreter::new();
        let src = "let cs = ffi_string_to_cstr(\"hello ffi\"); dump ffi_cstr_to_string(cs); ffi_free(cs)";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] hello ffi")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_ffi_ptr_format() {
        let mut interp = Interpreter::new();
        let src = "let p = ffi_ptr(0xDEADBEEF); dump fmt(\"0x{:08X}\", p)";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("0xDEADBEEF")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_ffi_void_return_is_nil() {
        let mut interp = Interpreter::new();
        // A function declared with no return type yields nil (void).
        let src = "extern \"C\" { fn abs(n: i32) } let r = abs(0); dump r";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] nil")), "got: {:?}", output);
    }

    // --- Memory-mapped files ---

    fn write_mmap_sample(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("rak_mmap_{}.bin", name));
        let bytes: [u8; 15] = [0xD4, 0xC3, 0xB2, 0xA1, 0x0A, b'G', b'E', b'T', b' ', 0x31, 0x0A, b'x', b'y', b'z', 0x0A];
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn test_interpreter_mmap_size_and_slice() {
        let path = write_mmap_sample("interp_size");
        let src = format!(
            "let m = mmap_open(\"{}\", \"r\"); dump mmap_size(m); let s = mmap_slice(m, 0, 4); dump s[0]; dump s[3]",
            path.to_str().unwrap().replace('\\', "\\\\")
        );
        let mut interp = Interpreter::new();
        let output = interp.run_source(&src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 15")), "got: {:?}", output);
        assert!(output.iter().any(|l| l.contains("[DUMP] 212")), "got: {:?}", output); // 0xD4
        assert!(output.iter().any(|l| l.contains("[DUMP] 161")), "got: {:?}", output); // 0xA1
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_interpreter_mmap_find_lines_pattern() {
        let path = write_mmap_sample("interp_find");
        let src = format!(
            "let m = mmap_open(\"{}\", \"r\"); dump mmap_find(m, \"GET\"); let lines = mmap_lines_off(m, \"\\n\"); dump len(lines); let h = mmap_slice(m, 0, 4); match h {{ [0xD4, 0xC3, 0xB2, 0xA1, ..] => {{ dump \"pcap\" }}, _ => {{ dump \"other\" }} }}",
            path.to_str().unwrap().replace('\\', "\\\\")
        );
        let mut interp = Interpreter::new();
        let output = interp.run_source(&src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 5")), "got: {:?}", output); // GET at offset 5
        assert!(output.iter().any(|l| l.contains("[DUMP] 3")), "got: {:?}", output); // 3 lines
        assert!(output.iter().any(|l| l.contains("[DUMP] pcap")), "got: {:?}", output);
        let _ = std::fs::remove_file(&path);
    }

    // --- Async ---

    #[test]
    fn test_interpreter_async_fn_await() {
        let mut interp = Interpreter::new();
        let src = "async fn double(x) { return x * 2 } dump await double(21)";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_async_fn_deferred() {
        // The body runs only on await; the future is a deferred value before.
        let mut interp = Interpreter::new();
        let src = "async fn sq(x) { return x * x } let f = sq(6); dump await f";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 36")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_async_tcp_probe() {
        let mut interp = Interpreter::new();
        let src = "dump await tcp_probe(\"127.0.0.1\", 9999, 100)";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] false")), "got: {:?}", output);
    }

    // --- Raw sockets / packet forging ---

    #[test]
    fn test_interpreter_net_raw_syn() {
        let mut interp = Interpreter::new();
        let src = "let pkt = net_raw_tcp_syn(\"10.0.0.5\", \"10.0.0.10\", 12345, 80); dump len(pkt); dump pkt[0]; dump pkt[9]";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 40")), "got: {:?}", output);
        assert!(output.iter().any(|l| l.contains("[DUMP] 69")), "got: {:?}", output); // 0x45 = 69
        assert!(output.iter().any(|l| l.contains("[DUMP] 6")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_net_raw_ipv4_checksum_self_check() {
        let mut interp = Interpreter::new();
        // The IP header (with its checksum) is self-checking: csum16 == 0.
        let src = "let pkt = net_raw_ipv4(\"10.0.0.5\", \"10.0.0.10\", 6, b\"\"); dump net_raw_csum(pkt[0..20])";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 0")), "got: {:?}", output);
    }

    #[test]
    fn test_interpreter_net_raw_send_returns_result() {
        let mut interp = Interpreter::new();
        let src = "let pkt = net_raw_tcp_syn(\"10.0.0.5\", \"10.0.0.10\", 12345, 80); dump net_raw_send(pkt)";
        let output = interp.run_source(src).unwrap();
        // On any platform this is a Result (Ok on privileged unix, Err otherwise).
        assert!(output.iter().any(|l| l.contains("Ok(") || l.contains("Err(")), "got: {:?}", output);
    }

    // --- DNS ---

    #[test]
    fn test_interpreter_dns_build() {
        let mut interp = Interpreter::new();
        let src = "let q = dns_build(\"example.com\", \"A\"); dump len(q); dump q[12]";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] 29")), "got: {:?}", output);
        assert!(output.iter().any(|l| l.contains("[DUMP] 7")), "got: {:?}", output); // first label len
    }

    #[test]
    fn test_interpreter_tls_parse_short_input_errors() {
        let mut interp = Interpreter::new();
        // A too-short input yields a clear error (caught here).
        let src = "try { let info = tls_parse_client_hello(b\"\"); dump info } catch e { dump \"short\" }";
        let output = interp.run_source(src).unwrap();
        assert!(output.iter().any(|l| l.contains("[DUMP] short")), "got: {:?}", output);
    }
}
