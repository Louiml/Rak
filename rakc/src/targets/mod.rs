//! The multi-target compilation framework.
//!
//! rakc used to have exactly two ways to execute a program (`run` on the
//! interpreter, `vm` on the bytecode VM) and one way to package it (`build`'s
//! self-extracting executable). This module turns those special cases into a
//! uniform rustc-shaped model:
//!
//! ```text
//! source ──lex──> tokens ──parse──> AST ──check──> Target::emit ──> Artifact
//!                                              (bytecode | c | wasm | llvm | exe | ...)
//! ```
//!
//! The front end is shared by every target; a target is anything that can turn
//! a checked [`ast::Module`] into [`Artifact`]s for a [`Triple`]. Adding a
//! target means implementing [`Target`] and adding it to [`BACKENDS`]; the CLI,
//! the `--emit` parser, and the error messages all follow from that table.

pub mod bytecode;
pub mod c;
pub mod exe;
pub mod expand;
pub mod llvm;
pub mod rkbc;
pub mod rkbc_ast;
pub mod toolchain;
pub mod triple;
pub mod wasm;

pub use triple::{find as find_triple, host_triple, Triple, SUPPORTED};

use crate::ast::Module;
use std::path::{Path, PathBuf};

/// What a compilation produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitKind {
    /// Run the program on the interpreter now; no artifact.
    Interp,
    /// A serialized bytecode container (`.rkbc`).
    Bytecode,
    /// A human-readable bytecode listing.
    BytecodeText,
    /// C source text.
    CSource,
    /// WebAssembly binary (`.wasm`).
    Wasm,
    /// LLVM IR text (`.ll`).
    LlvmIr,
    /// A native object file (via the host C toolchain).
    Object,
    /// A self-contained executable with the source embedded (the historical
    /// `rakc build` output).
    Exe,
    /// A linked native executable for the target.
    Link,
}

impl EmitKind {
    /// Parse one `--emit` name. The names are what the CLI accepts, so the
    /// parser and the help text cannot drift.
    pub fn parse(s: &str) -> Option<EmitKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "interp" | "interpret" | "run" => Some(EmitKind::Interp),
            "bytecode" | "rkbc" => Some(EmitKind::Bytecode),
            "bytecode-text" | "disasm" => Some(EmitKind::BytecodeText),
            "c" | "c-source" => Some(EmitKind::CSource),
            "wasm" => Some(EmitKind::Wasm),
            "llvm-ir" | "llir" => Some(EmitKind::LlvmIr),
            "obj" | "object" => Some(EmitKind::Object),
            "exe" => Some(EmitKind::Exe),
            "link" => Some(EmitKind::Link),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EmitKind::Interp => "interp",
            EmitKind::Bytecode => "bytecode",
            EmitKind::BytecodeText => "bytecode-text",
            EmitKind::CSource => "c",
            EmitKind::Wasm => "wasm",
            EmitKind::LlvmIr => "llvm-ir",
            EmitKind::Object => "obj",
            EmitKind::Exe => "exe",
            EmitKind::Link => "link",
        }
    }

    /// The file extension an artifact of this kind carries when written next to
    /// the source, given the target.
    pub fn extension(&self, triple: &Triple) -> &'static str {
        match self {
            EmitKind::Interp => "",
            EmitKind::Bytecode => ".rkbc",
            EmitKind::BytecodeText => ".rkbc.txt",
            EmitKind::CSource => ".c",
            EmitKind::Wasm => ".wasm",
            EmitKind::LlvmIr => ".ll",
            EmitKind::Object => {
                if triple.is_windows() {
                    ".obj"
                } else {
                    ".o"
                }
            }
            EmitKind::Exe | EmitKind::Link => {
                if triple.is_windows() {
                    ".exe"
                } else {
                    ""
                }
            }
        }
    }

    /// Whether this emit kind produces an artifact file at all.
    pub fn is_file(&self) -> bool {
        !matches!(self, EmitKind::Interp)
    }
}

