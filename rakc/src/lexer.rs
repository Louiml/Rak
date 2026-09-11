use logos::Logos;

#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*\n?")]
#[logos(skip r"/\*[^*]*\*+([^/*][^*]*\*+)*/")]
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
    #[token("do")]
    Do,
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
    #[token("extern")]
    Extern,
    #[token("macro")]
    Macro,
    #[token("const")]
    Const,
    #[token("import")]
    Import,
    #[token("from")]
    From,
    #[token("export")]
    Export,
    #[token("binstruct")]
    BinStruct,
    #[token("evidence")]
    Evidence,

    /// A macro placeholder `$name` inside a macro body.
    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    MacroVar(String),

    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?", |lex| lex.slice().replace('_', "").parse::<f64>().ok())]
    Float(f64),

    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?f32", |lex| lex.slice()[..lex.slice().len()-3].replace('_', "").parse::<f32>().ok())]
    Float32(f32),

    #[regex(r"[0-9][0-9_]*(i8|i16|i32|i64|u8|u16|u32|u64)", |lex| parse_typed_int(lex.slice()))]
    TypedInt(TypedIntData),

    #[regex(r"0x[0-9A-Fa-f][0-9A-Fa-f_]*", |lex| hex_to_u64(lex.slice()))]
    Hex(u64),

    #[regex(r"[0-9][0-9_]*", |lex| lex.slice().replace('_', "").parse::<i64>().ok())]
    Int(i64),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| parse_string(lex.slice()))]
    String(String),

    /// Triple-quoted multiline string: escapes processed, newlines allowed.
    /// (Embedded `"` are not supported inside; use `r"..."` or escape.)
    #[regex(r#""""[^"]*""""#, |lex| parse_triple_string(lex.slice()))]
    StringMulti(String),

    /// Raw string `r"..."`: no escape processing, no embedded quotes.
    #[regex(r#"r"([^"]*)""#, |lex| lex.slice()[2..lex.slice().len() - 1].to_string())]
    StringRaw(String),

    #[regex(r#"f"([^"\\]|\\.)*""#, |lex| parse_interp(lex.slice()))]
    Interp(String),

    #[regex(r#"b"([^"\\]|\\.)*""#, |lex| parse_bytes(lex.slice()))]
    Bytes(Vec<u8>),

    #[regex(r"'([^'\\]|\\x[0-9a-fA-F]{2}|\\.)'", |lex| parse_char(lex.slice()))]
    Char(char),

    /// A loop label `'name` (used in `'lbl: for ...`, `break 'lbl`).
    #[regex(r"'[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Label(String),

    /// A regular-expression literal `/pattern/flags`. Never produced
    /// directly by logos; synthesized in `tokenize()` after the fact.
    Regex((String, String)),

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
    #[token("|>")]
    PipeGt,
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
    #[token("&=")]
    AmpersandEq,
    #[token("|=")]
    PipeEq,
    #[token("^=")]
    CaretEq,
    #[token("<<=")]
    ShlEq,
    #[token(">>=")]
    ShrEq,

    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,

    #[token("?")]
    Question,
    #[token("??")]
    QuestionQuestion,
    #[token("?.")]
    QuestionDot,
    #[token("?[")]
    QuestionLBracket,

    #[token(".")]
    Dot,
    #[token("..")]
    DotDot,
    #[token("...")]
    Ellipsis,
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
    u64::from_str_radix(&s[2..].replace('_', ""), 16).ok()
}

fn parse_typed_int(s: &str) -> Option<TypedIntData> {
    let (num, suffix) = if let Some(p) = s.find(|c: char| c == 'i' || c == 'u') {
        (&s[..p], &s[p..])
    } else {
        return None;
    };
    let value = num.replace('_', "").parse::<i64>().ok()?;
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
    Some(TypedIntData { value, kind })
}

