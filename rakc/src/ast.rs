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
    Path(Vec<String>),
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    As(Box<Expr>, Type),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompoundOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
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
    Loop(Vec<Stmt>),
    While {
        cond: Box<Expr>,
        body: Vec<Stmt>,
    },
    For {
        name: String,
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
    Break,
    Continue,
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
    Struct(String, Vec<(String, Pattern)>),
    Range(Box<Pattern>, Box<Pattern>),
    Or(Vec<Pattern>),
    Some(Box<Pattern>),
    None,
    Ok(Box<Pattern>),
    Err(Box<Pattern>),
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_hint: Option<Type>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub path: Vec<String>,
    pub is_file: bool,
    pub alias: Option<String>,
}
