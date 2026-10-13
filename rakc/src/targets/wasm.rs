//! The WebAssembly target.
//!
//! Compiles a Rak program to a WebAssembly binary by routing through the C
//! backend with clang's wasm target (`--target=wasm32-wasi` or
//! `wasm32-unknown-unknown`). This is the same approach cargo's wasm targets
//! use: C code compiled with a wasm-targeted clang produces a `.wasm` binary.
//!
//! The wasm backend requires clang with wasm support. If clang is not available
//! or does not support wasm targets, the emit fails with a clear error naming
//! what is needed.

use super::c;
use super::toolchain::{self, Cc};
use super::{Artifact, CodegenOptions, EmitKind, Target, Triple};
use crate::ast::Module;
use std::path::PathBuf;

/// Emits [`EmitKind::Wasm`], [`EmitKind::Object`] and [`EmitKind::Link`]
/// for wasm triples.
pub struct WasmBackend;

impl Target for WasmBackend {
    fn id(&self) -> &'static str {
        "wasm"
    }

    fn supports(&self, kind: EmitKind, triple: &Triple) -> bool {
        if !triple.is_wasm() {
            return false;
        }
        matches!(kind, EmitKind::Wasm | EmitKind::Object | EmitKind::Link)
    }

    fn emit(
        &self,
        module: &Module,
        opts: &CodegenOptions,
        kind: EmitKind,
    ) -> Result<Artifact, String> {
        let c_source = c::translate(module, opts)?;

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
        if !matches!(cc.kind, toolchain::CcKind::Clang) {
            return Err(format!(
                "the wasm target requires clang (found {}). Install LLVM/clang from https://releases.llvm.org/",
                cc.kind.as_str()
            ));
        }

        let dest = super::destination_for(&kind, opts);
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("creating {}: {}", parent.display(), e))?;
            }
        }

        let wasm_target = if opts.triple.name == "wasm32-wasi" {
            "wasm32-wasi"
        } else {
            "wasm32-unknown-unknown"
        };

        let mut args: Vec<String> = vec![
            format!("--target={}", wasm_target),
            "-std=c11".to_string(),
            "-nostdlib".to_string(),
            "-Wl,--no-entry".to_string(),
            "-Wl,--export-all".to_string(),
        ];
        if opts.debuginfo > 0 {
            args.push("-g".to_string());
        }
        args.push(c_path.to_string_lossy().to_string());
        args.push("-o".to_string());
        args.push(dest.to_string_lossy().to_string());

        cc.run(&args)
            .map_err(|e| format!("compiling {}: {}", c_path.display(), e))?;

        Ok(Artifact::Written { kind, path: dest })
    }
}

fn c_file_path(opts: &CodegenOptions) -> PathBuf {
    match &opts.out_path {
        Some(p) if p.is_dir() => p.join(format!("{}.c", opts.stem())),
        _ => {
            let src = std::path::Path::new(&opts.source_path);
            PathBuf::from(format!("{}.c", src.with_extension("").to_string_lossy()))
        }
    }
}