/// Parse a comma-separated `--emit` list. An unknown name is an error naming
/// the accepted set — the same rule the CLI applies to every other flag.
pub fn parse_emit_list(s: &str) -> Result<Vec<EmitKind>, String> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match EmitKind::parse(part) {
            Some(kind) => {
                if !out.contains(&kind) {
                    out.push(kind);
                }
            }
            None => {
                return Err(format!(
                    "unknown emit kind '{}'\n\naccepted: {}",
                    part,
                    accepted_emit_list()
                ))
            }
        }
    }
    if out.is_empty() {
        return Err(format!(
            "empty --emit list\n\naccepted: {}",
            accepted_emit_list()
        ));
    }
    Ok(out)
}

/// The accepted `--emit` names, for error messages and `--help`.
pub fn accepted_emit_list() -> &'static str {
    "interp, bytecode, bytecode-text, c, wasm, llvm-ir, obj, exe, link"
}

/// Optimisation level, mirroring `rustc -C opt-level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptLevel {
    None,
    Less,
    Default,
    Aggressive,
    Size,
    SizeZ,
}

impl OptLevel {
    pub fn parse(s: &str) -> Option<OptLevel> {
        match s.trim() {
            "0" | "none" => Some(OptLevel::None),
            "1" | "less" => Some(OptLevel::Less),
            "2" | "default" => Some(OptLevel::Default),
            "3" | "aggressive" => Some(OptLevel::Aggressive),
            "s" | "size" => Some(OptLevel::Size),
            "z" => Some(OptLevel::SizeZ),
            _ => None,
        }
    }

    /// The flag a C toolchain understands (`-O0` .. `-O3`, `-Os`, `-Oz`).
    pub fn c_flag(&self) -> &'static str {
        match self {
            OptLevel::None => "-O0",
            OptLevel::Less => "-O1",
            OptLevel::Default => "-O2",
            OptLevel::Aggressive => "-O3",
            OptLevel::Size => "-Os",
            OptLevel::SizeZ => "-Oz",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            OptLevel::None => "0",
            OptLevel::Less => "1",
            OptLevel::Default => "2",
            OptLevel::Aggressive => "3",
            OptLevel::Size => "s",
            OptLevel::SizeZ => "z",
        }
    }
}

/// What happens on a Rak-level `raise` that nothing catches, and on overflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanicStrategy {
    /// Catch it, print `Runtime error: ...`, exit 1.
    Unwind,
    /// Terminate the process immediately (no unwinding).
    Abort,
}

/// Everything the `rakc build` flags control.
#[derive(Clone, Debug)]
pub struct CodegenOptions {
    pub emit: Vec<EmitKind>,
    pub triple: Triple,
    pub opt_level: OptLevel,
    pub debuginfo: u32,
    pub panic: PanicStrategy,
    /// `-o <path>` / `--out-dir <path>`. A file for one artifact, a directory
    /// for several.
    pub out_path: Option<PathBuf>,
    /// The source file, for default artifact naming and diagnostics.
    pub source_path: String,
    /// The directory name-based imports resolve against (the source's parent).
    pub base_dir: String,
}

impl CodegenOptions {
    pub fn new(source_path: &str) -> Self {
        let base_dir = Path::new(source_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ".".to_string());
        CodegenOptions {
            // The historical `rakc build` output: a standalone executable
            // with the source embedded. Everything else is opt-in through
            // `--emit`.
            emit: vec![EmitKind::Exe],
            triple: host_triple(),
            opt_level: OptLevel::Default,
            debuginfo: 0,
            panic: PanicStrategy::Unwind,
            out_path: None,
            source_path: source_path.to_string(),
            base_dir,
        }
    }

    /// The artifact stem for the default output name: `src/main.rak` -> `main`.
    pub fn stem(&self) -> String {
        let p = Path::new(&self.source_path);
        p.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "a".to_string())
    }
}

