#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Op {
    Nop,
    LoadConst,
    LoadLocal,
    StoreLocal,
    LoadGlobal,
    StoreGlobal,
    Pop,
    Dup,
    Swap,

    AddI,
    SubI,
    MulI,
    DivI,
    RemI,
    NegI,
    AddF,
    SubF,
    MulF,
    DivF,
    NegF,

    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    Shl,
    Shr,

    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    Not,

    True,
    False,
    Nil,

    NewTuple,
    NewArray,
    NewMap,
    NewStruct,
    NewEnum,

    IndexGet,
    IndexSet,
    FieldGet,
    FieldSet,

    Jump,
    JumpIfFalse,
    JumpIfTrue,

    Call,
    CallNative,
    Return,
    Yield,
    Await,
    Spawn,

    TryBegin,
    TryEnd,
    Raise,

    Print,
    Trace,

    Closure,
    GetUpvalue,
    SetUpvalue,
}

#[derive(Clone, Debug)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<crate::value::Value>,
    pub lines: Vec<u32>,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk { code: Vec::new(), constants: Vec::new(), lines: Vec::new() }
    }

    pub fn write(&mut self, byte: u8, line: u32) {
        self.code.push(byte);
        self.lines.push(line);
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

    pub fn read_u16(&self, offset: usize) -> usize {
        ((self.code[offset] as usize) << 8) | (self.code[offset + 1] as usize)
    }

    pub fn write_u16(&mut self, value: u16, line: u32) {
        self.write((value >> 8) as u8, line);
        self.write((value & 0xFF) as u8, line);
    }
}
