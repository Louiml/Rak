//! The `.rkbc` bytecode container: a serialized `Chunk`.
//!
//! `rakc vm` used to be the only way to run bytecode, and the only way to get
//! a bytecode chunk was to compile in-process. `.rkbc` makes the compiled
//! program a file: compile once (`rakc build --emit=bytecode`), ship it, run
//! it anywhere with `rakc vm program.rkbc` ג€” the same "compile once, run
//! anywhere" shape a bytecode VM exists for.
//!
//! The hard part is that a chunk is not just code. Its constant pool holds
//! *baked closures* ג€” `Value::Function { params, body, captures }` whose body
//! is a real `Vec<Stmt>`, and `Value::Closure` whose code is a nested chunk.
//! So serializing a chunk means serializing the AST that appears inside
//! function values, the captured environments, and the type tables. That is
//! what [`rkbc_ast`] is for.
//!
//! What deliberately cannot be serialized is a *runtime handle*: a stream, a
//! channel, an mmap, an FFI library. None of those can be a constant of a
//! compiled program ג€” they only exist once the program runs ג€” so meeting one
//! is a bug, and it is reported as one, naming the constant.

use crate::bytecode::{Chunk, ModuleCell};
use crate::value::{Env, Provenance, Value};

use super::rkbc_ast;

/// The container magic: `RKBC`.
pub const MAGIC: &[u8; 4] = b"RKBC";

/// The container format version. Bumped when any encoding changes in a way an
/// older `rakc` could misread; `rakc vm` refuses a newer version rather than
/// decoding garbage.
///
/// Version 2 added the capture list to a serialized closure. A closure in a
/// compiled chunk has no captures -- they are filled in when the closure is
/// made, at run time -- so the field is empty in practice, but the encoding
/// now describes the value rather than the subset of it that happens to
/// serialize.
pub const FORMAT_VERSION: u16 = 2;

// ---------------------------------------------------------------------------
// Primitives: a big-endian writer/reader pair.
// ---------------------------------------------------------------------------

/// A growable big-endian byte writer. Big-endian because it makes `.rkbc`
/// files comparable across hosts ג€” a checksum, a diff, a hex dump are all
/// stable ג€” and the container is never on a per-instruction hot path.
pub struct Writer {
    buf: Vec<u8>,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_bits().to_be_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_bits().to_be_bytes());
    }

    pub fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v);
    }

    pub fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }

    pub fn flag(&mut self, v: bool) {
        self.buf.push(if v { 1 } else { 0 });
    }

    pub fn option_u64(&mut self, v: Option<u64>) {
        match v {
            Some(x) => {
                self.flag(true);
                self.u64(x);
            }
            None => self.flag(false),
        }
    }
}

/// A big-endian reader over a byte slice. Every read checks bounds and returns
/// a truncated-file error naming the offset, because a truncated `.rkbc` is an
/// ordinary thing to be handed and must not panic.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| format!("{}: truncated file", self.where_()))?;
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn where_(&self) -> String {
        format!("at offset {}", self.pos)
    }

    pub fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> Result<u64, String> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_be_bytes(a))
    }

    pub fn i64(&mut self) -> Result<i64, String> {
        Ok(self.u64()? as i64)
    }

    pub fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_bits(self.u64()?))
    }

    pub fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn bytes(&mut self) -> Result<Vec<u8>, String> {
        let len = self.u32()? as usize;
        Ok(self.take(len)?.to_vec())
    }

    pub fn str(&mut self) -> Result<String, String> {
        let b = self.bytes()?;
        String::from_utf8(b).map_err(|e| format!("{}: invalid UTF-8: {}", self.where_(), e))
    }

    pub fn flag(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }

    pub fn option_u64(&mut self) -> Result<Option<u64>, String> {
        if self.flag()? {
            Ok(Some(self.u64()?))
        } else {
            Ok(None)
        }
    }

    /// A length-prefixed count, sanity-capped so a corrupt length cannot ask
    /// the decoder to pre-allocate an absurd vector. The cap is generous: the
    /// real ceiling is MAX_CHUNK_LEN (65535) for code-addressed tables, and a
    /// chunk's code stream is the largest thing in the file.
    pub fn count(&mut self, what: &str) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > 64 * 1024 * 1024 {
            return Err(format!(
                "{}: implausible {} count {}",
                self.where_(),
                what,
                n
            ));
        }
        Ok(n)
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }
}

