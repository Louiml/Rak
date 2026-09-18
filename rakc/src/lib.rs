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

/// Machine-readable classification of a Raven error. Mirrors the user-facing
/// `kind` exposed on the `Value::Error` runtime value and on `catch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Runtime,
    Io,
    Network,
    Parse,
    Compile,
    Type,
    Package,
    Permission,
    User,   // from `raise`/`throw` with a string
    Timeout,
    Cancel,
}

impl ErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorKind::Runtime => "runtime",
            ErrorKind::Io => "io",
            ErrorKind::Network => "network",
            ErrorKind::Parse => "parse",
            ErrorKind::Compile => "compile",
            ErrorKind::Type => "type",
            ErrorKind::Package => "package",
            ErrorKind::Permission => "permission",
            ErrorKind::User => "user",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cancel => "cancel",
        }
    }
}

/// Rich, structured error metadata carried by runtime `Value::Error` values and
/// by `RakError`. Includes message, kind, best-effort source location, cause,
/// a free-form context map, and a captured backtrace.
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub message: String,
    pub kind: ErrorKind,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub cause: Option<String>,
    pub context: Vec<(String, String)>,
    pub backtrace: String,
}

impl ErrorInfo {
    pub fn new(message: impl Into<String>) -> Self {
        ErrorInfo {
            message: message.into(),
            kind: ErrorKind::Runtime,
            file: None,
            line: None,
            col: None,
            cause: None,
            context: Vec::new(),
            backtrace: capture_backtrace(),
        }
    }
    pub fn with_kind(mut self, kind: ErrorKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn with_span(mut self, file: String, line: u32, col: u32) -> Self {
        self.file = Some(file);
        self.line = Some(line);
        self.col = Some(col);
        self
    }
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }
    pub fn with_context(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.context.push((k.into(), v.into()));
        self
    }
}

fn capture_backtrace() -> String {
    #[cfg(feature = "backtrace")]
    {
        let bt = std::backtrace::Backtrace::capture();
        bt.to_string()
    }
    #[cfg(not(feature = "backtrace"))]
    {
        // Light-weight caller frames captured during interpretation provides a
        // Rak-level backtrace; Rust native backtrace needs the "backtrace"
        // feature. Keep a placeholder here.
        String::from("<backtrace available with --features backtrace>")
    }
}

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

impl RakError {
    /// Convert into a structured `ErrorInfo`. `Internally`, exceptions in the
    /// interpreter are captured as runtime values (see `Value::Error`), so
    /// this mainly serves non-Raised runtime/parse errors surfaced to scripts.
    pub fn to_info(&self) -> ErrorInfo {
        match self {
            RakError::Lexer(s) => ErrorInfo::new(s.clone()).with_kind(ErrorKind::Parse),
            RakError::Parser(s) => ErrorInfo::new(s.clone()).with_kind(ErrorKind::Parse),
            RakError::Runtime(s) => ErrorInfo::new(s.clone()).with_kind(ErrorKind::Runtime),
            RakError::Raise(s) => ErrorInfo::new(s.clone()).with_kind(ErrorKind::User),
        }
    }
    pub fn kind(&self) -> ErrorKind {
        match self {
            RakError::Lexer(_) | RakError::Parser(_) => ErrorKind::Parse,
            RakError::Raise(_) => ErrorKind::User,
            RakError::Runtime(_) => ErrorKind::Runtime,
        }
    }
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

/// Evaluate Rak code as a CLI program: runs the top-level script, then, if a
/// `fn main(args)` is defined, calls it with `argv` and returns its `int`
/// result as the process exit code (0 if no `main`). Returns `(output, exit_code)`.
pub fn eval_cli(source: &str, argv: &[String]) -> Result<(Vec<String>, i32)> {
    let mut interpreter = interpreter::Interpreter::new();
    let output = interpreter.run_source(source)?;
    let code = interpreter.run_main(argv);
    Ok((output, code))
}

/// CLI variant of `eval_in`: resolves imports against `base_dir` and runs a
/// `fn main(args)` if present, returning `(output, exit_code)`.
pub fn eval_in_cli(source: &str, base_dir: &str, argv: &[String]) -> Result<(Vec<String>, i32)> {
    let mut interpreter = interpreter::Interpreter::with_base_dir(base_dir.to_string());
    let output = interpreter.run_source(source)?;
    let code = interpreter.run_main(argv);
    Ok((output, code))
}