fn parse_triple_string(s: &str) -> Option<String> {
    // s is """..."""
    let inner = &s[3..s.len() - 3];
    unescape(inner)
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

fn parse_char(s: &str) -> Option<char> {
    let inner = &s[1..s.len() - 1];
    let mut chars = inner.chars();
    match chars.next() {
        Some('\\') => match chars.next() {
            Some('n') => Some('\n'),
            Some('t') => Some('\t'),
            Some('r') => Some('\r'),
            Some('0') => Some('\0'),
            Some('\\') => Some('\\'),
            Some('\'') => Some('\''),
            Some('"') => Some('"'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                u8::from_str_radix(&hex, 16).ok().map(|n| n as char)
            }
            _ => None,
        },
        Some(c) => Some(c),
        None => None,
    }
}

pub fn tokenize(source: &str) -> crate::Result<Vec<(Token, usize)>> {
    let mut lex = Token::lexer(source);
    // Collect logos output as `Option<Token>`: `None` marks a span that logos
    // could not match (a "gap"). Gaps that fall inside a regex literal are
    // consumed by `postprocess_regexes`; any gap that survives is a genuine
    // lexer error.
    let mut raw: Vec<(Option<Token>, usize)> = Vec::new();
    while let Some(token) = lex.next() {
        match token {
            Ok(tok) => raw.push((Some(tok), lex.span().start)),
            Err(_) => raw.push((None, lex.span().start)),
        }
    }
    postprocess_regexes(source, raw)
}

/// Rewrite the token stream so that a `/` appearing in operand position
/// (where a value may not legally end) is treated as the start of a regex
/// literal `/pattern/flags`. Division is otherwise untouched.
fn postprocess_regexes(source: &str, raw: Vec<(Option<Token>, usize)>) -> crate::Result<Vec<(Token, usize)>> {
    let mut out: Vec<(Token, usize)> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match &raw[i] {
            (Some(tok), off) => {
                let off = *off;
                if *tok == Token::Slash && is_regex_context(out.last().map(|(t, _)| t)) {
                    if let Some((pattern, flags, end)) = scan_regex(source, off) {
                        out.push((Token::Regex((pattern, flags)), off));
                        while i < raw.len() && raw[i].1 < end {
                            i += 1;
                        }
                        continue;
                    }
                }
                out.push((tok.clone(), off));
                i += 1;
            }
            (None, off) => {
                let (line, col) = offset_to_line_col(source, *off);
                return Err(crate::RakError::Lexer(format!(
                    "Unexpected character at line {}, col {}",
                    line, col
                )));
            }
        }
    }
    Ok(out)
}

/// True when a `/` placed after `prev` cannot be a division operator.
fn is_regex_context(prev: Option<&Token>) -> bool {
    match prev {
        None => true,
        Some(t) => !can_end_expr(t),
    }
}

fn can_end_expr(t: &Token) -> bool {
    matches!(
        t,
        Token::Ident(_)
            | Token::Int(_)
            | Token::Float(_)
            | Token::Float32(_)
            | Token::TypedInt(_)
            | Token::String(_)
            | Token::StringMulti(_)
            | Token::StringRaw(_)
            | Token::Interp(_)
            | Token::Bytes(_)
            | Token::Hex(_)
            | Token::Char(_)
            | Token::Regex(_)
            | Token::True
            | Token::False
            | Token::Nil
            | Token::RParen
            | Token::RBracket
            | Token::RBrace
            | Token::Question
    )
}

