use crate::bytecode::{Chunk, Op};
use crate::value::Value;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::{Arc, Mutex};

/// VM-side tracking for native allocations made by `ffi_alloc` /
/// `ffi_string_to_cstr`, so `ffi_free` can release them with the correct
/// `Vec::from_raw_parts` layout.
static FFI_ALLOC: Mutex<Option<HashMap<u64, usize>>> = Mutex::new(None);

/// Holds a buffer alive for the duration of an FFI call so a `u64` argument
/// pointing into it stays valid.
enum MarshalGuardVm {
    #[allow(dead_code)]
    CStr(CString),
    #[allow(dead_code)]
    Bytes(Vec<u8>),
}

/// Marshal Rak `Value`s to `u64` bit patterns by runtime type (dynamic
/// `lib.call` / `ffi_call`). Returns the args plus guards that keep the
/// backing buffers alive for the duration of the call.
fn marshal_vm_args(args: &[Value]) -> Result<(Vec<u64>, Vec<MarshalGuardVm>), String> {
    let mut out = Vec::with_capacity(args.len());
    let mut guards = Vec::with_capacity(args.len());
    for a in args {
        match a {
            Value::I64(i) => out.push(*i as u64),
            Value::I32(i) => out.push(*i as u64),
            Value::U64(v) => out.push(*v),
            Value::U32(v) => out.push(*v as u64),
            Value::Hex(h, _) => out.push(*h),
            Value::Bool(b) => out.push(if *b { 1 } else { 0 }),
            Value::ForeignPtr(p) => out.push(*p),
            Value::Nil => out.push(0),
            Value::String(s) => {
                let c = CString::new(s.as_ref()).map_err(|e| format!("ffi: bad string: {}", e))?;
                let p = c.as_ptr() as u64;
                out.push(p);
                guards.push(MarshalGuardVm::CStr(c));
            }
            Value::Bytes(b) => {
                let mut v = b.to_vec();
                v.push(0);
                let p = v.as_ptr() as u64;
                out.push(p);
                guards.push(MarshalGuardVm::Bytes(v));
            }
            other => return Err(format!("ffi: cannot marshal {} to C", other.type_name())),
        }
    }
    Ok((out, guards))
}

/// Cache of loaded foreign libraries keyed by path (or `"<default>"`), so
/// repeated `extern "C"` calls don't `dlopen` on every invocation.
static FFI_LIB_CACHE: Mutex<Option<HashMap<String, Arc<Mutex<rak_stdlib::ffi::LibHandle>>>>> = Mutex::new(None);

/// Resolve (and cache) a foreign library handle for an `extern` block.
fn ffi_get_lib(path: &Option<String>) -> Result<Arc<Mutex<rak_stdlib::ffi::LibHandle>>, String> {
    let key = path.clone().unwrap_or_else(|| "<default>".to_string());
    let mut cache = FFI_LIB_CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(h) = cache.get(&key) {
        return Ok(h.clone());
    }
    let h = Arc::new(Mutex::new(match path {
        Some(p) => rak_stdlib::ffi::load(p)?,
        None => rak_stdlib::ffi::load_default()?,
    }));
    cache.insert(key, h.clone());
    Ok(h)
}

/// Marshal Rak `Value`s to `u64` bit patterns using declared `Param` types
/// (so float args land in the right register class).
fn marshal_vm_args_typed(args: &[Value], params: &[crate::ast::Param], varargs: bool) -> Result<(Vec<u64>, Vec<MarshalGuardVm>), String> {
    let mut out = Vec::with_capacity(args.len());
    let mut guards = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let ty = params.get(i).and_then(|p| p.type_hint.as_ref());
        match (a, ty) {
            (Value::String(s), _) => {
                let c = CString::new(s.as_ref()).map_err(|e| format!("ffi: bad string: {}", e))?;
                let p = c.as_ptr() as u64;
                out.push(p);
                guards.push(MarshalGuardVm::CStr(c));
            }
            (Value::Bytes(b), _) => {
                let mut v = b.to_vec();
                v.push(0);
                let p = v.as_ptr() as u64;
                out.push(p);
                guards.push(MarshalGuardVm::Bytes(v));
            }
            (Value::ForeignPtr(p), _) => out.push(*p),
            (Value::I64(i), _) => out.push(*i as u64),
            (Value::I32(i), _) => out.push(*i as u64),
            (Value::U64(v), _) => out.push(*v),
            (Value::U32(v), _) => out.push(*v as u64),
            (Value::Hex(h, _), _) => out.push(*h),
            (Value::Bool(b), _) => out.push(if *b { 1 } else { 0 }),
            (Value::F64(f), Some(crate::ast::Type::F64)) => out.push(f.to_bits()),
            (Value::F64(f), Some(crate::ast::Type::F32)) => out.push((*f as f32).to_bits() as u64),
            (Value::F32(f), Some(crate::ast::Type::F32)) => out.push(f.to_bits() as u64),
            (Value::F32(f), Some(crate::ast::Type::F64)) => out.push((*f as f64).to_bits()),
            (Value::F64(f), _) => out.push(*f as u64),
            (Value::F32(f), _) => out.push(*f as u64),
            (Value::Nil, _) => out.push(0),
            (other, _) => {
                if varargs && i >= params.len() {
                    match other {
                        Value::I64(i) => out.push(*i as u64),
                        Value::Hex(h, _) => out.push(*h),
                        Value::ForeignPtr(p) => out.push(*p),
                        Value::Bool(b) => out.push(if *b { 1 } else { 0 }),
                        Value::Nil => out.push(0),
                        _ => return Err(format!("ffi: cannot marshal {} as vararg", other.type_name())),
                    }
                } else {
                    return Err(format!("ffi: cannot marshal {} to C", other.type_name()));
                }
            }
        }
    }
    Ok((out, guards))
}

/// Convert a raw return word into a VM `Value` per the declared return type.
fn unmarshal_vm_ret(ret: &Option<crate::ast::Type>, bits: u64, is_float: bool) -> Value {
    use crate::ast::Type;
    match ret {
        None | Some(Type::Void) => Value::Nil,
        Some(Type::I8) => Value::I64((bits as u8) as i8 as i64),
        Some(Type::I16) => Value::I64((bits as u16) as i16 as i64),
        Some(Type::I32) | Some(Type::Int) => Value::I64(bits as u32 as i64),
        Some(Type::I64) => Value::I64(bits as i64),
        Some(Type::U8) => Value::U64(bits as u8 as u64),
        Some(Type::U16) => Value::U64(bits as u16 as u64),
        Some(Type::U32) => Value::U64(bits as u32 as u64),
        Some(Type::U64) | Some(Type::Hex(_)) => Value::U64(bits),
        Some(Type::F32) => Value::F64(f32::from_bits(bits as u32) as f64),
        Some(Type::F64) => Value::F64(f64::from_bits(bits)),
        Some(Type::Ptr(_)) => Value::ForeignPtr(bits),
        Some(Type::Custom(_)) => Value::ForeignPtr(bits),
        Some(_) => {
            if is_float { Value::F64(f64::from_bits(bits)) } else { Value::ForeignPtr(bits) }
        }
    }
}

/// Build a VM `Value::NativeFn` for one `extern "C"` declaration. The closure
/// lazily loads (and caches) the library, resolves the symbol, marshals args
/// per the declared types, calls, and unmarshals the return.
pub fn make_foreign_native(decl: crate::ast::ForeignFn, lib_path: Option<String>) -> Value {
    let name = Arc::from(decl.name.as_str());
    let native = move |args: &[Value]| -> Result<Value, String> {
        let lib = ffi_get_lib(&lib_path)?;
        let (marshalled, _g) = marshal_vm_args_typed(args, &decl.params, decl.varargs)?;
        let addr = { let h = lib.lock().unwrap(); rak_stdlib::ffi::sym_addr(&h, &decl.name)? };
        let is_float_ret = matches!(decl.return_type, Some(crate::ast::Type::F32) | Some(crate::ast::Type::F64));
        let bits = if is_float_ret {
            unsafe { rak_stdlib::ffi::call_float(addr, &marshalled).to_bits() }
        } else {
            unsafe { rak_stdlib::ffi::call_int(addr, &marshalled) }
        };
        Ok(unmarshal_vm_ret(&decl.return_type, bits, is_float_ret))
    };
    Value::NativeFn(name, Arc::new(native))
}

/// Build a VM `Value::NativeFn` that constructs an enum value:
/// `Event::Connect(host)` pushes a `Value::Enum { name, variant, data }`.
pub fn make_enum_ctor(enum_name: String, variant: String) -> Value {
    let name = Arc::from(format!("{}::{}", enum_name, variant).as_str());
    let native = move |args: &[Value]| -> Result<Value, String> {
        Ok(Value::Enum {
            name: Arc::from(enum_name.as_str()),
            variant: Arc::from(variant.as_str()),
            data: Arc::from(args.to_vec()),
        })
    };
    Value::NativeFn(name, Arc::new(native))
}

pub struct ChannelHandle {
    pub id: u64,
}

/// State of a VM `Value::Future`. Async I/O builtins produce `Pending`
/// (Tokio `JoinHandle`); `await` resolves them.
pub enum VmFutureState {
    Ready(Value),
    Pending(tokio::task::JoinHandle<Value>),
    Polled,
}

pub struct FutureHandle {
    pub state: Mutex<VmFutureState>,
}

pub struct Vm {
    pub globals: HashMap<String, Value>,
    output: Vec<String>,
    /// An in-flight error value (from `Op::Throw` / `Op::TryUnwrap`) that is
    /// being unwound to a `try` handler. Cleared when a handler binds it.
    pending_error: Option<Value>,
    // --- Debugger state ---
    debug_enabled: bool,
    debug_breakpoints: std::collections::HashSet<u32>,
    debug_step: bool,
    debug_handler: Option<Box<dyn FnMut(u32, Vec<(String, Value)>, std::collections::HashMap<String, Value>, Vec<String>) -> VmDebugAction + Send>>,
    /// Current call stack of frame names (pushed on function entry, popped on return).
    debug_callstack: Vec<String>,
}

/// A pending `try` region in a frame: where to jump on error, plus the stack /
/// locals / defer depths to restore before the handler runs.
struct CatchFrame {
    handler_offset: usize,
    stack_len: usize,
    locals_len: usize,
    /// Depth of the frame's defer stack when `Op::Try` executed; defers
    /// registered inside the try body (indices ≥ this) run before the handler.
    defers_len: usize,
}

/// Decision from the VM debugger handler.
#[derive(PartialEq, Clone, Copy)]
pub enum VmDebugAction {
    Continue,
    Step,
    Quit,
}

/// Arithmetic operator for overload dispatch (`impl Add for T { fn add(self, o) }`).
#[derive(Clone, Copy)]
enum BinArith {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}
impl BinArith {
    fn suffix(self) -> &'static str {
        match self {
            BinArith::Add => "add",
            BinArith::Sub => "sub",
            BinArith::Mul => "mul",
            BinArith::Div => "div",
            BinArith::Rem => "rem",
        }
    }
}

/// Comparison operator for overload dispatch (`fn eq`/`lt`/`gt`/`lte`/`gte`/`ne`).
#[derive(Clone, Copy)]
enum CompareOp {
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
}
impl CompareOp {
    fn suffix(self) -> &'static str {
        match self {
            CompareOp::Eq => "eq",
            CompareOp::NotEq => "ne",
            CompareOp::Lt => "lt",
            CompareOp::Gt => "gt",
            CompareOp::LtEq => "lte",
            CompareOp::GtEq => "gte",
        }
    }
}

/// Dispatch type of a value: struct/enum by name, everything else by runtime
/// type name (mirrors `Op::CallMethod`).
fn dispatch_type(v: &Value) -> String {
    match v {
        Value::Struct { name, .. } => name.to_string(),
        Value::Enum { name, .. } => name.to_string(),
        other => other.type_name().to_string(),
    }
}

fn is_numeric_vm(v: &Value) -> bool {
    matches!(
        v,
        Value::I64(_)
            | Value::I32(_)
            | Value::I16(_)
            | Value::I8(_)
            | Value::U64(_)
            | Value::U32(_)
            | Value::U16(_)
            | Value::U8(_)
            | Value::Hex(_, _)
            | Value::F32(_)
            | Value::F64(_)
    )
}

impl Vm {
    pub fn new() -> Self {
        let mut vm = Vm {
            globals: HashMap::new(),
            output: Vec::new(),
            pending_error: None,
            debug_enabled: false,
            debug_breakpoints: std::collections::HashSet::new(),
            debug_step: false,
            debug_handler: None,
            debug_callstack: Vec::new(),
        };
        vm.register_natives();
        vm
    }

    /// Enable the debugger, installing a handler that is invoked at source-line
    /// boundaries. `breakpoints` are 1-based source lines. The handler receives
    /// the current line, a snapshot of locals `(name, value)`, a snapshot of
    /// globals, and the current call stack, and returns a `VmDebugAction`.
    pub fn debugger<H>(&mut self, breakpoints: std::collections::HashSet<u32>, handler: H)
    where
        H: FnMut(u32, Vec<(String, Value)>, std::collections::HashMap<String, Value>, Vec<String>) -> VmDebugAction + Send + 'static,
    {
        self.debug_enabled = true;
        self.debug_breakpoints = breakpoints;
        self.debug_handler = Some(Box::new(handler));
    }

