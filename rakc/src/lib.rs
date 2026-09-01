pub mod lexer;
pub mod parser;
pub mod ast;
pub mod interpreter;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RakError {
    #[error("Lexer error: {0}")]
    Lexer(String),
    #[error("Parser error: {0}")]
    Parser(String),
    #[error("Runtime error: {0}")]
    Runtime(String),
}

pub type Result<T> = std::result::Result<T, RakError>;

/// Compile a Rak source file into an executable or intermediate representation.
pub fn compile(source: &str) -> Result<()> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let _ = ast;
    Ok(())
}

/// Evaluate Rak code in interpreter mode.
pub fn eval(source: &str) -> Result<Vec<String>> {
    let mut interpreter = interpreter::Interpreter::new();
    interpreter.run_source(source)
}