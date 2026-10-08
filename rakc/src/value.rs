use crate::ast::{Param, Stmt, Type};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// A compiled regular expression, shared cheaply via `Arc`.
/// Order two numeric values exactly.
///
/// Equality used to convert both sides with `as_f64`, so `9007199254740993 ==
/// 9007199254740993.0` was true: 2^53 + 1 is the first integer an f64 cannot hold, so
/// both sides rounded to the same value. The interpreter compares exactly, and a parity
/// split here would be worse than either answer alone.
///
/// Two integers are compared as `i128`, two floats as `f64`, and a mixed pair without
/// rounding the integer. An integral float converts exactly and the cast saturates past
/// `i128`; a float with a fraction is never equal and orders below the integer it rounds
/// up from.
pub fn cmp_numeric(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (integer_value(a), integer_value(b)) {
        (Some(x), Some(y)) => Some(x.cmp(&y)),
        (None, None) => Some(
            a.as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&b.as_f64().unwrap_or(0.0))
                .unwrap_or(Ordering::Equal),
        ),
        (Some(i), None) => Some(cmp_int_float(i, b.as_f64().unwrap_or(0.0))),
        (None, Some(i)) => Some(cmp_int_float(i, a.as_f64().unwrap_or(0.0)).reverse()),
    }
}

/// The integer a numeric value holds, or `None` for a float.
///
/// Widened to `i128`, which is exact for every width the VM models.
fn integer_value(v: &Value) -> Option<i128> {
    match v {
        Value::I8(n) => Some(*n as i128),
        Value::I16(n) => Some(*n as i128),
        Value::I32(n) => Some(*n as i128),
        Value::I64(n) => Some(*n as i128),
        Value::U8(n) => Some(*n as i128),
        Value::U16(n) => Some(*n as i128),
        Value::U32(n) => Some(*n as i128),
        Value::U64(n) => Some(*n as i128),
        Value::Hex(n) => Some(*n as i128),
        _ => None,
    }
}

/// Order an integer against a float without rounding the integer.
fn cmp_int_float(i: i128, f: f64) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if f.fract() == 0.0 {
        return i.cmp(&(f as i128));
    }
    match i.cmp(&(f.trunc() as i128)) {
        Ordering::Equal => Ordering::Less,
        other => other,
    }
}

pub struct RegexValue {
    pub pattern: String,
    pub flags: String,
    pub re: regex::Regex,
}

/// The call-time shape of one compiled function parameter.
///
/// The VM lowers a `fn` body to bytecode, which records nothing about how the
/// signature binds its arguments. The interpreter keeps the whole `Param` (with
/// its default `Expr`) and evaluates defaults in the callee scope; the VM has no
/// `Expr` at call time, so it records just enough to bind a rest parameter,
/// decide whether a missing argument is legal, and name one that is not.
///
/// Defaults themselves are *not* here: a default can reference an earlier
/// parameter (`fn f(a, b = a + 1)`), so evaluating it needs the callee's
/// bindings. `compile_function` emits each missing default into the callee
/// body's prologue instead, where the earlier parameters are already locals.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamShape {
    pub name: Arc<str>,
    /// `fn f(...rest)` -- collects every leftover positional into an array.
    pub rest: bool,
    /// `fn f(x?)` -- a missing argument is `nil` rather than an error.
    pub optional: bool,
    /// `fn f(x = <expr>)` -- a missing argument is legal and filled in by the
    /// callee prologue.
    pub has_default: bool,
}