    /// Invoked at a source-line boundary while debugging. Snapshots locals and
    /// globals, calls the installed handler, and returns its action. The
    /// handler owns breakpoint/stepping policy (it is called on every line).
    fn debug_pause(&mut self, frame: &Frame, line: u32) -> VmDebugAction {
        let locals: Vec<(String, Value)> = frame
            .locals
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("local[{}]", i), v.clone()))
            .collect();
        let globals: std::collections::HashMap<String, Value> = self.globals.clone();
        let callstack = self.debug_callstack.clone();
        match self.debug_handler.as_mut() {
            Some(h) => h(line, locals, globals, callstack),
            None => VmDebugAction::Continue,
        }
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    fn register_natives(&mut self) {
        // Result/Option constructors (the interpreter special-cases these in
        // `eval_call`; the VM registers them as natives so `Ok(42)?` works).
        self.insert_native("Ok", |args| {
            Ok(Value::Result(Some(Box::new(args.first().cloned().unwrap_or(Value::Nil))), None))
        });
        self.insert_native("Err", |args| {
            Ok(Value::Result(None, Some(Box::new(args.first().cloned().unwrap_or(Value::Nil)))))
        });
        self.insert_native("Some", |args| {
            Ok(Value::Option(Some(Box::new(args.first().cloned().unwrap_or(Value::Nil)))))
        });
        self.globals.insert("None".to_string(), Value::Option(None));
        self.insert_native("len", |args| {
            match args.first() {
                Some(Value::String(s)) => Ok(Value::I64(s.chars().count() as i64)),
                Some(Value::Array(a)) => Ok(Value::I64(a.len() as i64)),
                Some(Value::Tuple(t)) => Ok(Value::I64(t.len() as i64)),
                Some(Value::Bytes(b)) => Ok(Value::I64(b.len() as i64)),
                Some(Value::Map(m)) => Ok(Value::I64(m.len() as i64)),
                _ => Err("len() requires string/array/tuple/bytes/map".to_string()),
            }
        });
        self.insert_native("int", |args| match args.first() {
            Some(Value::I64(i)) => Ok(Value::I64(*i)),
            Some(Value::Hex(h, _)) => Ok(Value::I64(*h as i64)),
            Some(Value::F64(f)) => Ok(Value::I64(*f as i64)),
            Some(Value::String(s)) => s.parse::<i64>().map(Value::I64).map_err(|_| "int() parse error".to_string()),
            Some(Value::Bool(b)) => Ok(Value::I64(if *b { 1 } else { 0 })),
            _ => Ok(Value::I64(0)),
        });
        self.insert_native("float", |args| Ok(Value::F64(args.first().and_then(|v| v.as_f64()).unwrap_or(0.0))));
        self.insert_native("string", |args| {
            let s = match args.first() {
                Some(Value::MmapSlice(h, off, n)) => String::from_utf8_lossy(&h.as_slice()[*off..off + n]).into_owned(),
                Some(Value::Mmap(h)) => String::from_utf8_lossy(h.as_slice()).into_owned(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            Ok(Value::String(Arc::from(s.as_str())))
        });
        self.insert_native("upper", |args| Ok(Value::String(Arc::from(native_str(args.first()).to_uppercase().as_str()))));
        self.insert_native("lower", |args| Ok(Value::String(Arc::from(native_str(args.first()).to_lowercase().as_str()))));
        self.insert_native("md5", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::md5(&data).as_str())))
        });
        self.insert_native("sha1", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::sha1(&data).as_str())))
        });
        self.insert_native("sha256", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::sha256(&data).as_str())))
        });
        self.insert_native("hmac_sha256", |args| {
            let key = native_bytes(args.first());
            let data = native_bytes(args.get(1));
            Ok(Value::String(Arc::from(rak_stdlib::hmac_sha256(&key, &data).as_str())))
        });
        self.insert_native("aes_gcm_encrypt", |args| {
            let key = native_bytes(args.first());
            let nonce = native_bytes(args.get(1));
            let pt = native_bytes(args.get(2));
            rak_stdlib::aes_gcm_encrypt(&key, &nonce, &pt).map(|b| Value::Bytes(Arc::from(b.as_slice()))).map_err(|e| e)
        });
        self.insert_native("aes_gcm_decrypt", |args| {
            let key = native_bytes(args.first());
            let nonce = native_bytes(args.get(1));
            let ct = native_bytes(args.get(2));
            rak_stdlib::aes_gcm_decrypt(&key, &nonce, &ct).map(|b| Value::Bytes(Arc::from(b.as_slice()))).map_err(|e| e)
        });
        self.insert_native("ed25519_keypair", |args| {
            let seed = native_bytes(args.first());
            rak_stdlib::ed25519_keypair(&seed).map(|(pk, sk)| {
                Value::Tuple(Arc::from([Value::Bytes(Arc::from(pk.as_slice())), Value::Bytes(Arc::from(sk.as_slice()))]))
            }).map_err(|e| e)
        });
        self.insert_native("ed25519_sign", |args| {
            let sk = native_bytes(args.first());
            let msg = native_bytes(args.get(1));
            rak_stdlib::ed25519_sign(&sk, &msg).map(|s| Value::Bytes(Arc::from(s.as_slice()))).map_err(|e| e)
        });
        self.insert_native("ed25519_verify", |args| {
            let pk = native_bytes(args.first());
            let sig = native_bytes(args.get(1));
            let msg = native_bytes(args.get(2));
            rak_stdlib::ed25519_verify(&pk, &sig, &msg).map(Value::Bool).map_err(|e| e)
        });
        self.insert_native("x25519_keypair", |args| {
            let seed = native_bytes(args.first());
            rak_stdlib::tunnel::x25519_keypair(&seed).map(|(pk, sk)| {
                Value::Tuple(Arc::from([Value::Bytes(Arc::from(pk.as_slice())), Value::Bytes(Arc::from(sk.as_slice()))]))
            }).map_err(|e| e)
        });
        self.insert_native("x25519_shared", |args| {
            let secret = native_bytes(args.first());
            let peer = native_bytes(args.get(1));
            rak_stdlib::tunnel::x25519_shared(&secret, &peer)
                .map(|b| Value::Bytes(Arc::from(b.as_slice())))
                .map_err(|e| e)
        });
        self.insert_native("chacha20_encrypt", |args| {
            let key = native_bytes(args.first());
            let nonce = native_bytes(args.get(1));
            let aad = native_bytes(args.get(2));
            let pt = native_bytes(args.get(3));
            rak_stdlib::tunnel::chacha20_encrypt(&key, &nonce, &aad, &pt)
                .map(|b| Value::Bytes(Arc::from(b.as_slice())))
                .map_err(|e| e)
        });
        self.insert_native("chacha20_decrypt", |args| {
            let key = native_bytes(args.first());
            let nonce = native_bytes(args.get(1));
            let aad = native_bytes(args.get(2));
            let ct = native_bytes(args.get(3));
            rak_stdlib::tunnel::chacha20_decrypt(&key, &nonce, &aad, &ct)
                .map(|b| Value::Bytes(Arc::from(b.as_slice())))
                .map_err(|e| e)
        });
        self.insert_native("tunnel_preshared_key", |args| {
            let pass = native_str(args.first());
            let salt = native_bytes(args.get(1));
            let iters = args.get(2).and_then(|v| v.as_u64()).unwrap_or(100_000) as u32;
            let len = args.get(3).and_then(|v| v.as_u64()).unwrap_or(32) as u32;
            rak_stdlib::tunnel::psk_derive(&pass, &salt, iters, len)
                .map(|b| Value::Bytes(Arc::from(b.as_slice())))
                .map_err(|e| e)
        });
        self.insert_native("kdf_next", |args| {
            let prev = native_bytes(args.first());
            let counter = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let len = args.get(2).and_then(|v| v.as_u64()).unwrap_or(32) as u32;
            rak_stdlib::tunnel::kdf_next(&prev, counter, len)
                .map(|b| Value::Bytes(Arc::from(b.as_slice())))
                .map_err(|e| e)
        });
        self.insert_native("tunnel_frame", |args| {
            let seq = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            let payload = native_bytes(args.get(1));
            Ok(Value::Bytes(Arc::from(rak_stdlib::tunnel::tunnel_frame(seq, &payload).as_slice())))
        });
        self.insert_native("tunnel_unframe", |args| {
            let frame = native_bytes(args.first());
            rak_stdlib::tunnel::tunnel_unframe(&frame).map(|(seq, payload)| {
                Value::Tuple(Arc::from([Value::I64(seq as i64), Value::Bytes(Arc::from(payload.as_slice()))]))
            }).map_err(|e| e)
        });
        self.insert_native("tunnel_nonce", |args| {
            let seq = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            Ok(Value::Bytes(Arc::from(rak_stdlib::tunnel::nonce_for(seq).as_slice())))
        });
        // --- Structured errors ---
        self.insert_native("error", |args| {
            let kind = native_str(args.first());
            let message = native_str(args.get(1));
            let k = match kind.trim().to_lowercase().as_str() {
                "io" => crate::ErrorKind::Io,
                "network" => crate::ErrorKind::Network,
                "parse" => crate::ErrorKind::Parse,
                "compile" => crate::ErrorKind::Compile,
                "type" => crate::ErrorKind::Type,
                "package" => crate::ErrorKind::Package,
                "permission" => crate::ErrorKind::Permission,
                "user" => crate::ErrorKind::User,
                "timeout" => crate::ErrorKind::Timeout,
                "cancel" => crate::ErrorKind::Cancel,
                _ => crate::ErrorKind::Runtime,
            };
            Ok(Value::Error(Arc::new(crate::ErrorInfo::new(message.clone()).with_kind(k))))
        });
        self.insert_native("err_message", |args| {
            match args.first() {
                Some(Value::Error(e)) => Ok(Value::String(Arc::from(e.message.as_str()))),
                _ => Err("err_message: expected an error".to_string()),
            }
        });
        self.insert_native("err_kind", |args| {
            match args.first() {
                Some(Value::Error(e)) => Ok(Value::String(Arc::from(e.kind.as_str()))),
                _ => Err("err_kind: expected an error".to_string()),
            }
        });
        self.insert_native("err_line", |args| {
            match args.first() {
                Some(Value::Error(e)) => Ok(Value::I64(e.line.unwrap_or(0) as i64)),
                _ => Err("err_line: expected an error".to_string()),
            }
        });
        self.insert_native("err_col", |args| {
            match args.first() {
                Some(Value::Error(e)) => Ok(Value::I64(e.col.unwrap_or(0) as i64)),
                _ => Err("err_col: expected an error".to_string()),
            }
        });
        self.insert_native("err_file", |args| {
            match args.first() {
                Some(Value::Error(e)) => Ok(Value::String(Arc::from(e.file.clone().unwrap_or_default().as_str()))),
                _ => Err("err_file: expected an error".to_string()),
            }
        });
        self.insert_native("err_cause", |args| {
            match args.first() {
                Some(Value::Error(e)) => match &e.cause {
                    Some(c) => Ok(Value::String(Arc::from(c.clone().as_str()))),
                    None => Ok(Value::Nil),
                },
                _ => Err("err_cause: expected an error".to_string()),
            }
        });
        self.insert_native("err_context", |args| {
            match args.first() {
                Some(Value::Error(e)) => {
                    let m: std::collections::HashMap<String, Value> = e.context.iter()
                        .map(|(k, v)| (k.clone(), Value::String(Arc::from(v.clone().as_str()))))
                        .collect();
                    Ok(Value::Map(Arc::new(m)))
                }
                _ => Err("err_context: expected an error".to_string()),
            }
        });
        self.insert_native("err_with_context", |args| {
            match args.first() {
                Some(Value::Error(e)) => {
                    let k = native_str(args.get(1));
                    let v = native_str(args.get(2));
                    let mut new = e.as_ref().clone();
                    new.context.push((k, v));
                    Ok(Value::Error(Arc::new(new)))
                }
                _ => Err("err_with_context: expected an error".to_string()),
            }
        });
        self.insert_native("hex_encode", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::hex_encode(&data).as_str())))
        });
        self.insert_native("base64_encode", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::base64_encode(&data).as_str())))
        });
        self.insert_native("fmt", |args| {
            let fmt = native_str(args.first());
            let rest: Vec<Value> = args.iter().skip(1).cloned().collect();
            Ok(Value::String(Arc::from(format_rak(&fmt, &rest).as_str())))
        });
        self.insert_native("to_hex", |args| {
            Ok(Value::String(Arc::from(format!("0x{:X}", args.first().and_then(|v| v.as_u64()).unwrap_or(0)).as_str())))
        });
        // --- Regex ---
        self.insert_native("regex_new", |args| {
            let pattern = native_str(args.first());
            let flags = native_str(args.get(1));
            build_vm_regex(&pattern, &flags).map(Value::Regex)
        });
        self.insert_native("regex_match", |args| {
            let re = vm_regex(args.first())?;
            let hay = native_str(args.get(1));
            Ok(Value::Bool(re.is_match(&hay)))
        });
        self.insert_native("regex_find", |args| {
            let re = vm_regex(args.first())?;
            let hay = native_str(args.get(1));
            Ok(re.find(&hay).map(|m| Value::String(Arc::from(m.as_str()))).unwrap_or(Value::Nil))
        });
        self.insert_native("regex_find_all", |args| {
            let re = vm_regex(args.first())?;
            let hay = native_str(args.get(1));
            Ok(Value::Array(Arc::from(
                re.find_iter(&hay).map(|m| Value::String(Arc::from(m.as_str()))).collect::<Vec<_>>(),
            )))
        });
        self.insert_native("regex_replace", |args| {
            let re = vm_regex(args.first())?;
            let hay = native_str(args.get(1));
            let rep = native_str(args.get(2));
            let cow = re.replace_all(&hay, rep.as_str());
            Ok(Value::String(Arc::from(&*cow)))
        });
        // --- FFI ---
        self.insert_native("ffi_load", |args| {
            let path = native_str(args.first());
            match rak_stdlib::ffi::load(&path) {
                Ok(h) => Ok(Value::ForeignLib(Arc::new(Mutex::new(h)))),
                Err(e) => Err(e),
            }
        });
        self.insert_native("ffi_ptr", |args| {
            Ok(Value::ForeignPtr(args.first().and_then(|v| v.as_u64()).unwrap_or(0)))
        });
        self.insert_native("ffi_alloc", |args| {
            let n = args.first().and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let v = vec![0u8; n];
            let ptr = v.as_ptr() as u64;
            std::mem::forget(v);
            FFI_ALLOC.lock().unwrap().get_or_insert_with(HashMap::new).insert(ptr, n);
            Ok(Value::ForeignPtr(ptr))
        });
        self.insert_native("ffi_free", |args| {
            let ptr = match args.first() {
                Some(Value::ForeignPtr(p)) => *p,
                _ => return Err("ffi_free(ptr) requires a ptr".to_string()),
            };
            let mut tbl = FFI_ALLOC.lock().unwrap();
            if let Some(tbl) = tbl.as_mut() {
                if let Some(n) = tbl.remove(&ptr) {
                    unsafe { let _ = Vec::from_raw_parts(ptr as *mut u8, n, n); }
                    return Ok(Value::Nil);
                }
            }
            Err("ffi_free: pointer was not allocated by ffi_alloc/ffi_string_to_cstr".to_string())
        });
        self.insert_native("ffi_write", |args| {
            let ptr = match args.first() {
                Some(Value::ForeignPtr(p)) => *p as usize,
                _ => return Err("ffi_write(ptr, off, byte)".to_string()),
            };
            let off = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let byte = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            unsafe { *((ptr + off) as *mut u8) = byte; }
            Ok(Value::Nil)
        });
        self.insert_native("ffi_read", |args| {
            let ptr = match args.first() {
                Some(Value::ForeignPtr(p)) => *p as usize,
                _ => return Err("ffi_read(ptr, off)".to_string()),
            };
            let off = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let b = unsafe { *((ptr + off) as *const u8) };
            Ok(Value::I64(b as i64))
        });
        self.insert_native("ffi_cstr_to_string", |args| {
            let ptr = match args.first() {
                Some(Value::ForeignPtr(p)) => *p as usize,
                _ => return Err("ffi_cstr_to_string(ptr)".to_string()),
            };
            let s = unsafe {
                let mut len = 0usize;
                while *(ptr as *const u8).add(len) != 0 { len += 1; }
                let slice = std::slice::from_raw_parts(ptr as *const u8, len);
                String::from_utf8_lossy(slice).into_owned()
            };
            Ok(Value::String(Arc::from(s.as_str())))
        });
        self.insert_native("ffi_string_to_cstr", |args| {
            let s = native_str(args.first());
            let bytes = match CString::new(s.as_str()) {
                Ok(c) => c.into_bytes_with_nul(),
                Err(e) => return Err(format!("ffi_string_to_cstr: {}", e)),
            };
            let len = bytes.len();
            let ptr = bytes.as_ptr() as u64;
            std::mem::forget(bytes);
            FFI_ALLOC.lock().unwrap().get_or_insert_with(HashMap::new).insert(ptr, len);
            Ok(Value::ForeignPtr(ptr))
        });
        self.insert_native("ffi_call", |args| {
            let (lib, symbol) = match (args.first(), args.get(1)) {
                (Some(Value::ForeignLib(h)), Some(Value::String(s))) => (h.clone(), s.to_string()),
                _ => return Err("ffi_call(lib, symbol, args_array)".to_string()),
            };
            let c_args: Vec<Value> = match args.get(2) {
                Some(Value::Array(a)) => a.to_vec(),
                Some(Value::Nil) | None => Vec::new(),
                Some(other) => return Err(format!("ffi_call: args must be an array, got {}", other.type_name())),
            };
            let (marshalled, _g) = marshal_vm_args(&c_args)?;
            let addr = { let h = lib.lock().unwrap(); rak_stdlib::ffi::sym_addr(&h, &symbol)? };
            let ret = unsafe { rak_stdlib::ffi::call_int(addr, &marshalled) };
            Ok(Value::I64(ret as i64))
        });
        // --- Async I/O (Tokio-backed futures) ---
        self.insert_native("http_get_async", |args| {
            let url = native_str(args.first());
            let jh = crate::async_rt::runtime().spawn_blocking(move || {
                match rak_stdlib::net::http_get(&url, None) {
                    Ok(r) => Value::String(Arc::from(r.body.as_str())),
                    Err(e) => Value::String(Arc::from(format!("error: {}", e).as_str())),
                }
            });
            Ok(Value::Future(Arc::new(FutureHandle { state: Mutex::new(VmFutureState::Pending(jh)) })))
        });
        self.insert_native("tcp_probe", |args| {
            let host = native_str(args.first());
            let port = args.get(1).and_then(|v| v.as_u64()).unwrap_or(80) as u16;
            let timeout_ms = args.get(2).and_then(|v| v.as_u64()).unwrap_or(1000) as u64;
            let jh = crate::async_rt::runtime().spawn_blocking(move || {
                Value::Bool(rak_stdlib::net::tcp_scan(&host, port, timeout_ms))
            });
            Ok(Value::Future(Arc::new(FutureHandle { state: Mutex::new(VmFutureState::Pending(jh)) })))
        });
        self.insert_native("async_sleep", |args| {
            let ms = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            let jh = crate::async_rt::runtime().spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                Value::Nil
            });
            Ok(Value::Future(Arc::new(FutureHandle { state: Mutex::new(VmFutureState::Pending(jh)) })))
        });
        self.insert_native("async_yield", |args| {
            let jh = crate::async_rt::runtime().spawn(async move {
                tokio::task::yield_now().await;
                Value::Nil
            });
            Ok(Value::Future(Arc::new(FutureHandle { state: Mutex::new(VmFutureState::Pending(jh)) })))
        });
        // --- Raw sockets / packet forging ---
        self.insert_native("net_raw_csum", |args| {
            Ok(Value::I64(rak_stdlib::net_raw::csum16(&native_bytes(args.first())) as i64))
        });
        self.insert_native("net_raw_ipv4", |args| {
            let src = native_str(args.first());
            let dst = native_str(args.get(1));
            let proto = args.get(2).and_then(|v| v.as_u64()).unwrap_or(6) as u8;
            let payload = native_bytes(args.get(3));
            Ok(Value::Bytes(Arc::from(rak_stdlib::net_raw::ipv4(&src, &dst, proto, &payload)?.as_slice())))
        });
        self.insert_native("net_raw_tcp", |args| {
            let src_ip = native_str(args.first());
            let dst_ip = native_str(args.get(1));
            let src_port = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let dst_port = args.get(3).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let flags = { let f = native_str(args.get(4)); if f.is_empty() { "S".to_string() } else { f } };
            let seq = args.get(5).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let ack = args.get(6).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let payload = native_bytes(args.get(7));
            Ok(Value::Bytes(Arc::from(rak_stdlib::net_raw::tcp(&src_ip, &dst_ip, src_port, dst_port, &flags, seq, ack, &payload)?.as_slice())))
        });
        self.insert_native("net_raw_udp", |args| {
            let src_ip = native_str(args.first());
            let dst_ip = native_str(args.get(1));
            let src_port = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let dst_port = args.get(3).and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let payload = native_bytes(args.get(4));
            Ok(Value::Bytes(Arc::from(rak_stdlib::net_raw::udp(&src_ip, &dst_ip, src_port, dst_port, &payload)?.as_slice())))
        });
        self.insert_native("net_raw_tcp_syn", |args| {
            let src = native_str(args.first());
            let dst = native_str(args.get(1));
            let src_port = args.get(2).and_then(|v| v.as_u64()).unwrap_or(12345) as u16;
            let dport = args.get(3).and_then(|v| v.as_u64()).unwrap_or(80) as u16;
            Ok(Value::Bytes(Arc::from(rak_stdlib::net_raw::tcp_syn(&src, &dst, src_port, dport)?.as_slice())))
        });
        self.insert_native("net_raw_send", |args| {
            let pkt = native_bytes(args.first());
            match rak_stdlib::net_raw::send(&pkt) {
                Ok(n) => Ok(Value::Result(Some(Box::new(Value::I64(n as i64))), None)),
                Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(Arc::from(e.as_str())))))),
            }
        });
        self.insert_native("net_raw_recv", |args| {
            let max = args.first().and_then(|v| v.as_u64()).unwrap_or(4096) as usize;
            match rak_stdlib::net_raw::recv(max) {
                Ok(b) => Ok(Value::Result(Some(Box::new(Value::Bytes(Arc::from(b.as_slice())))), None)),
                Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(Arc::from(e.as_str())))))),
            }
        });
        // --- DNS ---
        self.insert_native("dns_query", |args| {
            let name = native_str(args.first());
            let rtype = { let r = native_str(args.get(1)); if r.is_empty() { "A".to_string() } else { r } };
            let server = args.get(2).map(|v| native_str(Some(v)));
            match rak_stdlib::dns::query(&name, &rtype, server.as_deref()) {
                Ok(resp) => {
                    let answers: Vec<Value> = resp.answers.into_iter().map(|r| {
                        let mut m = HashMap::new();
                        m.insert("name".to_string(), Value::String(Arc::from(r.name.as_str())));
                        m.insert("type".to_string(), Value::String(Arc::from(r.rtype.as_str())));
                        m.insert("ttl".to_string(), Value::I64(r.ttl as i64));
                        m.insert("rdata".to_string(), Value::String(Arc::from(r.rdata.as_str())));
                        Value::Map(Arc::from(m))
                    }).collect();
                    let mut out = HashMap::new();
                    out.insert("answers".to_string(), Value::Array(Arc::from(answers)));
                    out.insert("truncated".to_string(), Value::Bool(resp.truncated));
                    Ok(Value::Result(Some(Box::new(Value::Map(Arc::from(out)))), None))
                }
                Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(Arc::from(e.as_str())))))),
            }
        });
        self.insert_native("dns_build", |args| {
            let name = native_str(args.first());
            let rtype = { let r = native_str(args.get(1)); if r.is_empty() { "A".to_string() } else { r } };
            Ok(Value::Bytes(Arc::from(rak_stdlib::dns::build_query(&name, &rtype).as_slice())))
        });
        self.insert_native("dns_parse", |args| {
            let msg = native_bytes(args.first());
            let resp = rak_stdlib::dns::parse_response(&msg)?;
            let answers: Vec<Value> = resp.answers.into_iter().map(|r| {
                let mut m = HashMap::new();
                m.insert("name".to_string(), Value::String(Arc::from(r.name.as_str())));
                m.insert("type".to_string(), Value::String(Arc::from(r.rtype.as_str())));
                m.insert("ttl".to_string(), Value::I64(r.ttl as i64));
                m.insert("rdata".to_string(), Value::String(Arc::from(r.rdata.as_str())));
                Value::Map(Arc::from(m))
            }).collect();
            let mut out = HashMap::new();
            out.insert("answers".to_string(), Value::Array(Arc::from(answers)));
            out.insert("truncated".to_string(), Value::Bool(resp.truncated));
            Ok(Value::Map(Arc::from(out)))
        });
        // --- TLS ---
        self.insert_native("tls_parse_client_hello", |args| {
            let bytes = native_bytes(args.first());
            let info = rak_stdlib::tls::parse_client_hello(&bytes)?;
            let ciphers: Vec<Value> = info.ciphers.into_iter().map(|c| Value::Hex(c as u64, 16)).collect();
            let mut out = HashMap::new();
            out.insert("sni".to_string(), Value::String(Arc::from(info.sni.as_str())));
            out.insert("ciphers".to_string(), Value::Array(Arc::from(ciphers)));
            Ok(Value::Map(Arc::from(out)))
        });
        self.insert_native("tls_parse_cert_chain", |args| {
            let der = native_bytes(args.first());
            let certs: Vec<Value> = rak_stdlib::tls::parse_cert_chain(&der).into_iter().map(|c| {
                let mut m = HashMap::new();
                m.insert("subject".to_string(), Value::String(Arc::from(c.subject.as_str())));
                m.insert("issuer".to_string(), Value::String(Arc::from(c.issuer.as_str())));
                Value::Map(Arc::from(m))
            }).collect();
            Ok(Value::Array(Arc::from(certs)))
        });
        // --- PCAP ---
        self.insert_native("pcap_open", |args| {
            let path = native_str(args.first());
            match rak_stdlib::pcap::open(&path) {
                Ok(h) => Ok(Value::Result(Some(Box::new(Value::Pcap(Arc::new(Mutex::new(h))))), None)),
                Err(e) => Ok(Value::Result(None, Some(Box::new(Value::String(Arc::from(e.as_str())))))),
            }
        });
        self.insert_native("pcap_next", |args| {
            let h = match args.first() {
                Some(Value::Pcap(h)) => h.clone(),
                _ => return Err("pcap_next(handle)".to_string()),
            };
            let mut guard = h.lock().unwrap();
            match rak_stdlib::pcap::next(&mut guard) {
                Some(p) => {
                    let mut m = HashMap::new();
                    m.insert("timestamp".to_string(), Value::I64(p.timestamp));
                    m.insert("linktype".to_string(), Value::I64(p.linktype));
                    m.insert("payload".to_string(), Value::Bytes(Arc::from(p.payload.as_slice())));
                    Ok(Value::Map(Arc::from(m)))
                }
                None => Ok(Value::Nil),
            }
        });
        // --- Memory-mapped files ---
        self.insert_native("mmap_open", |args| {
            let path = native_str(args.first());
            let mode = { let m = native_str(args.get(1)); if m.is_empty() { "r".to_string() } else { m } };
            match rak_stdlib::mmap::open(&path, &mode) {
                Ok(h) => Ok(Value::Mmap(h)),
                Err(e) => Err(e),
            }
        });
        self.insert_native("mmap_slice", |args| {
            let (h, off, len) = match (args.first(), args.get(1), args.get(2)) {
                (Some(Value::Mmap(h)), Some(o), Some(l)) => (h.clone(), o.as_u64().unwrap_or(0) as usize, l.as_u64().unwrap_or(0) as usize),
                _ => return Err("mmap_slice(mmap, off, len)".to_string()),
            };
            let total = h.len();
            if off.saturating_add(len) > total {
                return Err(format!("mmap_slice: [off, off+len) = [{}, {}) out of range (len {})", off, off + len, total));
            }
            Ok(Value::MmapSlice(h, off, len))
        });
        self.insert_native("mmap_size", |args| match args.first() {
            Some(Value::Mmap(h)) => Ok(Value::I64(h.len() as i64)),
            Some(Value::MmapSlice(_, _, n)) => Ok(Value::I64(*n as i64)),
            _ => Err("mmap_size(mmap)".to_string()),
        });
        self.insert_native("mmap_close", |_| Ok(Value::Nil));
        self.insert_native("mmap_find", |args| {
            let h = match args.first() {
                Some(Value::Mmap(h)) => h.clone(),
                Some(Value::MmapSlice(h, _, _)) => h.clone(),
                _ => return Err("mmap_find(mmap, needle)".to_string()),
            };
            let needle = native_bytes(args.get(1));
            match rak_stdlib::mmap::find(&h, &needle) {
                Some(p) => Ok(Value::I64(p as i64)),
                None => Ok(Value::I64(-1)),
            }
        });
        self.insert_native("mmap_lines", |args| {
            let h = match args.first() {
                Some(Value::Mmap(h)) => h.clone(),
                _ => return Err("mmap_lines(mmap, delim?)".to_string()),
            };
            let delim = { let d = native_str(args.get(1)); if d.is_empty() { "\n".to_string() } else { d } };
            let ls = rak_stdlib::mmap::lines(&h, delim.as_bytes());
            Ok(Value::Array(Arc::from(ls.into_iter().map(|s| Value::String(Arc::from(s.as_str()))).collect::<Vec<_>>())))
        });
        self.insert_native("mmap_lines_off", |args| {
            let h = match args.first() {
                Some(Value::Mmap(h)) => h.clone(),
                _ => return Err("mmap_lines_off(mmap, delim?)".to_string()),
            };
            let delim = { let d = native_str(args.get(1)); if d.is_empty() { "\n".to_string() } else { d } };
            let offs = rak_stdlib::mmap::lines_off(&h, delim.as_bytes());
            let tup: Vec<Value> = offs.into_iter().map(|(o, l)| Value::Tuple(Arc::from([Value::I64(o as i64), Value::I64(l as i64)]))).collect();
            Ok(Value::Array(Arc::from(tup)))
        });
        // --- Forensic Structs / evidence provenance (VM) ---
        self.insert_native("__evidence_from", |args| {
            let v = args.first().cloned().unwrap_or(Value::Nil);
            let prov = Arc::new(crate::value::Provenance {
                tool: "manual".to_string(),
                target: String::new(),
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                raw_offset: None,
                raw_len: None,
                parent: match &v {
                    Value::Evidence { provenance, .. } => Some(provenance.clone()),
                    _ => None,
                },
            });
            Ok(Value::Evidence {
                inner: Box::new(unwrap_vm_evidence(&v)),
                provenance: prov,
            })
        });
        self.insert_native("cite", |args| {
            let v = args.first().cloned().unwrap_or(Value::Nil);
            let tool = match args.get(1) {
                Some(Value::String(s)) => s.to_string(),
                _ => "manual".to_string(),
            };
            let target = match args.get(2) {
                Some(Value::String(s)) => s.to_string(),
                _ => String::new(),
            };
            let parent = match &v {
                Value::Evidence { provenance, .. } => Some(provenance.clone()),
                _ => None,
            };
            let prov = Arc::new(crate::value::Provenance {
                tool,
                target,
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                raw_offset: None,
                raw_len: None,
                parent,
            });
            Ok(Value::Evidence {
                inner: Box::new(unwrap_vm_evidence(&v)),
                provenance: prov,
            })
        });
        self.insert_native("strip_evidence", |args| {
            Ok(unwrap_vm_evidence(args.first().unwrap_or(&Value::Nil)))
        });
        self.insert_native("provenance", |args| {
            provenance_to_vm_value(args.first().unwrap_or(&Value::Nil))
        });
        self.insert_native("report", |args| {
            let mut out = String::new();
            for (i, a) in args.iter().enumerate() {
                let inner = unwrap_vm_evidence(a);
                out.push_str(&format!("{}. {}\n", i + 1, inner));
            }
            out.push_str("\n--- Sources ---\n");
            for (i, a) in args.iter().enumerate() {
                if let Value::Evidence { provenance, .. } = a {
                    let mut chain: Vec<Arc<crate::value::Provenance>> = Vec::new();
                    let mut cur = Some(provenance.clone());
                    while let Some(p) = cur {
                        chain.push(p.clone());
                        cur = p.parent.clone();
                    }
                    for p in chain.into_iter().rev() {
                        let t = if p.target.is_empty() { String::new() } else { format!(" target={}", p.target) };
                        out.push_str(&format!("[{}] tool={}{} ts={}\n", i + 1, p.tool, t, p.ts));
                    }
                } else {
                    out.push_str(&format!("[{}] tool=manual\n", i + 1));
                }
            }
            Ok(Value::String(Arc::from(out.as_str())))
        });
        // --- Structured logging (#8) ---
        self.insert_native("log_level", |args| {
            let lvl = native_str(args.first());
            rak_stdlib::log::set_level(&lvl);
            Ok(Value::Nil)
        });
        self.insert_native("log_init", |args| {
            let path = native_str(args.first());
            rak_stdlib::log::init_file(&path)?;
            Ok(Value::Nil)
        });
        self.insert_native("log_info", |args| vm_log(&rak_stdlib::log::Level::Info, args));
        self.insert_native("log_warn", |args| vm_log(&rak_stdlib::log::Level::Warn, args));
        self.insert_native("log_error", |args| vm_log(&rak_stdlib::log::Level::Error, args));
        self.insert_native("log_debug", |args| vm_log(&rak_stdlib::log::Level::Debug, args));
        // --- Process API (#6) ---
        self.insert_native("process_spawn", |args| {
            let cmd = native_str(args.first());
            let mut a = Vec::new();
            if let Some(Value::Array(arr)) = args.get(1) {
                for v in arr.iter() {
                    a.push(native_str(Some(v)));
                }
            }
            let pid = rak_stdlib::process::spawn(&cmd, &a)?;
            Ok(Value::I64(pid as i64))
        });
        self.insert_native("process_wait", |args| {
            let pid = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            let code = rak_stdlib::process::wait(pid)?;
            Ok(Value::I64(code))
        });
        self.insert_native("process_stdout", |args| {
            let pid = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            let s = rak_stdlib::process::read_stdout(pid)?;
            Ok(Value::String(Arc::from(s.as_str())))
        });
        self.insert_native("process_stderr", |args| {
            let pid = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            let s = rak_stdlib::process::read_stderr(pid)?;
            Ok(Value::String(Arc::from(s.as_str())))
        });
        self.insert_native("process_kill", |args| {
            let pid = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            rak_stdlib::process::kill(pid)?;
            Ok(Value::Nil)
        });
        // --- DNS toolkit (#3) ---
        self.insert_native("dns_resolve", |args| {
            let name = native_str(args.first());
            let server = args.get(1).map(|v| native_str(Some(v)));
            let addrs = rak_stdlib::dns::resolve(&name, server.as_deref())?;
            Ok(Value::Array(Arc::from(addrs.into_iter().map(|s| Value::String(Arc::from(s.as_str()))).collect::<Vec<_>>())))
        });
        self.insert_native("dns_reverse", |args| {
            let ip = native_str(args.first());
            let server = args.get(1).map(|v| native_str(Some(v)));
            let names = rak_stdlib::dns::reverse(&ip, server.as_deref())?;
            Ok(Value::Array(Arc::from(names.into_iter().map(|s| Value::String(Arc::from(s.as_str()))).collect::<Vec<_>>())))
        });
        self.insert_native("dns_records", |args| {
            let name = native_str(args.first());
            let server = args.get(1).map(|v| native_str(Some(v)));
            let recs = rak_stdlib::dns::records(&name, server.as_deref())?;
            let arr: Vec<Value> = recs.into_iter().map(|r| {
                let mut m = HashMap::new();
                m.insert("name".to_string(), Value::String(Arc::from(r.name.as_str())));
                m.insert("type".to_string(), Value::String(Arc::from(r.rtype.as_str())));
                m.insert("ttl".to_string(), Value::I64(r.ttl as i64));
                m.insert("rdata".to_string(), Value::String(Arc::from(r.rdata.as_str())));
                Value::Map(Arc::from(m))
            }).collect();
            Ok(Value::Array(Arc::from(arr)))
        });
        self.insert_native("dns_walk", |args| {
            let domain = native_str(args.first());
            let mut prefixes = Vec::new();
            if let Some(Value::Array(a)) = args.get(1) {
                for v in a.iter() {
                    prefixes.push(native_str(Some(v)));
                }
            }
            let server = args.get(2).map(|v| native_str(Some(v)));
            let found = rak_stdlib::dns::walk(&domain, &prefixes, server.as_deref());
            Ok(Value::Array(Arc::from(found.into_iter().map(|s| Value::String(Arc::from(s.as_str()))).collect::<Vec<_>>())))
        });
        // --- Secrets API (#9) ---
        self.insert_native("secret_get", |args| {
            let name = native_str(args.first());
            match rak_stdlib::secrets::get(&name) {
                Some(v) => Ok(Value::String(Arc::from(v.as_str()))),
                None => Ok(Value::Nil),
            }
        });
        self.insert_native("secret_set", |args| {
            let name = native_str(args.first());
            let value = native_str(args.get(1));
            rak_stdlib::secrets::set(&name, &value);
            Ok(Value::Nil)
        });
        self.insert_native("secret_persist", |args| {
            let name = native_str(args.first());
            let value = native_str(args.get(1));
            rak_stdlib::secrets::persist(&name, &value)?;
            Ok(Value::Nil)
        });
        self.insert_native("secret_delete", |args| {
            let name = native_str(args.first());
            rak_stdlib::secrets::delete(&name)?;
            Ok(Value::Nil)
        });
        self.insert_native("secret_ls", |_args| {
            let names = rak_stdlib::secrets::list();
            Ok(Value::Array(Arc::from(names.into_iter().map(|s| Value::String(Arc::from(s.as_str()))).collect::<Vec<_>>())))
        });
        // --- HTTP server framework (#4) ---
        self.insert_native("http_server_start", |args| {
            let addr = native_str(args.first());
            let port = args.get(1).and_then(|v| v.as_u64()).unwrap_or(8080) as u16;
            let bound = rak_stdlib::http_server::server_start(&addr, port)?;
            Ok(Value::I64(bound as i64))
        });
        self.insert_native("http_server_poll", |_args| {
            match rak_stdlib::http_server::poll() {
                Some(req) => {
                    let mut m = HashMap::new();
                    m.insert("id".to_string(), Value::I64(req.id as i64));
                    m.insert("method".to_string(), Value::String(Arc::from(req.method.as_str())));
                    m.insert("path".to_string(), Value::String(Arc::from(req.path.as_str())));
                    m.insert("query".to_string(), vm_map(req.query));
                    m.insert("headers".to_string(), vm_map(req.headers));
                    m.insert("body".to_string(), Value::String(Arc::from(req.body.as_str())));
                    Ok(Value::Map(Arc::from(m)))
                }
                None => Ok(Value::Nil),
            }
        });
        self.insert_native("http_server_respond", |args| {
            let id = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            let status = args.get(1).and_then(|v| v.as_u64()).unwrap_or(200) as u16;
            let headers: Vec<(String, String)> = match args.get(2) {
                Some(Value::Map(m)) => m.iter().map(|(k, v)| (k.clone(), v.to_string())).collect(),
                _ => Vec::new(),
            };
            let body = native_str(args.get(3));
            rak_stdlib::http_server::respond(id, status, &headers, &body).map_err(|e| e)?;
            Ok(Value::Nil)
        });
        self.insert_native("http_server_stop", |_args| {
            rak_stdlib::http_server::server_stop();
            Ok(Value::Nil)
        });
        self.insert_native("sleep", |args| {
            let ms = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(Value::Nil)
        });
        self.insert_native("now_ms", |_args| {
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
            Ok(Value::I64(ms))
        });
    }

    fn insert_native(&mut self, name: &str, f: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static) {
        self.globals.insert(name.to_string(), Value::NativeFn(Arc::from(name), Arc::new(f)));
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<Vec<String>, String> {
        let mut frame = Frame { code: chunk, ip: 0, stack: Vec::new(), locals: Vec::new(), defers: Vec::new(), catches: Vec::new() };
        self.exec_frame(&mut frame)?;
        Ok(std::mem::take(&mut self.output))
    }

    /// Execute a frame, catching errors and unwinding to the nearest `try`
    /// handler in this frame. Frames without a handler propagate the error to
    /// the caller (which may have its own handler). A frame that fully unwinds
    /// (no handler) runs its deferred calls in LIFO order first.
    fn exec_frame(&mut self, frame: &mut Frame) -> Result<(), String> {
        loop {
            match self.exec_frame_inner(frame) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if let Some(c) = frame.catches.pop() {
                        // Run defers registered inside the try body (LIFO),
                        // then restore the stack/locals depths recorded at
                        // `Op::Try` and jump to the handler.
                        while frame.defers.len() > c.defers_len {
                            let (callee, args) = frame.defers.pop().unwrap();
                            self.call_value(frame, callee, args)?;
                        }
                        frame.stack.truncate(c.stack_len);
                        frame.locals.truncate(c.locals_len);
                        let errv = self.pending_error.take().unwrap_or_else(|| {
                            Value::Error(Arc::new(crate::ErrorInfo::new(e.clone()).with_kind(crate::ErrorKind::Runtime)))
                        });
                        frame.ip = c.handler_offset;
                        frame.push(errv);
                        continue;
                    }
                    self.run_frame_defers(frame)?;
                    return Err(e);
                }
            }
        }
    }

    fn exec_frame_inner(&mut self, frame: &mut Frame) -> Result<(), String> {
        while frame.ip < frame.code.code.len() {
            let op = Op::from_u8(frame.code.code[frame.ip]).ok_or_else(|| format!("bad opcode at {}", frame.ip))?;
            frame.ip += 1;
            // Debugger: pause at source-line boundaries (breakpoints/step/continue).
            if self.debug_enabled && !matches!(op, Op::LoopEnd | Op::LoopBegin) {
                let cur_line = frame.code.lines.get(frame.ip.saturating_sub(1)).copied().unwrap_or(0);
                let action = self.debug_pause(frame, cur_line);
                match action {
                    VmDebugAction::Quit => return Ok(()),
                    VmDebugAction::Continue | VmDebugAction::Step => {}
                }
            }
            match op {
                Op::Nop => {}
                Op::LoadConst => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    frame.push(frame.code.constants[ci].clone());
                }
                Op::LoadLocal => {
                    let slot = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    frame.push(frame.locals[slot].clone());
                }
                Op::StoreLocal => {
                    let slot = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let v = frame.pop();
                    if slot < frame.locals.len() {
                        frame.locals[slot] = v;
                    } else {
                        while frame.locals.len() < slot {
                            frame.locals.push(Value::Nil);
                        }
                        frame.locals.push(v);
                    }
                }
                Op::LoadGlobal => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let name = match &frame.code.constants[ci] {
                        Value::String(s) => s.to_string(),
                        _ => return Err("bad global name".to_string()),
                    };
                    match self.globals.get(&name) {
                        Some(v) => frame.push(v.clone()),
                        None => return Err(format!("Undefined: {}", name)),
                    }
                }
                Op::StoreGlobal => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let name = match &frame.code.constants[ci] {
                        Value::String(s) => s.to_string(),
                        _ => return Err("bad global name".to_string()),
                    };
                    let v = frame.pop();
                    self.globals.insert(name, v);
                }
                Op::Pop => { frame.pop(); }
                Op::Dup => { let v = frame.peek().clone(); frame.push(v); }
                Op::AddI => self.binop_arith(frame, BinArith::Add)?,
                Op::SubI => self.binop_arith(frame, BinArith::Sub)?,
                Op::MulI => self.binop_arith(frame, BinArith::Mul)?,
                Op::DivI => self.binop_arith(frame, BinArith::Div)?,
                Op::RemI => self.binop_arith(frame, BinArith::Rem)?,
                Op::NegI => self.binop_neg(frame)?,
                Op::AddF => bin_float(frame, |a, b| a + b),
                Op::SubF => bin_float(frame, |a, b| a - b),
                Op::MulF => bin_float(frame, |a, b| a * b),
                Op::DivF => bin_float(frame, |a, b| if b == 0.0 { 0.0 } else { a / b }),
                Op::NegF => { let v = frame.pop(); frame.push(Value::F64(-(v.as_f64().unwrap_or(0.0)))); }
                Op::BitAnd => bin_int(frame, |a, b| a & b, |_, _| 0.0),
                Op::BitOr => bin_int(frame, |a, b| a | b, |_, _| 0.0),
                Op::BitXor => bin_int(frame, |a, b| a ^ b, |_, _| 0.0),
                Op::BitNot => { let v = frame.pop(); frame.push(Value::I64(!(v.as_i64().unwrap_or(0)))); }
                Op::Shl => bin_int(frame, |a, b| a << b, |_, _| 0.0),
                Op::Shr => bin_int(frame, |a, b| a >> b, |_, _| 0.0),
                Op::Eq => self.binop_compare(frame, CompareOp::Eq)?,
                Op::NotEq => self.binop_compare(frame, CompareOp::NotEq)?,
                Op::Lt => self.binop_compare(frame, CompareOp::Lt)?,
                Op::Gt => self.binop_compare(frame, CompareOp::Gt)?,
                Op::LtEq => self.binop_compare(frame, CompareOp::LtEq)?,
                Op::GtEq => self.binop_compare(frame, CompareOp::GtEq)?,
                Op::Not => { let v = frame.pop(); frame.push(Value::Bool(!v.is_truthy())); }
                Op::True => frame.push(Value::Bool(true)),
                Op::False => frame.push(Value::Bool(false)),
                Op::Nil => frame.push(Value::Nil),
                Op::Jump => {
                    let target = frame.code.read_u16(frame.ip) as usize;
                    frame.ip = target;
                }
                Op::JumpIfFalse => {
                    let target = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    if !frame.peek().is_truthy() {
                        frame.ip = target;
                    }
                }
                Op::JumpIfTrue => {
                    let target = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    if frame.peek().is_truthy() {
                        frame.ip = target;
                    }
                }
                Op::JumpIfNil => {
                    let target = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let isnil = match frame.peek() {
                        Value::Nil => true,
                        Value::Option(None) => true,
                        _ => false,
                    };
                    if isnil {
                        frame.ip = target;
                    }
                }
                Op::Call => {
                    let argc = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let mut args: Vec<Value> = (0..argc).map(|_| frame.pop()).collect();
                    args.reverse();
                    let callee = frame.pop();
                    self.call_value(frame, callee, args)?;
                }
                Op::Return => {
                    // Preserve the return value, run this frame's deferred
                    // calls in LIFO order, then leave the return value as the
                    // result for the caller.
                    let ret = frame.pop();
                    self.run_frame_defers(frame)?;
                    frame.push(ret);
                    return Ok(());
                }
                Op::DeferCall => {
                    let n = frame.code.code[frame.ip];
                    frame.ip += 1; // skips the operand byte, handled below
                    let arity = n as usize;
                    let mut args = Vec::with_capacity(arity);
                    for _ in 0..arity {
                        args.push(frame.pop());
                    }
                    args.reverse();
                    let callee = frame.pop();
                    frame.defers.push((callee, args));
                }
                Op::Print => {
                    let v = frame.pop();
                    self.output.push(format!("[DUMP] {}", v));
                }
                Op::Trace => {
                    let v = frame.pop();
                    self.output.push(format!("[TRACE] {:?}", v));
                }
                Op::NewArray => {
                    let n = match frame.pop() { Value::I64(n) => n as usize, _ => 0 };
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n { items.push(frame.pop()); }
                    items.reverse();
                    frame.push(Value::Array(Arc::from(items)));
                }
                Op::NewTuple => {
                    let n = match frame.pop() { Value::I64(n) => n as usize, _ => 0 };
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n { items.push(frame.pop()); }
                    items.reverse();
                    frame.push(Value::Tuple(Arc::from(items)));
                }
                Op::NewMap => {
                    let n = match frame.pop() { Value::I64(n) => n as usize, _ => 0 };
                    let mut m = HashMap::new();
                    for _ in 0..n {
                        let v = frame.pop();
                        let k = frame.pop();
                        m.insert(k.to_string(), v);
                    }
                    frame.push(Value::Map(Arc::from(m)));
                }
                Op::IndexGet => {
                    let idx = frame.pop();
                    let obj = frame.pop();
                    let obj = unwrap_vm_evidence(&obj);
                    match (&obj, &idx) {
                        (Value::Array(a), Value::I64(i)) => {
                            frame.push(a.get(*i as usize).cloned().unwrap_or(Value::Nil));
                        }
                        (Value::Tuple(t), Value::I64(i)) => {
                            frame.push(t.get(*i as usize).cloned().unwrap_or(Value::Nil));
                        }
                        (Value::String(s), Value::I64(i)) => {
                            frame.push(s.chars().nth(*i as usize).map(|c| Value::String(Arc::from(c.to_string().as_str()))).unwrap_or(Value::Nil));
                        }
                        (Value::Map(m), Value::String(k)) => {
                            frame.push(m.get(k.as_ref()).cloned().unwrap_or(Value::Nil));
                        }
                        (Value::Mmap(h), Value::I64(i)) => {
                            let data = h.as_slice();
                            frame.push(Value::I64(data.get(*i as usize).copied().unwrap_or(0) as i64));
                        }
                        (Value::MmapSlice(h, off, n), Value::I64(i)) => {
                            let i = *i as usize;
                            let b = if i < *n { h.as_slice()[off + i] as i64 } else { 0 };
                            frame.push(Value::I64(b));
                        }
                        (Value::Bytes(b), Value::I64(i)) => {
                            frame.push(Value::I64(b.get(*i as usize).copied().unwrap_or(0) as i64));
                        }
                        _ => { frame.push(Value::Nil); }
                    }
                }
                Op::FieldGet => {
                    let field = frame.pop();
                    let obj = frame.pop();
                    // Transparently unwrap an evidence tag for field access.
                    let obj = unwrap_vm_evidence(&obj);
                    match (&obj, &field) {
                        (Value::Map(m), Value::String(k)) => {
                            frame.push(m.get(k.as_ref()).cloned().unwrap_or(Value::Nil));
                        }
                        (Value::Struct { fields, .. }, Value::String(k)) => {
                            frame.push(fields.get(k.as_ref()).cloned().unwrap_or(Value::Nil));
                        }
                        (Value::Tuple(t), Value::String(k)) => {
                            if let Ok(i) = k.parse::<usize>() {
                                frame.push(t.get(i).cloned().unwrap_or(Value::Nil));
                            } else {
                                frame.push(Value::Nil);
                            }
                        }
                        _ => { frame.push(Value::Nil); }
                    }
                }
                Op::Closure => {}
                Op::GetUpvalue | Op::SetUpvalue => {}
                Op::LoopBegin | Op::LoopEnd => {}
                Op::FFICall => {
                    let args_val = frame.pop();
                    let symbol = match frame.pop() {
                        Value::String(s) => s.to_string(),
                        other => return Err(format!("ffi: symbol must be a string, got {}", other.type_name())),
                    };
                    let lib = match frame.pop() {
                        Value::ForeignLib(h) => h,
                        other => return Err(format!("ffi: not a library: {}", other.type_name())),
                    };
                    let c_args: Vec<Value> = match args_val {
                        Value::Array(a) => a.to_vec(),
                        Value::Nil => Vec::new(),
                        other => return Err(format!("ffi: args must be an array, got {}", other.type_name())),
                    };
                    let (marshalled, _g) = marshal_vm_args(&c_args)?;
                    let addr = { let h = lib.lock().unwrap(); rak_stdlib::ffi::sym_addr(&h, &symbol)? };
                    let ret = unsafe { rak_stdlib::ffi::call_int(addr, &marshalled) };
                    frame.push(Value::I64(ret as i64));
                }
                Op::FFIClose => {
                    let _ = frame.pop();
                    frame.push(Value::Nil);
                }
                Op::Await => {
                    let v = frame.pop();
                    let resolved = match v {
                        Value::Future(h) => {
                            let state = std::mem::replace(&mut *h.state.lock().unwrap(), VmFutureState::Polled);
                            match state {
                                VmFutureState::Ready(v) => v,
                                VmFutureState::Pending(jh) => {
                                    let joined = crate::async_rt::runtime().block_on(async { jh.await })
                                        .map_err(|e| format!("await: task failed: {}", e))?;
                                    *h.state.lock().unwrap() = VmFutureState::Ready(joined.clone());
                                    joined
                                }
                                VmFutureState::Polled => return Err("await: future already polled".to_string()),
                            }
                        }
                        other => other,
                    };
                    frame.push(resolved);
                }
                Op::BuildModule => {
                    let n = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let mut m = HashMap::new();
                    for _ in 0..n {
                        let v = frame.pop();
                        let k = match frame.pop() {
                            Value::String(s) => s.to_string(),
                            _ => return Err("BuildModule: name must be a string".to_string()),
                        };
                        m.insert(k, v);
                    }
                    frame.push(Value::Map(Arc::from(m)));
                }
                Op::Try => {
                    let handler = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    frame.catches.push(CatchFrame {
                        handler_offset: handler,
                        stack_len: frame.stack.len(),
                        locals_len: frame.locals.len(),
                        defers_len: frame.defers.len(),
                    });
                }
                Op::CallMethod => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let argc = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let method = match &frame.code.constants[ci] {
                        Value::String(s) => s.to_string(),
                        _ => return Err("bad method name constant".to_string()),
                    };
                    let mut args: Vec<Value> = (0..argc).map(|_| frame.pop()).collect();
                    args.reverse();
                    let receiver = frame.pop();
                    self.call_method(frame, receiver, method, args)?;
                }
                Op::StructNew => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let n = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let name = match &frame.code.constants[ci] {
                        Value::String(s) => s.to_string(),
                        _ => return Err("bad struct name constant".to_string()),
                    };
                    let mut fields = HashMap::new();
                    for _ in 0..n {
                        let v = frame.pop();
                        let k = match frame.pop() {
                            Value::String(s) => s.to_string(),
                            _ => return Err("StructNew: field name must be a string".to_string()),
                        };
                        fields.insert(k, v);
                    }
                    frame.push(Value::Struct { name: Arc::from(name.as_str()), fields: Arc::from(fields) });
                }
                Op::MatchPat => {
                    let di = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let bi = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let bindnames = match &frame.code.constants[bi] {
                        Value::Array(a) => a.clone(),
                        _ => return Err("MatchPat: bad bind-names constant".to_string()),
                    };
                    let desc = match &frame.code.constants[di] {
                        Value::Array(a) => Value::Array(a.clone()),
                        _ => return Err("MatchPat: bad descriptor constant".to_string()),
                    };
                    // Compiler pushed [scrutinee, descriptor, bind-names] then
                    // MatchPat di bi; drop the two descriptor values (already
                    // read via their constant indices) and take the scrutinee.
                    let _bindnames_val = frame.pop();
                    let _desc_val = frame.pop();
                    let scrutinee = frame.pop();
                    let mut bounds: Vec<(String, Value)> = Vec::new();
                    match vm_pattern_match(&scrutinee, &desc, &mut bounds) {
                        Ok(true) => {
                            // Emit bound values in the descriptor's bind order.
                            let out: Vec<Value> = bindnames
                                .iter()
                                .map(|n| {
                                    let nm = n.to_string();
                                    bounds.iter().find(|(k, _)| *k == nm).map(|(_, v)| v.clone()).unwrap_or(Value::Nil)
                                })
                                .collect();
                            // A truthy indicator: a bare `true` when there are no
                            // binds (an empty array would be falsy in Rak).
                            if out.is_empty() {
                                frame.push(Value::Bool(true));
                            } else {
                                frame.push(Value::Array(Arc::from(out)));
                            }
                        }
                        Ok(false) => frame.push(Value::Nil),
                        Err(e) => return Err(e),
                    }
                }
                Op::IndexSet => {
                    let val = frame.pop();
                    let idx = frame.pop();
                    let mut obj = frame.pop();
                    vm_index_set(&mut obj, &idx, val)?;
                    frame.push(obj);
                }
                Op::FieldSet => {
                    let fi = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let field = match &frame.code.constants[fi] {
                        Value::String(s) => s.to_string(),
                        _ => return Err("FieldSet: bad field-name constant".to_string()),
                    };
                    let val = frame.pop();
                    let mut obj = frame.pop();
                    vm_field_set(&mut obj, &field, val)?;
                    frame.push(obj);
                }
                Op::CatchEnd => {
                    frame.catches.pop();
                }
                Op::Throw => {
                    // Raise: pop the raised value; an existing structured
                    // `Value::Error` is preserved, anything else is wrapped as
                    // a User-kind error (mirrors the interpreter's `raise`).
                    let v = frame.pop();
                    let msg = v.to_string();
                    self.pending_error = Some(match v {
                        Value::Error(_) => v,
                        other => Value::Error(Arc::new(
                            crate::ErrorInfo::new(other.to_string()).with_kind(crate::ErrorKind::User),
                        )),
                    });
                    return Err(msg);
                }
                Op::TryUnwrap => {
                    // `expr?` — unwrap Result/Option or raise.
                    let v = frame.pop();
                    let unwrapped: Option<Value> = match &v {
                        Value::Result(ok, err) => {
                            if let Some(e) = err {
                                self.pending_error = Some((**e).clone());
                                None
                            } else {
                                ok.as_deref().cloned().or(Some(Value::Nil))
                            }
                        }
                        Value::Option(opt) => match opt {
                            Some(v) => Some((**v).clone()),
                            None => {
                                self.pending_error = Some(Value::String(Arc::from("None")));
                                None
                            }
                        },
                        other => Some(other.clone()),
                    };
                    match unwrapped {
                        Some(inner) => frame.push(inner),
                        None => {
                            let msg = self.pending_error.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "None".to_string());
                            return Err(msg);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Invoke a function value (native fn or closure) with the given args and
    /// push the result onto `frame.stack`. Shared by `Op::Call` and defer.
    fn call_value(&mut self, frame: &mut Frame, callee: Value, mut args: Vec<Value>) -> Result<(), String> {
        match callee {
            Value::NativeFn(name, f) => {
                let result = f(&args).map_err(|e| format!("{}: {}", name, e))?;
                frame.push(result);
            }
            Value::Closure { code, nparams, name } => {
                if self.debug_enabled {
                    self.debug_callstack.push(name.to_string());
                }
                for _ in args.len()..nparams {
                    args.push(Value::Nil);
                }
                let mut sub = Frame {
                    code: &code,
                    ip: 0,
                    stack: Vec::new(),
                    locals: Vec::with_capacity(nparams),
                    defers: Vec::new(),
                    catches: Vec::new(),
                };
                for i in 0..nparams {
                    sub.locals.push(args.get(i).cloned().unwrap_or(Value::Nil));
                }
                self.exec_frame(&mut sub)?;
                if self.debug_enabled {
                    self.debug_callstack.pop();
                }
                frame.push(sub.stack.pop().unwrap_or(Value::Nil));
            }
            _ => return Err("cannot call non-function".to_string()),
        }
        Ok(())
    }

    /// Dispatch `receiver.method(args...)` (mirrors the interpreter's
    /// `eval_call` method branch): regex natives, baked `impl` methods
    /// (`__method_<type>_<name>` globals), then a field holding a callable.
    fn call_method(&mut self, frame: &mut Frame, receiver: Value, method: String, args: Vec<Value>) -> Result<(), String> {
        let recv = unwrap_vm_evidence(&receiver);
        if let Value::Regex(_) = &recv {
            return self.call_regex_method_vm(frame, &recv, &method, args);
        }
        // Structs/enums dispatch on their type *name* (mirrors the
        // interpreter, where `Value::Struct.type_name()` returns the name).
        let tn = match &recv {
            Value::Struct { name, .. } => name.to_string(),
            Value::Enum { name, .. } => name.to_string(),
            other => other.type_name().to_string(),
        };
        let key = format!("__method_{}_{}", tn, method);
        if let Some(f) = self.globals.get(&key).cloned() {
            let mut all = Vec::with_capacity(args.len() + 1);
            all.push(receiver);
            all.extend(args);
            return self.call_value(frame, f, all);
        }
        // Fallback: a field that itself holds a callable (modules, etc.).
        let field = match &recv {
            Value::Map(m) => m.get(&method).cloned(),
            Value::Struct { fields, .. } => fields.get(&method).cloned(),
            _ => None,
        };
        if let Some(f) = field {
            if matches!(f, Value::Closure { .. } | Value::NativeFn(..)) {
                return self.call_value(frame, f, args);
            }
        }
        Err(format!("No method '{}' on {}", method, tn))
    }

    /// Regex method dispatch on the VM (`re.match/hay`, `find`, `find_all`,
    /// `replace`) — mirrors the interpreter's `call_regex_method`.
    fn call_regex_method_vm(&mut self, frame: &mut Frame, re_val: &Value, method: &str, args: Vec<Value>) -> Result<(), String> {
        let re = match re_val {
            Value::Regex(r) => r.clone(),
            _ => return Err("not a regex".to_string()),
        };
        let hay = native_str(args.first());
        let result = match method {
            "match" | "is_match" => Value::Bool(re.re.is_match(&hay)),
            "find" => re.re.find(&hay).map(|m| Value::String(Arc::from(m.as_str()))).unwrap_or(Value::Nil),
            "find_all" => Value::Array(Arc::from(
                re.re.find_iter(&hay).map(|m| Value::String(Arc::from(m.as_str()))).collect::<Vec<_>>(),
            )),
            "replace" | "replace_all" => {
                let rep = native_str(args.get(1));
                Value::String(Arc::from(re.re.replace_all(&hay, rep.as_str()).into_owned().as_str()))
            }
            _ => return Err(format!("regex has no method '{}'", method)),
        };
        let _ = frame;
        frame.push(result);
        Ok(())
    }

    fn binop_arith(&mut self, frame: &mut Frame, op: BinArith) -> Result<(), String> {
        let r = frame.pop();
        let l = frame.pop();
        let lnum = is_numeric_vm(&l);
        let rnum = is_numeric_vm(&r);
        if lnum && rnum {
            use Value::*;
            let push = |frame: &mut Frame, v: Value| frame.push(v);
            match (l, r) {
                (I64(a), I64(b)) => match op {
                    BinArith::Div => {
                        if b == 0 {
                            return Err("div by zero".to_string());
                        }
                        push(frame, I64(a / b))
                    }
                    BinArith::Rem => {
                        if b == 0 {
                            return Err("rem by zero".to_string());
                        }
                        push(frame, I64(a % b))
                    }
                    BinArith::Add => push(frame, I64(a.wrapping_add(b))),
                    BinArith::Sub => push(frame, I64(a.wrapping_sub(b))),
                    BinArith::Mul => push(frame, I64(a.wrapping_mul(b))),
                },
                (F64(a), F64(b)) => push(frame, F64(match op {
                    BinArith::Add => a + b,
                    BinArith::Sub => a - b,
                    BinArith::Mul => a * b,
                    BinArith::Div => a / b,
                    BinArith::Rem => a % b,
                })),
                (a, b) => self.numeric_fallback(frame, &a, &b, op)?,
            }
            Ok(())
        } else {
            let key = format!("__method_{}_{}", dispatch_type(&l), op.suffix());
            if let Some(f) = self.globals.get(&key).cloned() {
                self.call_value(frame, f, vec![l, r])
            } else {
                self.numeric_fallback(frame, &l, &r, op)
            }
        }
    }

    /// Lenient numeric coercion for mixed/unknown operands (matches the original
    /// `bin_int` `(a, b)` arm).
    #[allow(clippy::float_cmp)]
    fn numeric_fallback(&mut self, frame: &mut Frame, l: &Value, r: &Value, op: BinArith) -> Result<(), String> {
        let av = l.as_f64().unwrap_or(0.0);
        let bv = r.as_f64().unwrap_or(0.0);
        frame.push(Value::F64(match op {
            BinArith::Add => av + bv,
            BinArith::Sub => av - bv,
            BinArith::Mul => av * bv,
            BinArith::Div => av / bv,
            BinArith::Rem => av % bv,
        }));
        Ok(())
    }

    fn binop_neg(&mut self, frame: &mut Frame) -> Result<(), String> {
        let v = frame.pop();
        match v {
            Value::I64(i) => {
                frame.push(Value::I64(-i));
                Ok(())
            }
            Value::F64(f) => {
                frame.push(Value::F64(-f));
                Ok(())
            }
            other => {
                let key = format!("__method_{}_neg", dispatch_type(&other));
                if let Some(f) = self.globals.get(&key).cloned() {
                    self.call_value(frame, f, vec![other])
                } else {
                    frame.push(Value::F64(-(other.as_f64().unwrap_or(0.0))));
                    Ok(())
                }
            }
        }
    }

    fn binop_compare(&mut self, frame: &mut Frame, op: CompareOp) -> Result<(), String> {
        let r = frame.pop();
        let l = frame.pop();
        let lnum = is_numeric_vm(&l);
        let rnum = is_numeric_vm(&r);
        let base = if lnum { "" } else { &dispatch_type(&l) };
        if !lnum && !rnum {
            // Try a user-defined comparison trait first.
            let key = format!("__method_{}_{}", base, op.suffix());
            if let Some(f) = self.globals.get(&key).cloned() {
                return self.call_value(frame, f, vec![l, r]);
            }
        }
        // Fall back to the default behaviour.
        let b = match op {
            CompareOp::Eq => l == r,
            CompareOp::NotEq => l != r,
            CompareOp::Lt => l.as_i64().unwrap_or(0) < r.as_i64().unwrap_or(0),
            CompareOp::Gt => l.as_i64().unwrap_or(0) > r.as_i64().unwrap_or(0),
            CompareOp::LtEq => l.as_i64().unwrap_or(0) <= r.as_i64().unwrap_or(0),
            CompareOp::GtEq => l.as_i64().unwrap_or(0) >= r.as_i64().unwrap_or(0),
        };
        frame.push(Value::Bool(b));
        Ok(())
    }

/// Run a frame's registered deferred calls in LIFO order (innermost
    /// `defer` runs last), invoking each with its captured args. Results are
    /// discarded as in the interpreter.
    fn run_frame_defers(&mut self, frame: &mut Frame) -> Result<(), String> {
        while let Some((callee, args)) = frame.defers.pop() {
            self.call_value(frame, callee, args)?;
        }
        Ok(())
    }
}

struct Frame<'a> {
    code: &'a Chunk,
    ip: usize,
    stack: Vec<Value>,
    locals: Vec<Value>,
    /// Deferred calls (callee value + pre-evaluated args) registered by
    /// `defer f(...)`, run in LIFO order when this frame returns.
    defers: Vec<(Value, Vec<Value>)>,
    /// Active `try` regions (most recent last), unwound on error.
    catches: Vec<CatchFrame>,
}

