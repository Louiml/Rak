use crate::bytecode::Chunk;
use crate::value::Value;

pub struct ChannelHandle {
    pub id: u64,
}

pub struct FutureHandle {
    pub id: u64,
}

pub struct Vm {
    pub chunk: Chunk,
    pub stack: Vec<Value>,
    pub locals: Vec<Value>,
    pub globals: std::collections::HashMap<String, Value>,
    pub ip: usize,
}

impl Vm {
    pub fn new(chunk: Chunk) -> Self {
        Vm {
            chunk,
            stack: Vec::new(),
            locals: Vec::new(),
            globals: std::collections::HashMap::new(),
            ip: 0,
        }
    }

    pub fn run(&mut self) -> Result<Value, String> {
        Ok(Value::Nil)
    }
}