#[derive(Clone)]
pub enum Value {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Hex(u64),
    String(Arc<str>),
    Char(char),
    Bytes(Arc<[u8]>),
    Bool(bool),
    Nil,
    Tuple(Arc<[Value]>),
    Array(Arc<Vec<Value>>),
    Map(Arc<HashMap<String, Value>>),
    /// An encrypted UDP conduit (spec 7A.5). Mirrors the interpreter's
    /// `UdpTransport` so `tunnel` and the `udp_*` builtins work on both.
    ///
    /// `rak_stdlib`'s helpers take `&Mutex<UdpTransport>` and `udp_bind` returns
    /// a `Mutex<UdpTransport>`, so the mutex is not an interpreter convention
    /// being repeated here — it is the type those functions are written against.
    UdpTransport(Arc<std::sync::Mutex<rak_stdlib::tunnel::UdpTransport>>),
    /// A lazy, pull-based stream (spec 7A.4). See `crate::ext_streams`.
    ///
    /// Behind an `Arc<Mutex<_>>` for the same reason `Value::Set` is: streams
    /// are shared values and `stream_next` has to advance the shared handle.
    Stream(Arc<std::sync::Mutex<Box<dyn crate::ext_streams::VmStream>>>),
    /// An insertion-ordered set (spec 7A.11).
    ///
    /// The inner `Arc<Mutex<_>>` is what lets `set_add` and `set_discard`
    /// mutate a set that other values may also be holding. Without it, a set
    /// would need a reference type to be updatable, and Rak has none.
    Set(Arc<std::sync::Mutex<crate::setrepr::SetRepr<Value>>>),
    /// An imported module, held as a *live* view of that module's globals.
    ///
    /// The VM inlines a module body into the same chunk, so the module's
    /// top-level bindings are ordinary chunk globals; `Op::StoreGlobal`
    /// republishes them into this namespace, which is what makes `m.X` track the
    /// module instead of a snapshot of it taken at import time. Mirrors the
    /// interpreter's `Module` variant so both backends agree on what `import m`
    /// hands out.
    Module(Arc<std::sync::Mutex<crate::modns::ModuleNamespace<Value>>>),
    Struct {
        name: Arc<str>,
        fields: Arc<HashMap<String, Value>>,
    },
    Enum {
        name: Arc<str>,
        variant: Arc<str>,
        data: Arc<[Value]>,
    },
    Function {
        params: Vec<Param>,
        body: Arc<[Stmt]>,
        captures: Arc<Env>,
    },
    NativeFn(
        Arc<str>,
        Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>,
    ),
    Closure {
        code: Arc<crate::bytecode::Chunk>,
        /// Parameter shape, shared cheaply. The VM needs it at call time to
        /// bind rest params, reject extra positionals, and name a missing
        /// required argument -- none of which the compiled body carries.
        params: Arc<[ParamShape]>,
        name: Arc<str>,
    },
    Result(Option<Box<Value>>, Option<Box<Value>>),
    Option(Option<Box<Value>>),
    Channel(Arc<crate::vm::ChannelHandle>),
    Future(Arc<crate::vm::FutureHandle>),
    Regex(Arc<RegexValue>),
    ForeignLib(Arc<std::sync::Mutex<rak_stdlib::ffi::LibHandle>>),
    ForeignPtr(u64),
    Mmap(Arc<rak_stdlib::mmap::MmapHandle>),
    MmapSlice(Arc<rak_stdlib::mmap::MmapHandle>, usize, usize),
    Pcap(Arc<std::sync::Mutex<rak_stdlib::pcap::PcapHandle>>),
    /// A structured runtime error value (mirrors the interpreter's `Error`).
    Error(Arc<crate::ErrorInfo>),
    /// A provenance-tagged value (`evidence<T>`).
    Evidence {
        inner: Box<Value>,
        provenance: Arc<Provenance>,
    },
}

/// Provenance metadata for an `evidence`-typed value on the VM. Mirrors the
/// interpreter's `Provenance`.
#[derive(Clone)]
pub struct Provenance {
    pub tool: String,
    pub target: String,
    pub ts: u64,
    pub raw_offset: Option<u64>,
    pub raw_len: Option<u64>,
    pub parent: Option<Arc<Provenance>>,
}

impl crate::setrepr::SetElement for crate::interpreter::Value {
    /// Mirrors the VM's `SetElement` impl so that a set behaves the same way
    /// whichever backend runs it.
    fn set_key(&self) -> String {
        // The interpreter's numeric accessors are private to its own module,
        // and the same key rule is already expressed once for it — in
        // `interpreter::set_key` — so delegate rather than reimplement and
        // risk the two drifting apart.
        crate::interpreter::set_key(self)
    }
}

