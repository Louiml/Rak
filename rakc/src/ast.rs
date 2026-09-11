#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Hex(u64),
    Int(i64),
    Float(f64),
    Float32(f32),
    TypedInt(i64, IntKind),
    String(String),
    Interp {
        template: String,
        parts: Vec<Expr>,
    },
    Bytes(Vec<u8>),
    Regex(String, String),
    Ident(String),
    Bool(bool),
    Nil,
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(String, Box<Expr>),
    CompoundAssign(CompoundOp, String, Box<Expr>),
    IndexAssign {
        obj: Box<Expr>,
        idx: Box<Expr>,
        value: Box<Expr>,
    },
    FieldAssign {
        obj: Box<Expr>,
        field: String,
        value: Box<Expr>,
    },
    FieldAccess(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>),
    Function {
        params: Vec<Param>,
        return_type: Option<Type>,
        body: Vec<Stmt>,
        captures: Vec<String>,
        is_async: bool,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        /// `f(a, x: 10)` — `name: value` pairs.
        named: Vec<(String, Expr)>,
    },
    If {
        cond: Box<Expr>,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<(Pattern, Option<Expr>, Vec<Stmt>)>,
    },
    Block(Vec<Stmt>),
    Lambda {
        params: Vec<Param>,
        body: Box<Expr>,
        captures: Vec<String>,
    },
    TryExpr(Box<Expr>),
    Await(Box<Expr>),
    Spawn(Box<Expr>),
    Raise(Box<Expr>),
    /// `cond ? then : else` — ternary expression.
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// `a ?? b` — nil-coalescing: `b` if `a` is nil/None, else `a`.
    NilCoalesce(Box<Expr>, Box<Expr>),
    /// `obj?.field` — optional field access (nil if obj is nil/None).
    OptField(Box<Expr>, String),
    /// `obj?[idx]` — optional index.
    OptIndex(Box<Expr>, Box<Expr>),
    /// `target1, target2 = value1, value2` — multiple / swap assignment.
    MultiAssign {
        targets: Vec<Expr>,
        values: Vec<Expr>,
    },
    /// `[x for x in iter if cond]` / `{k: v for k2 in iter if cond}` — comprehension.
    Comprehension {
        is_map: bool,
        var: Pattern,
        iterable: Box<Expr>,
        cond: Option<Box<Expr>>,
        /// array: the element expression; map: the key expression.
        elem: Box<Expr>,
        /// map only: the value expression.
        value: Option<Box<Expr>>,
    },
    Path(Vec<String>),
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    As(Box<Expr>, Type),
    /// A macro placeholder `$name` inside a macro body (replaced on expansion).
    MacroVar(String),
    /// `name!(args)` — a macro invocation, expanded before evaluation.
    MacroInvoke {
        name: String,
        args: Vec<Expr>,
    },
    /// `evidence<T> from expr` — wrap a value in an evidence (provenance) tag.
    EvidenceFrom {
        value: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompoundOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

/// A `break`/`continue` target: a named loop label or a numeric depth.
#[derive(Debug, Clone, PartialEq)]
pub enum BreakTarget {
    Label(String),
    Depth(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        pattern: Option<Pattern>,
        mutable: bool,
        value: Box<Expr>,
        type_hint: Option<Type>,
    },
    Expr(Box<Expr>),
    Return(Option<Box<Expr>>),
    If {
        cond: Box<Expr>,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    /// `do { … } while cond;`
    DoWhile {
        cond: Box<Expr>,
        body: Vec<Stmt>,
    },
    /// `if let P = e { … } else { … }`
    IfLet {
        pattern: Pattern,
        value: Box<Expr>,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    /// `while let P = e { … }`
    WhileLet {
        pattern: Pattern,
        value: Box<Expr>,
        body: Vec<Stmt>,
    },
    Loop {
        label: Option<String>,
        body: Vec<Stmt>,
    },
    While {
        label: Option<String>,
        cond: Box<Expr>,
        body: Vec<Stmt>,
    },
    For {
        label: Option<String>,
        pattern: Pattern,
        iterable: Box<Expr>,
        body: Vec<Stmt>,
    },
    Scan {
        target: Box<Expr>,
        options: Vec<(String, Expr)>,
        body: Option<Vec<Stmt>>,
    },
    Fetch {
        target: Box<Expr>,
        options: Vec<(String, Expr)>,
        body: Option<Vec<Stmt>>,
    },
    Dump {
        value: Box<Expr>,
        target: Option<Box<Expr>>,
    },
    Trace {
        value: Box<Expr>,
    },
    Break(Option<BreakTarget>),
    Continue(Option<BreakTarget>),
    Mod {
        name: String,
        items: Vec<Stmt>,
    },
    Struct {
        name: String,
        type_params: Vec<String>,
        fields: Vec<Param>,
    },
    Enum {
        name: String,
        type_params: Vec<String>,
        variants: Vec<EnumVariant>,
    },
    Impl {
        target: String,
        trait_name: Option<String>,
        methods: Vec<Stmt>,
    },
    Trait {
        name: String,
        methods: Vec<TraitMethod>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<(Pattern, Option<Expr>, Vec<Stmt>)>,
    },
    Try {
        body: Vec<Stmt>,
        catch_name: Option<String>,
        catch_body: Vec<Stmt>,
    },
    Raise(Box<Expr>),
    TypeAlias {
        name: String,
        alias: Type,
    },
    Use {
        path: Vec<String>,
        is_file: bool,
        alias: Option<String>,
    },
    Async(Vec<Stmt>),
    Export(Box<Stmt>),
    /// `extern "C" { fn name(params) -> ret, ... }` — declarative FFI bindings.
    Extern {
        abi: String,
        lib: Option<String>,
        decls: Vec<ForeignFn>,
    },
    /// `macro name($params) { body }` — an AST-expanding macro.
    MacroDef {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
    },
    /// `const NAME = expr` — a compile-time constant (eagerly evaluated).
    Const {
        name: String,
        value: Box<Expr>,
    },
    /// `binstruct Name { field: type, ... }` — a declarative wire-format
    /// layout that compiles to both a decoder and an encoder.
    BinStructDef {
        name: String,
        fields: Vec<BinField>,
    },
}

/// A single field inside a `binstruct` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct BinField {
    pub name: String,
    pub kind: BinKind,
    /// When set, the field is an array of `kind` with the given length
    /// (evaluated at decode/encode time). `None` means a scalar field.
    pub repeat: Option<Box<Expr>>,
}

/// The wire-kind of a `binstruct` field.
#[derive(Debug, Clone, PartialEq)]
pub enum BinKind {
    /// An unsigned integer with the given bit width (multiple of 8).
    Uint { bits: u8, endian: Endian },
    /// A signed integer with the given bit width.
    Int { bits: u8, endian: Endian },
    /// A fixed-length run of raw bytes.
    Bytes(usize),
    /// Everything remaining in the buffer.
    Rest,
    /// A nested `binstruct` referred to by name.
    Ref(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Endian {
    Big,
    Little,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wild,
    Ident(String),
    Hex(u64),
    Int(i64),
    String(String),
    Bool(bool),
    Nil,
    Tuple(Vec<Pattern>),
    Array(Vec<Pattern>),
    Byte(u8),
    Bytes(Vec<BytesPat>),
    Struct(String, Vec<(String, Pattern)>),
    Range(Box<Pattern>, Box<Pattern>),
    Or(Vec<Pattern>),
    Some(Box<Pattern>),
    None,
    Ok(Box<Pattern>),
    Err(Box<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BytesPat {
    Byte(u8),
    Rest,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Hex(usize),
    Int,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    String,
    Bytes,
    Bool,
    Nil,
    Array(Box<Type>),
    Tuple(Vec<Type>),
    Map(Box<Type>, Box<Type>),
    Function(Vec<Type>, Box<Type>),
    Custom(String),
    Generic(String),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    /// Raw pointer to the inner type (`*u8`, `*i8`, `*void`). Used by FFI.
    Ptr(Box<Type>),
    /// C `void`, used as an FFI return type for functions returning nothing.
    Void,
    /// A provenance-tagged value (`evidence<T>`).
    Evidence(Box<Type>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_hint: Option<Type>,
    /// Default value: `fn f(x: int = 0)`. Evaluated in the closure env on call.
    pub default: Option<Box<Expr>>,
    /// `fn f(...xs: array)` — collects leftover positionals into an array.
    pub rest: bool,
    /// `fn f(x?: int)` — optional: missing ⇒ nil (sugar for default = nil).
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnOp {
    Minus,
    Not,
    BitNot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntKind {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub imports: Vec<Import>,
    pub items: Vec<Stmt>,
}

/// The two import shapes: whole-module (`import m`) and from-import
/// (`from m import x`).
#[derive(Debug, Clone, PartialEq)]
pub enum ImportKind {
    /// `import m` / `use m` / `import "./f.rak"` — bind the whole module.
    Whole,
    /// `from m import x, y as z` / `from m import *` — bind selected names.
    From,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// Module target: a file path (single string element when `is_file`) or a
    /// dotted name (`["pkg", "sub"]`).
    pub path: Vec<String>,
    pub is_file: bool,
    pub alias: Option<String>,
    pub kind: ImportKind,
    /// `from m import x, y as z` — `(name, alias)` pairs.
    pub from_names: Vec<(String, Option<String>)>,
    /// `from m import *`.
    pub star: bool,
    /// `pub use ...` / `export use ...` — re-export the names from this module
    /// instead of binding them in the importer's scope.
    pub reexport: bool,
}

/// A single foreign function declaration inside an `extern "C" { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignFn {
    pub name: String,
    pub params: Vec<Param>,
    /// Set when the declaration ends with `...` (C varargs).
    pub varargs: bool,
    pub return_type: Option<Type>,
}
