//! The C target: Rak source translated to C, compiled with the host toolchain.
//!
//! This is the real "compile to anything" backend: the AST is translated to a
//! single self-contained C11 file carrying its own tagged-value runtime, so a
//! Rak program becomes an ordinary native binary built by clang/gcc/cl with no
//! Rak installation involved. See [`codegen`] for the translation and
//! [`runtime`] for the emitted runtime.
//!
//! The C target compiles a single module with no imports, and without the
//! runtime features whose implementation lives in Rust: no `spawn`/`await`, no
//! sockets, no FFI, no GUI. Those are compile errors that name the construct
//! and the target, rather than silent behavior changes — the same
//! written-down-limits policy the VM follows. An honest subset beats a broad
//! one that quietly miscompiles.

use super::toolchain;
use super::{Artifact, CodegenOptions, EmitKind};
use crate::ast::*;
use std::path::PathBuf;

mod codegen;
mod runtime;

/// Emits [`EmitKind::CSource`], [`EmitKind::Object`] and [`EmitKind::Link`]
/// for native triples.
pub struct CBackend;

impl super::Target for CBackend {
    fn id(&self) -> &'static str {
        "c"
    }

    fn supports(&self, kind: super::EmitKind, triple: &super::Triple) -> bool {
        match kind {
            super::EmitKind::CSource => true,
            // WebAssembly goes through the wasm backend, which drives clang
            // with a wasm target rather than the host.
            super::EmitKind::Object | super::EmitKind::Link => !triple.is_wasm(),
            _ => false,
        }
    }

    fn emit(
        &self,
        module: &Module,
        opts: &CodegenOptions,
        kind: super::EmitKind,
    ) -> Result<Artifact, String> {
        let source = codegen::translate(module, opts)?;
        match kind {
            super::EmitKind::CSource => Ok(Artifact::Text { kind, text: source }),
            super::EmitKind::Object | super::EmitKind::Link => {
                compile_to_native(&source, opts, kind == super::EmitKind::Link)
            }
            _ => unreachable!("supports() gates this"),
        }
    }
}

/// Translate a module to a complete C11 translation unit.
pub fn translate(module: &Module, opts: &CodegenOptions) -> Result<String, String> {
    codegen::translate(module, opts)
}

/// Compile the generated C into an object file or a linked executable,
/// invoking the host C toolchain.
///
/// The compiler is invoked with the artifact's final destination as `-o`, so
/// the file is written once, where the user asked for it. The generated `.c` is
/// kept either way: with `--emit=obj` it *is* the point, and with no compiler
/// installed the error names it as the fallback.
pub fn compile_to_native(
    c_source: &str,
    opts: &CodegenOptions,
    link: bool,
) -> Result<Artifact, String> {
    let kind = if link {
        EmitKind::Link
    } else {
        EmitKind::Object
    };
    let c_path = c_file_path(opts);
    if let Some(parent) = c_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {}", parent.display(), e))?;
        }
    }
    std::fs::write(&c_path, c_source.as_bytes())
        .map_err(|e| format!("writing {}: {}", c_path.display(), e))?;

    let Some(cc) = toolchain::find_cc() else {
        return Err(toolchain::missing_cc_error(&c_path));
    };

    let dest = super::destination_for(&kind, opts);
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {}", parent.display(), e))?;
        }
    }

    let mut args: Vec<String> = vec!["-std=c11".to_string(), opts.opt_level.c_flag().to_string()];
    if opts.debuginfo > 0 {
        args.push("-g".to_string());
    }
    if matches!(opts.panic, super::PanicStrategy::Abort) {
        // The runtime honours this at the raise site: a `raise` with no catch
        // exits immediately instead of unwinding to `main`.
        args.push("-DRAK_PANIC_ABORT".to_string());
    }
    args.push(c_path.to_string_lossy().to_string());
    let out_flag = match cc.kind {
        toolchain::CcKind::Msvc => "/Fe".to_string(),
        _ => "-o".to_string(),
    };
    args.push(out_flag);
    args.push(dest.to_string_lossy().to_string());
    // The runtime uses only libc; `-lm` covers the math functions and is a
    // no-op where libc already provides them.
    if matches!(cc.kind, toolchain::CcKind::Gcc | toolchain::CcKind::Clang) {
        args.push("-lm".to_string());
    }
    cc.run(&args)
        .map_err(|e| format!("compiling {}: {}", c_path.display(), e))?;

    Ok(Artifact::Written { kind, path: dest })
}

/// Where the generated `.c` file lives: next to the source, unless `-o`
/// redirects the whole build directory.
fn c_file_path(opts: &CodegenOptions) -> PathBuf {
    match &opts.out_path {
        Some(p) if p.is_dir() => p.join(format!("{}.c", opts.stem())),
        _ => default_c(opts),
    }
}

fn default_c(opts: &CodegenOptions) -> PathBuf {
    let src = std::path::Path::new(&opts.source_path);
    PathBuf::from(format!("{}.c", src.with_extension("").to_string_lossy()))
}

/// The link/object destination the toolchain writes to, which is also the
/// artifact destination: `-o main.exe` for an executable, `-o main.o` for an
/// object, honouring the target's suffix.
#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn destination_honours_the_triple_suffix() {
        let opts = CodegenOptions::new("src/main.rak");
        let win = crate::targets::find_triple("x86_64-pc-windows-msvc").unwrap();
        let windows_opts = CodegenOptions {
            triple: win,
            ..opts.clone()
        };
        // The host build's link destination carries the host triple's suffix.
        let dest = super::super::destination_for(&EmitKind::Link, &opts);
        assert!(
            dest.to_string_lossy().ends_with("main.exe")
                || dest.to_string_lossy().ends_with("main"),
            "{:?}",
            dest
        );
        assert_eq!(
            super::super::destination_for(&EmitKind::Link, &windows_opts),
            PathBuf::from("src/main.exe")
        );
        assert_eq!(
            super::super::destination_for(&EmitKind::Object, &windows_opts),
            PathBuf::from("src/main.obj")
        );
    }

    #[test]
    fn the_generated_c_sits_next_to_the_source() {
        let opts = CodegenOptions::new("src/main.rak");
        assert_eq!(c_file_path(&opts), PathBuf::from("src/main.c"));
    }
}
