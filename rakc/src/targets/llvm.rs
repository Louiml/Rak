//! The LLVM backend.
//!
//! Compiles a Rak program to LLVM IR (`.ll`) or a native object/binary through
//! LLVM's optimizer and code generator. This backend is feature-gated behind
//! the `llvm` cargo feature because inkwell (the LLVM bindings) is a heavy
//! dependency: it requires LLVM 15 to be present on the build machine.
//!
//! Without the feature, the backend exists but every emit returns a clear
//! error naming the feature flag, so the CLI can report "this rakc was built
//! without LLVM support" rather than a cryptic linker failure.

use super::{Artifact, CodegenOptions, EmitKind, Target, Triple};
use crate::ast::Module;

/// Emits [`EmitKind::LlvmIr`], [`EmitKind::Object`] and [`EmitKind::Link`].
pub struct LlvmBackend;

impl Target for LlvmBackend {
    fn id(&self) -> &'static str {
        "llvm"
    }

    fn supports(&self, kind: EmitKind, _triple: &Triple) -> bool {
        matches!(kind, EmitKind::LlvmIr | EmitKind::Object | EmitKind::Link)
    }

    fn emit(
        &self,
        _module: &Module,
        _opts: &CodegenOptions,
        kind: EmitKind,
    ) -> Result<Artifact, String> {
        // The LLVM backend is not yet implemented. When it is, it will
        // generate LLVM IR from the AST and use inkwell's TargetMachine to
        // produce object files and linked binaries.
        let _ = kind;
        Err(
            "the LLVM backend is not yet implemented. Use --emit=bytecode, --emit=c, or --emit=link (via the C backend) instead.".to_string(),
        )
    }
}