impl<'a> Frame<'a> {
    fn push(&mut self, v: Value) { self.stack.push(v); }
    fn pop(&mut self) -> Value { self.stack.pop().unwrap_or(Value::Nil) }
    fn peek(&self) -> &Value { self.stack.last().unwrap_or(&Value::Nil) }
}

fn bin_int(frame: &mut Frame, fi: impl Fn(i64, i64) -> i64, ff: impl Fn(f64, f64) -> f64) {
    let r = frame.pop();
    let l = frame.pop();
    match (l, r) {
        (Value::I64(a), Value::I64(b)) => frame.push(Value::I64(fi(a, b))),
        (Value::F64(a), Value::F64(b)) => frame.push(Value::F64(ff(a, b))),
        (a, b) => {
            let av = a.as_f64().unwrap_or(0.0);
            let bv = b.as_f64().unwrap_or(0.0);
            frame.push(Value::F64(ff(av, bv)));
        }
    }
}

fn bin_float(frame: &mut Frame, f: impl Fn(f64, f64) -> f64) {
    let r = frame.pop();
    let l = frame.pop();
    frame.push(Value::F64(f(l.as_f64().unwrap_or(0.0), r.as_f64().unwrap_or(0.0))));
}

fn native_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Bytes(b)) => String::from_utf8_lossy(b).to_string(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

fn native_bytes(v: Option<&Value>) -> Vec<u8> {
    match v {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::String(s)) => s.bytes().collect(),
        Some(Value::Hex(h, _)) => h.to_le_bytes().to_vec(),
        Some(Value::I64(i)) => i.to_le_bytes().to_vec(),
        Some(v) => v.to_string().into_bytes(),
        None => Vec::new(),
    }
}

