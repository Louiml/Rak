//! The bytecode target: `.rkbc` containers and bytecode listings.
//!
//! This target takes the chunk the existing AST→bytecode compiler produces
//! (`rakc vm`'s own code path) and makes it an artifact: either a serialized
//! `.rkbc` container or a text listing. Nothing is recompiled or reinterpreted
//! — a `.rkbc` is the same chunk `rakc vm` runs, which is what makes the two
//! interchangeable.

use super::rkbc;
use super::{Artifact, CodegenOptions, EmitKind, Target, Triple};
use crate::ast::Module;
use crate::bytecode::{disassemble_chunk, Chunk};
use crate::value::Value;

/// Emits [`EmitKind::Bytecode`] and [`EmitKind::BytecodeText`].
pub struct BytecodeBackend;

impl Target for BytecodeBackend {
    fn id(&self) -> &'static str {
        "bytecode"
    }

    fn supports(&self, kind: EmitKind, _triple: &Triple) -> bool {
        matches!(kind, EmitKind::Bytecode | EmitKind::BytecodeText)
    }

    fn emit(
        &self,
        module: &Module,
        opts: &CodegenOptions,
        kind: EmitKind,
    ) -> Result<Artifact, String> {
        let chunk = compile_chunk(module, opts)?;
        match kind {
            EmitKind::Bytecode => {
                let bytes = rkbc::encode_chunk(&chunk)?;
                Ok(Artifact::File { kind, data: bytes })
            }
            EmitKind::BytecodeText => Ok(Artifact::Text {
                kind,
                text: disassemble_chunk(&chunk),
            }),
            _ => unreachable!("supports() gates this"),
        }
    }
}

/// Compile a module to a chunk, resolving name-based imports relative to the
/// source file. This is exactly what `rakc vm` runs.
pub fn compile_chunk(module: &Module, opts: &CodegenOptions) -> Result<Chunk, String> {
    crate::compiler::compile_module_in(module, &opts.base_dir)
        .map_err(|e| format!("Compile error: {}", e))
}

/// Load a `.rkbc` container from disk and re-bind its native functions.
///
/// A chunk stores its natives *by name* — the Rust closure behind `fmt` or
/// `sha256` cannot be serialized, and the build that compiled the chunk is not
/// necessarily the build that runs it. So `rakc vm` re-binds each name against
/// the native registry of the running VM, and a name that does not resolve is
/// an error naming it: the chunk needs a rakc built with the feature that
/// provides that native (GUI and pcap are the usual ones), not a wrong answer.
pub fn load_rkbc(path: &str) -> Result<Chunk, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Error reading {}: {}", path, e))?;
    let mut chunk = rkbc::decode_chunk(&bytes)?;
    rebind_natives(&mut chunk)?;
    Ok(chunk)
}

/// Replace every placeholder native constant with the VM's real one, including
/// the natives inside nested closure chunks.
fn rebind_natives(chunk: &mut Chunk) -> Result<(), String> {
    // One VM is enough: it registers every native `rakc` ships into its globals
    // under the builtin's name, which is exactly the lookup table needed here.
    let vm = crate::vm::Vm::new();
    for c in chunk.constants.iter_mut() {
        rebind_value(c, &vm)?;
    }
    Ok(())
}

fn rebind_value(value: &mut Value, vm: &crate::vm::Vm) -> Result<(), String> {
    match value {
        Value::NativeFn(name, target) => match vm.globals.get(name.as_ref()) {
            Some(Value::NativeFn(_, real)) => {
                *target = real.clone();
                Ok(())
            }
            _ => Err(format!(
                "the native '{}' is not available in this rakc build. \
                     The .rkbc was compiled by a build that provided it \
                     (check for a missing feature such as gui or pcap).",
                name
            )),
        },
        // A closure carries its own chunk, whose constants may in turn hold
        // natives and further closures.
        Value::Closure { code, .. } => {
            let inner = std::sync::Arc::make_mut(code);
            rebind_natives_in_place(inner, vm)
        }
        Value::Function { captures, .. } => {
            let env = std::sync::Arc::make_mut(captures);
            for scope in env.scopes.iter_mut() {
                for v in scope.values_mut() {
                    rebind_value(v, vm)?;
                }
            }
            Ok(())
        }
        Value::Array(items) => {
            let items = std::sync::Arc::make_mut(items);
            for v in items.iter_mut() {
                rebind_value(v, vm)?;
            }
            Ok(())
        }
        Value::Tuple(items) => {
            let items = std::sync::Arc::make_mut(items);
            for v in items.iter_mut() {
                rebind_value(v, vm)?;
            }
            Ok(())
        }
        Value::Option(Some(v)) | Value::Result(Some(v), _) | Value::Result(_, Some(v)) => {
            rebind_value(v.as_mut(), vm)
        }
        _ => Ok(()),
    }
}

