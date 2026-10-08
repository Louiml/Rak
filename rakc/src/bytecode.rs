#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Op {
    Nop = 0,
    LoadConst,
    LoadLocal,
    StoreLocal,
    LoadGlobal,
    StoreGlobal,
    /// Check the value on top of the stack against a `Type`, leaving it in place.
    ///
    /// Peeks rather than pops, so the `StoreGlobal`/`StoreLocal` that follows still sees
    /// the value. The type index is a u16 into [`Chunk::types`].
    ///
    /// The interpreter enforces a `let` annotation at runtime through
    /// `check_value_type`; this is the same check, so an annotated `let` means the same
    /// thing on either backend. Without it the VM ignored every annotation, so a program
    /// could fail under `rakc run` and succeed under `rakc vm` purely by backend.
    CheckType,
    Pop,
    Dup,

    AddI,
    SubI,
    MulI,
    DivI,
    RemI,
    NegI,
    AddF,
    SubF,
    MulF,
    DivF,
    NegF,

    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    Shl,
    Shr,

    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Not,

    True,
    False,
    Nil,

    Jump,
    JumpIfFalse,
    JumpIfTrue,

    Call,
    Return,
    Print,
    /// `dump x, "path.txt"`: write the value to a file. Same capability gate as
    /// any other filesystem write.
    DumpToFile,
    Trace,

    NewArray,
    NewTuple,
    NewMap,

    IndexGet,
    FieldGet,

    Closure,
    GetUpvalue,
    SetUpvalue,

    LoopBegin,
    LoopEnd,

    /// Pop the args array value, pop the symbol string, pop the `ForeignLib`
    /// handle, unpack the array into C arguments, and call the symbol via the
    /// dynamic loader. Push the `i64` result. No operand.
    FFICall,
    /// Pop a `ForeignLib` handle (best-effort early release). Push nil.
    FFIClose,
    /// Pop a value; if it's a `Future`, resolve it (block on the async
    /// runtime). Push the resolved value (or the value itself if not a future).
    Await,
    /// Build a `Value::Module` from `n` (name, value) pairs. Operand: `n` (1 byte).
    /// Pops `2*n` values (alternating name string, value), pushes the Module.
    BuildModule,
    /// Push an empty module namespace. Operand: u16 const index of the name of
    /// the global that will hold the handle.
    ///
    /// The namespace starts with no exported names. `ModulePublish` adds them
    /// once the compiler knows the module's full export list, which it cannot
    /// know at this point because a `pub use` re-export inside the module has not
    /// been compiled yet. `Op::StoreGlobal` republishes values into the
    /// namespace in the meantime, so the values are correct before they become
    /// readable.
    MakeModule,
    /// Mark one name of a module namespace as exported. Operands: u16 const index
    /// of the namespace's global name, u16 const index of the exported name, and
    /// a 1-byte mutability flag (1 for `pub let mut`, 0 for `pub let` / `pub const`).
    ///
    /// The flag is what stops an importer assigning a name the module declared
    /// `let`. Without it the VM knew which names existed but not which were fixed,
    /// and `m.X = v` was refused for a `pub let mut` while the interpreter allowed
    /// it.
    ModulePublish,
    /// Pop a module namespace and bind it to the named global. Operand: u16 const
    /// index of the global name.
    ///
    /// This exists instead of `Op::StoreGlobal` because binding a module is not a
    /// write to a variable. `Op::StoreGlobal` republishes whatever it stores into
    /// every namespace that exports a global of the same name, so storing a handle
    /// through it replaced that name inside *other* modules: with
    /// `alpha.rak` exporting `pub fn beta()`, a later `import beta` stored the
    /// `beta` handle into the global `beta`, republished it into alpha's namespace
    /// under the export name `beta`, and `alpha.beta()` stopped resolving.
    ///
    /// A handle is a binding, so it is stored here and never republished.
    BindModule,
    /// Insert one entry into an existing module: pops (value, name, module) and
    /// pushes the module with `name` set to `value`.
    ///
    /// Separate from `BuildModule` because that always starts from an empty map,
    /// which loses the existing entries. Needed by `import pkg.sub` when `pkg` is
    /// also imported on its own — the package's own exports have to survive.
    MergeModule,
    /// Load the named global as a module, or push an empty map if it is not
    /// bound or is bound to something that is not a map. Operand: const index
    /// of the name (u16).
    ///
    /// A plain `LoadGlobal` would fail with `Undefined` for a file that does
    /// `import pkg.sub` without ever importing `pkg`, and the interpreter
    /// accepts that — it creates the package module on demand.
    LoadGlobalOrMap,
    /// Register a deferred call. The callee and its args (already evaluated)
    /// are pushed onto a per-frame stack and run in LIFO order when the frame
    /// returns. Operand: argument count `n` (1 byte). Pops `n` args then the
    /// callee.
    DeferCall,
    /// Begin a `try` region. Operand: u16 handler offset (patched by the
    /// compiler). Pushes a catch frame onto the VM's handler stack for this
    /// frame; on error the VM unwinds to the handler with the error value.
    Try,
    /// Pop the catch frame for the current `try` (the body completed without
    /// raising).
    CatchEnd,
    /// Raise: pop a value; if it's already a `Value::Error` bind it as-is,
    /// otherwise wrap its string form as a `User` error. Unwinds to the
    /// nearest enclosing `try` handler (or fails the run).
    Throw,
    /// `expr?` — pop a `Result`/`Option`; `Ok(v)`/`Some(v)` push `v`,
    /// `Err(e)`/`None` raise (bindable by `catch`).
    TryUnwrap,
    /// Method call `obj.method(args...)`. Operands: u16 method-name const
    /// index, u8 argument count. Pops `argc` args then the receiver; dispatch
    /// order: native regex methods, `__method_<type>_<name>` globals (baked
    /// from `impl` blocks), then a field holding a callable.
    CallMethod,
    /// Build a `Value::Struct`. Operands: u16 name-const index, u8 field
    /// count `n`. Pops `2*n` values (alternating field-name const, value).
    StructNew,
    /// Structural pattern match. Operands: u16 descriptor-const index, u16
    /// bind-names-const index. Pops the grow-the-bindings-array indicator
    /// then the descriptor then the scrutinee; pushes a truthy `Value::Array`
    /// of bound values (ordered as the descriptor's bind names, `Nil` where a
    /// bind was absent) on a match, or `Value::Nil` on no match.
    MatchPat,
    /// `obj[idx] = value`. Pops value, then index, then object; mutates the
    /// container in place (via copy-on-write) and pushes the container back so
    /// the caller can store it back to its binding.
    IndexSet,
    /// `obj.field = value`. Operand: u16 field-name const index. Pops value,
    /// then object; mutates and pushes the container back (copy-on-write).
    FieldSet,
    /// Jump if the top-of-stack is `nil` or `None` (does not pop). Used by
    /// `??`, `?.`, `?[`. Operand: u16 target offset.
    JumpIfNil,
    /// Materialize a container into its iteration items (an array): arrays and
    /// tuples pass through, maps become `(key, value)` tuples, strings become
    /// chars. Operand: u8 flag — 1 yields `(index, item)` pairs for
    /// arrays/strings (matching the interpreter's 2-tuple "indexed for").
    IterItems,
    /// Replace the top of stack with its length, as an `I64`. No operands.
    ///
    /// This exists because `for` used to decide whether to keep going by testing
    /// the *element's* truthiness, which ended the loop on the first `0`, `""` or
    /// `false`. A `0x00` byte is the single most common value in a binary file,
    /// so `for b in buffer` walked a whole buffer and then stopped dead at the
    /// first NUL. A loop bound has to be a comparison against a length, not a
    /// test of the value being carried.
    Len,
}

