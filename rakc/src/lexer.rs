use logos::Logos;

/// Tokens for the Rak programming language.
/// Keywords are English words. Hexadecimal is first-class.
#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(skip r"[ \t\n\f]+")]
#[logos(skip r"//[^\n]*\n?")]
pub enum Token {
    // --- English Keywords ---
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

    // --- Literals ---
    /// Hexadecimal integer: 0x1A2B, 0x00FF
    #[regex(r"0x[0-9A-Fa-f]+", |lex| hex_to_u64(lex.slice()))]
    Hex(u64),

    /// Decimal integer
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<u64>().ok())]
    Int(u64),

    /// String literal: "hello" or "hex:\x48\x65\x6C\x6C\x6F"
    #[regex(r#""([^"\\]|\\.)*""#, |lex| parse_string(lex.slice()))]
    String(String),

    /// Byte string / raw bytes: b"\x00\xFF"
    #[regex(r#"b"([^"\\]|\\.)*""#, |lex| parse_bytes(lex.slice()))]
    Bytes(Vec<u8>),

    // --- Identifiers ---
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    // --- Operators & Punctuation ---
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

    #[token(".")]
    Dot,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,

    // --- Delimiters ---
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

fn hex_to_u64(s: &str) -> Option<u64> {
    u64::from_str_radix(&s[2..], 16).ok()
}

fn parse_string(s: &str) -> Option<String> {
    let inner = &s[1..s.len() - 1];
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
}