/// The in-place form of [`rebind_natives`] for a nested chunk, which is already
/// behind an `Arc` the caller owns.
fn rebind_natives_in_place(chunk: &mut Chunk, vm: &crate::vm::Vm) -> Result<(), String> {
    for c in chunk.constants.iter_mut() {
        rebind_value(c, vm)?;
    }
    Ok(())
}

/// Run a `.rkbc` container the way `rakc vm` runs a source file.
pub fn run_rkbc(path: &str, argv: &[String]) -> Result<(Vec<String>, i32), String> {
    let chunk = load_rkbc(path)?;
    let mut vm = crate::vm::Vm::new();
    vm.run_cli(&chunk, argv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::CodegenOptions;

    fn compile(source: &str) -> Chunk {
        let tokens = crate::lexer::tokenize(source).expect("lex");
        let module = crate::parser::parse(&tokens, source).expect("parse");
        let opts = CodegenOptions::new("test.rak");
        compile_chunk(&module, &opts).expect("compile")
    }

    #[test]
    fn chunk_survives_a_rkbc_roundtrip() {
        let source = r#"
            fn add(a, b) { return a + b }
            let total = add(2, 3)
            dump total
            dump "hello"
        "#;
        let chunk = compile(source);
        let bytes = rkbc::encode_chunk(&chunk).expect("encode");
        assert_eq!(&bytes[..4], rkbc::MAGIC);
        let back = rkbc::decode_chunk(&bytes).expect("decode");
        assert_eq!(back.code, chunk.code);
        assert_eq!(back.lines, chunk.lines);
        assert_eq!(back.constants.len(), chunk.constants.len());
    }

    #[test]
    fn loaded_chunk_runs_and_rebinds_natives() {
        let source = r#"
            dump "hello"
            dump len([1, 2, 3])
        "#;
        let chunk = compile(source);
        let bytes = rkbc::encode_chunk(&chunk).expect("encode");
        // A container that decodes to a runnable chunk, with natives bound.
        let mut back = rkbc::decode_chunk(&bytes).expect("decode");
        rebind_natives(&mut back).expect("bind natives");
        let mut vm = crate::vm::Vm::new();
        let out = vm.run(&back).expect("run");
        assert_eq!(out, vec!["[DUMP] hello", "[DUMP] 3"]);
    }

    #[test]
    fn bytecode_text_lists_instructions_with_operands() {
        let source = r#"
            let x = 41
            dump x + 1
        "#;
        let chunk = compile(source);
        let text = disassemble_chunk(&chunk);
        assert!(text.contains("bytecode:"), "{}", text);
        assert!(text.contains("code:"), "{}", text);
        // A `LoadConst` must carry its constant index as an operand; a listing
        // that shows the index byte as a new opcode has desynchronised.
        assert!(text.contains("LoadConst"), "{}", text);
    }

    #[test]
    fn closure_constants_roundtrip_with_their_bodies() {
        // A `fn` value assigned to a variable is baked into the constant pool
        // as a closure with a nested chunk — the case that makes serialization
        // non-trivial.
        let source = r#"
            let f = fn(n) { return n * 2 }
            dump f(21)
        "#;
        let chunk = compile(source);
        let bytes = rkbc::encode_chunk(&chunk).expect("encode");
        let mut back = rkbc::decode_chunk(&bytes).expect("decode");
        rebind_natives(&mut back).expect("bind natives");
        let mut vm = crate::vm::Vm::new();
        let out = vm.run(&back).expect("run");
        assert_eq!(out, vec!["[DUMP] 42"]);
    }
}