impl Op {
    pub fn from_u8(b: u8) -> Option<Op> {
        if (b as usize) <= Op::Len as usize {
            Some(unsafe { std::mem::transmute(b) })
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<crate::value::Value>,
    pub lines: Vec<u32>,
    /// The module namespaces this chunk's globals are published into.
    ///
    /// The VM inlines an imported module's body into the *same* chunk, so a
    /// module's top-level bindings are ordinary chunk globals. `import m`
    /// hands the script a namespace rather than a copy of those globals, and
    /// this table is what keeps the two in step: every store to a global listed
    /// here is republished into that module's namespace under its exported name,
    /// so `m.X` reflects whatever the module's own code last did to `X`.
    ///
    /// Filled at compile time and read by the VM when it builds its publish
    /// index; it carries no runtime state of its own.
    pub module_cells: Vec<ModuleCell>,
    /// Declared types for `let` annotations, addressed by `Op::CheckType`.
    ///
    /// A `Type` is not a `Value`, so it cannot live in `constants`. Filled at compile
    /// time and read-only at runtime.
    pub types: Vec<crate::ast::Type>,
    /// `type X = T` declarations, name → aliased type.
    ///
    /// Collected at compile time so `Op::CheckType` can expand an annotation that names an
    /// alias. Collected up front rather than as each declaration is reached, so an
    /// annotation may name an alias declared later in the file.
    pub aliases: Vec<(String, crate::ast::Type)>,
}

/// The largest a constant pool or code stream can be.
///
/// Both are addressed by 16-bit operands -- a constant index, a jump target, a
/// `try` handler offset -- so 65535 is a hard ceiling, not an estimate. Exceeding
/// it used to wrap silently and produce a chunk that ran and gave wrong answers;
/// `add_const` and the compiler's offset helpers now stop with a message instead.
pub const MAX_CHUNK_LEN: usize = u16::MAX as usize;

/// The largest number of locals a chunk can declare, addressed by an 8-bit slot.
pub const MAX_LOCALS: usize = u8::MAX as usize;

/// One imported module's namespace: the global that will hold its handle, and
/// which chunk globals back each of its exported names.
#[derive(Clone, Debug, Default)]
pub struct ModuleCell {
    /// The chunk global the namespace handle is stored into by `MakeModule` +
    /// `BindModule`. Private by convention (`__rak_modcell_`).
    pub global: String,
    /// `(exported name, the chunk global that backs it)`.
    pub exports: Vec<(String, String)>,
    /// Exports the module declared `pub let mut`, and which may therefore be
    /// assigned through the module handle. A name absent from this is fixed, so
    /// `m.X = v` is refused even though `m.X` reads fine.
    pub mutable: Vec<String>,
}

impl ModuleCell {
    /// Flatten to `(global, cell_global, export)` triples, which is the shape the
    /// VM's publish index wants.
    pub fn triples(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.exports
            .iter()
            .map(move |(export, global)| (global.as_str(), self.global.as_str(), export.as_str()))
    }
}

impl Chunk {
    pub fn new() -> Self {
        Chunk::default()
    }

    pub fn write_op(&mut self, op: Op, line: u32) {
        self.code.push(op as u8);
        self.lines.push(line);
    }

    pub fn write_byte(&mut self, b: u8, line: u32) {
        self.code.push(b);
        self.lines.push(line);
    }

    pub fn write_u16(&mut self, v: u16, line: u32) {
        self.write_byte((v >> 8) as u8, line);
        self.write_byte((v & 0xFF) as u8, line);
    }

    /// Expand a `type` alias to what it names, transitively.
    ///
    /// Bounded so a self-referential alias terminates; such an alias is left unexpanded and
    /// compares by name, which is how it behaved before aliases were recorded.
    pub fn expand_alias(&self, ty: &crate::ast::Type) -> crate::ast::Type {
        let mut cur = ty.clone();
        for _ in 0..16 {
            let crate::ast::Type::Custom(name) = &cur else {
                break;
            };
            match self.aliases.iter().find(|(n, _)| n == name) {
                Some((_, next)) => cur = next.clone(),
                None => break,
            }
        }
        cur
    }

    /// Intern a declared type, returning the index `Op::CheckType` reads.
    pub fn add_type(&mut self, ty: crate::ast::Type) -> u16 {
        if let Some(i) = self.types.iter().position(|t| *t == ty) {
            return i as u16;
        }
        assert!(
            self.types.len() < MAX_CHUNK_LEN,
            "declared-type table overflow: a chunk may hold at most {} types. \
             Split the program into modules with `import`.",
            MAX_CHUNK_LEN
        );
        self.types.push(ty);
        (self.types.len() - 1) as u16
    }

    pub fn add_const(&mut self, value: crate::value::Value) -> u16 {
        for (i, c) in self.constants.iter().enumerate() {
            // `same_const_repr`, not `==`. `PartialEq` is numeric-aware -- `I64(2)` and
            // `F64(2.0)` are equal -- so deduplicating with it let `2.0` reuse the slot
            // holding `2`, and `LoadConst` then pushed an integer where a float was
            // written. That is not just a display difference: `-2.0` is a unary negation,
            // so it became `I64(-2)`, and `1.0 / 3.0` beside a `1` anywhere in the program
            // became integer division. Anything comparing the two values then answered a
            // different question from the one written.
            if c.same_const_repr(&value) {
                return i as u16;
            }
        }
        // The pool is addressed by a 16-bit index. Wrapping would make later
        // loads read the wrong constant, silently, so stop here instead.
        assert!(
            self.constants.len() < MAX_CHUNK_LEN,
            "constant pool overflow: a chunk may hold at most {} constants. \
             Split the program into modules with `import`.",
            MAX_CHUNK_LEN
        );
        let idx = self.constants.len() as u16;
        self.constants.push(value);
        idx
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        ((self.code[offset] as u16) << 8) | (self.code[offset + 1] as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant pool is addressed by a 16-bit index. Filling it to the
    /// ceiling must still work, and one past it must fail loudly -- wrapping
    /// here means every later `LoadConst` reads the wrong entry, with no
    /// diagnostic.
    #[test]
    fn constant_pool_stops_at_the_16_bit_ceiling() {
        let mut chunk = Chunk::default();
        for i in 0..MAX_CHUNK_LEN {
            // Distinct values, so nothing dedups and the pool really grows.
            let v = crate::value::Value::I64(i as i64);
            let idx = chunk.add_const(v);
            assert_eq!(idx as usize, i);
        }
        assert_eq!(chunk.constants.len(), MAX_CHUNK_LEN);

        let before = chunk.constants.len();
        let overflow = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            chunk.add_const(crate::value::Value::I64(-1));
        }));
        assert!(
            overflow.is_err(),
            "the pool should refuse to grow past the limit"
        );
        assert_eq!(
            chunk.constants.len(),
            before,
            "a refused add must not have pushed anything"
        );
    }

    /// Dedup must still work at scale, or the ceiling would be reached by a
    /// program that reuses one constant a few thousand times.
    #[test]
    fn constant_pool_still_dedups() {
        let mut chunk = Chunk::default();
        let first = chunk.add_const(crate::value::Value::I64(7));
        for _ in 0..1000 {
            assert_eq!(chunk.add_const(crate::value::Value::I64(7)), first);
        }
        assert_eq!(chunk.constants.len(), 1);
    }
}