impl crate::setrepr::SetElement for Value {
    /// Type tag plus rendered form. The tag is what keeps `1` and `"1"` apart
    /// while `1`, `0x1` and `1.0` collapse, matching Rak's cross-representation
    /// numeric equality.
    fn set_key(&self) -> String {
        if self.is_numeric() {
            return format!("n:{}", self.as_f64().unwrap_or(0.0) as i64);
        }
        match self {
            Value::String(_) | Value::Char(_) => format!("s:{}", self),
            Value::Bytes(b) => format!(
                "b:{}",
                b.iter().map(|x| format!("{:02x}", x)).collect::<String>()
            ),
            other => format!("{:?}:{}", other.type_name(), other),
        }
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::I8(_) => "i8",
            Value::I16(_) => "i16",
            Value::I32(_) => "i32",
            Value::I64(_) => "i64",
            Value::U8(_) => "u8",
            Value::U16(_) => "u16",
            Value::U32(_) => "u32",
            Value::U64(_) => "u64",
            Value::F32(_) => "f32",
            Value::F64(_) => "f64",
            Value::Hex(_) => "hex",
            Value::String(_) => "string",
            Value::Char(_) => "char",
            Value::Bytes(_) => "bytes",
            Value::Bool(_) => "bool",
            Value::Nil => "nil",
            Value::Tuple(_) => "tuple",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
            Value::Module(_) => "module",
            Value::UdpTransport(_) => "udp-transport",
            Value::Stream(_) => "stream",
            Value::Set(_) => "set",
            Value::Struct { .. } => "struct",
            Value::Enum { .. } => "enum",
            Value::Function { .. } => "function",
            Value::NativeFn(..) => "native_fn",
            Value::Closure { .. } => "closure",
            Value::Result(..) => "result",
            Value::Option(..) => "option",
            Value::Channel(_) => "channel",
            Value::Future(_) => "future",
            Value::Regex(_) => "regex",
            Value::ForeignLib(_) => "ffi-lib",
            Value::ForeignPtr(_) => "ptr",
            Value::Mmap(_) => "mmap",
            Value::MmapSlice(_, _, _) => "mmap-slice",
            Value::Pcap(_) => "pcap",
            Value::Error(_) => "error",
            Value::Evidence { .. } => "evidence",
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::I8(v) => Some(*v as i64),
            Value::I16(v) => Some(*v as i64),
            Value::I32(v) => Some(*v as i64),
            Value::I64(v) => Some(*v),
            Value::U8(v) => Some(*v as i64),
            Value::U16(v) => Some(*v as i64),
            Value::U32(v) => Some(*v as i64),
            Value::U64(v) => Some(*v as i64),
            Value::F32(v) => Some(*v as i64),
            Value::F64(v) => Some(*v as i64),
            Value::Hex(v) => Some(*v as i64),
            Value::Char(c) => Some(*c as i64),
            Value::ForeignPtr(p) => Some(*p as i64),
            Value::Evidence { inner, .. } => inner.as_i64(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Hex(v) => Some(*v),
            Value::ForeignPtr(p) => Some(*p),
            Value::Evidence { inner, .. } => inner.as_u64(),
            _ => self.as_i64().map(|v| v as u64),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F32(v) => Some(*v as f64),
            Value::F64(v) => Some(*v),
            Value::I8(v) => Some(*v as f64),
            Value::I16(v) => Some(*v as f64),
            Value::I32(v) => Some(*v as f64),
            Value::I64(v) => Some(*v as f64),
            Value::U8(v) => Some(*v as f64),
            Value::U16(v) => Some(*v as f64),
            Value::U32(v) => Some(*v as f64),
            Value::U64(v) => Some(*v as f64),
            Value::Hex(v) => Some(*v as f64),
            Value::Char(c) => Some(*c as i64 as f64),
            _ => None,
        }
    }

    /// True for integer and float value variants, enabling cross-representation
    /// numeric equality (`0xA == 10 == 10.0`).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Value::I8(_)
                | Value::I16(_)
                | Value::I32(_)
                | Value::I64(_)
                | Value::U8(_)
                | Value::U16(_)
                | Value::U32(_)
                | Value::U64(_)
                | Value::F32(_)
                | Value::F64(_)
                | Value::Hex(_)
        )
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Nil => false,
            Value::I64(0) | Value::I32(0) | Value::U64(0) | Value::U32(0) => false,
            Value::Hex(0) => false,
            Value::String(s) => !s.is_empty(),
            Value::Char(_) => true,
            Value::Bytes(b) => !b.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Map(m) => !m.is_empty(),
            Value::Set(s) => !s.lock().unwrap().is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::Option(Some(_)) => true,
            Value::Option(None) => false,
            Value::Result(Some(_), _) => true,
            Value::Result(None, _) => false,
            Value::Evidence { inner, .. } => inner.is_truthy(),
            _ => true,
        }
    }
}

