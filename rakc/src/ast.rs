/// Abstract Syntax Tree for Rak.
/// Simple, expression-oriented, with OSINT-specific statement types.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 0x1A2B
    Hex(u64),
    /// 42
    Int(u64),
    /// "hello"
    String(String),
    /// b"\x00\xFF"
    Bytes(Vec<u8>),
    /// variable_name
    Ident(String),
    /// true, false
    Bool(bool),
    /// nil
    Nil,
    /// -expr, !expr, ~expr
    Unary(UnOp, Box<Expr>),
    /// a + b, a == b
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// a = b
    Assign(String, Box<Expr>),
    /// fn(params) -> Type { body }
    Function {
        params: Vec<Param>,
        return_type: Option<Type>,
        body: Vec<Stmt>,
    },
    /// call(args)
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// struct.field
    FieldAccess(Box<Expr>, String),
    /// arr[idx]
    Index(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// let x = 5
    Let {
        name: String,
        mutable: bool,
        value: Box<Expr>,
        type_hint: Option<Type>,
    },
    /// expr;
    Expr(Box<Expr>),
    /// return expr;
    Return(Option<Box<Expr>>),
    /// if cond { block } else { block }
    If {
        cond: Box<Expr>,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    /// loop { block }
    Loop(Vec<Stmt>),
    /// while cond { block }
    While {
        cond: Box<Expr>,
        body: Vec<Stmt>,
    },
    /// for item in iterable { block }
    For {
        name: String,
        iterable: Box<Expr>,
        body: Vec<Stmt>,
    },
    /// scan target { ... }
    /// OSINT-specific: port scan, web scan, etc.
    Scan {
        target: Box<Expr>,
        options: Vec<(String, Expr)>,
        body: Option<Vec<Stmt>>,
    },
    /// fetch url { ... }
    /// OSINT-specific: HTTP/TCP fetch with response handling
    Fetch {
        target: Box<Expr>,
        options: Vec<(String, Expr)>,
        body: Option<Vec<Stmt>>,
    },
    /// dump expr;
    /// OSINT-specific: output data to file/network/stdout
    Dump {
        value: Box<Expr>,
        target: Option<Box<Expr>>,
    },
    /// trace expr;
    /// OSINT-specific: trace packet/header/response
    Trace {
        value: Box<Expr>,
    },
    /// break;
    Break,
    /// continue;
    Continue,
    /// mod name { ... }
    Mod {
        name: String,
        items: Vec<Stmt>,
    },
    /// struct Name { fields }
    Struct {
        name: String,
        fields: Vec<Param>,
    },
    /// enum Name { variants }
    Enum {
        name: String,
        variants: Vec<String>,
    },
    /// impl Type { methods }
    Impl {
        target: String,
        methods: Vec<Stmt>,
    },
    /// match expr { arms }
    Match {
        value: Box<Expr>,
        arms: Vec<(Pattern, Vec<Stmt>)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wild,
    Ident(String),
    Hex(u64),
    Int(u64),
    String(String),
    Bool(bool),
    Nil,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// hex8, hex16, hex32, hex64
    Hex(usize),
    /// int
    Int,
    /// string
    String,
    /// bytes
    Bytes,
    /// bool
    Bool,
    /// nil / void
    Nil,
    /// [T]
    Array(Box<Type>),
    /// {K: V}
    Map(Box<Type>, Box<Type>),
    /// fn(A, B) -> C
    Function(Vec<Type>, Box<Type>),
    /// user-defined
    Custom(String),
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

/// A complete Rak source file.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub imports: Vec<Import>,
    pub items: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub path: Vec<String>,
    pub alias: Option<String>,
}