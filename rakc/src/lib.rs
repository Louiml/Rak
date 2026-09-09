pub mod lexer;
pub mod parser;
pub mod ast;
pub mod interpreter;
pub mod value;
pub mod bytecode;
pub mod compiler;
pub mod vm;
pub mod async_rt;
pub mod modules;
pub mod repl;

#[cfg(feature = "gui")]
pub mod gui;
#[cfg(feature = "lsp")]
pub mod lsp;
#[cfg(feature = "bindgen")]
pub mod bindgen;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RakError {
    #[error("Lexer error: {0}")]
    Lexer(String),
    #[error("Parser error: {0}")]
    Parser(String),
    #[error("Runtime error: {0}")]
    Runtime(String),
    #[error("Raised: {0}")]
    Raise(String),
}

pub type Result<T> = std::result::Result<T, RakError>;

/// Compile a Rak source file into an executable or intermediate representation.
pub fn compile(source: &str) -> Result<()> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens, source)?;
    let _ = ast;
    Ok(())
}

/// Evaluate Rak code in interpreter mode.
pub fn eval(source: &str) -> Result<Vec<String>> {
    let mut interpreter = interpreter::Interpreter::new();
    interpreter.run_source(source)
}

/// Evaluate Rak code in interpreter mode with a base directory (used to resolve
/// name-based `import m` relative to the importing file).
pub fn eval_in(source: &str, base_dir: &str) -> Result<Vec<String>> {
    let mut interpreter = interpreter::Interpreter::with_base_dir(base_dir.to_string());
    interpreter.run_source(source)
}