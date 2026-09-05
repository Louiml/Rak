use logos::Logos;

#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*\n?")]
pub enum Token {
    #[token("scan")]
    Scan,
    #[token("fetch")]
    Fetch,
    #[token("dump")]
    Dump,
    #[token("trace")]
    Trace,
    #[token("loop")]
    Loop,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("fn")]
    Fn,
    #[token("let")]
    Let,
    #[token("mut")]
    Mut,
    #[token("return")]
    Return,
    #[token("use")]
    Use,
    #[token("mod")]
    Mod,
    #[token("pub")]
    Pub,
    #[token("struct")]
    Struct,
    #[token("enum")]
    Enum,
    #[token("impl")]
    Impl,
    #[token("match")]
    Match,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("while")]
    While,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("nil")]
    Nil,

    #[token("try")]
    Try,
    #[token("catch")]
    Catch,
    #[token("raise")]
    #[token("throw")]
    Raise,
    #[token("trait")]
    Trait,
    #[token("async")]
    Async,
    #[token("await")]
    Await,
    #[token("spawn")]
    Spawn,
    #[token("as")]
    As,
    #[token("type")]
    Type,

    #[regex(r"0x[0-9A-Fa-f]+", |lex| hex_to_u64(lex.slice()))]
    Hex(u64),

    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?f32", |lex| lex.slice()[..lex.slice().len()-3].parse::<f32>().ok())]
    Float32(f32),

    #[regex(r"[0-9]+(i8|i16|i32|i64|u8|u16|u32|u64)", |lex| parse_typed_int(lex.slice()))]
    TypedInt(TypedIntData),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Int(i64),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| parse_string(lex.slice()))]
    String(String),

    #[regex(r#"f"([^"\\]|\\.)*""#, |lex| parse_interp(lex.slice()))]
    Interp(String),

    #[regex(r#"b"([^"\\]|\\.)*""#, |lex| parse_bytes(lex.slice()))]
    Bytes(Vec<u8>),

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("&")]
    Ampersand,
    #[token("|")]
    Pipe,
    #[token("^")]
    Caret,
    #[token("!")]
    Bang,
    #[token("~")]
    Tilde,
    #[token("<<")]
    Shl,
    #[token(">>")]
    Shr,

    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,

    #[token("=")]
    Eq,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("%=")]
    PercentEq,

    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,

    #[token("?")]
    Question,

    #[token(".")]
    Dot,
    #[token("..")]
    DotDot,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("::")]
    ColonColon,
    #[token(";")]
    Semi,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedIntData {
    pub value: i64,
    pub kind: IntKind,
}

#[derive(Clone, Debug, PartialEq)]
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

fn hex_to_u64(s: &str) -> Option<u64> {
    u64::from_str_radix(&s[2..], 16).ok()
}

fn parse_typed_int(s: &str) -> Option<TypedIntData> {
    let (num, suffix) = if let Some(p) = s.find(|c: char| c == 'i' || c == 'u') {
        (&s[..p], &s[p..])
    } else {
        return None;
    };
    let kind = match suffix {
        "i8" => IntKind::I8,
        "i16" => IntKind::I16,
        "i32" => IntKind::I32,
        "i64" => IntKind::I64,
        "u8" => IntKind::U8,
        "u16" => IntKind::U16,
        "u32" => IntKind::U32,
        "u64" => IntKind::U64,
        _ => return None,
    };
    num.parse::<i64>().ok().map(|v| TypedIntData { value: v, kind })
}

fn parse_string(s: &str) -> Option<String> {
    let inner = &s[1..s.len() - 1];
    unescape(inner)
}

pub fn unescape(inner: &str) -> Option<String> {
    let mut result = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('0') => result.push('\0'),
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(n) = u8::from_str_radix(&hex, 16) {
                        result.push(n as char);
                    }
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    Some(result)
}

fn parse_interp(s: &str) -> Option<String> {
    let inner = &s[2..s.len() - 1];
    unescape(inner)
}

fn parse_bytes(s: &str) -> Option<Vec<u8>> {
    let inner = &s[2..s.len() - 1];
    let mut result = Vec::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(n) = u8::from_str_radix(&hex, 16) {
                        result.push(n);
                    }
                }
                Some('n') => result.push(b'\n'),
                Some('t') => result.push(b'\t'),
                Some('r') => result.push(b'\r'),
                Some('\\') => result.push(b'\\'),
                Some('"') => result.push(b'"'),
                Some(other) => {
                    result.push(b'\\');
                    result.push(other as u8);
                }
                None => result.push(b'\\'),
            }
        } else {
            result.push(c as u8);
        }
    }
    Some(result)
}

pub fn tokenize(source: &str) -> crate::Result<Vec<Token>> {
    let mut lex = Token::lexer(source);
    let mut tokens = Vec::new();
    while let Some(token) = lex.next() {
        match token {
            Ok(tok) => tokens.push(tok),
            Err(_) => {
                return Err(crate::RakError::Lexer(format!(
                    "Unexpected character at offset {}",
                    lex.span().start
                )))
            }
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_token() {
        let toks = tokenize("0xDEAD 0xBEEF").unwrap();
        assert_eq!(toks, vec![Token::Hex(0xDEAD), Token::Hex(0xBEEF)]);
    }

    #[test]
    fn test_keywords() {
        let toks = tokenize("scan fetch dump trace if else fn").unwrap();
        assert_eq!(
            toks,
            vec![
                Token::Scan,
                Token::Fetch,
                Token::Dump,
                Token::Trace,
                Token::If,
                Token::Else,
                Token::Fn,
            ]
        );
    }

    #[test]
    fn test_float_and_typed_int() {
        let toks = tokenize("3.14 42i32 10u8 7").unwrap();
        assert_eq!(toks[0], Token::Float(3.14));
        assert_eq!(toks[1], Token::TypedInt(TypedIntData { value: 42, kind: IntKind::I32 }));
        assert_eq!(toks[2], Token::TypedInt(TypedIntData { value: 10, kind: IntKind::U8 }));
        assert_eq!(toks[3], Token::Int(7));
    }
}