/// One compiled output. `Text` artifacts print to stdout when they have no
/// destination, which is what makes `--emit=c` usable without `-o`.
pub enum Artifact {
    /// Raw bytes to write to the destination.
    File { kind: EmitKind, data: Vec<u8> },
    /// Text that goes to stdout unless `-o` names a file.
    Text { kind: EmitKind, text: String },
    /// Already written by the backend at `path`. Used by the backends that
    /// invoke an external toolchain (the C compiler): the file is the tool's
    /// own output at its final location, so re-writing it would be a wasted
    /// copy of a possibly large binary.
    Written { kind: EmitKind, path: PathBuf },
}

impl Artifact {
    pub fn kind(&self) -> EmitKind {
        match self {
            Artifact::File { kind, .. }
            | Artifact::Text { kind, .. }
            | Artifact::Written { kind, .. } => *kind,
        }
    }
}

/// A compilation target: something that can turn a checked module into
/// artifacts for a triple.
pub trait Target {
    /// The `--emit` name(s) this backend produces.
    fn id(&self) -> &'static str;

    /// Whether this backend can produce `kind` for `triple`. Returning false
    /// yields a clear error rather than a wrong artifact.
    fn supports(&self, kind: EmitKind, triple: &Triple) -> bool;

    /// Compile `module` into the requested artifacts.
    fn emit(
        &self,
        module: &Module,
        opts: &CodegenOptions,
        kind: EmitKind,
    ) -> Result<Artifact, String>;
}

/// Every backend rakc ships. The order is only for error messages that list
/// which backend could have handled an emit kind.
pub fn backends() -> Vec<Box<dyn Target>> {
    let mut v: Vec<Box<dyn Target>> = vec![
        Box::new(exe::SelfExe),
        Box::new(bytecode::BytecodeBackend),
        Box::new(c::CBackend),
        Box::new(wasm::WasmBackend),
    ];
    #[cfg(feature = "llvm")]
    v.push(Box::new(llvm::LlvmBackend));
    v
}

/// Compile `source` (already read from `opts.source_path`) into the artifacts
/// `opts.emit` asks for. Runs the shared front end exactly once, then hands
/// the checked module to whichever backend claims the emit kind.
///
/// `EmitKind::Interp` short-circuits: it runs the program and yields no
/// artifact, which is what makes `rakc build --emit=interp` an alias of `run`.
pub fn compile_and_emit(source: &str, opts: &CodegenOptions) -> Result<Vec<Artifact>, String> {
    // Front end, shared by every target: lex, parse, type-check.
    let tokens = crate::lexer::tokenize(source).map_err(|e| format!("Lexer error: {}", e))?;
    let module =
        crate::parser::parse(&tokens, source).map_err(|e| format!("Parser error: {}", e))?;
    let mut tc = crate::typecheck::TypeChecker::new(source, &opts.source_path);
    let diagnostics = tc.check_module(&module);
    if let Some(d) = diagnostics.first() {
        return Err(format!("{}", d.render()));
    }

    let registry = backends();
    let mut out = Vec::new();

    for kind in &opts.emit {
        if *kind == EmitKind::Interp {
            // Run it now, on the big stack every other entry point uses.
            let src = source.to_string();
            let dir = opts.base_dir.clone();
            let (output, code) =
                crate::run_on_big_stack(move || crate::eval_in_cli(&src, &dir, &[], None))
                    .map_err(|e| format!("Runtime error: {}", e))?;
            for line in &output {
                println!("{}", line);
            }
            if code != 0 {
                std::process::exit(code);
            }
            continue;
        }

        out.push(dispatch(&registry, &module, opts, *kind)?);
    }

    Ok(out)
}

/// Hand one emit kind to the first backend that supports it.
fn dispatch(
    registry: &[Box<dyn Target>],
    module: &Module,
    opts: &CodegenOptions,
    kind: EmitKind,
) -> Result<Artifact, String> {
    for backend in registry {
        if backend.supports(kind, &opts.triple) {
            return backend.emit(module, opts, kind);
        }
    }
    // No backend claimed it. Name the backend that got closest, so the reader
    // knows whether they asked for the wrong emit kind or the wrong target.
    Err(format!(
        "no backend emits '{}' for target '{}'. Available emits: {}",
        kind.as_str(),
        opts.triple,
        accepted_emit_list()
    ))
}