// ---------------------------------------------------------------------------
// Value tags.
// ---------------------------------------------------------------------------

const T_NIL: u8 = 0;
const T_BOOL: u8 = 1;
const T_I8: u8 = 2;
const T_I16: u8 = 3;
const T_I32: u8 = 4;
const T_I64: u8 = 5;
const T_U8: u8 = 6;
const T_U16: u8 = 7;
const T_U32: u8 = 8;
const T_U64: u8 = 9;
const T_F32: u8 = 10;
const T_F64: u8 = 11;
const T_HEX: u8 = 12;
const T_STRING: u8 = 13;
const T_CHAR: u8 = 14;
const T_BYTES: u8 = 15;
const T_TUPLE: u8 = 16;
const T_ARRAY: u8 = 17;
const T_MAP: u8 = 18;
const T_STRUCT: u8 = 19;
const T_ENUM: u8 = 20;
const T_FUNCTION: u8 = 21;
const T_NATIVE: u8 = 22;
const T_CLOSURE: u8 = 23;
const T_RESULT: u8 = 24;
const T_OPTION: u8 = 25;
const T_REGEX: u8 = 26;
const T_EVIDENCE: u8 = 27;

/// The reader half of the constant-tag map. Kept beside the tags so the
/// decode side cannot drift from the encode side.
impl Reader<'_> {
    fn value(&mut self) -> Result<Value, String> {
        let tag = self.u8()?;
        match tag {
            T_NIL => Ok(Value::Nil),
            T_BOOL => Ok(Value::Bool(self.flag()?)),
            T_I8 => Ok(Value::I8(self.u8()? as i8)),
            T_I16 => Ok(Value::I16(self.u16()? as i16)),
            T_I32 => Ok(Value::I32(self.u32()? as i32)),
            T_I64 => Ok(Value::I64(self.i64()?)),
            T_U8 => Ok(Value::U8(self.u8()?)),
            T_U16 => Ok(Value::U16(self.u16()?)),
            T_U32 => Ok(Value::U32(self.u32()?)),
            T_U64 => Ok(Value::U64(self.u64()?)),
            T_F32 => Ok(Value::F32(self.f32()?)),
            T_F64 => Ok(Value::F64(self.f64()?)),
            T_HEX => Ok(Value::Hex(self.u64()?)),
            T_STRING => Ok(Value::String(std::sync::Arc::from(self.str()?.as_str()))),
            T_CHAR => {
                let c = self.u32()?;
                char::from_u32(c)
                    .map(Value::Char)
                    .ok_or_else(|| format!("{}: invalid char code point {}", self.where_(), c))
            }
            T_BYTES => Ok(Value::Bytes(std::sync::Arc::from(self.bytes()?.as_slice()))),
            T_TUPLE => {
                let n = self.count("tuple")?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.value()?);
                }
                Ok(Value::Tuple(std::sync::Arc::from(v.into_boxed_slice())))
            }
            T_ARRAY => {
                let n = self.count("array")?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.value()?);
                }
                Ok(Value::Array(std::sync::Arc::new(v)))
            }
            T_MAP => {
                let n = self.count("map")?;
                let mut m = std::collections::HashMap::with_capacity(n);
                for _ in 0..n {
                    let k = self.str()?;
                    let v = self.value()?;
                    m.insert(k, v);
                }
                Ok(Value::Map(std::sync::Arc::new(m)))
            }
            T_STRUCT => {
                let name = self.str()?;
                let n = self.count("struct fields")?;
                let mut m = std::collections::HashMap::with_capacity(n);
                for _ in 0..n {
                    let k = self.str()?;
                    let v = self.value()?;
                    m.insert(k, v);
                }
                Ok(Value::Struct {
                    name: std::sync::Arc::from(name.as_str()),
                    fields: std::sync::Arc::new(m),
                })
            }
            T_ENUM => {
                let name = self.str()?;
                let variant = self.str()?;
                let n = self.count("enum payload")?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.value()?);
                }
                Ok(Value::Enum {
                    name: std::sync::Arc::from(name.as_str()),
                    variant: std::sync::Arc::from(variant.as_str()),
                    data: std::sync::Arc::from(v.into_boxed_slice()),
                })
            }
            T_FUNCTION => {
                let params = rkbc_ast::read_params(self)?;
                let body = rkbc_ast::read_stmts(self)?;
                let captures = self.env()?;
                Ok(Value::Function {
                    params,
                    body: std::sync::Arc::from(body.into_boxed_slice()),
                    captures: std::sync::Arc::new(captures),
                })
            }
            T_NATIVE => {
                // A native is remembered by name only; the caller re-binds it
                // against the VM's native registry at load time.
                let name = self.str()?;
                Ok(Value::NativeFn(
                    std::sync::Arc::from(name.as_str()),
                    // A placeholder that is replaced before the chunk runs.
                    // It exists so decoding a file for inspection never fails.
                    std::sync::Arc::new(|_args| {
                        Err("native is not bound; load the chunk through rakc".to_string())
                    }),
                ))
            }
            T_CLOSURE => {
                let code = std::sync::Arc::new(self.chunk()?);
                let params = rkbc_ast::read_param_shapes(self)?;
                let name = self.str()?;
                // The captures a closure holds. A closure in a *compiled*
                // chunk has none -- a capture is filled in when the closure is
                // made, at run time -- but the field is written so the format
                // describes the value rather than the subset of it that
                // happens to be serializable today.
                let n = self.count("closure captures")?;
                let mut captures = Vec::with_capacity(n);
                for _ in 0..n {
                    captures.push(self.value()?);
                }
                Ok(Value::Closure {
                    code,
                    params: std::sync::Arc::from(params.into_boxed_slice()),
                    name: std::sync::Arc::from(name.as_str()),
                    captures: std::sync::Arc::new(std::sync::Mutex::new(captures)),
                })
            }
            T_RESULT => {
                let ok = self.opt_value()?;
                let err = self.opt_value()?;
                Ok(Value::Result(ok.map(Box::new), err.map(Box::new)))
            }
            T_OPTION => {
                let v = self.opt_value()?;
                Ok(Value::Option(v.map(Box::new)))
            }
            T_REGEX => {
                let pattern = self.str()?;
                let flags = self.str()?;
                Ok(Value::Regex(std::sync::Arc::new(
                    crate::value::RegexValue {
                        pattern: pattern.clone(),
                        flags,
                        re: regex::Regex::new(&pattern).map_err(|e| {
                            format!("invalid regex /{}/ in bytecode: {}", pattern, e)
                        })?,
                    },
                )))
            }
            T_EVIDENCE => {
                let inner = self.value()?;
                let tool = self.str()?;
                let target = self.str()?;
                let ts = self.u64()?;
                let raw_offset = self.option_u64()?;
                let raw_len = self.option_u64()?;
                let parent = if self.flag()? {
                    Some(std::sync::Arc::new(self.provenance()?))
                } else {
                    None
                };
                Ok(Value::Evidence {
                    inner: Box::new(inner),
                    provenance: std::sync::Arc::new(Provenance {
                        tool,
                        target,
                        ts,
                        raw_offset,
                        raw_len,
                        parent,
                    }),
                })
            }
            other => Err(format!(
                "{}: unknown constant tag {} (file is not a .rkbc, or is corrupt)",
                self.where_(),
                other
            )),
        }
    }

    fn opt_value(&mut self) -> Result<Option<Value>, String> {
        if self.flag()? {
            Ok(Some(self.value()?))
        } else {
            Ok(None)
        }
    }

    fn provenance(&mut self) -> Result<Provenance, String> {
        Ok(Provenance {
            tool: self.str()?,
            target: self.str()?,
            ts: self.u64()?,
            raw_offset: self.option_u64()?,
            raw_len: self.option_u64()?,
            parent: if self.flag()? {
                Some(std::sync::Arc::new(self.provenance()?))
            } else {
                None
            },
        })
    }

    /// A captured environment: ordered scopes, each a nameג†’value map. Scopes
    /// are serialized in order with their entries sorted by name, so the same
    /// capture set always produces the same bytes.
    fn env(&mut self) -> Result<Env, String> {
        let n = self.count("env scopes")?;
        let mut scopes = Vec::with_capacity(n);
        for _ in 0..n {
            let m = self.count("env entries")?;
            let mut map = std::collections::HashMap::with_capacity(m);
            for _ in 0..m {
                let k = self.str()?;
                let v = self.value()?;
                map.insert(k, v);
            }
            scopes.push(map);
        }
        Ok(Env { scopes })
    }

    /// Read one chunk: code, line table, constants, module namespaces,
    /// declared types and aliases.
    pub fn chunk(&mut self) -> Result<Chunk, String> {
        let code = self.bytes()?;
        let lines_n = self.count("line table")?;
        let mut lines = Vec::with_capacity(lines_n);
        for _ in 0..lines_n {
            lines.push(self.u32()?);
        }
        if lines.len() != code.len() {
            return Err(format!(
                "line table has {} entries for a {} byte code stream; the file is corrupt",
                lines.len(),
                code.len()
            ));
        }
        let consts_n = self.count("constant pool")?;
        let mut constants = Vec::with_capacity(consts_n);
        for _ in 0..consts_n {
            constants.push(self.value()?);
        }
        let cells_n = self.count("module cells")?;
        let mut module_cells = Vec::with_capacity(cells_n);
        for _ in 0..cells_n {
            let global = self.str()?;
            let n = self.count("module exports")?;
            let mut exports = Vec::with_capacity(n);
            for _ in 0..n {
                let e = self.str()?;
                let g = self.str()?;
                exports.push((e, g));
            }
            let m = self.count("module mutability")?;
            let mut mutable = Vec::with_capacity(m);
            for _ in 0..m {
                mutable.push(self.str()?);
            }
            module_cells.push(ModuleCell {
                global,
                exports,
                mutable,
            });
        }
        let types_n = self.count("type table")?;
        let mut types = Vec::with_capacity(types_n);
        for _ in 0..types_n {
            types.push(rkbc_ast::read_type(self)?);
        }
        let aliases_n = self.count("alias table")?;
        let mut aliases = Vec::with_capacity(aliases_n);
        for _ in 0..aliases_n {
            let name = self.str()?;
            let t = rkbc_ast::read_type(self)?;
            aliases.push((name, t));
        }
        Ok(Chunk {
            code,
            constants,
            lines,
            module_cells,
            types,
            aliases,
        })
    }
}