/// Scan a `/pattern/flags` literal starting at byte offset `start` (which must
/// point at the leading `/`). Returns `(pattern, flags, end_offset)` where
/// `end_offset` is the byte offset just past the closing flags.
fn scan_regex(source: &str, start: usize) -> Option<(String, String, usize)> {
    let b = source.as_bytes();
    if start >= b.len() || b[start] != b'/' {
        return None;
    }
    let mut i = start + 1;
    let mut pattern = String::new();
    let mut in_class = false;
    while i < b.len() {
        let c = source[i..].chars().next()?;
        match c {
            '\\' => {
                pattern.push('\\');
                let mut it = source[i..].chars();
                let _ = it.next();
                if let Some(nc) = it.next() {
                    pattern.push(nc);
                    i += 1 + nc.len_utf8();
                } else {
                    i += 1;
                }
            }
            '[' => {
                in_class = true;
                pattern.push('[');
                i += 1;
            }
            ']' => {
                in_class = false;
                pattern.push(']');
                i += 1;
            }
            '/' if !in_class => {
                break;
            }
            other => {
                pattern.push(other);
                i += other.len_utf8();
            }
        }
    }
    if i >= b.len() {
        return None;
    }
    i += 1; // skip closing '/'
    let flags_start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let flags = source[flags_start..i].to_string();
    Some((pattern, flags, i))
}

pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_token() {
        let toks = tokenize("0xDEAD 0xBEEF").unwrap();
        assert_eq!(toks.len(), 2);
        assert_eq!(toks[0].0, Token::Hex(0xDEAD));
        assert_eq!(toks[1].0, Token::Hex(0xBEEF));
    }

    #[test]
    fn test_keywords() {
        let toks = tokenize("scan fetch dump trace if else fn").unwrap();
        let kinds: Vec<&Token> = toks.iter().map(|(t, _)| t).collect();
        assert_eq!(
            kinds,
            vec![
                &Token::Scan,
                &Token::Fetch,
                &Token::Dump,
                &Token::Trace,
                &Token::If,
                &Token::Else,
                &Token::Fn,
            ]
        );
    }

    #[test]
    fn test_float_and_typed_int() {
        let toks = tokenize("3.14 42i32 10u8 7").unwrap();
        assert_eq!(toks[0].0, Token::Float(3.14));
        assert_eq!(toks[1].0, Token::TypedInt(TypedIntData { value: 42, kind: IntKind::I32 }));
        assert_eq!(toks[2].0, Token::TypedInt(TypedIntData { value: 10, kind: IntKind::U8 }));
        assert_eq!(toks[3].0, Token::Int(7));
    }

    #[test]
    fn test_pipe_token() {
        let toks = tokenize("x |> f").unwrap();
        let kinds: Vec<&Token> = toks.iter().map(|(t, _)| t).collect();
        assert_eq!(kinds, vec![&Token::Ident("x".to_string()), &Token::PipeGt, &Token::Ident("f".to_string())]);
    }

    #[test]
    fn test_char_token() {
        let toks = tokenize("'P' '\\n' '\\x41'").unwrap();
        assert_eq!(toks[0].0, Token::Char('P'));
        assert_eq!(toks[1].0, Token::Char('\n'));
        assert_eq!(toks[2].0, Token::Char('A'));
    }

    #[test]
    fn test_regex_in_operand_context() {
        // After `=`, `/` begins a regex literal.
        let toks = tokenize("let r = /\\d+/g;").unwrap();
        let regex = toks.iter().find(|(t, _)| matches!(t, Token::Regex(_)));
        assert!(regex.is_some(), "expected a Regex token");
        if let Some((Token::Regex((pat, flags)), _)) = regex {
            assert_eq!(pat, "\\d+");
            assert_eq!(flags, "g");
        }
    }

    #[test]
    fn test_division_not_regex() {
        // After an identifier, `/` is division, not a regex.
        let toks = tokenize("a / b").unwrap();
        assert!(toks.iter().all(|(t, _)| !matches!(t, Token::Regex(_))));
        assert!(toks.iter().any(|(t, _)| matches!(t, Token::Slash)));
    }

    #[test]
    fn test_regex_after_paren() {
        let toks = tokenize("dump(/foo/i)").unwrap();
        assert!(toks.iter().any(|(t, _)| matches!(t, Token::Regex((ref p, ref f)) if p == "foo" && f == "i")));
    }
}