/// Where an artifact lands.
///
/// With `-o`, an explicit file destination wins for a single artifact; a
/// directory (or more than one artifact) means "everything lands in here",
/// with each artifact taking its default extension. Without `-o`, text
/// artifacts print and file artifacts are written next to the source
/// (`main.rak` compiles to `main.rkbc`).
///
/// Public because backends that invoke an external toolchain need the final
/// path up front: the compiler is invoked with it as `-o`, so the file is
/// written once, at its destination, by the tool that produced it.
pub fn destination_for(kind: &EmitKind, opts: &CodegenOptions) -> PathBuf {
    let Some(out) = opts.out_path.as_ref() else {
        return default_path(kind, opts);
    };
    let mut looks_like_dir = out.as_os_str().is_empty()
        || out.to_string_lossy().ends_with('/')
        || out.to_string_lossy().ends_with('\\')
        || out.is_dir();
    // Several emit kinds cannot all be one file, so an explicit `-o` is read as
    // a directory in that case even when nothing exists there yet.
    if opts.emit.len() > 1 {
        looks_like_dir = true;
    }
    if looks_like_dir {
        out.join(format!("{}{}", opts.stem(), kind.extension(&opts.triple)))
    } else {
        out.clone()
    }
}

/// Where an artifact lands when `-o` was not given: next to the source, with
/// the kind's default extension. The `.rak` extension is always stripped, so
/// `main.rak` with `--emit=link` is `main` on Unix and `main.exe` on Windows.
fn default_path(kind: &EmitKind, opts: &CodegenOptions) -> PathBuf {
    let src = Path::new(&opts.source_path);
    let stem = src.with_extension("").to_string_lossy().into_owned();
    PathBuf::from(format!("{}{}", stem, kind.extension(&opts.triple)))
}

/// Write one artifact to disk, or print it.
///
/// The default (no `-o`) for a text artifact is stdout, which is what makes
/// `rakc build --emit=c | clang -x c - -o prog` work.
pub fn write_artifact(
    artifact: &Artifact,
    opts: &CodegenOptions,
) -> Result<Option<PathBuf>, String> {
    let kind = artifact.kind();
    match artifact {
        Artifact::Text { text, .. } => {
            // No `-o`: text goes to stdout, the way `rakc fmt` prints.
            if opts.out_path.is_none() {
                print!("{}", text);
                if !text.ends_with('\n') {
                    println!();
                }
                return Ok(None);
            }
            let dest = destination_for(&kind, opts);
            write_file(&dest, text.as_bytes())?;
            println!("Emitted: {} ({} bytes)", dest.display(), text.len());
            Ok(Some(dest))
        }
        Artifact::File { data, .. } => {
            let dest = destination_for(&kind, opts);
            write_file(&dest, data)?;
            println!("Emitted: {} ({} bytes)", dest.display(), data.len());
            Ok(Some(dest))
        }
        Artifact::Written { path, .. } => {
            // The toolchain already put it there.
            println!("Emitted: {} ({} bytes)", path.display(), fs_len(path));
            Ok(Some(path.clone()))
        }
    }
}