/// The writer half of the constant-tag map, plus the chunk codec.
impl Writer {
    /// Serialize one constant. A runtime handle is an error: none can be a
    /// constant of a compiled program, so seeing one means the compiler
    /// produced something `.rkbc` cannot faithfully represent ג€” and writing a
    /// file that silently drops it would be worse.
    pub fn value(&mut self, v: &Value) -> Result<(), String> {
        match v {
            Value::Nil => self.u8(T_NIL),
            Value::Bool(b) => {
                self.u8(T_BOOL);
                self.flag(*b);
            }
            Value::I8(n) => {
                self.u8(T_I8);
                self.u8(*n as u8);
            }
            Value::I16(n) => {
                self.u8(T_I16);
                self.u16(*n as u16);
            }
            Value::I32(n) => {
                self.u8(T_I32);
                self.u32(*n as u32);
            }
            Value::I64(n) => {
                self.u8(T_I64);
                self.i64(*n);
            }
            Value::U8(n) => {
                self.u8(T_U8);
                self.u8(*n);
            }
            Value::U16(n) => {
                self.u8(T_U16);
                self.u16(*n);
            }
            Value::U32(n) => {
                self.u8(T_U32);
                self.u32(*n);
            }
            Value::U64(n) => {
                self.u8(T_U64);
                self.u64(*n);
            }
            Value::F32(n) => {
                self.u8(T_F32);
                self.f32(*n);
            }
            Value::F64(n) => {
                self.u8(T_F64);
                self.f64(*n);
            }
            Value::Hex(n) => {
                self.u8(T_HEX);
                self.u64(*n);
            }
            Value::String(s) => {
                self.u8(T_STRING);
                self.str(s);
            }
            Value::Char(c) => {
                self.u8(T_CHAR);
                self.u32(*c as u32);
            }
            Value::Bytes(b) => {
                self.u8(T_BYTES);
                self.bytes(b);
            }
            Value::Tuple(items) => {
                self.u8(T_TUPLE);
                self.u32(items.len() as u32);
                for item in items.iter() {
                    self.value(item)?;
                }
            }
            Value::Array(items) => {
                self.u8(T_ARRAY);
                self.u32(items.len() as u32);
                for item in items.iter() {
                    self.value(item)?;
                }
            }
            Value::Map(m) => {
                self.u8(T_MAP);
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                self.u32(keys.len() as u32);
                for k in keys {
                    self.str(k);
                    self.value(&m[k])?;
                }
            }
            Value::Struct { name, fields } => {
                self.u8(T_STRUCT);
                self.str(name);
                let mut keys: Vec<&String> = fields.keys().collect();
                keys.sort();
                self.u32(keys.len() as u32);
                for k in keys {
                    self.str(k);
                    self.value(&fields[k])?;
                }
            }
            Value::Enum {
                name,
                variant,
                data,
            } => {
                self.u8(T_ENUM);
                self.str(name);
                self.str(variant);
                self.u32(data.len() as u32);
                for item in data.iter() {
                    self.value(item)?;
                }
            }
            Value::Function {
                params,
                body,
                captures,
            } => {
                self.u8(T_FUNCTION);
                rkbc_ast::write_params(self, params)?;
                rkbc_ast::write_stmts(self, body)?;
                self.env(captures)?;
            }
            Value::NativeFn(name, _) => {
                self.u8(T_NATIVE);
                self.str(name);
            }
            Value::Closure {
                code,
                params,
                name,
                captures,
            } => {
                self.u8(T_CLOSURE);
                self.chunk(code)?;
                rkbc_ast::write_param_shapes(self, params)?;
                self.str(name);
                let caps = captures.lock().unwrap();
                self.u32(caps.len() as u32);
                for v in caps.iter() {
                    self.value(v)?;
                }
            }
            Value::Result(ok, err) => {
                self.u8(T_RESULT);
                self.opt_value(ok.as_deref())?;
                self.opt_value(err.as_deref())?;
            }
            Value::Option(v) => {
                self.u8(T_OPTION);
                self.opt_value(v.as_deref())?;
            }
            Value::Regex(r) => {
                self.u8(T_REGEX);
                self.str(&r.pattern);
                self.str(&r.flags);
            }
            Value::Evidence { inner, provenance } => {
                self.u8(T_EVIDENCE);
                self.value(inner)?;
                self.provenance(provenance)?;
            }
            other => {
                return Err(format!(
                    "constant of type '{}' cannot be written to a .rkbc file: \
                     runtime handles (streams, channels, sockets, mmaps, FFI) only exist \
                     once a program is running, so a chunk holding one is not a \
                     self-contained artifact. This is a compiler bug; please report it.",
                    other.type_name()
                ))
            }
        }
        Ok(())
    }