impl Value {
    /// Whether two values are interchangeable *as constants*.
    ///
    /// Deliberately stricter than [`PartialEq`]. `PartialEq` answers "are these the same
    /// number", which is what the language wants for `0xA == 10 == 10.0`. The constant
    /// pool needs a different question: "will loading this slot give me a value of the
    /// type I wrote down".
    ///
    /// Using `PartialEq` there let a later constant reuse an earlier one's slot across
    /// representations, so `let q = 2` followed by `dump 2.0` loaded an `I64` -- and then
    /// `-2.0` was a negated integer rather than a negated float, which quietly changed
    /// what a comparison meant. The two representations compare equal and must not share
    /// a slot.
    ///
    /// The discriminant check covers the numeric widths and `Hex`, which is where the
    /// collision actually happened. Containers are not constant-folded into the pool, so
    /// they are not a concern here.
    pub fn same_const_repr(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other) && self == other
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::I8(a), Value::I8(b)) => a == b,
            (Value::I16(a), Value::I16(b)) => a == b,
            (Value::I32(a), Value::I32(b)) => a == b,
            (Value::I64(a), Value::I64(b)) => a == b,
            (Value::U8(a), Value::U8(b)) => a == b,
            (Value::U16(a), Value::U16(b)) => a == b,
            (Value::U32(a), Value::U32(b)) => a == b,
            (Value::U64(a), Value::U64(b)) => a == b,
            (Value::F32(a), Value::F32(b)) => a == b,
            (Value::F64(a), Value::F64(b)) => a == b,
            (Value::Hex(a), Value::Hex(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            // Sets compare by membership, not by insertion order: `{1,2}` and
            // `{2,1}` are the same set even though they iterate differently.
            (Value::Set(a), Value::Set(b)) => {
                let (a, b) = (a.lock().unwrap(), b.lock().unwrap());
                a.len() == b.len() && a.iter().all(|v| b.contains(v))
            }
            (Value::Option(a), Value::Option(b)) => a == b,
            // Cross-representation numeric equality: `0xA == 10 == 10.0`.
            (a, b) if a.is_numeric() && b.is_numeric() => cmp_numeric(a, b)
                .map(std::cmp::Ordering::is_eq)
                .unwrap_or(false),
            (Value::Regex(a), Value::Regex(b)) => a.pattern == b.pattern && a.flags == b.flags,
            (Value::ForeignPtr(a), Value::ForeignPtr(b)) => a == b,
            (Value::Closure { code: a, .. }, Value::Closure { code: b, .. }) => Arc::ptr_eq(a, b),
            (Value::NativeFn(na, _), Value::NativeFn(nb, _)) => na == nb,
            (Value::Evidence { inner: a, .. }, Value::Evidence { inner: b, .. }) => a == b,
            (Value::Evidence { inner: a, .. }, other) => (**a).eq(other),
            (other, Value::Evidence { inner: b, .. }) => other.eq(&**b),
            // Structural, by name and by content. The catch-all below compared
            // discriminants, so on the VM as on the interpreter every struct equalled every
            // other struct of the same type:
            //
            //     struct P { x: int }
            //     P { x: 1 } == P { x: 2 }   // true, before
            //
            // The VM models fields as an `Arc<HashMap<..>>`, so the content comparison
            // needs an explicit walk rather than `==` on the map itself, whose iteration
            // order is not the same as anything meaningful here.
            (
                Value::Struct {
                    name: an,
                    fields: af,
                },
                Value::Struct {
                    name: bn,
                    fields: bf,
                },
            ) => {
                an == bn
                    && af.len() == bf.len()
                    && af
                        .iter()
                        .all(|(k, v)| bf.get(k).map(|o| o == v).unwrap_or(false))
            }
            (
                Value::Enum {
                    name: an,
                    variant: av,
                    data: ad,
                },
                Value::Enum {
                    name: bn,
                    variant: bv,
                    data: bd,
                },
            ) => an == bn && av == bv && ad == bd,
            (Value::Result(a, b), Value::Result(c, d)) => a == c && b == d,
            // Two values of different shapes are never equal. Shapes with no content
            // comparison (`Module`, the socket and stream handles, `Future`, `Pcap`,
            // `Mmap`) fall through here and are reported unequal, which is the honest
            // answer: no equality is defined for them. `Closure` and `NativeFn` above
            // compare by identity, so a function compared with itself still works.
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I8(v) => write!(f, "{}", v),
            Value::I16(v) => write!(f, "{}", v),
            Value::I32(v) => write!(f, "{}", v),
            Value::I64(v) => write!(f, "{}", v),
            Value::U8(v) => write!(f, "{}", v),
            Value::U16(v) => write!(f, "{}", v),
            Value::U32(v) => write!(f, "{}", v),
            Value::U64(v) => write!(f, "{}", v),
            Value::F32(v) => write!(f, "{}", v),
            Value::F64(v) => write!(f, "{}", v),
            // Minimal digits, the way the interpreter's `0x{:X}` prints. This used to
            // pad to a width carried on the value, but nothing ever set that width
            // from the source -- the compiler hardcoded 64 -- so every hex literal
            // rendered as 16 digits and captured output (a dump, a log line, a
            // saved file) carried the padding.
            Value::Hex(h) => write!(f, "0x{:X}", h),
            Value::String(s) => write!(f, "{}", s),
            Value::Char(c) => write!(f, "'{}'", c),
            Value::Bytes(b) => {
                write!(f, "b\"")?;
                for byte in b.iter() {
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
                // Sorted by key. A `HashMap` iterates in a per-process order, so the
                // same program printed differently between runs -- and differently
                // from the interpreter, which sorts.
                let mut entries: Vec<(&String, &Value)> = map.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
            Value::Set(set) => {
                let guard = set.lock().unwrap();
                let parts: Vec<String> = guard.iter().map(|v| v.to_string()).collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
            Value::Struct { name, fields } => {
                // Sorted by field name, for the same reason `Map` is above.
                let mut entries: Vec<(&String, &Value)> = fields.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{} {{{}}}", name, parts.join(", "))
            }
            Value::Enum {
                name,
                variant,
                data,
            } => {
                if data.is_empty() {
                    write!(f, "{}::{}", name, variant)
                } else {
                    let parts: Vec<String> = data.iter().map(|v| v.to_string()).collect();
                    write!(f, "{}::{}({})", name, variant, parts.join(", "))
                }
            }
            Value::Function { .. } => write!(f, "<function>"),
            Value::NativeFn(name, _) => write!(f, "<native {}>", name),
            Value::Closure { name, .. } => write!(f, "<closure {}>", name),
            Value::Result(Some(ok), _) => write!(f, "Ok({})", ok),
            Value::Result(_, Some(err)) => write!(f, "Err({})", err),
            Value::Result(None, None) => write!(f, "Ok(nil)"),
            Value::Option(Some(v)) => write!(f, "Some({})", v),
            Value::Option(None) => write!(f, "None"),
            Value::Channel(_) => write!(f, "<channel>"),
            Value::Future(_) => write!(f, "<future>"),
            Value::Regex(r) => write!(f, "/{}/{}", r.pattern, r.flags),
            Value::ForeignLib(_) => write!(f, "<ffi-lib>"),
            Value::ForeignPtr(p) => write!(f, "0x{:X}", p),
            Value::Mmap(_) => write!(f, "<mmap>"),
            Value::Module(_) => write!(f, "<module>"),
            Value::UdpTransport(_) => write!(f, "<udp-transport>"),
            Value::Stream(_) => write!(f, "<stream>"),
            Value::MmapSlice(_, _, n) => write!(f, "<mmap-slice {}B>", n),
            Value::Pcap(_) => write!(f, "<pcap>"),
            Value::Error(err) => write!(f, "{}", err.message),
            Value::Evidence { inner, .. } => write!(f, "{}", inner),
        }
    }
}

#[derive(Clone, Default)]
pub struct Env {
    pub scopes: Vec<HashMap<String, Value>>,
}

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("scopes", &self.scopes.len())
            .finish()
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
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
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), value);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val.clone());
            }
        }
        None
    }

    pub fn assign(&mut self, name: &str, value: Value) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return Ok(());
            }
        }
        Err(format!("Undefined variable: {}", name))
    }
}

