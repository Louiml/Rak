use crate::ast::*;
use crate::bytecode::{Chunk, Op};
use crate::value::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Compiler {
    chunk: Chunk,
    locals: Vec<(String, usize)>,
    scope_depth: usize,
    func_names: std::collections::HashSet<String>,
    func_closures: HashMap<String, Value>,
    /// `macro name(params) { body }` definitions, for compile-time expansion.
    macros: HashMap<String, (Vec<Param>, Vec<Stmt>)>,
    /// `binstruct Name { ... }` definitions, keyed by name. Registered in the
    /// compile pre-pass so `Name.decode(...)` / `Name.encode(...)` resolve at
    /// compile time to native-fn globals.
    binstructs: HashMap<String, Vec<BinField>>,
    /// Names exported by the module currently being compiled (`pub`/`export`).
    current_exports: Vec<String>,
    /// Import-once cache: canonical module path → its exported global names.
    module_cache: HashMap<std::path::PathBuf, Vec<String>>,
    /// Modules currently being inlined (circular-import detection).
    compiling: std::collections::HashSet<std::path::PathBuf>,
    /// Base directory for resolving name-based imports of the current module.
    base_dir: String,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
            func_names: std::collections::HashSet::new(),
            func_closures: HashMap::new(),
            macros: HashMap::new(),
            binstructs: HashMap::new(),
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
        for stmt in &module.items {
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
        for stmt in &module.items {
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
        0
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
        slot
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        self.locals.iter().rposition(|(n, _)| n == name).map(|i| i as u8)
    }

    fn const_str(&mut self, s: &str) -> u16 {
        self.emit_const(Value::String(Arc::from(s)))
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
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
            Stmt::While { cond, body } => {
                let loop_start = self.chunk.code.len();
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
            }
            Stmt::Loop(body) => {
                let loop_start = self.chunk.code.len();
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
                self.emit_jump_back(loop_start);
            }
            Stmt::For { name, iterable, body } => self.compile_for(name, iterable, body)?,
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

    fn compile_for(&mut self, name: &str, iterable: &Expr, body: &[Stmt]) -> Result<(), String> {
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
                }
                Ok(())
            }
            _ => {
                self.compile_expr(iterable)?;
                let arr_slot = self.add_local("__for_arr".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(arr_slot);
                self.load_const(Value::I64(0));
                let idx_slot = self.add_local("__for_idx".to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(idx_slot);
                let loop_start = self.chunk.code.len();
                self.emit_op(Op::LoadLocal);
                self.emit_byte(arr_slot);
                self.emit_op(Op::LoadLocal);
                self.emit_byte(idx_slot);
                self.emit_op(Op::IndexGet);
                let jexit = self.emit_jump(Op::JumpIfFalse);
                // item is truthy and still on the stack; store it into the loop var.
                let item_slot = self.add_local(name.to_string());
                self.emit_op(Op::StoreLocal);
                self.emit_byte(item_slot);
                self.begin_scope();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
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
                Ok(())
            }
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
            Expr::Array(items) => {
                for e in items {
                    self.compile_expr(e)?;
                }
                let ci = self.emit_const(Value::I64(items.len() as i64));
                self.emit_op(Op::LoadConst);
                self.emit_u16(ci);
                self.emit_op(Op::NewArray);
            }
            Expr::Call { callee, args } => {
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
            let bind = self.compile_pattern(pattern)?;
            let jnext = self.emit_jump(Op::JumpIfFalse);
            self.emit_op(Op::Pop);
            if let Some(name) = bind {
                self.begin_scope();
                let slot = self.add_local(name);
                self.emit_op(Op::LoadLocal);
                self.emit_byte(v_slot);
                self.emit_op(Op::StoreLocal);
                self.emit_byte(slot);
            } else {
                self.begin_scope();
            }
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
                let _ = jg;
            } else {
                self.compile_block_value(body)?;
                self.end_scope_for_match();
                let jend = self.emit_jump(Op::Jump);
                end_jumps.push(jend);
                self.patch_jump(jnext);
            }
        }
        self.emit_op(Op::Nil);
        for j in end_jumps {
            self.patch_jump(j);
        }
        Ok(())
    }

    fn end_scope_for_match(&mut self) {
        while let Some((_, d)) = self.locals.last() {
            if *d > self.scope_depth {
                self.locals.pop();
            } else {
                break;
            }
        }
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
            let mut map: HashMap<String, Value> = HashMap::new();
            for f in &fields {
                let (v, no) = decode_resolved(f, &bytes, off)?;
                map.insert(f.name.clone(), v);
                off = no;
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

/// Decode a single resolved field. Returns `(value, new_offset)`.
fn decode_resolved(
    f: &ResolvedBinField,
    bytes: &[u8],
    off: usize,
) -> Result<(Value, usize), String> {
    use crate::ast::Endian::*;
    match &f.kind {
        ResolvedBinKind::Rest => {
            let v = if off >= bytes.len() { Vec::new() } else { bytes[off..].to_vec() };
            Ok((Value::Bytes(Arc::from(v.as_slice())), bytes.len()))
        }
        ResolvedBinKind::Bytes(n) => {
            if off + n > bytes.len() {
                return Err(format!("binstruct: field '{}' outruns buffer", f.name));
            }
            Ok((Value::Bytes(Arc::from(&bytes[off..off + n])), off + n))
        }
        ResolvedBinKind::Uint { bits, endian } => {
            let n = (*bits as usize) / 8;
            if off + n > bytes.len() {
                return Err(format!("binstruct: field '{}' outruns buffer", f.name));
            }
            let mut acc: u64 = 0;
            match endian {
                Big => for i in 0..n { acc = (acc << 8) | bytes[off + i] as u64; },
                Little => for i in 0..n { acc |= (bytes[off + i] as u64) << (8 * i); },
            }
            Ok((Value::Hex(acc, *bits as usize), off + n))
        }
        ResolvedBinKind::Int { bits, endian } => {
            let n = (*bits as usize) / 8;
            if off + n > bytes.len() {
                return Err(format!("binstruct: field '{}' outruns buffer", f.name));
            }
            let mut acc: u64 = 0;
            match endian {
                Big => for i in 0..n { acc = (acc << 8) | bytes[off + i] as u64; },
                Little => for i in 0..n { acc |= (bytes[off + i] as u64) << (8 * i); },
            }
            let v = match *bits {
                8 => bytes[off] as i8 as i64,
                16 => acc as u16 as i16 as i64,
                32 => acc as u32 as i32 as i64,
                64 => acc as i64,
                _ => acc as i64,
            };
            Ok((Value::I64(v), off + n))
        }
        ResolvedBinKind::Ref(inner) => {
            let mut map: HashMap<String, Value> = HashMap::new();
            let mut io = off;
            for nf in inner {
                let (v, no) = decode_resolved(nf, bytes, io)?;
                map.insert(nf.name.clone(), v);
                io = no;
            }
            Ok((
                Value::Struct {
                    name: Arc::from("nested"),
                    fields: Arc::from(map),
                },
                io,
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
            for f in &fields {
                encode_resolved(f, &map, &mut out)?;
            }
            Ok(Value::Bytes(Arc::from(out.as_slice())))
        }),
    )
}

fn encode_resolved(
    f: &ResolvedBinField,
    map: &HashMap<String, Value>,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    use crate::ast::Endian::*;
    let val = match map.get(&f.name) {
        Some(v) => match v {
            Value::Evidence { inner, .. } => (**inner).clone(),
            other => other.clone(),
        },
        None => Value::Nil,
    };
    match &f.kind {
        ResolvedBinKind::Rest => match val {
            Value::Bytes(b) => out.extend(b.iter()),
            Value::String(s) => out.extend(s.to_string().into_bytes()),
            _ => {}
        },
        ResolvedBinKind::Bytes(n) => {
            let b = match val {
                Value::Bytes(b) => b.to_vec(),
                Value::String(s) => s.to_string().into_bytes(),
                _ => vec![0u8; *n],
            };
            let mut padded = b;
            if padded.len() < *n { padded.resize(*n, 0); }
            out.extend(padded.into_iter().take(*n));
        }
        ResolvedBinKind::Uint { bits, endian } => {
            let v = val.as_u64().unwrap_or(0);
            let n = (*bits as usize) / 8;
            match endian {
                Big => for i in (0..n).rev() { out.push(((v >> (8 * i)) & 0xFF) as u8); },
                Little => for i in 0..n { out.push(((v >> (8 * i)) & 0xFF) as u8); },
            }
        }
        ResolvedBinKind::Int { bits, endian } => {
            let v = val.as_i64().unwrap_or(0) as u64;
            let n = (*bits as usize) / 8;
            match endian {
                Big => for i in (0..n).rev() { out.push(((v >> (8 * i)) & 0xFF) as u8); },
                Little => for i in 0..n { out.push(((v >> (8 * i)) & 0xFF) as u8); },
            }
        }
        ResolvedBinKind::Ref(inner) => {
            let inner_map = match val {
                Value::Struct { fields, .. } => (*fields).clone(),
                Value::Map(m) => (*m).clone(),
                other => return Err(format!("encode: nested field '{}' expects struct, got {}", f.name, other.type_name())),
            };
            for nf in inner {
                encode_resolved(nf, &inner_map, out)?;
            }
        }
    }
    Ok(())
}
