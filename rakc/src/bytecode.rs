#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Op {
    Nop = 0,
    LoadConst,
    LoadLocal,
    StoreLocal,
    LoadGlobal,
    StoreGlobal,
    Pop,
    Dup,

    AddI, SubI, MulI, DivI, RemI, NegI,
    AddF, SubF, MulF, DivF, NegF,

    BitAnd, BitOr, BitXor, BitNot, Shl, Shr,

    Eq, NotEq, Lt, Gt, LtEq, GtEq,
    Not,

    True, False, Nil,

    Jump, JumpIfFalse, JumpIfTrue,

    Call,
    Return,
    Print,
    Trace,

    NewArray,
    NewTuple,
    NewMap,

    IndexGet,
    FieldGet,

    Closure,
    GetUpvalue,
    SetUpvalue,

    LoopBegin,
    LoopEnd,

    /// Pop the args array value, pop the symbol string, pop the `ForeignLib`
    /// handle, unpack the array into C arguments, and call the symbol via the
    /// dynamic loader. Push the `i64` result. No operand.
    FFICall,
    /// Pop a `ForeignLib` handle (best-effort early release). Push nil.
    FFIClose,
    /// Pop a value; if it's a `Future`, resolve it (block on the async
    /// runtime). Push the resolved value (or the value itself if not a future).
    Await,
    /// Build a `Value::Module` from `n` (name, value) pairs. Operand: `n` (1 byte).
    /// Pops `2*n` values (alternating name string, value), pushes the Module.
    BuildModule,
    /// Register a deferred call. The callee and its args (already evaluated)
    /// are pushed onto a per-frame stack and run in LIFO order when the frame
    /// returns. Operand: argument count `n` (1 byte). Pops `n` args then the
    /// callee.
    DeferCall,
    /// Begin a `try` region. Operand: u16 handler offset (patched by the
    /// compiler). Pushes a catch frame onto the VM's handler stack for this
    /// frame; on error the VM unwinds to the handler with the error value.
    Try,
    /// Pop the catch frame for the current `try` (the body completed without
    /// raising).
    CatchEnd,
    /// Raise: pop a value; if it's already a `Value::Error` bind it as-is,
    /// otherwise wrap its string form as a `User` error. Unwinds to the
    /// nearest enclosing `try` handler (or fails the run).
    Throw,
    /// `expr?` — pop a `Result`/`Option`; `Ok(v)`/`Some(v)` push `v`,
    /// `Err(e)`/`None` raise (bindable by `catch`).
    TryUnwrap,
    /// Method call `obj.method(args...)`. Operands: u16 method-name const
    /// index, u8 argument count. Pops `argc` args then the receiver; dispatch
    /// order: native regex methods, `__method_<type>_<name>` globals (baked
    /// from `impl` blocks), then a field holding a callable.
    CallMethod,
    /// Build a `Value::Struct`. Operands: u16 name-const index, u8 field
    /// count `n`. Pops `2*n` values (alternating field-name const, value).
    StructNew,
    /// Structural pattern match. Operands: u16 descriptor-const index, u16
    /// bind-names-const index. Pops the grow-the-bindings-array indicator
    /// then the descriptor then the scrutinee; pushes a truthy `Value::Array`
    /// of bound values (ordered as the descriptor's bind names, `Nil` where a
    /// bind was absent) on a match, or `Value::Nil` on no match.
    MatchPat,
    /// `obj[idx] = value`. Pops value, then index, then object; mutates the
    /// container in place (via copy-on-write) and pushes the container back so
    /// the caller can store it back to its binding.
    IndexSet,
    /// `obj.field = value`. Operand: u16 field-name const index. Pops value,
    /// then object; mutates and pushes the container back (copy-on-write).
    FieldSet,
    /// Jump if the top-of-stack is `nil` or `None` (does not pop). Used by
    /// `??`, `?.`, `?[`. Operand: u16 target offset.
    JumpIfNil,
    /// Materialize a container into its iteration items (an array): arrays and
    /// tuples pass through, maps become `(key, value)` tuples, strings become
    /// chars. Operand: u8 flag — 1 yields `(index, item)` pairs for
    /// arrays/strings (matching the interpreter's 2-tuple "indexed for").
    IterItems,
}

impl Op {
    pub fn from_u8(b: u8) -> Option<Op> {
        if (b as usize) <= Op::IterItems as usize {
            Some(unsafe { std::mem::transmute(b) })
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<crate::value::Value>,
    pub lines: Vec<u32>,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk::default()
    }

    pub fn write_op(&mut self, op: Op, line: u32) {
        self.code.push(op as u8);
        self.lines.push(line);
    }

    pub fn write_byte(&mut self, b: u8, line: u32) {
        self.code.push(b);
        self.lines.push(line);
    }

    pub fn write_u16(&mut self, v: u16, line: u32) {
        self.write_byte((v >> 8) as u8, line);
        self.write_byte((v & 0xFF) as u8, line);
    }

    pub fn add_const(&mut self, value: crate::value::Value) -> u16 {
        for (i, c) in self.constants.iter().enumerate() {
            if c == &value {
                return i as u16;
            }
        }
        let idx = self.constants.len() as u16;
        self.constants.push(value);
        idx
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        ((self.code[offset] as u16) << 8) | (self.code[offset + 1] as u16)
    }
}
