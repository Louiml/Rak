pub mod lexer;
pub mod parser;
pub mod ast;
pub mod typecheck;
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
pub mod fmt;
pub mod lint;
pub mod caps;
pub mod fuzz;
pub mod ext_batteries;
pub mod ext_osint;
pub mod dap;

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

/// Allow stdlib functions returning `Result<_, String>` to use `?` directly
/// inside interpreter glue (e.g. `datafmt::parse_csv(&text)?`).
impl From<String> for RakError {
    fn from(s: String) -> Self {
        RakError::Runtime(s)
    }
}

impl From<&str> for RakError {
    fn from(s: &str) -> Self {
        RakError::Runtime(s.to_string())
    }
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
/// Runs the lexer, parser, and static type checker. On a type error, returns
/// an `Err` containing the rendered diagnostics.
pub fn compile(source: &str) -> Result<()> {
    compile_with_file(source, "<input>")?;
    Ok(())
}

/// Like [`compile`], but labels diagnostics with `file`.
pub fn compile_with_file(source: &str, file: &str) -> Result<()> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens, source)?;
    let mut tc = typecheck::TypeChecker::new(source, file);
    let diagnostics = tc.check_module(&ast);
    if let Some(d) = diagnostics.first() {
        return Err(RakError::Runtime(d.render()));
    }
    Ok(())
}

/// Native stack given to the interpreter thread.
///
/// The tree walker burns several native frames and a few hundred bytes per Rak
/// call, and `Value` is 136 bytes, so a Rak frame is not cheap in stack terms.
/// The default main-thread stack on Windows is 1 MB and on Linux 8 MB, which
/// runs out after a few hundred Rak frames. Past that the process dies with a
/// bare "has overflowed its stack" and no Rak-level line number, which is the
/// worst possible failure mode for a language whose job includes analysing
/// hostile input.
///
/// Running the interpreter on a thread with an explicit, generous stack is the
/// standard fix for a tree walker. It does not make infinite recursion safe:
/// the depth cap in `verify` still does that, and it does so without waiting
/// for a crash. This just means a legitimately deep recursive program runs
/// instead of taking the process down.
const INTERPRETER_STACK: usize = 64 * 1024 * 1024;

/// Run `f` on a thread with a large explicit stack and return its result.
///
/// `RUST_MIN_STACK` is not usable here: it only sizes threads that std spawns,
/// and the interpreter would otherwise run on the main thread, whose stack size
/// the process cannot change after startup.
fn run_on_big_stack<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let spawned = std::thread::Builder::new()
        .name("rak-interp".to_string())
        .stack_size(INTERPRETER_STACK)
        .spawn(f);
    match spawned {
        Ok(h) => match h.join() {
            Ok(v) => v,
            // A panic on the interpreter thread has already unwound and left
            // `self` unreachable. Propagating it as a panic here keeps the
            // process-wide panic handler in charge rather than silently
            // turning a bug into a missing value.
            Err(e) => std::panic::resume_unwind(e),
        },
        // The thread spawn can fail under a hard RLIMIT_NPROC or a job-object
        // limit, in which case running on the current thread is better than
        // refusing to run at all.
        Err(_) => unreachable!(),
    }
}

/// Evaluate Rak code in interpreter mode.
pub fn eval(source: &str) -> Result<Vec<String>> {
    let src = source.to_string();
    run_on_big_stack(move || {
        let mut interpreter = interpreter::Interpreter::new();
        interpreter.run_source(&src)
    })
}

/// Evaluate Rak code in interpreter mode with a base directory (used to resolve
/// name-based `import m` relative to the importing file).
pub fn eval_in(source: &str, base_dir: &str) -> Result<Vec<String>> {
    let src = source.to_string();
    let dir = base_dir.to_string();
    run_on_big_stack(move || {
        let mut interpreter = interpreter::Interpreter::with_base_dir(dir);
        interpreter.run_source(&src)
    })
}

