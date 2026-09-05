use crate::ast::{Param, Stmt, Type};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

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
    Hex(u64, usize),
    String(Arc<str>),
    Bytes(Arc<[u8]>),
    Bool(bool),
    Nil,
    Tuple(Arc<[Value]>),
    Array(Arc<Vec<Value>>),
    Map(Arc<HashMap<String, Value>>),
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
    NativeFn(Arc<str>, Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>),
    Closure {
        code: Arc<crate::bytecode::Chunk>,
        nparams: usize,
        name: Arc<str>,
    },
    Result(Option<Box<Value>>, Option<Box<Value>>),
    Option(Option<Box<Value>>),
    Channel(Arc<crate::vm::ChannelHandle>),
    Future(Arc<crate::vm::FutureHandle>),
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
            Value::Hex(_, _) => "hex",
            Value::String(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Bool(_) => "bool",
            Value::Nil => "nil",
            Value::Tuple(_) => "tuple",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
            Value::Struct { .. } => "struct",
            Value::Enum { .. } => "enum",
            Value::Function { .. } => "function",
            Value::NativeFn(..) => "native_fn",
            Value::Closure { .. } => "closure",
            Value::Result(..) => "result",
            Value::Option(..) => "option",
            Value::Channel(_) => "channel",
            Value::Future(_) => "future",
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
            Value::Hex(v, _) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_i64().map(|v| v as u64)
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
            Value::Hex(v, _) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Nil => false,
            Value::I64(0) | Value::I32(0) | Value::U64(0) | Value::U32(0) => false,
            Value::Hex(0, _) => false,
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
            (Value::Hex(a, _), Value::Hex(b, _)) => a == b,
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
            Value::Hex(h, w) => write!(f, "0x{:0width$X}", h, width = w / 4),
            Value::String(s) => write!(f, "{}", s),
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
        }
    }
}

#[derive(Clone, Default)]
pub struct Env {
    pub scopes: Vec<HashMap<String, Value>>,
}

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env").field("scopes", &self.scopes.len()).finish()
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
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

pub fn type_of(t: &Type) -> String {
    match t {
        Type::Hex(_) => "hex".to_string(),
        Type::Int => "int".to_string(),
        Type::String => "string".to_string(),
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
    }
}