/// The size of an already-written file, for the progress line. A file that
/// vanished between compiling and reporting gets 0 rather than an error: the
/// build succeeded, and the size is decoration.
fn fs_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn write_file(dest: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {}", parent.display(), e))?;
        }
    }
    std::fs::write(dest, data).map_err(|e| format!("writing {}: {}", dest.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_emit_list_accepts_and_dedups() {
        let kinds = parse_emit_list("bytecode, c ,bytecode").unwrap();
        assert_eq!(kinds, vec![EmitKind::Bytecode, EmitKind::CSource]);
    }

    #[test]
    fn parse_emit_list_rejects_unknown() {
        let err = parse_emit_list("bytecode,java").unwrap_err();
        assert!(err.contains("java"), "{}", err);
        assert!(err.contains("link"));
    }

    #[test]
    fn emit_kind_names_round_trip() {
        for name in [
            "interp", "bytecode", "c", "wasm", "llvm-ir", "obj", "exe", "link",
        ] {
            let kind = EmitKind::parse(name).unwrap();
            assert_eq!(kind.as_str(), name);
        }
    }

    #[test]
    fn opt_level_maps_to_c_flags() {
        assert_eq!(OptLevel::parse("3").unwrap().c_flag(), "-O3");
        assert_eq!(OptLevel::parse("z").unwrap().c_flag(), "-Oz");
        assert_eq!(OptLevel::parse("s").unwrap().c_flag(), "-Os");
        assert!(OptLevel::parse("4").is_none());
    }

    #[test]
    fn default_options_build_an_exe_for_the_host() {
        let opts = CodegenOptions::new("src/main.rak");
        // Back-compat: a bare `rakc build x.rak` still means the historical
        // self-extracting executable, not a native compile.
        assert_eq!(opts.emit, vec![EmitKind::Exe]);
        assert_eq!(opts.triple.name, host_triple().name);
        assert_eq!(opts.base_dir, "src");
        assert_eq!(opts.stem(), "main");
    }

    #[test]
    fn default_output_paths_carry_the_right_extension() {
        let opts = CodegenOptions::new("src/main.rak");
        let linux = find_triple("x86_64-unknown-linux-gnu").unwrap();
        let win = find_triple("x86_64-pc-windows-msvc").unwrap();
        assert_eq!(
            default_path(&EmitKind::Bytecode, &opts),
            PathBuf::from("src/main.rkbc")
        );
        assert_eq!(
            default_path(&EmitKind::CSource, &opts),
            PathBuf::from("src/main.c")
        );
        assert_eq!(
            default_path(&EmitKind::Wasm, &opts),
            PathBuf::from("src/main.wasm")
        );
        // The target, not the host, decides the object/exe suffix.
        assert_eq!(EmitKind::Object.extension(&win), ".obj");
        assert_eq!(EmitKind::Object.extension(&linux), ".o");
        assert_eq!(EmitKind::Link.extension(&win), ".exe");
        assert_eq!(EmitKind::Link.extension(&linux), "");
        // The link output strips `.rak` whatever the suffix, and the suffix
        // comes from the triple — so the assertion is the same on every
        // machine the suite runs on.
        assert_eq!(
            default_path(
                &EmitKind::Link,
                &CodegenOptions {
                    triple: linux,
                    ..opts.clone()
                }
            ),
            PathBuf::from("src/main")
        );
        assert_eq!(
            default_path(
                &EmitKind::Link,
                &CodegenOptions {
                    triple: win,
                    ..opts
                }
            ),
            PathBuf::from("src/main.exe")
        );
    }

    #[test]
    fn destination_honours_o_and_multi_artifact() {
        let mut opts = CodegenOptions::new("src/main.rak");
        opts.out_path = Some(PathBuf::from("out"));
        // Several artifacts: a directory is implied even when `out` does not exist yet.
        opts.emit = vec![EmitKind::Bytecode, EmitKind::CSource];
        let dest = destination_for(&EmitKind::Bytecode, &opts);
        assert_eq!(dest, PathBuf::from("out").join("main.rkbc"));
        // One artifact: the exact path wins.
        opts.emit = vec![EmitKind::CSource];
        let dest = destination_for(&EmitKind::CSource, &opts);
        assert_eq!(dest, PathBuf::from("out"));
    }

    #[test]
    fn no_backend_claim_is_a_clear_error() {
        let module = Module {
            imports: vec![],
            items: vec![],
        };
        let opts = CodegenOptions::new("x.rak");
        let registry = backends();
        let err = dispatch(&registry, &module, &opts, EmitKind::LlvmIr);
        // `llvm-ir` only exists behind the cargo feature; without it every
        // backend declines and the error must say so rather than panic.
        // With the feature on, the backend claims it, so allow both outcomes.
        if cfg!(not(feature = "llvm")) {
            assert!(err.is_err());
        }
    }
}