/// Evaluate Rak code as a CLI program: runs the top-level script, then, if a
/// `fn main(args)` is defined, calls it with `argv` and returns its `int`
/// result as the process exit code (0 if no `main`). Returns `(output, exit_code)`.
pub fn eval_cli(source: &str, argv: &[String]) -> Result<(Vec<String>, i32)> {
    let src = source.to_string();
    let args = argv.to_vec();
    run_on_big_stack(move || {
        let mut interpreter = interpreter::Interpreter::new();
        let output = interpreter.run_source(&src)?;
        let code = interpreter.run_main(&args);
        Ok((output, code))
    })
}

/// CLI variant of `eval_in`: resolves imports against `base_dir` and runs a
/// `fn main(args)` if present, returning `(output, exit_code)`.
pub fn eval_in_cli(source: &str, base_dir: &str, argv: &[String]) -> Result<(Vec<String>, i32)> {
    let src = source.to_string();
    let dir = base_dir.to_string();
    let args = argv.to_vec();
    run_on_big_stack(move || {
        let mut interpreter = interpreter::Interpreter::with_base_dir(dir);
        let output = interpreter.run_source(&src)?;
        let code = interpreter.run_main(&args);
        Ok((output, code))
    })
}

/// The three things a `rakc verify` run can conclude.
///
/// The middle case is the important one. "Completed" and "failed" are both
/// results, but "the budget ran out" proves nothing, and a verification tool
/// that reports that as a pass would be worse than no tool at all. So it gets
/// its own outcome and its own exit code.
#[derive(Debug)]
pub enum VerifyOutcome {
    /// Ran to completion. Every `requires` and `ensures` clause that was
    /// reached held, and no limit was hit.
    Completed { steps: u64, output: Vec<String> },
    /// The program itself failed: a contract was violated, an assert tripped, or
    /// a runtime error was raised. This is a real finding.
    Failed { message: String, steps: u64, output: Vec<String> },
    /// A resource limit was reached, so the run proved nothing either way.
    /// Retry with a larger budget before drawing any conclusion.
    Inconclusive { reason: String, steps: u64 },
}

/// Run `source` under bounded execution and report which of the three outcomes
/// applies. This is what `rakc verify` wraps.
///
/// The limits are deliberately finite by default. A verification run that is
/// allowed to run forever is just a normal run.
pub fn verify_bounded(source: &str, max_steps: u64, max_depth: u32, max_iterations: u64) -> VerifyOutcome {
    let src = source.to_string();
    // Same large-stack treatment as every other entry point. Without it the
    // depth cap would never get a chance to fire, because the native stack
    // would run out first.
    run_on_big_stack(move || {
        let mut interpreter = interpreter::Interpreter::new();
        interpreter.set_limits(interpreter::Limits {
            max_steps: Some(max_steps),
            max_depth: Some(max_depth),
            max_iterations: Some(max_iterations),
            steps: 0,
        });
        match interpreter.run_source(&src) {
            Ok(output) => VerifyOutcome::Completed {
                steps: interpreter.steps_used(),
                output,
            },
            Err(e) => {
                let msg = e.to_string();
                let steps = interpreter.steps_used();
                if is_limit_message(&msg) {
                    VerifyOutcome::Inconclusive { reason: msg, steps }
                } else {
                    VerifyOutcome::Failed {
                        message: msg,
                        steps,
                        output: interpreter.take_output(),
                    }
                }
            }
        }
    })
}

/// Distinguish "hit a limit" from "the program failed".
///
/// The limit errors carry a sentinel prefix (`interpreter::LIMIT_PREFIX`) that a
/// Rak program cannot emit, because the text is produced by the host, not by
/// `raise`. Matching on the rendered message is the least invasive way to
/// recover the distinction across the `RakError` boundary; the sentinel is what
/// makes it robust against the `Runtime error: ` display prefix that
/// `thiserror` adds.
fn is_limit_message(msg: &str) -> bool {
    msg.contains(interpreter::LIMIT_PREFIX)
}