    fn opt_value(&mut self, v: Option<&Value>) -> Result<(), String> {
        match v {
            Some(x) => {
                self.flag(true);
                self.value(x)
            }
            None => {
                self.flag(false);
                Ok(())
            }
        }
    }

    fn provenance(&mut self, p: &Provenance) -> Result<(), String> {
        self.str(&p.tool);
        self.str(&p.target);
        self.u64(p.ts);
        self.option_u64(p.raw_offset);
        self.option_u64(p.raw_len);
        match &p.parent {
            Some(parent) => {
                self.flag(true);
                self.provenance(parent)?;
            }
            None => self.flag(false),
        }
        Ok(())
    }

    fn env(&mut self, env: &Env) -> Result<(), String> {
        self.u32(env.scopes.len() as u32);
        for scope in &env.scopes {
            let mut keys: Vec<&String> = scope.keys().collect();
            keys.sort();
            self.u32(keys.len() as u32);
            for k in keys {
                self.str(k);
                self.value(&scope[k])?;
            }
        }
        Ok(())
    }

    /// One `ModuleCell` (an imported module's namespace table).
    fn module_cell(&mut self, cell: &ModuleCell) -> Result<(), String> {
        self.str(&cell.global);
        self.u32(cell.exports.len() as u32);
        for (export, global) in &cell.exports {
            self.str(export);
            self.str(global);
        }
        self.u32(cell.mutable.len() as u32);
        for m in &cell.mutable {
            self.str(m);
        }
        Ok(())
    }