/// Convert a VM value to structured-log JSON fields. Mirrors the interpreter's
/// `value_to_json`.
fn vm_log(level: &rak_stdlib::log::Level, args: &[Value]) -> Result<Value, String> {
    fn conv(v: &Value) -> rak_stdlib::log::Json {
        match v {
            Value::String(s) => rak_stdlib::log::Json::Str(s.to_string()),
            Value::Bytes(b) => rak_stdlib::log::Json::Str(String::from_utf8_lossy(b).into_owned()),
            Value::Bool(b) => rak_stdlib::log::Json::Bool(*b),
            Value::Nil => rak_stdlib::log::Json::Nil,
            Value::I64(n) => rak_stdlib::log::Json::Num(*n as f64),
            Value::Hex(h, _) => rak_stdlib::log::Json::Num(*h as f64),
            Value::F64(f) => rak_stdlib::log::Json::Num(*f),
            Value::Array(a) => rak_stdlib::log::Json::Arr(a.iter().map(conv).collect()),
            Value::Map(m) => {
                let mut obj = std::collections::HashMap::new();
                for (k, v) in m.iter() {
                    obj.insert(k.clone(), conv(v));
                }
                rak_stdlib::log::Json::Obj(obj)
            }
            other => rak_stdlib::log::Json::Str(other.to_string()),
        }
    }
    let key = native_str(args.first());
    let fields: std::collections::HashMap<String, rak_stdlib::log::Json> = match args.get(1) {
        Some(Value::Map(m)) => {
            let mut out = std::collections::HashMap::new();
            for (k, v) in m.iter() {
                out.insert(k.clone(), conv(v));
            }
            out
        }
        Some(other) => {
            let mut out = std::collections::HashMap::new();
            out.insert("value".to_string(), conv(other));
            out
        }
        None => std::collections::HashMap::new(),
    };
    rak_stdlib::log::log(level.clone(), &key, &fields)?;
    Ok(Value::Nil)
}