/// A runtime value named the way Rak spells it.
///
/// `Value::type_name` reports the *storage* width -- "i64", "f64" -- because that is what
/// the VM's own diagnostics are about. A type-mismatch message has to read the same on both
/// backends, and the interpreter says "int" and "float", so this is the Rak spelling for
/// that one message rather than changing every other.
pub fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::I8(_) | Value::I16(_) | Value::I32(_) | Value::I64(_) => "int",
        Value::U8(_) | Value::U16(_) | Value::U32(_) | Value::U64(_) => "uint",
        Value::F32(_) | Value::F64(_) => "float",
        other => other.type_name(),
    }
}

pub fn type_of(t: &Type) -> String {
    match t {
        Type::Hex(_) => "hex".to_string(),
        Type::Int => "int".to_string(),
        Type::String => "string".to_string(),
        Type::Char => "char".to_string(),
        Type::Bytes => "bytes".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Nil => "nil".to_string(),
        Type::Array(_) => "array".to_string(),
        Type::Map(_, _) => "map".to_string(),
        Type::Function(_, _) => "function".to_string(),
        Type::Option(_) => "option".to_string(),
        Type::Result(_, _) => "result".to_string(),
        Type::Generic(g) => g.clone(),
        Type::Tuple(_) => "tuple".to_string(),
        Type::Custom(c) => c.clone(),
        Type::I8 | Type::I16 | Type::I32 | Type::I64 => "int".to_string(),
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => "uint".to_string(),
        Type::F32 | Type::F64 => "float".to_string(),
        Type::Ptr(_) => "ptr".to_string(),
        Type::Void => "void".to_string(),
        Type::Evidence(inner) => format!("evidence<{}>", type_of(inner)),
    }
}
