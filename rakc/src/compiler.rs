use crate::ast::Module;
use crate::bytecode::Chunk;

pub struct Compiler {
    chunk: Chunk,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler { chunk: Chunk::new() }
    }

    pub fn compile(&mut self, _module: &Module) -> Result<Chunk, String> {
        Ok(std::mem::replace(&mut self.chunk, Chunk::new()))
    }
}

pub fn compile_module(module: &Module) -> Result<Chunk, String> {
    let mut compiler = Compiler::new();
    compiler.compile(module)
}
