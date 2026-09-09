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
}

impl Vm {
    pub fn new() -> Self {
        let mut vm = Vm {
            globals: HashMap::new(),
            output: Vec::new(),
        };
        vm.register_natives();
        vm
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    fn register_natives(&mut self) {
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
    }

    fn insert_native(&mut self, name: &str, f: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static) {
        self.globals.insert(name.to_string(), Value::NativeFn(Arc::from(name), Arc::new(f)));
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<Vec<String>, String> {
        let mut frame = Frame { code: chunk, ip: 0, stack: Vec::new(), locals: Vec::new() };
        self.exec_frame(&mut frame)?;
        Ok(std::mem::take(&mut self.output))
    }

    fn exec_frame(&mut self, frame: &mut Frame) -> Result<(), String> {
        while frame.ip < frame.code.code.len() {
            let op = Op::from_u8(frame.code.code[frame.ip]).ok_or_else(|| format!("bad opcode at {}", frame.ip))?;
            frame.ip += 1;
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
                Op::AddI => bin_int(frame, |a, b| a.wrapping_add(b), |a, b| a + b),
                Op::SubI => bin_int(frame, |a, b| a.wrapping_sub(b), |a, b| a - b),
                Op::MulI => bin_int(frame, |a, b| a.wrapping_mul(b), |a, b| a * b),
                Op::DivI => {
                    let r = frame.pop(); let l = frame.pop();
                    match (l, r) {
                        (Value::I64(a), Value::I64(b)) => if b == 0 { return Err("div by zero".to_string()); } else { frame.push(Value::I64(a / b)); },
                        (Value::F64(a), Value::F64(b)) => if b == 0.0 { return Err("div by zero".to_string()); } else { frame.push(Value::F64(a / b)); },
                        (a, b) => { let av = a.as_f64().unwrap_or(0.0); let bv = b.as_f64().unwrap_or(0.0); if bv == 0.0 { return Err("div by zero".to_string()); } frame.push(Value::F64(av / bv)); },
                    }
                }
                Op::RemI => {
                    let r = frame.pop(); let l = frame.pop();
                    match (l, r) {
                        (Value::I64(a), Value::I64(b)) => if b == 0 { return Err("rem by zero".to_string()); } else { frame.push(Value::I64(a % b)); },
                        (Value::F64(a), Value::F64(b)) => frame.push(Value::F64(a % b)),
                        (a, b) => { let av = a.as_f64().unwrap_or(0.0); let bv = b.as_f64().unwrap_or(0.0); frame.push(Value::F64(av % bv)); },
                    }
                }
                Op::NegI => {
                    let v = frame.pop();
                    match v {
                        Value::I64(i) => frame.push(Value::I64(-i)),
                        Value::F64(f) => frame.push(Value::F64(-f)),
                        other => frame.push(Value::F64(-(other.as_f64().unwrap_or(0.0)))),
                    }
                }
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
                Op::Eq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l == r)); }
                Op::NotEq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l != r)); }
                Op::Lt => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) < r.as_i64().unwrap_or(0))); }
                Op::Gt => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) > r.as_i64().unwrap_or(0))); }
                Op::LtEq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) <= r.as_i64().unwrap_or(0))); }
                Op::GtEq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) >= r.as_i64().unwrap_or(0))); }
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
                Op::Call => {
                    let argc = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let mut args: Vec<Value> = (0..argc).map(|_| frame.pop()).collect();
                    args.reverse();
                    let callee = frame.pop();
                    match callee {
                        Value::NativeFn(name, f) => {
                            let result = f(&args).map_err(|e| format!("{}: {}", name, e))?;
                            frame.push(result);
                        }
                        Value::Closure { code, nparams, .. } => {
                            let mut sub = Frame { code: &code, ip: 0, stack: Vec::new(), locals: Vec::with_capacity(nparams) };
                            for i in 0..nparams {
                                sub.locals.push(args.get(i).cloned().unwrap_or(Value::Nil));
                            }
                            self.exec_frame(&mut sub)?;
                            frame.push(sub.stack.pop().unwrap_or(Value::Nil));
                        }
                        _ => return Err("cannot call non-function".to_string()),
                    }
                }
                Op::Return => {
                    return Ok(());
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
            }
        }
        Ok(())
    }
}

struct Frame<'a> {
    code: &'a Chunk,
    ip: usize,
    stack: Vec<Value>,
    locals: Vec<Value>,
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
    fn test_vm_while_loop() {
        let out = run("let i = 0; let s = 0; while i < 10 { s = s + i; i = i + 1 } dump s");
        assert!(out.iter().any(|l| l.contains("[DUMP] 45")));
    }

    #[test]
    fn test_vm_function() {
        let out = run("fn add(a, b) { return a + b } dump add(3, 4)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")));
    }

    #[test]
    fn test_vm_for_range() {
        let out = run("let s = 0; for i in 1..100 { s = s + i } dump s");
        assert!(out.iter().any(|l| l.contains("[DUMP] 5050")));
    }

    #[test]
    fn test_vm_for_array() {
        let out = run("let s = 0; for x in [10, 20, 30] { s = s + x } dump s");
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
}