    /// Serialize a whole chunk: code, constants, line table, module namespaces,
    /// declared types and type aliases.
    pub fn chunk(&mut self, chunk: &Chunk) -> Result<(), String> {
        self.bytes(&chunk.code);
        self.u32(chunk.lines.len() as u32);
        for l in &chunk.lines {
            self.u32(*l);
        }
        self.u32(chunk.constants.len() as u32);
        for c in &chunk.constants {
            self.value(c)?;
        }
        self.u32(chunk.module_cells.len() as u32);
        for cell in &chunk.module_cells {
            self.module_cell(cell)?;
        }
        self.u32(chunk.types.len() as u32);
        for t in &chunk.types {
            rkbc_ast::write_type(self, t)?;
        }
        self.u32(chunk.aliases.len() as u32);
        for (name, t) in &chunk.aliases {
            self.str(name);
            rkbc_ast::write_type(self, t)?;
        }
        Ok(())
    }
}

/// Serialize a chunk into a complete `.rkbc` container.
pub fn encode_chunk(chunk: &Chunk) -> Result<Vec<u8>, String> {
    let mut w = Writer::new();
    w.buf.extend_from_slice(MAGIC);
    w.u16(FORMAT_VERSION);
    w.chunk(chunk)?;
    Ok(w.into_bytes())
}

