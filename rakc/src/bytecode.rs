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
}

impl Op {
    pub fn from_u8(b: u8) -> Option<Op> {
        if (b as usize) <= Op::LoopEnd as usize {
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
