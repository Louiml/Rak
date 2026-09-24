use crate::ast::*;
use crate::bytecode::{Chunk, Op};
use crate::value::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Compiler {
    chunk: Chunk,
    locals: Vec<(String, usize)>,
    /// Parallel to `locals`: whether each slot was declared immutable (`let`).
    immutable_locals: Vec<bool>,
    scope_depth: usize,
    /// Globals declared immutable (`let x = ...` at top level, non-`pub`).
    immutable_globals: std::collections::HashSet<String>,
    /// Current top-level statement index (1-based) used as the source-line
    /// marker for `chunk.lines`; enables the VM debugger's line breakpoints.
    statement_line: u32,
    func_names: std::collections::HashSet<String>,
    func_closures: HashMap<String, Value>,
    /// `macro name(params) { body }` definitions, for compile-time expansion.
    macros: HashMap<String, (Vec<Param>, Vec<Stmt>)>,
    /// `binstruct Name { ... }` definitions, keyed by name. Registered in the
    /// compile pre-pass so `Name.decode(...)` / `Name.encode(...)` resolve at
    /// compile time to native-fn globals.
    binstructs: HashMap<String, Vec<BinField>>,
    /// `impl <Trait> for <Type>` / inherent `impl <Type>` methods, baked as
    /// closures and registered as `__method_<type>_<name>` globals so
    /// `Op::CallMethod` can dispatch on the receiver's runtime type.
    method_closures: Vec<(String, Value)>,
    /// `enum` constructor natives keyed `__enum_new_<Enum>_<Variant>`.
    enum_ctors: Vec<(String, Value)>,
    /// Set of baked enum-ctor global names, for `Expr::Path` lookup.
    enum_ctor_names: std::collections::HashSet<String>,
    /// Active loops for `break`/`continue` (most recent last). Outer scope
    /// entries carry the loop's optional label, its `continue` jump target,
    /// and any floating `break` jumps to patch when the loop ends.
    loop_stack: Vec<LoopInfo>,
    /// Names exported by the module currently being compiled (`pub`/`export`).
    current_exports: Vec<String>,
    /// Import-once cache: canonical module path → its exported global names.
    module_cache: HashMap<std::path::PathBuf, Vec<String>>,
    /// Modules currently being inlined (circular-import detection).
    compiling: std::collections::HashSet<std::path::PathBuf>,
    /// Base directory for resolving name-based imports of the current module.
    base_dir: String,
}