/// Parse a `.rkbc` container back into a chunk. A wrong magic, a newer format
/// version, or trailing garbage is an error that says which.
pub fn decode_chunk(bytes: &[u8]) -> Result<Chunk, String> {
    if bytes.len() < 6 || &bytes[..4] != MAGIC {
        return Err("not a .rkbc file (bad magic)".to_string());
    }
    let mut r = Reader::new(bytes);
    r.pos = 4;
    let version = r.u16()?;
    if version > FORMAT_VERSION {
        return Err(format!(
            "this .rkbc is format version {}, but this rakc understands up to {}. \
             Recompile with `rakc build --emit=bytecode`.",
            version, FORMAT_VERSION
        ));
    }

    let chunk = r.chunk()?;

    if !r.is_empty() {
        return Err(format!(
            "{} trailing bytes after the chunk; the file is corrupt",
            r.buf.len() - r.pos
        ));
    }

    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk_with(c: Value) -> Chunk {
        Chunk {
            code: vec![1, 2, 3],
            constants: vec![c],
            lines: vec![10, 20, 30],
            module_cells: vec![],
            types: vec![],
            aliases: vec![],
        }
    }

    #[test]
    fn roundtrip_scalar_constants() {
        let values = vec![
            Value::Nil,
            Value::Bool(true),
            Value::I64(-42),
            Value::Hex(0xDEAD_BEEF),
            Value::F64(1.0 / 3.0),
            Value::F32(0.5),
            Value::String(std::sync::Arc::from("hi")),
            Value::Char('A'),
            Value::Bytes(std::sync::Arc::from([1u8, 2, 255].as_slice())),
        ];
        for v in values {
            let chunk = chunk_with(v.clone());
            let bytes = encode_chunk(&chunk).expect("encode");
            let back = decode_chunk(&bytes).expect("decode");
            assert_eq!(back.code, chunk.code, "{:?}", v);
            assert_eq!(back.lines, chunk.lines);
            assert_eq!(back.constants.len(), 1, "{:?}", v);
            // Numeric representations compare through PartialEq; the point of
            // the roundtrip is that nothing panicked or changed shape.
            assert!(
                back.constants[0] == v || format!("{:?}", back.constants[0]) == format!("{:?}", v),
                "{:?}",
                v
            );
        }
    }

    #[test]
    fn container_rejects_a_non_rkbc_file() {
        let err = decode_chunk(b"this is not bytecode").unwrap_err();
        assert!(err.contains("magic"), "{}", err);
    }

    #[test]
    fn container_rejects_truncation_without_panicking() {
        let chunk = chunk_with(Value::String(std::sync::Arc::from("hello")));
        let bytes = encode_chunk(&chunk).unwrap();
        for cut in 0..bytes.len() {
            let _ = decode_chunk(&bytes[..cut]);
        }
        // The final assert: full length still decodes.
        assert!(decode_chunk(&bytes).is_ok());
    }

    #[test]
    fn trailing_bytes_are_reported() {
        let chunk = chunk_with(Value::Nil);
        let mut bytes = encode_chunk(&chunk).unwrap();
        bytes.extend_from_slice(b"junkjunk");
        let err = decode_chunk(&bytes).unwrap_err();
        assert!(err.contains("trailing"), "{}", err);
    }

    #[test]
    fn line_table_must_match_the_code_stream() {
        let mut chunk = chunk_with(Value::Nil);
        chunk.lines = vec![1, 2]; // code is 3 bytes
        let bytes = encode_chunk(&chunk).unwrap();
        let err = decode_chunk(&bytes).unwrap_err();
        assert!(err.contains("corrupt"), "{}", err);
    }
}