/// Wrap a String map as a VM `Value::Map`.
fn vm_map(map: std::collections::HashMap<String, String>) -> Value {
    let mut m = std::collections::HashMap::new();
    for (k, v) in map {
        m.insert(k, Value::String(Arc::from(v.as_str())));
    }
    Value::Map(Arc::from(m))
}

/// Recursively unwrap a `Value::Evidence` wrapper to its inner value.
fn unwrap_vm_evidence(v: &Value) -> Value {
    match v {
        Value::Evidence { inner, .. } => unwrap_vm_evidence(inner),
        other => other.clone(),
    }
}

/// Descriptor tag helpers for [pattern] matching. The descriptor is a
/// `Value::Array` with a string tag at index 0; `arg(i)` reads `desc[i + 1]`.
fn pat_tag(d: &[Value]) -> &str {
    match d.first() {
        Some(Value::String(s)) => s.as_ref(),
        _ => "",
    }
}

fn pat_arg<'a>(d: &'a [Value], i: usize) -> Option<&'a Value> {
    d.get(i + 1)
}

/// Structural pattern match against a descriptor (mirrors the interpreter's
/// `pattern_matches`). Returns whether the value matched; on success, `bounds`
/// is filled with `(name, value)` pairs from `["bind", name, sub]` nodes.
fn vm_pattern_match(v: &Value, d: &Value, bounds: &mut Vec<(String, Value)>) -> Result<bool, String> {
    let d = match d {
        Value::Array(a) => a,
        _ => return Err("MatchPat: descriptor must be an array".to_string()),
    };
    match pat_tag(d) {
        "wild" => Ok(true),
        "bind" => {
            let name = match pat_arg(d, 0) {
                Some(Value::String(s)) => s.to_string(),
                _ => return Err("MatchPat: bind name must be a string".to_string()),
            };
            let sub = pat_arg(d, 1).cloned().unwrap_or_else(|| Value::Array(Arc::from(vec![Value::String(Arc::from("wild"))])));
            if vm_pattern_match(v, &sub, bounds)? {
                bounds.push((name, v.clone()));
                Ok(true)
            } else {
                Ok(false)
            }
        }
        "int" => {
            let want = pat_arg(d, 0).and_then(|x| x.as_i64());
            Ok(want.is_some() && v.as_i64() == want)
        }
        "hex" => {
            let want = pat_arg(d, 0).and_then(|x| x.as_u64());
            Ok(want.is_some() && v.as_u64() == want)
        }
        "string" => {
            let want = match pat_arg(d, 0) {
                Some(Value::String(s)) => s.as_ref(),
                _ => "",
            };
            Ok(matches!(v, Value::String(s) if s.as_ref() == want))
        }
        "bool" => {
            let want = matches!(pat_arg(d, 0), Some(Value::Bool(true)));
            Ok(matches!(v, Value::Bool(b) if *b == want))
        }
        "nil" => Ok(matches!(v, Value::Nil)),
        "byte" => {
            // `Pattern::Byte` matches a 1-byte `Bytes` or an equal `Int`.
            let want = pat_arg(d, 0).and_then(|x| x.as_i64()).unwrap_or(0) as u8;
            Ok(match v {
                Value::Bytes(b) => b.len() == 1 && b[0] == want,
                _ => v.as_i64() == Some(want as i64),
            })
        }
        "bytes" => {
            let elems = match pat_arg(d, 0) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Err("MatchPat: bytes elems not an array".to_string()),
            };
            let rest = matches!(pat_arg(d, 1), Some(Value::Bool(true)));
            let data: Vec<u8> = match v {
                Value::Bytes(b) => b.to_vec(),
                Value::MmapSlice(h, off, n) => h.as_slice()[*off..off + n].to_vec(),
                Value::Mmap(h) => h.as_slice().to_vec(),
                _ => return Ok(false),
            };
            let mut vi = 0usize;
            let mut pi = 0usize;
            while pi < elems.len() {
                let b = elems[pi].as_i64().unwrap_or(0) as u8;
                if vi >= data.len() || data[vi] != b {
                    return Ok(false);
                }
                vi += 1;
                pi += 1;
            }
            Ok(if rest { true } else { vi == data.len() })
        }
        "tuple" => {
            let subs = match pat_arg(d, 0) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Err("MatchPat: tuple subs not an array".to_string()),
            };
            match v {
                Value::Tuple(t) => {
                    if t.len() != subs.len() {
                        return Ok(false);
                    }
                    for (i, sub) in subs.iter().enumerate() {
                        if !vm_pattern_match(&t[i], sub, bounds)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        "array" => {
            let subs = match pat_arg(d, 0) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Err("MatchPat: array subs not an array".to_string()),
            };
            match v {
                Value::Array(a) => {
                    if a.len() != subs.len() {
                        return Ok(false);
                    }
                    for (i, sub) in subs.iter().enumerate() {
                        if !vm_pattern_match(&a[i], sub, bounds)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                // Exact-length byte-slice matching via an array pattern.
                Value::Bytes(b) => {
                    if b.len() != subs.len() {
                        return Ok(false);
                    }
                    for (i, sub) in subs.iter().enumerate() {
                        let ok = match (sub, b[i]) {
                            (Value::Array(e), byte) => has_byte_literal(e, byte),
                            _ => false,
                        };
                        if !ok {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        "struct" => {
            let want_name = match pat_arg(d, 0) {
                Some(Value::String(s)) => s.as_ref(),
                _ => "",
            };
            let fields = match pat_arg(d, 1) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Err("MatchPat: struct fields not an array".to_string()),
            };
            match v {
                Value::Struct { name, fields: fmap } => {
                    if name.as_ref() != want_name {
                        return Ok(false);
                    }
                    let mut i = 0;
                    while i + 1 < fields.len() {
                        let fname = match &fields[i] {
                            Value::String(s) => s.as_ref(),
                            _ => return Err("MatchPat: struct field name not a string".to_string()),
                        };
                        let sub = &fields[i + 1];
                        match fmap.get(fname) {
                            Some(fv) => {
                                if !vm_pattern_match(fv, sub, bounds)? {
                                    return Ok(false);
                                }
                            }
                            None => return Ok(false),
                        }
                        i += 2;
                    }
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        "enum" => {
            let want_enum = match pat_arg(d, 0) {
                Some(Value::String(s)) => s.as_ref(),
                _ => "",
            };
            let want_var = match pat_arg(d, 1) {
                Some(Value::String(s)) => s.as_ref(),
                _ => "",
            };
            let subs = match pat_arg(d, 2) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Err("MatchPat: enum subs not an array".to_string()),
            };
            match v {
                Value::Enum { name, variant, data } => {
                    if name.as_ref() != want_enum || variant.as_ref() != want_var || data.len() != subs.len() {
                        return Ok(false);
                    }
                    for (i, sub) in subs.iter().enumerate() {
                        if !vm_pattern_match(&data[i], sub, bounds)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        "range" => {
            let lo = pat_arg(d, 0);
            let hi = pat_arg(d, 1);
            let lo_val = bounded_const(lo);
            let hi_val = bounded_const(hi);
            match (lo_val, hi_val, v.as_i64()) {
                (Some(l), Some(h), Some(x)) => Ok(x >= l && x <= h),
                _ => Ok(false),
            }
        }
        "or" => {
            let opts = match pat_arg(d, 0) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Err("MatchPat: or subs not an array".to_string()),
            };
            for o in opts.iter() {
                if vm_pattern_match(v, o, bounds)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        "some" => match v {
            Value::Option(Some(inner)) => vm_pattern_match(inner, pat_arg(d, 0).ok_or("MatchPat: some needs sub")?, bounds),
            _ => Ok(false),
        },
        "none" => Ok(matches!(v, Value::Option(None))),
        "ok" => match v {
            Value::Result(Some(inner), _) => vm_pattern_match(inner, pat_arg(d, 0).ok_or("MatchPat: ok needs sub")?, bounds),
            _ => Ok(false),
        },
        "err" => match v {
            Value::Result(_, Some(inner)) => vm_pattern_match(inner, pat_arg(d, 0).ok_or("MatchPat: err needs sub")?, bounds),
            _ => Ok(false),
        },
        _ => Err(format!("MatchPat: unknown pattern tag '{}'", pat_tag(d))),
    }
}

/// For `["byte"|"int"|"hex", n]` bound inside a `Range` descriptor.
fn bounded_const(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::I64(i)) => Some(*i),
        Some(Value::U64(u)) => Some(*u as i64),
        Some(Value::Array(e)) => match pat_arg(e, 0) {
            Some(Value::I64(i)) => Some(*i),
            Some(Value::U64(u)) => Some(*u as i64),
            _ => None,
        },
        _ => None,
    }
}

/// True if a byte-literal descriptor node matches the given byte (used by the
/// exact-length `array`-pattern over `bytes`).
fn has_byte_literal(e: &[Value], byte: u8) -> bool {
    match e.first() {
        Some(Value::String(s)) => match s.as_ref() {
            "hex" => e.get(1).and_then(|x| x.as_u64()) == Some(byte as u64),
            "int" => e.get(1).and_then(|x| x.as_i64()) == Some(byte as i64),
            "byte" => e.get(1).and_then(|x| x.as_i64()) == Some(byte as i64),
            _ => false,
        },
        _ => false,
    }
}

/// `obj[idx] = value` — copy-on-write mutation (fast path when `obj` is the
/// only reference). Mirrors the interpreter's `Expr::IndexAssign`.
fn vm_index_set(obj: &mut Value, idx: &Value, val: Value) -> Result<(), String> {
    match obj {
        Value::Array(a) => {
            let i = idx.as_i64().ok_or_else(|| "index-assign: index must be an int".to_string())? as usize;
            let m = Arc::make_mut(a);
            if i >= m.len() {
                return Err(format!("index-assign: index {} out of bounds (len {})", i, m.len()));
            }
            m[i] = val;
            Ok(())
        }
        Value::Map(m) => {
            let k = idx.to_string();
            Arc::make_mut(m).insert(k, val);
            Ok(())
        }
        Value::Bytes(b) => {
            let i = idx.as_i64().ok_or_else(|| "index-assign: index must be an int".to_string())? as usize;
            let m = Arc::make_mut(b);
            if i >= m.len() {
                return Err(format!("index-assign: index {} out of bounds (len {})", i, m.len()));
            }
            m[i] = val.as_i64().ok_or_else(|| "index-assign: expected a byte".to_string())? as u8;
            Ok(())
        }
        other => Err(format!("cannot index-assign this value ({})", other.type_name())),
    }
}

/// `obj.field = value` — copy-on-write mutation for structs and maps.
fn vm_field_set(obj: &mut Value, field: &str, val: Value) -> Result<(), String> {
    match obj {
        Value::Map(m) => {
            Arc::make_mut(m).insert(field.to_string(), val);
            Ok(())
        }
        Value::Struct { fields, .. } => {
            let fm = Arc::make_mut(fields);
            if fm.contains_key(field) {
                fm.insert(field.to_string(), val);
                Ok(())
            } else {
                Err(format!("field-assign: field '{}' not found", field))
            }
        }
        other => Err(format!("cannot field-assign this value ({})", other.type_name())),
    }
}

/// Render a value's provenance chain as a VM map (mirrors the interpreter's
/// `provenance` builtin).
fn provenance_to_vm_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::Evidence { provenance, .. } => {
            let mut m: HashMap<String, Value> = HashMap::new();
            m.insert("tool".to_string(), Value::String(Arc::from(provenance.tool.as_str())));
            m.insert("target".to_string(), Value::String(Arc::from(provenance.target.as_str())));
            m.insert("ts".to_string(), Value::I64(provenance.ts as i64));
            if let Some(o) = provenance.raw_offset {
                m.insert("raw_offset".to_string(), Value::I64(o as i64));
            }
            if let Some(l) = provenance.raw_len {
                m.insert("raw_len".to_string(), Value::I64(l as i64));
            }
            if let Some(p) = &provenance.parent {
                m.insert("parent".to_string(), provenance_to_vm_value(&Value::Evidence {
                    inner: Box::new(Value::Nil),
                    provenance: p.clone(),
                })?);
            }
            Ok(Value::Map(Arc::from(m)))
        }
        _ => Ok(Value::Nil),
    }
}

/// Compile a regex pattern+flags into a shared `RegexValue` for the VM.
fn build_vm_regex(pattern: &str, flags: &str) -> Result<Arc<crate::value::RegexValue>, String> {
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
    Ok(Arc::new(crate::value::RegexValue {
        pattern: pattern.to_string(),
        flags: flags.to_string(),
        re,
    }))
}

/// Borrow the compiled regex from a `Value::Regex`, or build one from a string.
fn vm_regex(v: Option<&Value>) -> Result<regex::Regex, String> {
    match v {
        Some(Value::Regex(r)) => Ok(r.re.clone()),
        Some(Value::String(s)) => {
            let rv = build_vm_regex(s, "")?;
            Ok(rv.re.clone())
        }
        _ => Err("expected a regex or pattern string".to_string()),
    }
}

fn format_rak(fmt: &str, args: &[Value]) -> String {
    let mut result = String::new();
    let mut idx = 0;
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') { chars.next(); result.push('{'); continue; }
            let mut spec = String::new();
            while let Some(c2) = chars.next() { if c2 == '}' { break; } spec.push(c2); }
            if idx < args.len() {
                let a = &args[idx];
                if spec.contains(":04X") {
                    result.push_str(&format!("{:04X}", a.as_u64().unwrap_or(0)));
                } else if spec.contains(":08X") {
                    result.push_str(&format!("{:08X}", a.as_u64().unwrap_or(0)));
                } else if spec.contains('X') || spec.contains('x') {
                    result.push_str(&format!("{:X}", a.as_u64().unwrap_or(0)));
                } else {
                    result.push_str(&a.to_string());
                }
                idx += 1;
            }
        } else if c == '}' {
            if chars.peek() == Some(&'}') { chars.next(); result.push('}'); }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile_module;

    fn run(src: &str) -> Vec<String> {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let module = crate::parser::parse(&tokens, src).unwrap();
        let chunk = compile_module(&module).unwrap();
        let mut vm = Vm::new();
        vm.run(&chunk).unwrap()
    }

    #[test]
    fn test_vm_arith() {
        let out = run("dump 2 + 3 * 4");
        assert!(out.iter().any(|l| l.contains("[DUMP] 14")));
    }

    #[test]
    fn test_vm_mutability_rejects_immutable_assign() {
        let tokens = crate::lexer::tokenize("let imm = 10\nimm = 20").unwrap();
        let module = crate::parser::parse(&tokens, "let imm = 10\nimm = 20").unwrap();
        let err = compile_module(&module).unwrap_err();
        assert!(
            err.contains("cannot assign to immutable variable `imm`"),
            "got: {:?}",
            err
        );
    }

    #[test]
    fn test_vm_mutability_allows_mut_assign() {
        let out = run("let mut x = 10\nx = 20\ndump x");
        assert!(out.iter().any(|l| l.contains("[DUMP] 20")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_char_literal() {
        let out = run("dump 'A'\ndump '\\u{03B1}'");
        assert!(out.iter().any(|l| l.contains("[DUMP] 'A'")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 'α'")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_defer_lifo_order() {
        let out = run("fn a() { dump \"aaa\" }\nfn b() { dump \"bbb\" }\nfn f() { defer a()\ndefer b()\ndump \"body\" } f()");
        let body = out.iter().position(|l| l == "[DUMP] body").unwrap();
        let pos_a = out.iter().position(|l| l == "[DUMP] aaa").unwrap();
        let pos_b = out.iter().position(|l| l == "[DUMP] bbb").unwrap();
        assert!(body < pos_b && pos_b < pos_a, "got: {:?}", out);
    }

    #[test]
    fn test_vm_defer_preserves_return_value() {
        let out = run("fn cleanup() { dump \"clean\" }\nfn f(x: int) -> int { defer cleanup()\nreturn x * 2 } dump f(21)");
        assert!(out.iter().any(|l| l.contains("[DUMP] clean")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_generic_function_identity() {
        let out = run("fn identity<T>(value: T) -> T { return value }\ndump identity<int>(42)\ndump identity<string>(\"hello\")\ndump identity(99)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] hello")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 99")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_while_loop() {
        let out = run("let mut i = 0; let mut s = 0; while i < 10 { s = s + i; i = i + 1 } dump s");
        assert!(out.iter().any(|l| l.contains("[DUMP] 45")));
    }

    #[test]
    fn test_vm_function() {
        let out = run("fn add(a, b) { return a + b } dump add(3, 4)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")));
    }

    #[test]
    fn test_vm_for_range() {
        let out = run("let mut s = 0; for i in 1..100 { s = s + i } dump s");
        assert!(out.iter().any(|l| l.contains("[DUMP] 5050")));
    }

    #[test]
    fn test_vm_for_array() {
        let out = run("let mut s = 0; for x in [10, 20, 30] { s = s + x } dump s");
        assert!(out.iter().any(|l| l.contains("[DUMP] 60")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_fib() {
        let out = run("fn fib(n) { if n < 2 { return n } return fib(n - 1) + fib(n - 2) } dump fib(20)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 6765")));
    }

    #[test]
    fn test_vm_array_index() {
        let out = run("let a = [10, 20, 30] dump a[1]");
        assert!(out.iter().any(|l| l.contains("[DUMP] 20")));
    }

    #[test]
    fn test_vm_tuple() {
        let out = run("let t = (1, 2, 3) dump t.0");
        assert!(out.iter().any(|l| l.contains("[DUMP] 1")));
    }

    #[test]
    fn test_vm_map() {
        let out = run("let m = {x: 5, y: 7} dump m.y");
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")));
    }

    #[test]
    fn test_vm_interp() {
        let out = run("let n = 42 dump f\"n={n}\"");
        assert!(out.iter().any(|l| l.contains("[DUMP] n=42")));
    }

    #[test]
    fn test_vm_match() {
        let out = run("let x = 2; match x { 1 => { dump \"one\" }, 2 => { dump \"two\" }, _ => { dump \"other\" } }");
        assert!(out.iter().any(|l| l.contains("[DUMP] two")));
    }

    #[test]
    fn test_vm_pipeline() {
        // `|>` desugars to a call, so the VM runs it for free.
        let out = run("fn inc(n) { return n + 1 } fn dbl(n) { return n * 2 } dump 5 |> inc |> dbl");
        assert!(out.iter().any(|l| l.contains("[DUMP] 12")));
    }

    #[test]
    fn test_vm_regex_literal() {
        let out = run(r#"let re = /\d+/g; dump regex_match(re, "abc123")"#);
        assert!(out.iter().any(|l| l.contains("true")));
    }

    #[test]
    fn test_vm_regex_find_all() {
        let out = run(r#"let re = /[a-z]+/g; dump regex_find_all(re, "a1bc2def")"#);
        // Value::Display does not quote strings inside arrays.
        assert!(out.iter().any(|l| l.contains("[DUMP] [a, bc, def]")));
    }

    // --- FFI ---

    #[test]
    fn test_vm_ffi_extern_abs() {
        let out = run("extern \"C\" { fn abs(n: i32) -> i32 } dump abs(-42)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_ffi_alloc_write_read() {
        let out = run("let buf = ffi_alloc(4); ffi_write(buf, 0, 0x41); ffi_write(buf, 1, 0x00); dump ffi_read(buf, 0); dump ffi_cstr_to_string(buf)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 65")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] A")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_ffi_string_to_cstr_roundtrip() {
        let out = run("let cs = ffi_string_to_cstr(\"hello ffi\"); dump ffi_cstr_to_string(cs)");
        assert!(out.iter().any(|l| l.contains("[DUMP] hello ffi")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_ffi_ptr() {
        let out = run("let p = ffi_ptr(0xDEADBEEF); dump fmt(\"0x{:08X}\", p)");
        assert!(out.iter().any(|l| l.contains("0xDEADBEEF")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_ffi_void_return_is_nil() {
        let out = run("extern \"C\" { fn abs(n: i32) } let r = abs(0); dump r");
        assert!(out.iter().any(|l| l.contains("[DUMP] nil")), "got: {:?}", out);
    }

    // --- Memory-mapped files ---

    fn write_vm_mmap_sample(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("rak_mmap_vm_{}.bin", name));
        let bytes: [u8; 15] = [0xD4, 0xC3, 0xB2, 0xA1, 0x0A, b'G', b'E', b'T', b' ', 0x31, 0x0A, b'x', b'y', b'z', 0x0A];
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn test_vm_mmap_size_and_index() {
        let path = write_vm_mmap_sample("size");
        let src = format!(
            "let m = mmap_open(\"{}\", \"r\"); dump mmap_size(m); let s = mmap_slice(m, 0, 4); dump s[0]; dump s[3]",
            path.to_str().unwrap().replace('\\', "\\\\")
        );
        let out = run(&src);
        assert!(out.iter().any(|l| l.contains("[DUMP] 15")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 212")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 161")), "got: {:?}", out);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_vm_mmap_find_lines() {
        let path = write_vm_mmap_sample("find");
        let src = format!(
            "let m = mmap_open(\"{}\", \"r\"); dump mmap_find(m, \"GET\"); let lines = mmap_lines_off(m, \"\\n\"); dump len(lines)",
            path.to_str().unwrap().replace('\\', "\\\\")
        );
        let out = run(&src);
        assert!(out.iter().any(|l| l.contains("[DUMP] 5")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 3")), "got: {:?}", out);
        let _ = std::fs::remove_file(&path);
    }

    // --- Async ---

    #[test]
    fn test_vm_async_fn_await() {
        let out = run("async fn double(x) { return x * 2 } dump await double(21)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_async_tcp_probe() {
        let out = run("dump await tcp_probe(\"127.0.0.1\", 9999, 100)");
        assert!(out.iter().any(|l| l.contains("[DUMP] false")), "got: {:?}", out);
    }

    // --- Raw sockets / packet forging ---

    #[test]
    fn test_vm_net_raw_syn() {
        let out = run("let pkt = net_raw_tcp_syn(\"10.0.0.5\", \"10.0.0.10\", 12345, 80); dump len(pkt); dump pkt[0]; dump pkt[9]");
        assert!(out.iter().any(|l| l.contains("[DUMP] 40")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 69")), "got: {:?}", out); // 0x45
        assert!(out.iter().any(|l| l.contains("[DUMP] 6")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_net_raw_udp() {
        let out = run("let u = net_raw_udp(\"10.0.0.5\", \"10.0.0.10\", 1234, 53, b\"\"); dump len(u)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 8")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_net_raw_send_returns_result() {
        let out = run("let pkt = net_raw_tcp_syn(\"10.0.0.5\", \"10.0.0.10\", 12345, 80); dump net_raw_send(pkt)");
        assert!(out.iter().any(|l| l.contains("Ok(") || l.contains("Err(")), "got: {:?}", out);
    }

    // --- DNS ---

    #[test]
    fn test_vm_dns_build() {
        let out = run("let q = dns_build(\"example.com\", \"A\"); dump len(q); dump q[12]");
        assert!(out.iter().any(|l| l.contains("[DUMP] 29")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_pcap_open_returns_result() {
        let out = run("dump pcap_open(\"nope.pcap\")");
        assert!(out.iter().any(|l| l.contains("Err(")), "got: {:?}", out);
    }

    // --- Macros ---

    #[test]
    fn test_vm_macro_expr() {
        let out = run("macro add1(x: expr) { $x + 1 } dump add1!(41)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_macro_multi_arg_splice() {
        let out = run("macro add3(a: expr, b: expr, c: expr) { $a + $b + $c } dump add3!(10, 20, 30)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 60")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_macro_array_build() {
        let out = run("macro pair(a: expr, b: expr) { [$a, $b] } dump pair!(1, 2)");
        assert!(out.iter().any(|l| l.contains("[DUMP] [1, 2]")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_const_binding() {
        let out = run("const MAX = 256; dump MAX");
        assert!(out.iter().any(|l| l.contains("[DUMP] 256")), "got: {:?}", out);
    }

    // --- Imports & exports (VM) ---

    #[test]
    fn test_vm_import_whole_and_from() {
        let dir = std::env::temp_dir().join(format!("rak_vm_import_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.rak"), "pub let PI = 3.14\npub fn add(a, b) { return a + b }").unwrap();
        let src = "import m\ndump m.PI\ndump m.add(2, 3)\nfrom m import add as plus\ndump plus(10, 20)";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let module = crate::parser::parse(&tokens, src).unwrap();
        let chunk = crate::compiler::compile_module_in(&module, dir.to_string_lossy().as_ref()).unwrap();
        let mut vm = Vm::new();
        let out = vm.run(&chunk).unwrap();
        assert!(out.iter().any(|l| l.contains("[DUMP] 3.14")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 5")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 30")), "got: {:?}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_vm_import_star() {
        let dir = std::env::temp_dir().join(format!("rak_vm_star_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.rak"), "pub let X = 1\npub let Y = 2").unwrap();
        let src = "let X = 99\nfrom m import *\ndump X\ndump Y";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let module = crate::parser::parse(&tokens, src).unwrap();
        let chunk = crate::compiler::compile_module_in(&module, dir.to_string_lossy().as_ref()).unwrap();
        let mut vm = Vm::new();
        let out = vm.run(&chunk).unwrap();
        assert!(out.iter().any(|l| l.contains("[DUMP] 99")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 2")), "got: {:?}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Forensic Structs (binstruct) + evidence provenance (VM) ---

    #[test]
    fn test_vm_binstruct_decode() {
        let out = run(r#"binstruct Hdr { id: u16be, ver: u8, kind: u8, rest: rest }
let raw = b"\x12\x34\x01\x02hello"
let h = Hdr.decode(raw)
dump h.id
dump h.ver
dump h.kind
dump len(h.rest)"#);
        assert!(out.iter().any(|l| l == "[DUMP] 0x1234"), "id got: {:?}", out);
        assert!(out.iter().any(|l| l == "[DUMP] 0x01"), "ver got: {:?}", out);
        assert!(out.iter().any(|l| l == "[DUMP] 0x02"), "kind got: {:?}", out);
        assert!(out.iter().any(|l| l == "[DUMP] 5"), "rest len got: {:?}", out);
    }

    #[test]
    fn test_vm_binstruct_encode_roundtrip() {
        let out = run(r#"binstruct Hdr { id: u16be, n: u32le }
let raw = b"\x12\x34\x05\x00\x00\x00"
let h = Hdr.decode(raw)
let back = Hdr.encode(h)
dump back[0]
dump back[2]
let h2 = Hdr.decode(back)
dump h2.id"#);
        // back[0] = 0x12 (bytes index -> Int) ; back[2] = 5
        assert!(out.iter().any(|l| l == "[DUMP] 18"), "back[0] got: {:?}", out);
        assert!(out.iter().any(|l| l == "[DUMP] 5"), "back[2] got: {:?}", out);
        assert!(out.iter().any(|l| l == "[DUMP] 0x1234"), "h2.id got: {:?}", out);
    }

    #[test]
    fn test_vm_evidence_from_and_report() {
        let out = run(r#"let ip = evidence<string> from "93.184.216.34"
dump ip
dump strip_evidence(ip)
let r = report(ip)
dump r"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 93.184.216.34")), "ip got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("tool=manual")), "report got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("Sources")), "report got: {:?}", out);
    }

    // --- try/catch + `?` on the VM (Part 7A.1) ---

    #[test]
    fn test_vm_try_catch_raise() {
        let out = run(r#"try {
    raise "boom"
} catch e {
    dump e
}
dump "after""#);
        assert!(out.iter().any(|l| l.contains("[DUMP] boom")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] after")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_try_catch_structured() {
        let out = run(r#"try {
    raise "boom"
} catch e {
    dump err_kind(e)
    dump err_message(e)
}"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] user")), "kind got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] boom")), "message got: {:?}", out);
    }

    #[test]
    fn test_vm_try_catches_runtime_error() {
        let out = run(r#"try {
    let x = 1 / 0
    dump "never"
} catch e {
    dump "caught"
}
dump "done""#);
        assert!(out.iter().any(|l| l.contains("[DUMP] caught")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] done")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_try_nested_and_skip() {
        let out = run(r#"try {
    dump "no-raise"
} catch e {
    dump "should-not-run"
}
dump "ok""#);
        assert!(out.iter().any(|l| l.contains("[DUMP] no-raise")), "got: {:?}", out);
        assert!(!out.iter().any(|l| l.contains("should-not-run")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] ok")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_try_catches_caller_error() {
        let out = run(r#"fn inner() {
    raise "deep"
}
try {
    inner()
} catch e {
    dump e
}"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] deep")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_try_defer_runs_in_body() {
        let out = run(r#"fn cleanup() { dump "clean" }
try {
    defer cleanup()
    raise "x"
} catch e {
    dump "caught"
}"#);
        let clean = out.iter().position(|l| l.contains("[DUMP] clean")).unwrap();
        let caught = out.iter().position(|l| l.contains("[DUMP] caught")).unwrap();
        assert!(clean < caught, "defers must run before the handler: {:?}", out);
    }

    #[test]
    fn test_vm_raise_propagates_without_catch() {
        let tokens = crate::lexer::tokenize("raise \"no handler\"").unwrap();
        let module = crate::parser::parse(&tokens, "raise \"no handler\"").unwrap();
        let chunk = compile_module(&module).unwrap();
        let mut vm = Vm::new();
        let err = vm.run(&chunk).unwrap_err();
        assert!(err.contains("no handler"), "got: {}", err);
    }

    #[test]
    fn test_vm_try_catch_return_value() {
        let out = run(r#"fn risky(n) {
    try {
        if n > 0 { raise "too big" }
        return 1
    } catch e {
        return -1
    }
}
dump risky(5)
dump risky(-1)"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] -1")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 1")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_question_operator_result() {
        let out = run(r#"fn go() {
    let v = Ok(42)?
    return v
}
dump go()
try {
    let v = Err("bad")?
    dump "never"
} catch e {
    dump e
}"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] bad")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_question_operator_option() {
        let out = run(r#"try {
    let v = Some(7)?
    dump v
    let n = None?
    dump "never"
} catch e {
    dump "caught-none"
}"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] caught-none")), "got: {:?}", out);
    }

    // --- Method dispatch, struct literals, enum ctors on the VM (7A.2) ---

    #[test]
    fn test_vm_struct_literal_and_field() {
        let out = run(r#"struct Point { x: int, y: int }
let p = Point { x: 3, y: 4 }
dump p.x
dump p.y"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 3")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 4")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_inherent_method() {
        let out = run(r#"struct Point { x: int, y: int }
impl Point {
    fn sum(self) { return self.x + self.y }
}
let p = Point { x: 3, y: 4 }
dump p.sum()"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_trait_method_fmt() {
        let out = run(r#"struct Point { x: int, y: int }
impl Display for Point {
    fn fmt(self) { return f"({self.x}, {self.y})" }
}
let p = Point { x: 3, y: 4 }
dump p.fmt()"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] (3, 4)")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_regex_method() {
        let out = run(r#"let re = /\d+/g
dump re.is_match("abc123")
dump re.find_all("a1 b22 c333")
dump (/\s+/g).replace("a  b", "_")"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] true")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[1, 22, 333]")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("a_b")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_enum_ctor_and_value() {
        let out = run(r#"enum Event { Connect(string), Disconnect }
let e = Event::Connect("host1")
dump e
match e {
    Event::Connect(h) => { dump h },
    _ => { dump "other" },
}"#);
        assert!(out.iter().any(|l| l.contains("Event")), "ctor got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("host1")), "match got: {:?}", out);
    }

    // --- Structural pattern matching on the VM (7A.3) ---

    #[test]
    fn test_vm_match_struct_pattern() {
        let out = run(r#"struct Point { x: int, y: int }
let p = Point { x: 3, y: 4 }
match p {
    Point { x, y } => { dump x + y },
    _ => { dump "no" },
}"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_match_bytes_binary() {
        let out = run(r#"fn sniff(d) {
    match d {
        [0x89, 'P', 'N', 'G', ..] => { return "png" },
        [0xFF, 0xD8, 0xFF, ..] => { return "jpeg" },
        _ => { return "unknown" },
    }
}
dump sniff(b"\x89PNG\x0d\x0a")
dump sniff(b"\xff\xd8\xff\xe0")
dump sniff(b"nope")"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] png")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] jpeg")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] unknown")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_match_tuple_and_or_and_range() {
        let out = run(r#"let t = (1, 2)
match t {
    (1, x) => { dump x },
    _ => { dump "no" },
}
match 5 {
    | 1 | 2 => { dump "low" },
    3..10 => { dump "mid" },
    _ => { dump "other" },
}
match 7 {
    Some(x) => { dump "some" },
    None => { dump "none" },
}"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 2")), "tuple got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] mid")), "range got: {:?}", out);
    }

    #[test]
    fn test_vm_match_some_ok_err_none() {
        let out = run(r#"match Ok(42) {
    Err(e) => { dump "err" },
    Ok(v) => { dump v },
}
match None {
    Some(x) => { dump "some" },
    None => { dump "none" },
}
try {
    match Err("bad") {
        Ok(v) => { dump v },
        Err(e) => { dump e },
    }
}"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "ok got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] none")), "none got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] bad")), "err got: {:?}", out);
    }

    #[test]
    fn test_vm_match_guard_falls_through() {
        let out = run(r#"fn classify(n) {
    match n {
        x if x < 10 => { return "small" },
        10 => { return "ten" },
        _ => { return "big" },
    }
}
dump classify(3)
dump classify(10)
dump classify(50)"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] small")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] ten")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] big")), "got: {:?}", out);
    }

    // --- Operator overloading on the VM (7A.7) ---

    #[test]
    fn test_vm_operator_overload_add_eq() {
        let out = run(r#"struct Vec3 { x: int, y: int }
impl Add for Vec3 {
    fn add(self, o) { return Vec3 { x: self.x + o.x, y: self.y + o.y } }
}
impl Eq for Vec3 {
    fn eq(self, o) { return self.x == o.x && self.y == o.y }
}
let a = Vec3 { x: 1, y: 2 }
let b = Vec3 { x: 10, y: 20 }
let c = a + b
dump c.x
dump c.y
dump a == b
dump a == Vec3 { x: 1, y: 2 }"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 11")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 22")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] false")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] true")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_operator_overload_compare() {
        let out = run(r#"struct Box { w: int }
impl Compare for Box {
    fn lt(self, o) { return self.w < o.w }
}
let a = Box { w: 3 }
let b = Box { w: 9 }
dump a < b
dump b < a"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] true")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] false")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_operator_overload_neg_and_mixed() {
        let out = run(r#"struct Temp { c: int }
impl Neg for Temp {
    fn neg(self) { return Temp { c: -self.c } }
}
let t = Temp { c: 5 }
let z = -t
dump z.c
// Non-numeric + non-numeric without an impl falls back to the old coercion.
let m = {}
let n = 1
dump (m + n)"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] -5")), "got: {:?}", out);
    }

    // --- Assignment + coalescing expressions on the VM ---

    #[test]
    fn test_vm_index_and_field_assign() {
        let out = run(r#"let mut a = [1, 2, 3]
a[0] = 9
dump a[0]
let mut m = {}
m["k"] = "v"
dump m["k"]
struct P { x: int }
let mut p = P { x: 1 }
p.x = 42
dump p.x
let mut n = 5
n += 3
n *= 2
dump n"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 9")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] v")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 42")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 16")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_ternary_and_coalesce() {
        let out = run(r#"let x = 5
dump (x > 3 ? "big" : "small")
dump (nil ?? "fallback")
dump (Some(1) ?? "fb")
let m = { port: 80 }
dump m?.port
dump (nil?.field ?? "missing")
let a = [10, 20]
dump (a?[1] ?? "no")"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] big")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] fallback")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] Some(1)")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 80")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] missing")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 20")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_multi_assign() {
        let out = run(r#"let mut a = 0
let mut b = 0
a, b = 1, 2
dump a
dump b
let mut x = 0
let mut y = 0
x, y = y, x
dump x"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 1")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 2")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] 0")), "got: {:?}", out);
    }

    // --- IfLet / WhileLet / DoWhile on the VM ---

    #[test]
    fn test_vm_if_let() {
        let out = run(r#"if let Some(v) = Some(9) { dump v } else { dump "no" }
if let Some(w) = None { dump "yes" } else { dump "none" }"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 9")), "got: {:?}", out);
        assert!(out.iter().any(|l| l.contains("[DUMP] none")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_while_let() {
        let out = run(r#"let mut n = 0
while let Some(x) = Some(5) {
    n = x
    break
}
dump n"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 5")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_do_while() {
        let out = run(r#"let mut i = 0
do { i = i + 1 } while i < 3
dump i"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 3")), "got: {:?}", out);
    }

    // --- Control flow on the VM (labeled break/continue) ---

    #[test]
    fn test_vm_labeled_break() {
        let out = run(r#"let mut s = 0
'outer: for i in 1..5 {
    for j in 1..5 {
        if i * j > 6 { break 'outer }
        s = s + 1
    }
}
dump s"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 8")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_labeled_continue() {
        let out = run(r#"let mut s = 0
'outer: for i in 1..4 {
    for j in 1..4 {
        if j == 2 { continue 'outer }
        s = s + 1
    }
}
dump s"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 4")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_loop_break() {
        let out = run(r#"let mut n = 0
loop {
    n = n + 1
    if n == 5 { break }
}
dump n"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 5")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_while_continue() {
        let out = run(r#"let mut s = 0
let mut i = 0
while i < 10 {
    i = i + 1
    if i % 2 == 0 { continue }
    s = s + i
}
dump s"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 25")), "got: {:?}", out);
    }

    #[test]
    fn test_vm_for_continue() {
        let out = run(r#"let mut s = 0
for x in [1, 2, 3, 4] {
    if x == 2 { continue }
    s = s + x
}
dump s"#);
        assert!(out.iter().any(|l| l.contains("[DUMP] 8")), "got: {:?}", out);
    }

    // Direct descriptor/matcher debugging ---
    #[test]
    fn debug_vm_pattern_match_direct() {
        use crate::ast::Pattern;
        let p = Pattern::Struct("Point".into(), vec![
            ("x".into(), Pattern::Ident("x".into())),
            ("y".into(), Pattern::Ident("y".into())),
        ]);
        let mut c = crate::compiler::Compiler::new();
        let src = Value::Struct { name: Arc::from("Point"), fields: Arc::from({
            let mut m = std::collections::HashMap::new();
            m.insert("x".to_string(), Value::I64(3));
            m.insert("y".to_string(), Value::I64(4));
            m
        }) };
        let desc = c.pattern_descriptor(&p).unwrap();
        let mut bounds = Vec::new();
        let ok = vm_pattern_match(&src, &desc, &mut bounds).unwrap();
        assert!(ok, "structure match failed; desc={:?}", desc);
        assert_eq!(bounds.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(), vec!["x".to_string(), "y".to_string()]);
    }
}