struct LoopInfo {
    label: Option<String>,
    /// Floating `break` jumps, patched to the loop's end.
    break_jumps: Vec<usize>,
    /// Floating `continue` jumps, patched to the loop's continue point
    /// (the increment for `for`, loop-start for `while`/`loop`).
    continue_jumps: Vec<usize>,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            immutable_locals: Vec::new(),
            immutable_globals: std::collections::HashSet::new(),
            scope_depth: 0,
            statement_line: 0,
            func_names: std::collections::HashSet::new(),
            func_closures: HashMap::new(),
            macros: HashMap::new(),
            binstructs: HashMap::new(),
            method_closures: Vec::new(),
            enum_ctors: Vec::new(),
            enum_ctor_names: std::collections::HashSet::new(),
            loop_stack: Vec::new(),
            current_exports: Vec::new(),
            module_cache: HashMap::new(),
            compiling: std::collections::HashSet::new(),
            base_dir: ".".to_string(),
        }
    }

    pub fn compile(&mut self, module: &Module) -> Result<Chunk, String> {
        self.current_exports.clear();
        // Pre-pass: collect top-level functions (incl. `pub fn`), macros (incl.
        // `pub macro`), externs, and the exported-name list.
        for (idx, stmt) in module.items.iter().enumerate() {
            // Consistent statement-index line markers: the VM debugger breaks on
            // the Nth top-level statement.
            self.statement_line = (idx + 1) as u32;
            match stmt {
                Stmt::Let { name, value, .. } => {
                    if let Expr::Function { params, body, .. } = value.as_ref() {
                        self.func_names.insert(name.clone());
                        let closure = self.compile_function(name, params, body)?;
                        self.func_closures.insert(name.clone(), closure);
                    }
                }
                Stmt::Export(inner) => match inner.as_ref() {
                    Stmt::Let { name, value, .. } => {
                        if let Expr::Function { params, body, .. } = value.as_ref() {
                            self.func_names.insert(name.clone());
                            let closure = self.compile_function(name, params, body)?;
                            self.func_closures.insert(name.clone(), closure);
                        }
                        self.current_exports.push(name.clone());
                    }
                    Stmt::Const { name, .. } | Stmt::Struct { name, .. } | Stmt::Enum { name, .. } => {
                        self.current_exports.push(name.clone());
                    }
                    Stmt::MacroDef { name, params, body } => {
                        self.macros.insert(name.clone(), (params.clone(), body.clone()));
                    }
                    _ => {}
                },
                Stmt::MacroDef { name, params, body } => {
                    self.macros.insert(name.clone(), (params.clone(), body.clone()));
                }
                Stmt::BinStructDef { name, fields } => {
                    self.binstructs.insert(name.clone(), fields.clone());
                }
                Stmt::Enum { name, variants, .. } => {
                    // Bake a constructor native per variant as a global
                    // `__enum_new_<Enum>_<Variant>`; `Event::Connect(x)`
                    // compiles to `LoadGlobal ctor; args; Call`.
                    for v in variants {
                        let ctor = crate::vm::make_enum_ctor(name.clone(), v.name.clone());
                        let key = format!("__enum_new_{}_{}", name, v.name);
                        self.enum_ctor_names.insert(key.clone());
                        self.enum_ctors.push((key, ctor));
                    }
                }
                Stmt::Impl { target, trait_name, methods } => {
                    // Mirror the interpreter's registration: `impl Display for
                    // Point` -> type "Point"; inherent `impl Point` -> target
                    // is the type. Methods are `Stmt::Let` fn values.
                    let type_name = trait_name.clone().unwrap_or_else(|| target.clone());
                    for m in methods {
                        if let Stmt::Let { name: mname, value, .. } = m {
                            if let Expr::Function { params, body, .. } = value.as_ref() {
                                let key = format!("__method_{}_{}", type_name, mname);
                                let closure = self.compile_function(&key, params, body)?;
                                self.method_closures.push((key, closure));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // Bake a self-contained decode/encode native for each `binstruct` and
        // register it as a global (so `Name.decode(...)` lowers to a plain
        // `Op::Call` on a `Value::NativeFn` — no new VM opcodes).
        let bin_names: Vec<String> = self.binstructs.keys().cloned().collect();
        for name in &bin_names {
            let resolved = resolve_binstruct(name, &self.binstructs)?;
            let dn = make_bin_decode_native(name.clone(), resolved.clone());
            let en = make_bin_encode_native(name.clone(), resolved);
            self.load_const(dn);
            let ci = self.const_str(&format!("__bin_decode_{}", name));
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
            self.load_const(en);
            let ci = self.const_str(&format!("__bin_encode_{}", name));
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        // Register `impl`-block methods as globals so `Op::CallMethod` can
        // dispatch `obj.method(...)` on the receiver's runtime type name.
        let methods: Vec<(String, Value)> = std::mem::take(&mut self.method_closures);
        for (name, closure) in methods {
            self.load_const(closure);
            let ci = self.const_str(&name);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        // Register enum constructor natives.
        let ctors: Vec<(String, Value)> = std::mem::take(&mut self.enum_ctors);
        for (name, ctor) in ctors {
            self.load_const(ctor);
            let ci = self.const_str(&name);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        // Register `extern "C"` declarations as native-fn globals, so
        // `getpid()` resolves to `Op::Call` on a `Value::NativeFn`.
        let mut foreigns: Vec<(String, Value)> = Vec::new();
        for stmt in &module.items {
            if let Stmt::Extern { lib, decls, .. } = stmt {
                for decl in decls {
                    let native = crate::vm::make_foreign_native(decl.clone(), lib.clone());
                    foreigns.push((decl.name.clone(), native));
                }
            }
        }
        let closures: Vec<(String, Value)> = self.func_closures.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (name, closure) in closures {
            self.load_const(closure);
            let ci = self.const_str(&name);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        for (name, native) in foreigns {
            self.load_const(native);
            let ci = self.const_str(&name);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        // Process imports: inline imported modules' exported items as globals
        // and emit linkage (`import m` builds a Value::Module, `from m import
        // x` copies/aliases globals).
        let imports = module.imports.clone();
        for imp in &imports {
            self.compile_import(imp)?;
        }
        // Compile the module's own non-function items.
        for (idx, stmt) in module.items.iter().enumerate() {
            // Consistent statement-index line markers.
            self.statement_line = (idx + 1) as u32;
            if let Stmt::Let { value, .. } = stmt {
                if matches!(value.as_ref(), Expr::Function { .. }) {
                    continue;
                }
            }
            if let Stmt::Export(inner) = stmt {
                if let Stmt::Let { value, .. } = inner.as_ref() {
                    if matches!(value.as_ref(), Expr::Function { .. }) {
                        continue; // already compiled as a closure in the pre-pass
                    }
                }
                if matches!(inner.as_ref(), Stmt::MacroDef { .. }) {
                    continue;
                }
            }
            if matches!(stmt, Stmt::Extern { .. }) {
                continue;
            }
            if matches!(stmt, Stmt::MacroDef { .. }) {
                continue;
            }
            if matches!(stmt, Stmt::BinStructDef { .. }) {
                continue;
            }
            // `impl` blocks are handled in the pre-pass (baked `__method_*`
            // globals); `Stmt::Trait` declarations are documentation-only.
            // `struct`/`enum`/`type` declarations are compile-time metadata
            // (struct literals and baked enum ctors need no runtime def).
            let is_def = |s: &Stmt| matches!(s, Stmt::Struct { .. } | Stmt::Enum { .. } | Stmt::TypeAlias { .. } | Stmt::Impl { .. });
            let is_def_export = matches!(stmt, Stmt::Export(inner) if is_def(inner.as_ref()));
            if is_def(stmt) || is_def_export || matches!(stmt, Stmt::Trait { .. }) {
                continue;
            }
            self.compile_stmt(stmt)?;
        }
        self.emit_op(Op::Nil);
        self.emit_op(Op::Return);
        Ok(std::mem::replace(&mut self.chunk, Chunk::new()))
    }

    /// Resolve an import spec's target (file path or dotted name) to a file.
    fn resolve_target(&self, spec: &Import) -> Result<crate::modules::DottedResolve, String> {
        if spec.is_file {
            let rel = &spec.path[0];
            let full = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                Path::new(&self.base_dir).join(rel)
            };
            Ok(crate::modules::DottedResolve { init: None, leaf: full })
        } else {
            crate::modules::resolve_dotted(Path::new(&self.base_dir), &spec.path).ok_or_else(|| {
                format!("import: cannot find module '{}' (searched: {}, packages, RAK_PATH)", spec.path.join("."), self.base_dir)
            })
        }
    }

    /// Inline an imported module's exported items as globals into the current
    /// chunk (recursively, cached). Returns the exported global names.
    fn inline_module(&mut self, leaf: PathBuf, init: Option<PathBuf>) -> Result<Vec<String>, String> {
        let canon = crate::modules::canonical(&leaf);
        if let Some(names) = self.module_cache.get(&canon).cloned() {
            return Ok(names);
        }
        if self.compiling.contains(&canon) {
            return Ok(self.module_cache.get(&canon).cloned().unwrap_or_default());
        }
        // Load the package init first.
        if let Some(init_path) = init {
            let _ = self.inline_module(init_path.clone(), None)?;
        }
        self.compiling.insert(canon.clone());
        self.module_cache.insert(canon.clone(), Vec::new()); // partial for cycles

        let source = std::fs::read_to_string(&leaf)
            .map_err(|e| format!("import: cannot read '{}': {}", leaf.display(), e))?;
        let tokens = crate::lexer::tokenize(&source).map_err(|e| e.to_string())?;
        let module = crate::parser::parse(&tokens, &source).map_err(|e| e.to_string())?;
        let saved_base = self.base_dir.clone();
        if let Some(parent) = leaf.parent() {
            self.base_dir = parent.to_string_lossy().to_string();
        }
        let saved_exports = std::mem::take(&mut self.current_exports);

        // Pre-pass for this imported module: collect fns (incl. pub fn), macros,
        // externs. Exported names are recorded in `self.current_exports`.
        let mut local_closures: Vec<(String, Value)> = Vec::new();
        let mut local_foreigns: Vec<(String, Value)> = Vec::new();
        for stmt in &module.items {
            match stmt {
                Stmt::Let { name, value, .. } => {
                    if let Expr::Function { params, body, .. } = value.as_ref() {
                        self.func_names.insert(name.clone());
                        let cl = self.compile_function(name, params, body)?;
                        self.func_closures.insert(name.clone(), cl.clone());
                        local_closures.push((name.clone(), cl));
                    }
                }
                Stmt::Export(inner) => match inner.as_ref() {
                    Stmt::Let { name, value, .. } => {
                        if let Expr::Function { params, body, .. } = value.as_ref() {
                            self.func_names.insert(name.clone());
                            let cl = self.compile_function(name, params, body)?;
                            self.func_closures.insert(name.clone(), cl.clone());
                            local_closures.push((name.clone(), cl));
                        }
                        self.current_exports.push(name.clone());
                    }
                    Stmt::Const { name, .. } | Stmt::Struct { name, .. } | Stmt::Enum { name, .. } => {
                        self.current_exports.push(name.clone());
                    }
                    Stmt::MacroDef { name, params, body } => {
                        self.macros.insert(name.clone(), (params.clone(), body.clone()));
                    }
                    _ => {}
                },
                Stmt::MacroDef { name, params, body } => {
                    self.macros.insert(name.clone(), (params.clone(), body.clone()));
                }
                Stmt::BinStructDef { name, fields } => {
                    self.binstructs.insert(name.clone(), fields.clone());
                }
                Stmt::Extern { lib, decls, .. } => {
                    for decl in decls {
                        let native = crate::vm::make_foreign_native(decl.clone(), lib.clone());
                        local_foreigns.push((decl.name.clone(), native));
                    }
                }
                _ => {}
            }
        }
        for (n, cl) in &local_closures {
            self.load_const(cl.clone());
            let ci = self.const_str(n);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        for (n, nf) in &local_foreigns {
            self.load_const(nf.clone());
            let ci = self.const_str(n);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        // Bake per-binstruct decode/encode natives for this module's binstructs.
        let bin_names: Vec<String> = self.binstructs.keys().cloned().collect();
        for name in &bin_names {
            // Only bake binstructs defined in this module (not ones already
            // present from a parent compile) — skip if already baked this run.
            let resolved = resolve_binstruct(name, &self.binstructs)?;
            let dn = make_bin_decode_native(name.clone(), resolved.clone());
            let en = make_bin_encode_native(name.clone(), resolved);
            self.load_const(dn);
            let ci = self.const_str(&format!("__bin_decode_{}", name));
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
            self.load_const(en);
            let ci = self.const_str(&format!("__bin_encode_{}", name));
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        // Process this module's own imports (recursive inline).
        for imp in &module.imports {
            self.compile_import(imp)?;
        }
        // Compile its non-fn items (pub let/const/struct/enum...).
        for stmt in &module.items {
            if let Stmt::Let { value, .. } = stmt {
                if matches!(value.as_ref(), Expr::Function { .. }) {
                    continue;
                }
            }
            if let Stmt::Export(inner) = stmt {
                if let Stmt::Let { value, .. } = inner.as_ref() {
                    if matches!(value.as_ref(), Expr::Function { .. }) {
                        continue;
                    }
                }
                if matches!(inner.as_ref(), Stmt::MacroDef { .. }) {
                    continue;
                }
            }
            if matches!(stmt, Stmt::Extern { .. }) {
                continue;
            }
            if matches!(stmt, Stmt::MacroDef { .. }) {
                continue;
            }
            if matches!(stmt, Stmt::BinStructDef { .. }) {
                continue;
            }
            self.compile_stmt(stmt)?;
        }

        // Capture this module's exports (including re-exports added during
        // import processing) and restore the parent's export list.
        let local_exports = std::mem::replace(&mut self.current_exports, saved_exports);
        self.module_cache.insert(canon.clone(), local_exports.clone());
        self.base_dir = saved_base;
        self.compiling.remove(&canon);
        Ok(local_exports)
    }

    /// Emit, at run time, a `Value::Module` built from `exports` (a list of
    /// global names), then `StoreGlobal name`.
    fn emit_build_module(&mut self, exports: &[String], name: &str) {
        for n in exports {
            // BuildModule pops (value, name) per pair, so push name then value.
            let ki = self.const_str(n);
            self.emit_op(Op::LoadConst);
            self.emit_u16(ki);
            let ci = self.const_str(n);
            self.emit_op(Op::LoadGlobal);
            self.emit_u16(ci);
        }
        self.emit_op(Op::BuildModule);
        self.emit_byte(exports.len() as u8);
        let gi = self.const_str(name);
        self.emit_op(Op::StoreGlobal);
        self.emit_u16(gi);
    }

    fn compile_import(&mut self, import: &Import) -> Result<(), String> {
        let resolved = self.resolve_target(import)?;
        match import.kind {
            ImportKind::Whole => {
                let exports = self.inline_module(resolved.leaf.clone(), resolved.init.clone())?;
                let bind_name = if let Some(a) = &import.alias {
                    a.clone()
                } else if import.is_file {
                    Path::new(&import.path[0]).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| import.path[0].clone())
                } else {
                    import.path[0].clone()
                };
                if import.reexport {
                    // Re-export all of the module's exports from the current module.
                    for n in &exports {
                        if !self.current_exports.contains(n) {
                            self.current_exports.push(n.clone());
                        }
                    }
                    return Ok(());
                }
                if import.is_file || import.path.len() == 1 {
                    self.emit_build_module(&exports, &bind_name);
                } else {
                    // `import pkg.sub` directory-package nesting on the VM
                    // requires per-module global scopes; use `from pkg.sub
                    // import x` instead (documented VM subset).
                    return Err("import pkg.sub: VM nesting not supported in this build (use from pkg.sub import x)".to_string());
                }
                Ok(())
            }
            ImportKind::From => {
                let exports = self.inline_module(resolved.leaf.clone(), resolved.init.clone())?;
                if import.reexport {
                    if import.star {
                        for n in &exports {
                            if !self.current_exports.contains(n) {
                                self.current_exports.push(n.clone());
                            }
                        }
                    } else {
                        for (n, _) in &import.from_names {
                            if !exports.contains(n) {
                                return Err(format!("from {} import {}: '{}' is not exported", import.path.join("."), n, n));
                            }
                            if !self.current_exports.contains(n) {
                                self.current_exports.push(n.clone());
                            }
                        }
                    }
                    return Ok(());
                }
                if import.star {
                    // All exported globals are already inlined; nothing to copy.
                    return Ok(());
                }
                for (n, alias) in &import.from_names {
                    if !exports.contains(n) {
                        return Err(format!("from {} import {}: '{}' is not exported", import.path.join("."), n, n));
                    }
                    if let Some(a) = alias {
                        let ci = self.const_str(n);
                        self.emit_op(Op::LoadGlobal);
                        self.emit_u16(ci);
                        let ai = self.const_str(a);
                        self.emit_op(Op::StoreGlobal);
                        self.emit_u16(ai);
                    }
                    // No alias: the global `n` is already present (inlined).
                }
                Ok(())
            }
        }
    }

    fn compile_function(&self, name: &str, params: &[Param], body: &[Stmt]) -> Result<Value, String> {
        let mut sub = Compiler::new();
        sub.scope_depth = 1;
        for p in params {
            sub.add_local(p.name.clone());
        }
        sub.func_names = self.func_names.clone();
        sub.func_closures = self.func_closures.clone();
        sub.macros = self.macros.clone();
        sub.module_cache = self.module_cache.clone();
        sub.base_dir = self.base_dir.clone();
        for s in body {
            sub.compile_stmt(s)?;
        }
        sub.emit_op(Op::Nil);
        sub.emit_op(Op::Return);
        let sub_chunk = std::mem::replace(&mut sub.chunk, Chunk::new());
        Ok(Value::Closure {
            code: Arc::from(sub_chunk),
            nparams: params.len(),
            name: Arc::from(name),
        })
    }

    fn line(&self) -> u32 {
        self.statement_line
    }

    fn emit_op(&mut self, op: Op) {
        self.chunk.write_op(op, self.line());
    }

    fn emit_byte(&mut self, b: u8) {
        self.chunk.write_byte(b, self.line());
    }

    fn emit_u16(&mut self, v: u16) {
        self.chunk.write_u16(v, self.line());
    }

    fn emit_const(&mut self, value: Value) -> u16 {
        self.chunk.add_const(value)
    }

    fn load_const(&mut self, value: Value) {
        let ci = self.chunk.add_const(value);
        self.emit_op(Op::LoadConst);
        self.emit_u16(ci);
    }

    fn emit_jump(&mut self, op: Op) -> usize {
        self.emit_op(op);
        let pos = self.chunk.code.len();
        self.emit_byte(0);
        self.emit_byte(0);
        pos
    }

    fn patch_jump(&mut self, pos: usize) {
        let target = self.chunk.code.len() as u16;
        self.chunk.code[pos] = (target >> 8) as u8;
        self.chunk.code[pos + 1] = (target & 0xFF) as u8;
    }

    fn add_local(&mut self, name: String) -> u8 {
        let slot = self.locals.len() as u8;
        self.locals.push((name, self.scope_depth));
        self.immutable_locals.push(false);
        slot
    }

    fn add_local_mut(&mut self, name: String, mutable: bool) -> u8 {
        let slot = self.locals.len() as u8;
        self.locals.push((name, self.scope_depth));
        self.immutable_locals.push(!mutable);
        slot
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        self.locals.iter().rposition(|(n, _)| n == name).map(|i| i as u8)
    }

    /// True if the named local is bound and immutable.
    fn local_is_immutable(&self, name: &str) -> bool {
        self.locals
            .iter()
            .rposition(|(n, _)| n == name)
            .map(|i| self.immutable_locals[i])
            .unwrap_or(false)
    }

    fn const_str(&mut self, s: &str) -> u16 {
        self.emit_const(Value::String(Arc::from(s)))
    }

    /// Emit a load of the named variable (local if bound, else global).
    fn compile_ident_load(&mut self, name: &str) -> Result<(), String> {
        if let Some(slot) = self.resolve_local(name) {
            self.emit_op(Op::LoadLocal);
            self.emit_byte(slot);
        } else {
            let ci = self.const_str(name);
            self.emit_op(Op::LoadGlobal);
            self.emit_u16(ci);
        }
        Ok(())
    }

    /// Emit a store of the top-of-stack value to the named variable.
    fn store_ident_to(&mut self, name: &str) -> Result<(), String> {
        if let Some(slot) = self.resolve_local(name) {
            self.emit_op(Op::StoreLocal);
            self.emit_byte(slot);
        } else {
            let ci = self.const_str(name);
            self.emit_op(Op::StoreGlobal);
            self.emit_u16(ci);
        }
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { name, value, mutable, .. } => {
                self.compile_expr(value)?;
                if self.scope_depth == 0 {
                    let ci = self.const_str(name);
                    self.emit_op(Op::StoreGlobal);
                    self.emit_u16(ci);
                    if !mutable {
                        self.immutable_globals.insert(name.clone());
                    }
                } else {
                    let slot = self.add_local_mut(name.clone(), *mutable);
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                }
            }
            Stmt::Expr(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::Pop);
            }
            Stmt::Dump { value, target: _ } => {
                self.compile_expr(value)?;
                self.emit_op(Op::Print);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.compile_expr(e)?;
                } else {
                    self.emit_op(Op::Nil);
                }
                self.emit_op(Op::Return);
            }
            Stmt::Try { body, catch_name, catch_body } => {
                // Layout:
                //   Op::Try <handler>   [body]   Jump end
                //   handler: [StoreLocal e | Pop]  [catch_body]  CatchEnd
                //   end:
                self.emit_op(Op::Try);
                let try_pos = self.chunk.code.len();
                self.emit_byte(0xFF);
                self.emit_byte(0xFF);
                for s in body {
                    self.compile_stmt(s)?;
                }
                let jend = self.emit_jump(Op::Jump);
                // Handler: patch the Try operand to point here.
                let handler = self.chunk.code.len() as u16;
                self.chunk.code[try_pos] = (handler >> 8) as u8;
                self.chunk.code[try_pos + 1] = (handler & 0xFF) as u8;
                if let Some(cn) = catch_name {
                    let slot = self.add_local(cn.clone());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                } else {
                    self.emit_op(Op::Pop);
                }
                self.begin_scope();
                for s in catch_body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_op(Op::CatchEnd);
                self.patch_jump(jend);
            }
            Stmt::Raise(expr) => {
                self.compile_expr(expr)?;
                self.emit_op(Op::Throw);
            }
            Stmt::If { cond, then_branch, else_branch } => {
                self.compile_expr(cond)?;
                let jfalse = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.begin_scope();
                for s in then_branch {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                let jend = self.emit_jump(Op::Jump);
                self.patch_jump(jfalse);
                self.emit_op(Op::Pop);
                if let Some(else_stmts) = else_branch {
                    self.begin_scope();
                    for s in else_stmts {
                        self.compile_stmt(s)?;
                    }
                    self.end_scope();
                }
                self.patch_jump(jend);
            }
            Stmt::While { label, cond, body, .. } => {
                let loop_start = self.chunk.code.len();
                self.loop_stack.push(LoopInfo { label: label.clone(), break_jumps: Vec::new(), continue_jumps: Vec::new() });
                self.compile_expr(cond)?;
                let jexit = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_jump_back(loop_start);
                self.patch_jump(jexit);
                self.emit_op(Op::Pop);
                self.end_loop_with_continue(loop_start);
            }
            Stmt::IfLet { pattern, value, then_branch, else_branch } => {
                self.compile_expr(value)?;
                let v_slot = self.add_local("__iflet_v".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(v_slot);
                let binds = self.emit_pattern_match(pattern, v_slot)?;
                let jelse = self.emit_jump(Op::JumpIfFalse);
                self.begin_scope();
                for (i, name) in binds.iter().enumerate() {
                    self.emit_op(Op::Dup);
                    self.load_const(Value::I64(i as i64));
                    self.emit_op(Op::IndexGet);
                    let slot = self.add_local(name.clone());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                }
                self.emit_op(Op::Pop); // drop the indicator (array or true)
                for s in then_branch {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                let jend = self.emit_jump(Op::Jump);
                self.patch_jump(jelse);
                self.emit_op(Op::Pop); // drop the nil
                if let Some(els) = else_branch {
                    self.begin_scope();
                    for s in els {
                        self.compile_stmt(s)?;
                    }
                    self.end_scope();
                }
                self.patch_jump(jend);
            }
            Stmt::WhileLet { pattern, value, body } => {
                let loop_start = self.chunk.code.len();
                self.loop_stack.push(LoopInfo { label: None, break_jumps: Vec::new(), continue_jumps: Vec::new() });
                self.compile_expr(value)?;
                let v_slot = self.add_local("__whilelet_v".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(v_slot);
                let binds = self.emit_pattern_match(pattern, v_slot)?;
                let jexit = self.emit_jump(Op::JumpIfFalse);
                self.begin_scope();
                for (i, name) in binds.iter().enumerate() {
                    self.emit_op(Op::Dup);
                    self.load_const(Value::I64(i as i64));
                    self.emit_op(Op::IndexGet);
                    let slot = self.add_local(name.clone());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                }
                self.emit_op(Op::Pop); // drop the indicator
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_jump_back(loop_start);
                self.patch_jump(jexit);
                self.emit_op(Op::Pop); // drop the nil
                self.end_loop_with_continue(loop_start);
            }
            Stmt::DoWhile { cond, body } => {
                let loop_start = self.chunk.code.len();
                self.loop_stack.push(LoopInfo { label: None, break_jumps: Vec::new(), continue_jumps: Vec::new() });
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.compile_expr(cond)?;
                let jexit = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.emit_jump_back(loop_start);
                // `do..while` body runs at least once; the continue point is the
                // condition check.
                let cont = self.chunk.code.len();
                self.end_loop_with_continue(cont);
                self.patch_jump(jexit);
                self.emit_op(Op::Pop);
            }
            Stmt::Loop { label, body } => {
                let loop_start = self.chunk.code.len();
                self.loop_stack.push(LoopInfo { label: label.clone(), break_jumps: Vec::new(), continue_jumps: Vec::new() });
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_jump_back(loop_start);
                self.end_loop_with_continue(loop_start);
            }
            Stmt::For { label, pattern, iterable, body } => self.compile_for(label, pattern, iterable, body)?,
            Stmt::Break(target) => self.compile_loop_jump(target, false)?,
            Stmt::Continue(target) => self.compile_loop_jump(target, true)?,
            Stmt::Const { name, value } => {
                // `const NAME = expr` compiles like a `let` (eagerly evaluated
                // at the call site and bound; immutable by convention).
                self.compile_expr(value)?;
                if self.scope_depth == 0 {
                    let ci = self.const_str(name);
                    self.emit_op(Op::StoreGlobal);
                    self.emit_u16(ci);
                } else {
                    let slot = self.add_local(name.clone());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                }
            }
            Stmt::MacroDef { .. } => {} // registered in the pre-pass
            Stmt::Export(inner) => {
                // Compile the inner declaration; `pub fn`/`pub macro` are
                // already handled in the pre-pass, so only `pub let` (non-fn),
                // `pub const`, and (unsupported) `pub struct`/`pub enum` reach here.
                match inner.as_ref() {
                    Stmt::Let { name, value, .. } => {
                        self.compile_expr(value)?;
                        if self.scope_depth == 0 {
                            let ci = self.const_str(name);
                            self.emit_op(Op::StoreGlobal);
                            self.emit_u16(ci);
                        } else {
                            let slot = self.add_local(name.clone());
                            self.emit_op(Op::StoreLocal);
                            self.emit_byte(slot);
                        }
                    }
                    Stmt::Const { name, value } => {
                        self.compile_expr(value)?;
                        if self.scope_depth == 0 {
                            let ci = self.const_str(name);
                            self.emit_op(Op::StoreGlobal);
                            self.emit_u16(ci);
                        } else {
                            let slot = self.add_local(name.clone());
                            self.emit_op(Op::StoreLocal);
                            self.emit_byte(slot);
                        }
                    }
                    other => {
                        return Err(format!("VM does not support exporting: {:?}", other));
                    }
                }
            }
            Stmt::Tunnel { .. } => {
                return Err("VM does not support 'tunnel' statement (use `rakc run` with the interpreter)".to_string());
            }
            Stmt::Defer(expr) => {
                self.compile_defer_call(expr)?;
            }
            other => {
                return Err(format!("VM does not support statement: {:?}", other));
            }
        }
        Ok(())
    }

    fn emit_jump_back(&mut self, target: usize) {
        self.emit_op(Op::Jump);
        let t = target as u16;
        self.emit_byte((t >> 8) as u8);
        self.emit_byte((t & 0xFF) as u8);
    }

    /// Compile a `defer callee(args...)` statement. The callee and arguments
    /// are evaluated at the point the `defer` is reached and registered on the
    /// VM frame's defer stack, to be run in LIFO order when the frame returns.
    fn compile_defer_call(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Call { callee, args, named } => {
                if !named.is_empty() {
                    return Err("VM does not support named arguments in defer".to_string());
                }
                // Evaluate callee (function value) then args, leaving the callee
                // plus args on the stack for Op::DeferCall.
                self.compile_expr(callee)?;
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(Op::DeferCall);
                self.emit_byte(args.len() as u8);
                Ok(())
            }
            // Non-call defers aren't supported by the VM codegen.
            other => Err(format!("VM does not support defer of non-call expression: {:?}", other)),
        }
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        if self.scope_depth > 0 {
            self.scope_depth -= 1;
        }
        while let Some((_, d)) = self.locals.last() {
            if *d > self.scope_depth {
                self.locals.pop();
                self.immutable_locals.pop();
                self.emit_op(Op::Pop);
            } else {
                break;
            }
        }
    }

    fn compile_macro_invoke(&mut self, name: &str, args: &[Expr]) -> Result<(), String> {
        let (params, body) = match self.macros.get(name) {
            Some(d) => (d.0.clone(), d.1.clone()),
            None => return Err(format!("undefined macro '{}!'", name)),
        };
        if args.len() != params.len() {
            return Err(format!("macro '{}!' expects {} args, got {}", name, params.len(), args.len()));
        }
        let mut bindings: HashMap<String, Expr> = HashMap::new();
        for (p, a) in params.iter().zip(args.iter()) {
            bindings.insert(p.name.clone(), a.clone());
        }
        let expanded = crate::interpreter::substitute_stmts(&body, &bindings);
        // Compile the expanded body as an expression: all but the last stmt
        // are statements (popped); the last stmt's value is the result. No
        // scope wrapping (locals leak to the enclosing block, matching
        // `Expr::Block`).
        let n = expanded.len();
        for (i, s) in expanded.iter().enumerate() {
            if i == n - 1 {
                if let Stmt::Expr(e) = s {
                    self.compile_expr(e)?;
                } else {
                    self.compile_stmt(s)?;
                    self.emit_op(Op::Nil);
                }
            } else {
                self.compile_stmt(s)?;
            }
        }
        Ok(())
    }

    fn compile_for(&mut self, label: &Option<String>, pattern: &Pattern, iterable: &Expr, body: &[Stmt]) -> Result<(), String> {
        // VM support covers identifier and (k, v) tuple bindings (see
        // `bind_for_pattern`); deeper patterns run on the interpreter only.
        let name = match pattern {
            Pattern::Ident(n) => n.as_str(),
            Pattern::Wild => "",
            Pattern::Tuple(ps) => {
                // Validate the sub-patterns are plain binds.
                for sub in ps {
                    if !matches!(sub, Pattern::Ident(_) | Pattern::Wild) {
                        return Err(format!("VM does not support for-pattern: {:?}", pattern));
                    }
                }
                ""
            }
            other => return Err(format!("VM does not support for-pattern: {:?}", other)),
        };
        let _ = name;
        match iterable {
            Expr::Range(lo, hi) => {
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    self.compile_expr(lo)?;
                    let lo_slot = self.add_local("__for_lo".to_string());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(lo_slot);
                    self.compile_expr(hi)?;
                    let hi_slot = self.add_local("__for_hi".to_string());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(hi_slot);
                    let loop_start = self.chunk.code.len();
                    self.loop_stack.push(LoopInfo { label: label.clone(), break_jumps: Vec::new(), continue_jumps: Vec::new() });
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(lo_slot);
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(hi_slot);
                    self.emit_op(Op::LtEq);
                    let jexit = self.emit_jump(Op::JumpIfFalse);
                    self.emit_op(Op::Pop);
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(lo_slot);
                    let item_slot = self.add_local(name.to_string());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(item_slot);
                    self.begin_scope();
                    for s in body {
                        self.compile_stmt(s)?;
                    }
                    self.end_scope();
                    // `continue` in a range `for` skips the increment, not the
                    // loop-start (otherwise the bound never advances).
                    let inc_target = self.chunk.code.len();
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(lo_slot);
                    self.emit_op(Op::Dup);
                    self.load_const(Value::I64(1));
                    self.emit_op(Op::AddI);
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(lo_slot);
                    self.emit_jump_back(loop_start);
                    self.patch_jump(jexit);
                    self.emit_op(Op::Pop);
                    self.emit_op(Op::Pop);
                    self.emit_op(Op::Pop);
                    self.end_loop_with_continue(inc_target);
                }
                Ok(())
            }
            _ => {
                self.compile_expr(iterable)?;
                // Materialize the container into iteration items (maps become
                // (key, value) tuples; with a 2-tuple pattern, arrays/strings
                // yield (index, item) — matching the interpreter).
                let indexed = matches!(pattern, Pattern::Tuple(p) if p.len() == 2);
                self.emit_op(Op::IterItems);
                self.emit_byte(if indexed { 1 } else { 0 });
                let arr_slot = self.add_local("__for_arr".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(arr_slot);
                self.load_const(Value::I64(0));
                let idx_slot = self.add_local("__for_idx".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(idx_slot);
                let loop_start = self.chunk.code.len();
                self.loop_stack.push(LoopInfo { label: label.clone(), break_jumps: Vec::new(), continue_jumps: Vec::new() });
                self.emit_op(Op::LoadLocal);
                self.emit_byte(arr_slot);
                self.emit_op(Op::LoadLocal);
                self.emit_byte(idx_slot);
                self.emit_op(Op::IndexGet);
                let jexit = self.emit_jump(Op::JumpIfFalse);
                // item is truthy and still on the stack; store it into the loop var.
                let item_slot = self.add_local("__for_item".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(item_slot);
                self.begin_scope();
                self.bind_for_pattern(pattern, item_slot)?;
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                // `continue` in a `for` skips to the next element (the increment).
                let inc_target = self.chunk.code.len();
                self.emit_op(Op::LoadLocal);
                self.emit_byte(idx_slot);
                self.load_const(Value::I64(1));
                self.emit_op(Op::AddI);
                self.emit_op(Op::StoreLocal);
                self.emit_byte(idx_slot);
                self.emit_jump_back(loop_start);
                self.patch_jump(jexit);
                // JumpIfFalse left the (falsy) item on the stack; pop it.
                self.emit_op(Op::Pop);
                self.end_loop_with_continue(inc_target);
                Ok(())
            }
        }
    }

    /// Bind a `for` pattern from the current iteration item in local slot
    /// `item_slot`. Identifier patterns bind the whole item; tuple patterns
    /// destructure each element via `IndexGet`.
    fn bind_for_pattern(&mut self, pattern: &Pattern, item_slot: u8) -> Result<(), String> {
        match pattern {
            Pattern::Wild => Ok(()),
            Pattern::Ident(n) => {
                let slot = self.add_local(n.clone());
                self.emit_op(Op::LoadLocal);
                self.emit_byte(item_slot);
                self.emit_op(Op::StoreLocal);
                self.emit_byte(slot);
                Ok(())
            }
            Pattern::Tuple(ps) => {
                for (i, sub) in ps.iter().enumerate() {
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(item_slot);
                    self.load_const(Value::I64(i as i64));
                    self.emit_op(Op::IndexGet);
                    let sub_slot = self.add_local("__for_elem".to_string());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(sub_slot);
                    self.bind_for_pattern(sub, sub_slot)?;
                }
                Ok(())
            }
            other => Err(format!("VM does not support for-pattern: {:?}", other)),
        }
    }

    fn compile_block_value(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        if stmts.is_empty() {
            self.emit_op(Op::Nil);
            return Ok(());
        }
        let (last, rest) = stmts.split_last().unwrap();
        for s in rest {
            self.compile_stmt(s)?;
        }
        match last {
            Stmt::Expr(e) => self.compile_expr(e)?,
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.compile_expr(e)?;
                } else {
                    self.emit_op(Op::Nil);
                }
                self.emit_op(Op::Return);
            }
            other => {
                self.compile_stmt(other)?;
                self.emit_op(Op::Nil);
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Int(i) => {
                let ci = self.emit_const(Value::I64(*i));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Hex(h) => {
                let ci = self.emit_const(Value::Hex(*h, 64));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::BinLit(b) => {
                let ci = self.emit_const(Value::Hex(*b, 64));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::OctLit(o) => {
                let ci = self.emit_const(Value::Hex(*o, 64));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Char(c) => {
                let ci = self.emit_const(Value::Char(*c));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Float(f) => {
                let ci = self.emit_const(Value::F64(*f));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Float32(f) => {
                let ci = self.emit_const(Value::F32(*f));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::String(s) => {
                let ci = self.const_str(s);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Bytes(b) => {
                let ci = self.emit_const(Value::Bytes(Arc::from(b.as_slice())));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::Bool(b) => {
                self.emit_op(if *b { Op::True } else { Op::False });
            }
            Expr::Nil => {
                self.emit_op(Op::Nil);
            }
            Expr::Ident(name) => {
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(slot);
                } else {
                    let ci = self.const_str(name);
                    self.emit_op(Op::LoadGlobal);
                    self.emit_u16(ci);
                }
            }
            Expr::Unary(op, e) => {
                self.compile_expr(e)?;
                match op {
                    UnOp::Minus => self.emit_op(Op::NegI),
                    UnOp::Not => self.emit_op(Op::Not),
                    UnOp::BitNot => self.emit_op(Op::BitNot),
                }
            }
            Expr::Binary(op, l, r) => {
                self.compile_expr(l)?;
                self.compile_expr(r)?;
                self.emit_op(match op {
                    BinOp::Add => Op::AddI,
                    BinOp::Sub => Op::SubI,
                    BinOp::Mul => Op::MulI,
                    BinOp::Div => Op::DivI,
                    BinOp::Rem => Op::RemI,
                    BinOp::BitAnd => Op::BitAnd,
                    BinOp::BitOr => Op::BitOr,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::Shl => Op::Shl,
                    BinOp::Shr => Op::Shr,
                    BinOp::Eq => Op::Eq,
                    BinOp::NotEq => Op::NotEq,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::LtEq => Op::LtEq,
                    BinOp::GtEq => Op::GtEq,
                    BinOp::And => Op::Nop,
                    BinOp::Or => Op::Nop,
                });
            }
            Expr::Assign(name, value) => {
                // Enforce Rak's mutability semantics: an immutable `let x` binding
                // cannot be reassigned.
                if self.local_is_immutable(name) || self.immutable_globals.contains(name) {
                    return Err(format!("cannot assign to immutable variable `{}`", name));
                }
                self.compile_expr(value)?;
                self.emit_op(Op::Dup);
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                } else {
                    let ci = self.const_str(name);
                    self.emit_op(Op::StoreGlobal);
                    self.emit_u16(ci);
                }
            }
            Expr::CompoundAssign(op, name, value) => {
                // `x op= rhs` — cur = cur op rhs; yields the result.
                if self.local_is_immutable(name) || self.immutable_globals.contains(name) {
                    return Err(format!("cannot assign to immutable variable `{}`", name));
                }
                self.compile_ident_load(name)?;
                self.compile_expr(value)?;
                self.emit_op(match op {
                    CompoundOp::Add => Op::AddI,
                    CompoundOp::Sub => Op::SubI,
                    CompoundOp::Mul => Op::MulI,
                    CompoundOp::Div => Op::DivI,
                    CompoundOp::Rem => Op::RemI,
                    CompoundOp::BitAnd => Op::BitAnd,
                    CompoundOp::BitOr => Op::BitOr,
                    CompoundOp::BitXor => Op::BitXor,
                    CompoundOp::Shl => Op::Shl,
                    CompoundOp::Shr => Op::Shr,
                });
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                } else {
                    let ci = self.const_str(name);
                    self.emit_op(Op::StoreGlobal);
                    self.emit_u16(ci);
                }
            }
            Expr::IndexAssign { obj, idx, value } => {
                // `obj[idx] = value` for a named (local/global) container:
                // container; index; value; IndexSet (leaves mutated container);
                // store it back to the variable; yield nil.
                let obj_expr = obj.as_ref();
                let name = match obj_expr {
                    Expr::Ident(n) => n,
                    _ => return Err("VM index-assign: target must be a plain variable".to_string()),
                };
                if self.local_is_immutable(name) || self.immutable_globals.contains(name) {
                    return Err(format!("cannot assign to immutable variable `{}`", name));
                }
                self.compile_ident_load(name)?;
                self.compile_expr(idx)?;
                self.compile_expr(value)?;
                self.emit_op(Op::IndexSet);
                self.store_ident_to(name)?;
                self.emit_op(Op::Nil);
            }
            Expr::FieldAssign { obj, field, value } => {
                let obj_expr = obj.as_ref();
                let name = match obj_expr {
                    Expr::Ident(n) => n,
                    _ => return Err("VM field-assign: target must be a plain variable".to_string()),
                };
                if self.local_is_immutable(name) || self.immutable_globals.contains(name) {
                    return Err(format!("cannot assign to immutable variable `{}`", name));
                }
                self.compile_ident_load(name)?;
                self.compile_expr(value)?;
                let fi = self.const_str(field);
                self.emit_op(Op::FieldSet);
                self.emit_u16(fi);
                self.store_ident_to(name)?;
                self.emit_op(Op::Nil);
            }
            Expr::Array(items) => {
                for e in items {
                    self.compile_expr(e)?;
                }
                let ci = self.emit_const(Value::I64(items.len() as i64));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::NewArray);
            }
            Expr::Call { callee, args, named } => {
                if !named.is_empty() {
                    return Err("VM does not support named arguments".to_string());
                }
                // `Enum::Variant(args...)` — call the baked ctor native.
                if let Expr::Path(segs) = callee.as_ref() {
                    if segs.len() == 2 {
                        let key = format!("__enum_new_{}_{}", segs[0], segs[1]);
                        if self.enum_ctor_names.contains(&key) {
                            let ci = self.const_str(&key);
                            self.emit_op(Op::LoadGlobal);
                            self.emit_u16(ci);
                            for a in args {
                                self.compile_expr(a)?;
                            }
                            self.emit_op(Op::Call);
                            self.emit_byte(args.len() as u8);
                            return Ok(());
                        }
                    }
                }
                if let Expr::Ident(name) = callee.as_ref() {
                    if self.func_names.contains(name) {
                        let ci = self.const_str(name);
                        self.emit_op(Op::LoadGlobal);
                        self.emit_u16(ci);
                        for a in args {
                            self.compile_expr(a)?;
                        }
                        self.emit_op(Op::Call);
                        self.emit_byte(args.len() as u8);
                        return Ok(());
                    }
                }
                // `lib.call(symbol, args_array)` and `lib.close()` desugar to FFI opcodes.
                if let Expr::FieldAccess(obj, method) = callee.as_ref() {
                    if method == "call" && args.len() == 2 {
                        self.compile_expr(obj)?;
                        for a in args {
                            self.compile_expr(a)?;
                        }
                        self.emit_op(Op::FFICall);
                        return Ok(());
                    }
                    if method == "close" && args.is_empty() {
                        self.compile_expr(obj)?;
                        self.emit_op(Op::FFIClose);
                        return Ok(());
                    }
                    // `Name.decode(bytes)` / `Name.encode(value)` where `Name`
                    // is a registered `binstruct` — lower to a call to a
                    // per-binstruct baked native global `__bin_decode_<Name>` /
                    // `__bin_encode_<Name>` (self-contained closure, built in
                    // the pre-pass — matches the `make_foreign_native` precedent,
                    // no new VM opcodes).
                    if let Expr::Ident(name) = obj.as_ref() {
                        if self.binstructs.contains_key(name) {
                            let native_name = match method.as_str() {
                                "decode" => format!("__bin_decode_{}", name),
                                "encode" => format!("__bin_encode_{}", name),
                                _ => String::new(),
                            };
                            if !native_name.is_empty() {
                                let ni = self.const_str(&native_name);
                                self.emit_op(Op::LoadGlobal);
                                self.emit_u16(ni);
                                for a in args {
                                    self.compile_expr(a)?;
                                }
                                self.emit_op(Op::Call);
                                self.emit_byte(args.len() as u8);
                                return Ok(());
                            }
                        }
                    }
                    // General method call `obj.method(args...)` — dispatch at
                    // runtime via `Op::CallMethod` (regex natives, baked impl
                    // methods, then a field holding a callable).
                    if !named.is_empty() {
                        return Err("VM does not support named arguments".to_string());
                    }
                    self.compile_expr(obj)?;
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    let mi = self.const_str(method);
                    self.emit_op(Op::CallMethod);
                    self.emit_u16(mi);
                    self.emit_byte(args.len() as u8);
                    return Ok(());
                }
                self.compile_expr(callee)?;
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(Op::Call);
                self.emit_byte(args.len() as u8);
            }
            Expr::If { cond, then_branch, else_branch } => {
                self.compile_expr(cond)?;
                let jfalse = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.compile_block_value(then_branch)?;
                let jend = self.emit_jump(Op::Jump);
                self.patch_jump(jfalse);
                self.emit_op(Op::Pop);
                if let Some(else_stmts) = else_branch {
                    self.compile_block_value(else_stmts)?;
                } else {
                    self.emit_op(Op::Nil);
                }
                self.patch_jump(jend);
            }
            Expr::Block(stmts) => {
                self.compile_block_value(stmts)?;
            }
            Expr::Tuple(items) => {
                for e in items {
                    self.compile_expr(e)?;
                }
                self.load_const(Value::I64(items.len() as i64));
                self.emit_op(Op::NewTuple);
            }
            Expr::Map(pairs) => {
                for (k, v) in pairs {
                    self.compile_expr(k)?;
                    self.compile_expr(v)?;
                }
                self.load_const(Value::I64(pairs.len() as i64));
                self.emit_op(Op::NewMap);
            }
            Expr::Index(obj, idx) => {
                self.compile_expr(obj)?;
                self.compile_expr(idx)?;
                self.emit_op(Op::IndexGet);
            }
            Expr::FieldAccess(obj, field) => {
                self.compile_expr(obj)?;
                let ci = self.const_str(field);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::FieldGet);
            }
            Expr::Await(inner) => {
                self.compile_expr(inner)?;
                self.emit_op(Op::Await);
            }
            Expr::Function { params, body, .. } => {
                // Compile a function literal to a closure constant (used by
                // `pub fn` inlining and nested function values).
                let closure = self.compile_function("<anon>", params, body)?;
                self.load_const(closure);
            }
            Expr::MacroVar(name) => {
                return Err(format!("macro variable '${}' used outside a macro body", name));
            }
            Expr::MacroInvoke { name, args } => {
                self.compile_macro_invoke(name, args)?;
            }
            Expr::Interp { template, parts } => {
                let fmt_str = interp_to_fmt(template);
                let fmt_ci = self.const_str("fmt");
                let tpl_ci = self.const_str(&fmt_str);
                self.emit_op(Op::LoadGlobal);
                self.emit_u16(fmt_ci);
                self.emit_op(Op::LoadConst);
                self.emit_u16(tpl_ci);
                for p in parts {
                    self.compile_expr(p)?;
                }
                self.emit_op(Op::Call);
                self.emit_byte((1 + parts.len()) as u8);
            }
            Expr::Match { value, arms } => {
                self.compile_match(value, arms)?;
            }
            Expr::Regex(pattern, flags) => {
                let v = make_regex_value(pattern, flags)?;
                let ci = self.emit_const(v);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
            }
            Expr::EvidenceFrom { value } => {
                // `evidence<T> from expr` — on the VM, evaluate the inner value
                // and wrap it in a `Value::Evidence` with a root provenance via
                // the `__evidence_from` native. Call convention: [callee, arg]
                // then Call(argc=1).
                let ci = self.const_str("__evidence_from");
                self.emit_op(Op::LoadGlobal);
                self.emit_u16(ci);
                self.compile_expr(value)?;
                self.emit_op(Op::Call);
                self.emit_byte(1);
            }
            Expr::TryExpr(inner) => {
                // `expr?` — unwrap Result/Option or raise (bindable by catch).
                self.compile_expr(inner)?;
                self.emit_op(Op::TryUnwrap);
            }
            Expr::Ternary { cond, then, els } => {
                self.compile_expr(cond)?;
                let jf = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.compile_expr(then)?;
                let je = self.emit_jump(Op::Jump);
                self.patch_jump(jf);
                self.emit_op(Op::Pop);
                self.compile_expr(els)?;
                self.patch_jump(je);
            }
            Expr::NilCoalesce(l, r) => {
                // `a ?? b` — push `l`; if nil/None, jump to `r`. Keeps the
                // value on the stack on the non-nil path.
                self.compile_expr(l)?;
                self.emit_op(Op::Dup);
                // is it nil/None? nil-coalescing needs a nil test op.
                let jnil = self.emit_jump(Op::JumpIfNil);
                self.emit_op(Op::Pop); // pop the duplicated value
                let je = self.emit_jump(Op::Jump);
                // nil path: pop the nil, evaluate r
                self.patch_jump(jnil);
                self.emit_op(Op::Pop);
                self.compile_expr(r)?;
                self.patch_jump(je);
            }
            Expr::OptField(obj, field) => {
                // `obj?.field` — nil if obj is nil/None, else field.
                self.compile_expr(obj)?;
                self.emit_op(Op::Dup);
                let jnil = self.emit_jump(Op::JumpIfNil);
                self.emit_op(Op::Pop);
                let ci = self.const_str(field);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::FieldGet);
                let je = self.emit_jump(Op::Jump);
                self.patch_jump(jnil);
                self.emit_op(Op::Pop);
                self.emit_op(Op::Nil);
                self.patch_jump(je);
            }
            Expr::OptIndex(obj, idx) => {
                self.compile_expr(obj)?;
                self.emit_op(Op::Dup);
                let jnil = self.emit_jump(Op::JumpIfNil);
                self.emit_op(Op::Pop);
                self.compile_expr(idx)?;
                self.emit_op(Op::IndexGet);
                let je = self.emit_jump(Op::Jump);
                self.patch_jump(jnil);
                self.emit_op(Op::Pop);
                self.emit_op(Op::Nil);
                self.patch_jump(je);
            }
            Expr::MultiAssign { targets, values } => {
                // `a, b = x, y` — evaluate all RHS into temps, then store to
                // each (Ident) target; yields nil.
                if targets.len() != values.len() {
                    return Err("multi-assign: target/value count mismatch".to_string());
                }
                let mut slots: Vec<u8> = Vec::new();
                for v in values {
                    self.compile_expr(v)?;
                    let s = self.add_local("__ma".to_string());
                    slots.push(s);
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(s);
                }
                for (i, t) in targets.iter().enumerate() {
                    match t {
                        Expr::Ident(name) => {
                            if self.local_is_immutable(name) || self.immutable_globals.contains(name) {
                                return Err(format!("cannot assign to immutable variable `{}`", name));
                            }
                            self.emit_op(Op::LoadLocal);
                            self.emit_byte(slots[i]);
                            self.store_ident_to(name)?;
                        }
                        _ => return Err("VM multi-assign: target must be a plain variable".to_string()),
                    }
                }
                self.emit_op(Op::Nil);
            }
            Expr::StructLit { name, fields } => {
                // Build a `Value::Struct`: push (name-const, value) pairs then
                // `Op::StructNew <name> <n>`.
                for (fname, fval) in fields {
                    let ni = self.const_str(fname);
                    self.emit_op(Op::LoadConst);
                    self.emit_u16(ni);
                    self.compile_expr(fval)?;
                }
                let name_ci = self.const_str(name);
                self.emit_op(Op::StructNew);
                self.emit_u16(name_ci);
                self.emit_byte(fields.len() as u8);
            }
            Expr::Path(segs) => {
                // `Enum::Variant` (unit variant, no args) — call the baked
                // ctor native with zero args.
                if segs.len() == 2 {
                    let key = format!("__enum_new_{}_{}", segs[0], segs[1]);
                    if self.enum_ctor_names.contains(&key) {
                        let ci = self.const_str(&key);
                        self.emit_op(Op::LoadGlobal);
                        self.emit_u16(ci);
                        self.emit_op(Op::Call);
                        self.emit_byte(0);
                        return Ok(());
                    }
                }
                return Err(format!("VM does not support path expression: {}", segs.join("::")));
            }
            other => {
                return Err(format!("VM does not support expression: {:?}", other));
            }
        }
        Ok(())
    }

    fn compile_match(&mut self, value: &Expr, arms: &[(Pattern, Option<Expr>, Vec<Stmt>)]) -> Result<(), String> {
        self.compile_expr(value)?;
        let v_slot = self.add_local("__match_v".to_string());
        self.emit_op(Op::StoreLocal);
        self.emit_byte(v_slot);
        let mut end_jumps: Vec<usize> = Vec::new();
        for (pattern, guard, body) in arms {
            self.emit_op(Op::LoadLocal);
            self.emit_byte(v_slot);
            let is_structural = matches!(
                pattern,
                Pattern::Tuple(_)
                    | Pattern::Array(_)
                    | Pattern::Byte(_)
                    | Pattern::Bytes(_)
                    | Pattern::Struct(_, _)
                    | Pattern::EnumVariant(_, _, _)
                    | Pattern::Range(_, _)
                    | Pattern::Or(_)
                    | Pattern::Some(_)
                    | Pattern::None
                    | Pattern::Ok(_)
                    | Pattern::Err(_)
            );
            if is_structural {
                // Structural patterns: emit the descriptor, run `Op::MatchPat`,
                // then bind extracted values into fresh per-arm locals. The
                // bindings array doubles as the truthiness indicator: a match
                // leaves a non-empty array (or `true`) on the stack, a failed
                // match leaves `nil`.
                let (desc, binds) = self.structural_pattern_desc(pattern)?;
                let di = self.emit_const(desc);
                let bindnames = Value::Array(Arc::from(
                    binds.iter().map(|n| Value::String(Arc::from(n.as_str()))).collect::<Vec<_>>(),
                ));
                let bi = self.emit_const(bindnames);
                self.emit_op(Op::LoadConst);
                self.emit_u16(di);
                self.emit_op(Op::LoadConst);
                self.emit_u16(bi);
                self.emit_op(Op::MatchPat);
                self.emit_u16(di);
                self.emit_u16(bi);
                let jnext = self.emit_jump(Op::JumpIfFalse);
                self.begin_scope();
                for (i, name) in binds.iter().enumerate() {
                    self.emit_op(Op::Dup);
                    self.load_const(Value::I64(i as i64));
                    self.emit_op(Op::IndexGet);
                    let slot = self.add_local(name.clone());
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                }
                // For a structural pattern with no binds, MatchPat left a bare
                // `true` (stack has one value); with binds it left the array.
                if !binds.is_empty() {
                    self.emit_op(Op::Pop); // drop the bindings array
                }
                self.compile_match_arm(guard, body, &mut end_jumps, jnext)?;
            } else {
                // Scalar inline path (existing codegen): pop scrutinee, push bool.
                let bind = self.compile_pattern(pattern)?;
                let jnext = self.emit_jump(Op::JumpIfFalse);
                self.emit_op(Op::Pop);
                self.begin_scope();
                if let Some(name) = bind {
                    let slot = self.add_local(name);
                    self.emit_op(Op::LoadLocal);
                    self.emit_byte(v_slot);
                    self.emit_op(Op::StoreLocal);
                    self.emit_byte(slot);
                }
                self.compile_match_arm(guard, body, &mut end_jumps, jnext)?;
            }
        }
        self.emit_op(Op::Nil);
        for j in end_jumps {
            self.patch_jump(j);
        }
        Ok(())
    }

    /// Compile the (possibly guarded) body of a match arm and the join edges for
    /// the next arm. `jnext` is the `JumpIfFalse` that forwards an unmatched
    /// scrutinee; it is patched to skip this arm, and the unmatched indicator
    /// left on the stack is popped so the next arm starts clean.
    fn compile_match_arm(
        &mut self,
        guard: &Option<Expr>,
        body: &[Stmt],
        end_jumps: &mut Vec<usize>,
        jnext: usize,
    ) -> Result<(), String> {
        if let Some(g) = guard {
            self.compile_expr(g)?;
            let jg = self.emit_jump(Op::JumpIfFalse);
            self.emit_op(Op::Pop);
            self.compile_block_value(body)?;
            self.end_scope_for_match();
            let jend = self.emit_jump(Op::Jump);
            end_jumps.push(jend);
            self.patch_jump(jg);
            self.emit_op(Op::Pop);
            self.patch_jump(jnext);
            self.emit_op(Op::Pop);
        } else {
            self.compile_block_value(body)?;
            self.end_scope_for_match();
            let jend = self.emit_jump(Op::Jump);
            end_jumps.push(jend);
            self.patch_jump(jnext);
            self.emit_op(Op::Pop);
        }
        Ok(())
    }

    fn end_scope_for_match(&mut self) {
        while let Some((_, d)) = self.locals.last() {
            if *d > self.scope_depth {
                self.locals.pop();
                self.immutable_locals.pop();
            } else {
                break;
            }
        }
    }

    /// Emit the descriptor-based match of a pattern against a scrutinee in
    /// local slot `v_slot`, leaving a truthy indicator on the stack (a
    /// bindings array on a match, `nil` otherwise). Returns the bound names.
    fn emit_pattern_match(&mut self, pattern: &Pattern, v_slot: u8) -> Result<Vec<String>, String> {
        let desc = self.pattern_descriptor(pattern)?;
        let binds = self.pattern_binds(pattern);
        let di = self.emit_const(desc);
        let bindnames = Value::Array(Arc::from(
            binds.iter().map(|n| Value::String(Arc::from(n.as_str()))).collect::<Vec<_>>(),
        ));
        let bi = self.emit_const(bindnames);
        self.emit_op(Op::LoadLocal);
        self.emit_byte(v_slot);
        self.emit_op(Op::LoadConst);
        self.emit_u16(di);
        self.emit_op(Op::LoadConst);
        self.emit_u16(bi);
        self.emit_op(Op::MatchPat);
        self.emit_u16(di);
        self.emit_u16(bi);
        Ok(binds)
    }

    /// Patch every floating `break` and `continue` of the innermost loop (which
    /// just ended), then pop it. `continue_target` is the loop's continue point
    /// (its increment for `for`; its start for `while`/`loop`).
    fn end_loop_with_continue(&mut self, continue_target: usize) {
        if let Some(mut li) = self.loop_stack.pop() {
            for j in li.continue_jumps.drain(..) {
                self.chunk.code[j] = (continue_target >> 8) as u8;
                self.chunk.code[j + 1] = (continue_target & 0xFF) as u8;
            }
            for b in li.break_jumps {
                self.patch_jump(b);
            }
        }
    }

    /// `break [target]` / `continue [target]` on the VM: resolve the target
    /// loop from the loop stack and emit a forward placeholder jump patched
    /// when the loop ends (`continue` to its increment/start, `break` to its
    /// end). Numeric-depth targets index outward from the innermost loop; a
    /// label matches by name.
    fn compile_loop_jump(&mut self, target: &Option<BreakTarget>, is_continue: bool) -> Result<(), String> {
        let idx = match target {
            None => 0,
            Some(BreakTarget::Depth(n)) => (*n - 1) as usize,
            Some(BreakTarget::Label(l)) => {
                let mut found = None;
                for (i, li) in self.loop_stack.iter().enumerate().rev() {
                    if li.label.as_deref() == Some(l.as_str()) {
                        found = Some(i);
                        break;
                    }
                }
                found.ok_or_else(|| format!("VM loop label '{}' not found", l))?
            }
        };
        self.loop_stack.get(idx).ok_or_else(|| "VM break/continue outside a loop".to_string())?;
        let j = self.emit_jump(Op::Jump);
        if is_continue {
            self.loop_stack.get_mut(idx).unwrap().continue_jumps.push(j);
        } else {
            self.loop_stack.get_mut(idx).unwrap().break_jumps.push(j);
        }
        Ok(())
    }

    fn compile_pattern(&mut self, pattern: &Pattern) -> Result<Option<String>, String> {
        match pattern {
            Pattern::Wild => {
                self.emit_op(Op::Pop);
                self.emit_op(Op::True);
                Ok(None)
            }
            Pattern::Ident(n) => {
                self.emit_op(Op::Pop);
                self.emit_op(Op::True);
                Ok(Some(n.clone()))
            }
            Pattern::Int(i) => {
                self.load_const(Value::I64(*i));
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::Hex(h) => {
                self.load_const(Value::Hex(*h, 64));
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::String(s) => {
                let ci = self.const_str(s);
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::Bool(b) => {
                self.emit_op(if *b { Op::True } else { Op::False });
                self.emit_op(Op::Eq);
                Ok(None)
            }
            Pattern::Nil => {
                self.emit_op(Op::Nil);
                self.emit_op(Op::Eq);
                Ok(None)
            }
            other => Err(format!("VM match does not support pattern: {:?}", other)),
        }
    }

    fn val_str(&self, s: &str) -> Value {
        Value::String(Arc::from(s))
    }

    /// Collect, depth-first, every name bound by a pattern (for `MatchPat`
    /// bindings). `Ident` binds; `_`/`Wild` and scalar/byte literals do not.
    fn pattern_binds(&self, p: &Pattern) -> Vec<String> {
        match p {
            Pattern::Ident(n) => vec![n.clone()],
            Pattern::Tuple(ps) | Pattern::Array(ps) | Pattern::Or(ps) => {
                ps.iter().flat_map(|x| self.pattern_binds(x)).collect()
            }
            Pattern::Struct(_, fields) => fields.iter().flat_map(|(_, sub)| self.pattern_binds(sub)).collect(),
            Pattern::EnumVariant(_, _, subs) => subs.iter().flat_map(|x| self.pattern_binds(x)).collect(),
            Pattern::Some(inner) | Pattern::Ok(inner) | Pattern::Err(inner) => self.pattern_binds(inner),
            Pattern::Range(lo, hi) => {
                let mut v = self.pattern_binds(lo);
                v.extend(self.pattern_binds(hi));
                v
            }
            _ => Vec::new(),
        }
    }

    /// Build a `PatternDef` descriptor (a `Value::Array`) for a structural
    /// pattern plus its bound names. Returns `Err` for unsupported patterns.
    fn structural_pattern_desc(&mut self, p: &Pattern) -> Result<(Value, Vec<String>), String> {
        let desc = self.pattern_descriptor(p)?;
        let binds = self.pattern_binds(p);
        Ok((desc, binds))
    }

    /// Build the recursive pattern descriptor. `Ident` becomes a capture node
    /// `["bind", name, ["wild"]]`; every other node carries a tag string at
    /// index 0 (mirroring `vm::vm_pattern_match`).
    pub(crate) fn pattern_descriptor(&mut self, p: &Pattern) -> Result<Value, String> {
        fn arr(vals: Vec<Value>) -> Value {
            Value::Array(Arc::from(vals))
        }
        fn arr1(v: Value) -> Value {
            Value::Array(Arc::from(vec![v]))
        }
        let tag = |s: &str| Value::String(Arc::from(s));
        Ok(match p {
            Pattern::Wild => arr1(tag("wild")),
            Pattern::Ident(name) => arr(vec![tag("bind"), tag(name), arr1(tag("wild"))]),
            Pattern::Int(i) => arr(vec![tag("int"), Value::I64(*i)]),
            Pattern::Hex(h) => arr(vec![tag("hex"), Value::U64(*h)]),
            Pattern::String(s) => arr(vec![tag("string"), tag(s)]),
            Pattern::Bool(b) => arr(vec![tag("bool"), Value::Bool(*b)]),
            Pattern::Nil => arr1(tag("nil")),
            Pattern::Byte(b) => arr(vec![tag("byte"), Value::I64(*b as i64)]),
            Pattern::Bytes(pats) => {
                let mut bytes: Vec<Value> = Vec::new();
                let mut rest = false;
                for b in pats {
                    match b {
                        BytesPat::Byte(x) => bytes.push(Value::I64(*x as i64)),
                        BytesPat::Rest => rest = true,
                    }
                }
                arr(vec![tag("bytes"), arr(bytes), Value::Bool(rest)])
            }
            Pattern::Tuple(ps) | Pattern::Array(ps) => {
                let subs = ps.iter().map(|x| self.pattern_descriptor(x)).collect::<Result<Vec<_>, _>>()?;
                let kind = if matches!(p, Pattern::Tuple(_)) { "tuple" } else { "array" };
                arr(vec![tag(kind), arr(subs)])
            }
            Pattern::Struct(name, fields) => {
                let mut fs: Vec<Value> = Vec::new();
                for (fname, sub) in fields {
                    fs.push(tag(fname));
                    fs.push(self.pattern_descriptor(sub)?);
                }
                arr(vec![tag("struct"), tag(name), arr(fs)])
            }
            Pattern::EnumVariant(ename, variant, subs) => {
                let subs = subs.iter().map(|x| self.pattern_descriptor(x)).collect::<Result<Vec<_>, _>>()?;
                arr(vec![tag("enum"), tag(ename), tag(variant), arr(subs)])
            }
            Pattern::Range(lo, hi) => arr(vec![
                tag("range"),
                self.pattern_descriptor(lo)?,
                self.pattern_descriptor(hi)?,
            ]),
            Pattern::Or(ps) => {
                let subs = ps.iter().map(|x| self.pattern_descriptor(x)).collect::<Result<Vec<_>, _>>()?;
                arr(vec![tag("or"), arr(subs)])
            }
            Pattern::Some(inner) => arr(vec![tag("some"), self.pattern_descriptor(inner)?]),
            Pattern::None => arr1(tag("none")),
            Pattern::Ok(inner) => arr(vec![tag("ok"), self.pattern_descriptor(inner)?]),
            Pattern::Err(inner) => arr(vec![tag("err"), self.pattern_descriptor(inner)?]),
        })
    }
}

fn interp_to_fmt(template: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                out.push_str("{{");
            } else {
                out.push_str("{}");
                let mut depth = 1;
                while let Some(c2) = chars.next() {
                    if c2 == '{' {
                        depth += 1;
                    } else if c2 == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
            }
        } else if c == '}' {
            if chars.peek() == Some(&'}') {
                chars.next();
                out.push_str("}}");
            } else {
                out.push('}');
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn compile_module(module: &Module) -> Result<Chunk, String> {
    let mut c = Compiler::new();
    c.compile(module)
}

/// Compile a module located at `file`, resolving name-based imports relative
/// to its parent directory. Used for VM module loading.
pub fn compile_module_in(module: &Module, base_dir: &str) -> Result<Chunk, String> {
    let mut c = Compiler::new();
    c.base_dir = base_dir.to_string();
    c.compile(module)
}

/// Build a `Value::Regex` constant, compiling the pattern at compile time so
/// the VM never pays for regex construction at runtime.
fn make_regex_value(pattern: &str, flags: &str) -> Result<Value, String> {
    let mut b = regex::RegexBuilder::new(pattern);
    for f in flags.chars() {
        match f {
            'i' | 'I' => b.case_insensitive(true),
            'm' | 'M' => b.multi_line(true),
            's' | 'S' => b.dot_matches_new_line(true),
            'x' | 'X' => b.ignore_whitespace(true),
            'g' | 'G' => continue,
            _ => return Err(format!("unknown regex flag '{}'", f)),
        };
    }
    let re = b.build().map_err(|e| format!("invalid regex /{}/{}: {}", pattern, flags, e))?;
    Ok(Value::Regex(Arc::new(crate::value::RegexValue {
        pattern: pattern.to_string(),
        flags: flags.to_string(),
        re,
    })))
}

/// A `binstruct` field with any nested `Ref` resolved to its concrete fields at
/// compile time, so the VM decode/encode natives are fully self-contained.
#[derive(Clone)]
struct ResolvedBinField {
    name: String,
    kind: ResolvedBinKind,
}

#[derive(Clone)]
enum ResolvedBinKind {
    Uint { bits: u8, endian: Endian },
    Int { bits: u8, endian: Endian },
    /// A non-byte-aligned unsigned bitfield, LSB-first.
    Bits { bits: u8 },
    Bytes(usize),
    Rest,
    Ref(Vec<ResolvedBinField>),
}

/// Resolve a `binstruct`'s flat field list (expanding `Ref` fields recursively)
/// into a self-contained `Vec<ResolvedBinField>`. Errors on unknown nested refs
/// or cycles.
fn resolve_binstruct(name: &str, all: &HashMap<String, Vec<BinField>>) -> Result<Vec<ResolvedBinField>, String> {
    fn resolve_one(
        name: &str,
        all: &HashMap<String, Vec<BinField>>,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<Vec<ResolvedBinField>, String> {
        if !seen.insert(name.to_string()) {
            return Err(format!("binstruct '{}': circular ref", name));
        }
        let fields = all
            .get(name)
            .ok_or_else(|| format!("unknown binstruct '{}'", name))?;
        let mut out = Vec::with_capacity(fields.len());
        for f in fields {
            let kind = match &f.kind {
                BinKind::Uint { bits, endian } => ResolvedBinKind::Uint { bits: *bits, endian: *endian },
                BinKind::Int { bits, endian } => ResolvedBinKind::Int { bits: *bits, endian: *endian },
                BinKind::Bits { bits } => ResolvedBinKind::Bits { bits: *bits },
                BinKind::Bytes(n) => ResolvedBinKind::Bytes(*n),
                BinKind::Rest => ResolvedBinKind::Rest,
                BinKind::Ref(r) => ResolvedBinKind::Ref(resolve_one(r, all, seen)?),
            };
            out.push(ResolvedBinField { name: f.name.clone(), kind });
        }
        Ok(out)
    }
    let mut seen = std::collections::HashSet::new();
    resolve_one(name, all, &mut seen)
}

fn now_secs_vm() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Build a self-contained `Value::NativeFn` that decodes raw bytes per the
/// resolved binstruct layout, returning a `Value::Evidence` wrapping a
/// `Value::Struct`. Used by the VM via `Op::Call` — no new opcode.
fn make_bin_decode_native(name: String, fields: Vec<ResolvedBinField>) -> Value {
    use crate::value::Provenance;
    let nm: Arc<str> = Arc::from(name.as_str());
    Value::NativeFn(
        Arc::from(format!("__bin_decode_{}", name).as_str()),
        Arc::new(move |args: &[Value]| {
            let bytes: Vec<u8> = match args.first() {
                Some(Value::Bytes(b)) => b.to_vec(),
                Some(Value::MmapSlice(h, off, n)) => {
                    let s = h.as_slice();
                    let st = (*off).min(s.len());
                    let en = (off + n).min(s.len());
                    s[st..en].to_vec()
                }
                Some(Value::Mmap(h)) => h.as_slice().to_vec(),
                Some(Value::String(s)) => s.to_string().into_bytes(),
                Some(v) => v.to_string().into_bytes(),
                None => return Err("decode expects bytes".to_string()),
            };
            let mut off = 0usize;
            let mut bit = 0usize;
            let mut map: HashMap<String, Value> = HashMap::new();
            for f in &fields {
                let (v, no, nb) = decode_resolved(f, &bytes, off, bit)?;
                map.insert(f.name.clone(), v);
                off = no;
                bit = nb;
            }
            let prov = Arc::new(Provenance {
                tool: format!("binstruct:{}", nm),
                target: String::new(),
                ts: now_secs_vm(),
                raw_offset: Some(0),
                raw_len: Some(bytes.len() as u64),
                parent: None,
            });
            Ok(Value::Evidence {
                inner: Box::new(Value::Struct {
                    name: Arc::from(nm.as_ref()),
                    fields: Arc::from(map),
                }),
                provenance: prov,
            })
        }),
    )
}

/// Decode a single resolved field. Returns `(value, new_byte_offset,
/// new_bit_offset)`. Bitfields share bytes LSB-first until a byte-aligned
/// field (or the struct end) flushes the bit cursor to the next byte.
fn decode_resolved(
    f: &ResolvedBinField,
    bytes: &[u8],
    off: usize,
    bit_off: usize,
) -> Result<(Value, usize, usize), String> {
    use crate::ast::Endian::*;
    match &f.kind {
        ResolvedBinKind::Rest => {
            let start = off + bit_off / 8;
            let v = if start >= bytes.len() { Vec::new() } else { bytes[start..].to_vec() };
            Ok((Value::Bytes(Arc::from(v.as_slice())), bytes.len(), 0))
        }
        ResolvedBinKind::Bytes(n) => {
            let start = off + bit_off.div_ceil(8);
            if start + n > bytes.len() {
                return Err(format!("binstruct: field '{}' outruns buffer", f.name));
            }
            Ok((Value::Bytes(Arc::from(&bytes[start..start + n])), start + n, 0))
        }
        ResolvedBinKind::Uint { bits, endian } => {
            let n = (*bits as usize) / 8;
            let start = off + bit_off.div_ceil(8);
            if start + n > bytes.len() {
                return Err(format!("binstruct: field '{}' outruns buffer", f.name));
            }
            let mut acc: u64 = 0;
            match endian {
                Big => for i in 0..n { acc = (acc << 8) | bytes[start + i] as u64; },
                Little => for i in 0..n { acc |= (bytes[start + i] as u64) << (8 * i); },
            }
            Ok((Value::Hex(acc, *bits as usize), start + n, 0))
        }
        ResolvedBinKind::Int { bits, endian } => {
            let n = (*bits as usize) / 8;
            let start = off + bit_off.div_ceil(8);
            if start + n > bytes.len() {
                return Err(format!("binstruct: field '{}' outruns buffer", f.name));
            }
            let mut acc: u64 = 0;
            match endian {
                Big => for i in 0..n { acc = (acc << 8) | bytes[start + i] as u64; },
                Little => for i in 0..n { acc |= (bytes[start + i] as u64) << (8 * i); },
            }
            let v = match *bits {
                8 => bytes[start] as i8 as i64,
                16 => acc as u16 as i16 as i64,
                32 => acc as u32 as i32 as i64,
                64 => acc as i64,
                _ => acc as i64,
            };
            Ok((Value::I64(v), start + n, 0))
        }
        ResolvedBinKind::Bits { bits } => {
            let width = *bits as usize;
            if off + bit_off / 8 >= bytes.len() {
                return Err(format!("binstruct: field '{}' outruns buffer", f.name));
            }
            // Read `width` bits LSB-first starting at absolute bit position
            // `off*8 + bit_off`.
            let mut acc: u64 = 0;
            for j in 0..width {
                let pos = bit_off + j;
                let byte = bytes.get(off + pos / 8).copied().unwrap_or(0);
                let bit = (byte >> (pos % 8)) & 1;
                acc |= (bit as u64) << j;
            }
            let new_bit = bit_off + width;
            Ok((Value::Hex(acc, width), off + new_bit / 8, new_bit % 8))
        }
        ResolvedBinKind::Ref(inner) => {
            let mut map: HashMap<String, Value> = HashMap::new();
            let mut io = off + bit_off.div_ceil(8);
            let mut ibit = 0usize;
            for nf in inner {
                let (v, no, nb) = decode_resolved(nf, bytes, io, ibit)?;
                map.insert(nf.name.clone(), v);
                io = no;
                ibit = nb;
            }
            Ok((
                Value::Struct {
                    name: Arc::from("nested"),
                    fields: Arc::from(map),
                },
                io,
                ibit,
            ))
        }
    }
}

/// Build a self-contained `Value::NativeFn` that encodes a struct/map back
/// into raw bytes per the resolved binstruct layout. Inverse of decode.
fn make_bin_encode_native(name: String, fields: Vec<ResolvedBinField>) -> Value {
    let _ = &name;
    Value::NativeFn(
        Arc::from(format!("__bin_encode_{}", name).as_str()),
        Arc::new(move |args: &[Value]| {
            let val = args.first().cloned().unwrap_or(Value::Nil);
            let inner = match &val {
                Value::Evidence { inner, .. } => (**inner).clone(),
                other => other.clone(),
            };
            let map = match inner {
                Value::Struct { fields, .. } => (*fields).clone(),
                Value::Map(m) => (*m).clone(),
                other => return Err(format!("encode expects struct/map, got {}", other.type_name())),
            };
            let mut out: Vec<u8> = Vec::new();
            let mut bit = 0usize;
            for f in &fields {
                bit = encode_resolved(f, &map, &mut out, bit)?;
            }
            if bit % 8 != 0 {
                // Flush trailing partial byte (zero-padded).
                out.push(0);
            }
            Ok(Value::Bytes(Arc::from(out.as_slice())))
        }),
    )
}

fn encode_resolved(
    f: &ResolvedBinField,
    map: &HashMap<String, Value>,
    out: &mut Vec<u8>,
    bit_off: usize,
) -> Result<usize, String> {
    use crate::ast::Endian::*;
    let val = match map.get(&f.name) {
        Some(v) => match v {
            Value::Evidence { inner, .. } => (**inner).clone(),
            other => other.clone(),
        },
        None => Value::Nil,
    };
    match &f.kind {
        ResolvedBinKind::Rest => {
            match val {
                Value::Bytes(b) => out.extend(b.iter()),
                Value::String(s) => out.extend(s.to_string().into_bytes()),
                _ => {}
            }
            Ok(0)
        }
        ResolvedBinKind::Bytes(n) => {
            let b = match val {
                Value::Bytes(b) => b.to_vec(),
                Value::String(s) => s.to_string().into_bytes(),
                _ => vec![0u8; *n],
            };
            let mut padded = b;
            if padded.len() < *n { padded.resize(*n, 0); }
            out.extend(padded.into_iter().take(*n));
            Ok(0)
        }
        ResolvedBinKind::Uint { bits, endian } => {
            let v = val.as_u64().unwrap_or(0);
            let n = (*bits as usize) / 8;
            match endian {
                Big => for i in (0..n).rev() { out.push(((v >> (8 * i)) & 0xFF) as u8); },
                Little => for i in 0..n { out.push(((v >> (8 * i)) & 0xFF) as u8); },
            }
            Ok(0)
        }
        ResolvedBinKind::Int { bits, endian } => {
            let v = val.as_i64().unwrap_or(0) as u64;
            let n = (*bits as usize) / 8;
            match endian {
                Big => for i in (0..n).rev() { out.push(((v >> (8 * i)) & 0xFF) as u8); },
                Little => for i in 0..n { out.push(((v >> (8 * i)) & 0xFF) as u8); },
            }
            Ok(0)
        }
        ResolvedBinKind::Bits { bits } => {
            let width = *bits as usize;
            let v = val.as_u64().unwrap_or(0);
            // Pack `width` bits LSB-first into the current partial byte.
            let mut i = 0usize;
            while i < width {
                let bit = ((v >> i) & 1) as u8;
                let pos = bit_off + i;
                let byte_idx = pos / 8;
                let bit_idx = pos % 8;
                while out.len() <= byte_idx {
                    out.push(0);
                }
                if bit == 1 {
                    out[byte_idx] |= 1 << bit_idx;
                }
                i += 1;
            }
            Ok(bit_off + width)
        }
        ResolvedBinKind::Ref(inner) => {
            let inner_map = match val {
                Value::Struct { fields, .. } => (*fields).clone(),
                Value::Map(m) => (*m).clone(),
                other => return Err(format!("encode: nested field '{}' expects struct, got {}", f.name, other.type_name())),
            };
            let mut bit = bit_off;
            for nf in inner {
                bit = encode_resolved(nf, &inner_map, out, bit)?;
            }
            Ok(bit)
        }
    }
}
