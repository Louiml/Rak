#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Op {
    Nop = 0,
    LoadConst,
    LoadLocal,
    StoreLocal,
    LoadGlobal,
    StoreGlobal,
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
}

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

    pub fn add_const(&mut self, value: crate::value::Value) -> u16 {
        for (i, c) in self.constants.iter().enumerate() {
            if c == &value {
                return i as u16;
            }
        }
        let idx = self.constants.len() as u16;
        self.constants.push(value);
        idx
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        ((self.code[offset] as u16) << 8) | (self.code[offset + 1] as u16)
    }
}
