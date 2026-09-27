//! YARA-lite: a small, dependency-free rule engine in the spirit of YARA.
//!
//! Supported rule surface:
//!
//! ```yara
//! rule pe_implant {
//!     strings:
//!         $mz = { 4D 5A }
//!         $sus = "VirtualAlloc" nocase
//!         $w = "WSADuplicateSocket"
//!     condition:
//!         $mz at 0 and any of ($sus, $w)
//! }
//! ```
//!
//! - string definitions: text literals (optional `nocase`) and hex literals
//!   with `??` wildcards
//! - conditions: `$id`, `at <n>`, `in (lo..hi)`, `all of them`, `any of them`,
//!   `none of them`, `all/any/none of ($a, $b)`, `and`/`or`/`not`, parens
//!
//! Everything is offline and deterministic; `scan` returns which rules hit and
//! the byte offsets of each matched string (for evidence framing).

use std::collections::HashMap;

/// A compiled rule set (ready to scan with `scan`).
#[derive(Debug, Clone)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
}

/// One YARA-lite rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub strings: Vec<RuleString>,
    pub condition: Cond,
}

/// A named pattern inside a rule.
#[derive(Debug, Clone)]
pub struct RuleString {
    pub id: String,
    pub pattern: Pattern,
    pub nocase: bool,
}

/// A matching pattern.
#[derive(Debug, Clone)]
pub enum Pattern {
    Bytes(Vec<u8>),
    /// Byte sequence where `None` entries are `??` wildcards.
    Hex(Vec<Option<u8>>),
}

/// Condition AST.
#[derive(Debug, Clone)]
pub enum Cond {
    Id(String),
    At(String, u64),
    In(String, u64, u64),
    AllOfThem,
    AnyOfThem,
    NoneOfThem,
    AllOf(Vec<String>),
    AnyOf(Vec<String>),
    NoneOf(Vec<String>),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
}

/// A rule that matched, with hit offsets for each contributing string.
#[derive(Debug, Clone)]
pub struct RuleMatch {
    pub rule: String,
    pub hits: Vec<(String, u64)>,
}

/// A candidate hit for one string id (id + byte offset).
#[derive(Clone)]
struct Hit {
    id: String,
    offset: u64,
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

/// Parse YARA-lite source into a `RuleSet`.
pub fn compile(source: &str) -> Result<RuleSet, String> {
    let tokens = tokenize(source)?;
    let mut p = Parser { tokens, pos: 0 };
    let rules = p.parse_rules()?;
    if !p.at_end() {
        return Err(format!("yara: unexpected token '{}'", p.peek_display()));
    }
    Ok(RuleSet { rules })
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Rule,
    Lbrace,
    Rbrace,
    Lparen,
    Rparen,
    Comma,
    Strings,
    Condition,
    Assign,
    Dollar,
    Ident(String),
    Str(String),
    Hex(Vec<Option<u8>>),
    Nocase,
    All,
    Any,
    None,
    Of,
    Them,
    At,
    In,
    DotDot,
    Int(u64),
    And,
    Or,
    Not,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // comments: `//` and `#` run to end of line.
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match c {
            '{' => {
                // A `{ ... }` group whose (minimal) contents are hex-like is a
                // hex string: bytes and `??` wildcards only. Rule bodies start
                // with `{` too, but they contain `:`/keywords, so they fall
                // through as a plain Lbrace token.
                let next_close = chars[i + 1..].iter().position(|&c| c == '}');
                let hexlike = match next_close {
                    Some(rel) => {
                        let body: String = chars[i + 1..i + 1 + rel].iter().collect();
                        !body.is_empty()
                            && body
                                .chars()
                                .all(|c| c.is_ascii_hexdigit() || c.is_whitespace() || c == '?')
                    }
                    None => false,
                };
                if hexlike {
                    let rel = next_close.unwrap();
                    let body: String = chars[i + 1..i + 1 + rel].iter().collect();
                    let bytes = tokenize_hex(&body)
                        .map_err(|m| format!("{m} (line {})", line_at(src, i)))?;
                    toks.push(Tok::Hex(bytes));
                    // Skip past the whole `{ ... }` group (closing brace at
                    // index i + 1 + rel plus the loop's trailing increment).
                    i += rel + 2;
                    continue;
                }
                toks.push(Tok::Lbrace);
            }
            '}' => toks.push(Tok::Rbrace),
            // YARA uses `strings:` / `condition:` section headers.
            ':' => {}
            '(' => toks.push(Tok::Lparen),
            ')' => toks.push(Tok::Rparen),
            ',' => toks.push(Tok::Comma),
            '=' => toks.push(Tok::Assign),
            '$' => toks.push(Tok::Dollar),
            '.' if i + 1 < chars.len() && chars[i + 1] == '.' => {
                toks.push(Tok::DotDot);
                i += 1;
            }
            '"' => {
                let (s, next) = read_string(&chars, i)
                    .map_err(|m| format!("yara: {m} (line {})", line_at(src, i)))?;
                toks.push(Tok::Str(s));
                i = next;
            }
            _ if c.is_ascii_digit() => {
                let mut j = i;
                while j < chars.len()
                    && (chars[j].is_ascii_hexdigit()
                        || ((chars[j] == 'x' || chars[j] == 'X') && j == i + 1))
                    && j - i < 10
                {
                    j += 1;
                }
                let num: String = chars[i..j].iter().collect();
                let v = u64::from_str_radix(num.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .map_err(|_| format!("yara: bad integer '{}'", num))?;
                toks.push(Tok::Int(v));
                // Position for the loop's trailing `i += 1` to land on `j`.
                i = j.saturating_sub(1);
            }
            _ if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let tok = match word.as_str() {
                    "rule" => Tok::Rule,
                    "strings" => Tok::Strings,
                    "condition" => Tok::Condition,
                    "nocase" => Tok::Nocase,
                    "all" => Tok::All,
                    "any" => Tok::Any,
                    "none" => Tok::None,
                    "of" => Tok::Of,
                    "them" => Tok::Them,
                    "at" => Tok::At,
                    "in" => Tok::In,
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    _ => Tok::Ident(word),
                };
                toks.push(tok);
            }
            _ => {
                return Err(format!(
                    "yara: unexpected character '{}' (line {})",
                    c,
                    line_at(src, i)
                ))
            }
        }
        i += 1;
    }
    Ok(toks)
}

fn line_at(src: &str, idx: usize) -> usize {
    src[..idx.min(src.len())].chars().filter(|&c| c == '\n').count() + 1
}

fn read_string(chars: &[char], start: usize) -> Result<(String, usize), String> {
    let mut i = start + 1;
    let mut out = String::new();
    while i < chars.len() {
        match chars[i] {
            '"' => return Ok((out, i)),
            '\\' if i + 1 < chars.len() => {
                let e = chars[i + 1];
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    'x'
                        if i + 3 < chars.len()
                            && chars[i + 2].is_ascii_hexdigit()
                            && chars[i + 3].is_ascii_hexdigit() =>
                    {
                        let hex: String = chars[i + 2..i + 4].iter().collect();
                        if let Ok(b) = u8::from_str_radix(&hex, 16) {
                            out.push(b as char);
                        }
                        i += 2;
                    }
                    other => out.push(other),
                }
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Err("unterminated string literal".to_string())
}

/// Parse a raw hex-string body like ` 4D 5A ?? AA `.
fn tokenize_hex(chunk: &str) -> Result<Vec<Option<u8>>, String> {
    let clean: String = chunk.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = Vec::new();
    let bytes: Vec<char> = clean.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == '?' && bytes[i + 1] == '?' {
            out.push(None);
            i += 2;
            continue;
        }
        if i + 1 >= bytes.len() {
            return Err(format!("odd hex nibble in hex string near '{}'", clean));
        }
        let pair: String = bytes[i..i + 2].iter().collect();
        let b = u8::from_str_radix(&pair, 16).map_err(|_| format!("invalid hex byte '{}'", pair))?;
        out.push(Some(b));
        i += 2;
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }
    fn peek_display(&self) -> String {
        match self.peek() {
            Some(t) => format!("{:?}", t),
            None => "<eof>".to_string(),
        }
    }
    fn expect(&mut self, t: Tok, what: &str) -> Result<(), String> {
        match self.next() {
            Some(x) if x == t => Ok(()),
            other => Err(format!("yara: expected {} in rule, found {:?}", what, other)),
        }
    }

    fn parse_rules(&mut self) -> Result<Vec<Rule>, String> {
        let mut rules = Vec::new();
        while !self.at_end() {
            self.expect(Tok::Rule, "'rule'")?;
            let name = match self.next() {
                Some(Tok::Ident(n)) => n,
                other => return Err(format!("yara: expected rule name, found {:?}", other)),
            };
            self.expect(Tok::Lbrace, "'{'")?;
            rules.push(self.parse_rule_body(&name)?);
        }
        Ok(rules)
    }

    fn parse_rule_body(&mut self, name: &str) -> Result<Rule, String> {
        let mut strings: Vec<RuleString> = Vec::new();
        let mut condition: Option<Cond> = None;
        loop {
            match self.peek() {
                Some(Tok::Strings) => {
                    if condition.is_some() {
                        return Err(format!("yara: 'strings' after 'condition' in rule '{}'", name));
                    }
                    self.next();
                    while matches!(self.peek(), Some(Tok::Dollar)) {
                        strings.push(self.parse_string_def()?);
                    }
                }
                Some(Tok::Condition) => {
                    self.next();
                    condition = Some(self.parse_cond()?);
                    break;
                }
                Some(Tok::Rbrace) => break,
                other => {
                    return Err(format!(
                        "yara: expected 'strings:'/'condition:' in rule '{}', found {:?}",
                        name, other
                    ))
                }
            }
        }
        self.expect(Tok::Rbrace, "'}'")?;
        if strings.is_empty() {
            return Err(format!("yara: rule '{}' declares no strings", name));
        }
        let condition =
            condition.ok_or_else(|| format!("yara: rule '{}' has no condition", name))?;
        Ok(Rule { name: name.to_string(), strings, condition })
    }

    fn parse_string_def(&mut self) -> Result<RuleString, String> {
        self.expect(Tok::Dollar, "'$'")?;
        let id = match self.next() {
            Some(Tok::Ident(n)) => n,
            other => return Err(format!("yara: expected string id after '$', found {:?}", other)),
        };
        self.expect(Tok::Assign, "'='")?;
        let (pattern, was_hex) = match self.next() {
            Some(Tok::Str(s)) => (Pattern::Bytes(s.into_bytes()), false),
            Some(Tok::Hex(bytes)) => (Pattern::Hex(bytes), true),
            other => {
                return Err(format!(
                    "yara: expected string literal for '${}', found {:?}",
                    id, other
                ))
            }
        };
        let mut nocase = false;
        if !was_hex && self.peek() == Some(&Tok::Nocase) {
            self.next();
            nocase = true;
        }
        Ok(RuleString { id, pattern, nocase })
    }

    fn parse_cond(&mut self) -> Result<Cond, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Cond, String> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Tok::Or) {
            self.next();
            let right = self.parse_and()?;
            left = Cond::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Cond, String> {
        let mut left = self.parse_not()?;
        while self.peek() == Some(&Tok::And) {
            self.next();
            let right = self.parse_not()?;
            left = Cond::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Cond, String> {
        if self.peek() == Some(&Tok::Not) {
            self.next();
            return Ok(Cond::Not(Box::new(self.parse_not()?)));
        }
        if self.peek() == Some(&Tok::Lparen) {
            self.next();
            let inner = self.parse_or()?;
            self.expect(Tok::Rparen, "')'")?;
            return Ok(inner);
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<Cond, String> {
        match self.next() {
            Some(Tok::Dollar) => {
                let id = match self.next() {
                    Some(Tok::Ident(n)) => n,
                    other => {
                        return Err(format!("yara: expected string id after '$', found {:?}", other))
                    }
                };
                match self.peek() {
                    Some(Tok::At) => {
                        self.next();
                        let n = self.expect_int()?;
                        Ok(Cond::At(id, n))
                    }
                    Some(Tok::In) => {
                        self.next();
                        self.expect(Tok::Lparen, "'(' after 'in'")?;
                        let lo = self.expect_int()?;
                        self.expect(Tok::DotDot, "'..'")?;
                        let hi = self.expect_int()?;
                        self.expect(Tok::Rparen, "')'")?;
                        Ok(Cond::In(id, lo, hi))
                    }
                    _ => Ok(Cond::Id(id)),
                }
            }
            Some(Tok::All) => self.parse_quantifier(Cond::AllOf),
            Some(Tok::Any) => self.parse_quantifier(Cond::AnyOf),
            Some(Tok::None) => self.parse_quantifier(Cond::NoneOf),
            other => Err(format!("yara: unexpected token in condition: {:?}", other)),
        }
    }

    fn expect_int(&mut self) -> Result<u64, String> {
        match self.next() {
            Some(Tok::Int(n)) => Ok(n),
            other => Err(format!("yara: expected integer, found {:?}", other)),
        }
    }

    /// `All|Any|None of them | of ($a, $b)`.
    fn parse_quantifier(&mut self, kind: fn(Vec<String>) -> Cond) -> Result<Cond, String> {
        self.expect(Tok::Of, "'of'")?;
        if self.peek() == Some(&Tok::Them) {
            self.next();
            return Ok(match kind(vec![]) {
                Cond::AllOf(_) => Cond::AllOfThem,
                Cond::AnyOf(_) => Cond::AnyOfThem,
                _ => Cond::NoneOfThem,
            });
        }
        if self.peek() == Some(&Tok::Lparen) {
            self.next();
            let mut ids = Vec::new();
            loop {
                self.expect(Tok::Dollar, "'$'")?;
                match self.next() {
                    Some(Tok::Ident(n)) => ids.push(n),
                    other => return Err(format!("yara: expected string id, found {:?}", other)),
                }
                if self.peek() != Some(&Tok::Comma) {
                    break;
                }
                self.next();
            }
            self.expect(Tok::Rparen, "')'")?;
            return Ok(kind(ids));
        }
        Err("yara: expected 'them' or '($a, $b)' after 'of'".to_string())
    }
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Scan `data` against the compiled rules; returns matches in rule order.
pub fn scan(rules: &RuleSet, data: &[u8]) -> Vec<RuleMatch> {
    let mut out = Vec::new();
    for rule in &rules.rules {
        let mut hits: Vec<Hit> = Vec::new();
        for rs in &rule.strings {
            for off in find_pattern(rs, data) {
                hits.push(Hit { id: rs.id.clone(), offset: off });
            }
        }
        let present = matches_present(&hits, &rule.strings);
        if eval_cond(&rule.condition, &hits, &present) {
            out.push(RuleMatch {
                rule: rule.name.clone(),
                hits: hits.into_iter().map(|h| (h.id, h.offset)).collect(),
            });
        }
    }
    out
}

fn matches_present(hits: &[Hit], strings: &[RuleString]) -> HashMap<String, Vec<u64>> {
    let mut m: HashMap<String, Vec<u64>> = HashMap::new();
    for s in strings {
        m.insert(s.id.clone(), Vec::new());
    }
    for h in hits {
        m.entry(h.id.clone()).or_default().push(h.offset);
    }
    m
}

fn has_hit(present: &HashMap<String, Vec<u64>>, id: &str) -> bool {
    present.get(id).map(|v| !v.is_empty()).unwrap_or(false)
}

fn eval_cond(cond: &Cond, hits: &[Hit], present: &HashMap<String, Vec<u64>>) -> bool {
    match cond {
        Cond::Id(id) => has_hit(present, id),
        Cond::At(id, pos) => present.get(id).map(|v: &Vec<u64>| v.contains(pos)).unwrap_or(false),
        Cond::In(id, lo, hi) => {
            present.get(id).map(|v| v.iter().any(|o| o >= lo && o <= hi)).unwrap_or(false)
        }
        Cond::AllOfThem => !present.is_empty() && present.values().all(|v| !v.is_empty()),
        Cond::AnyOfThem => present.values().any(|v| !v.is_empty()),
        Cond::NoneOfThem => present.values().all(|v| v.is_empty()),
        Cond::AllOf(ids) => !ids.is_empty() && ids.iter().all(|id| has_hit(present, id)),
        Cond::AnyOf(ids) => ids.iter().any(|id| has_hit(present, id)),
        Cond::NoneOf(ids) => ids.iter().all(|id| !has_hit(present, id)),
        Cond::And(a, b) => eval_cond(a, hits, present) && eval_cond(b, hits, present),
        Cond::Or(a, b) => eval_cond(a, hits, present) || eval_cond(b, hits, present),
        Cond::Not(a) => !eval_cond(a, hits, present),
    }
}

/// Locate every occurrence of a rule string in `data`.
fn find_pattern(rs: &RuleString, data: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    match &rs.pattern {
        Pattern::Bytes(bytes) => {
            if bytes.is_empty() || bytes.len() > data.len() {
                return out;
            }
            if rs.nocase {
                let needle: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
                let hay: Vec<u8> = data.iter().map(|b| b.to_ascii_lowercase()).collect();
                naive_search(&hay, &needle, &mut out);
            } else {
                naive_search(data, bytes, &mut out);
            }
        }
        Pattern::Hex(seq) => {
            if seq.is_empty() || seq.len() > data.len() {
                return out;
            }
            'outer: for start in 0..=(data.len() - seq.len()) {
                for (k, b) in seq.iter().enumerate() {
                    if let Some(want) = b {
                        let cur = data[start + k];
                        let cur = if rs.nocase { cur.to_ascii_lowercase() } else { cur };
                        let want = if rs.nocase { want.to_ascii_lowercase() } else { *want };
                        if cur != want {
                            continue 'outer;
                        }
                    }
                }
                out.push(start as u64);
            }
        }
    }
    out
}

fn naive_search(hay: &[u8], needle: &[u8], out: &mut Vec<u64>) {
    if needle.is_empty() || needle.len() > hay.len() {
        return;
    }
    for start in 0..=(hay.len() - needle.len()) {
        if hay[start..start + needle.len()] == *needle {
            out.push(start as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"
rule pe_header {
    strings:
        $mz = { 4D 5A }
        $pe = "PE\0\0"
    condition:
        $mz at 0 and $pe
}

rule evil_import {
    strings:
        $a = "VirtualAlloc" nocase
        $b = "CreateThread" nocase
    condition:
        any of them
}
"#;

    fn pe_blob() -> Vec<u8> {
        let mut d = b"MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00".to_vec();
        d.extend_from_slice(b"\xff\xff\x00\x00\xb8\x00\x00\x00\x00\x00\x00\x00\x40\x00\x00\x00");
        d.extend_from_slice(b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00");
        d.extend_from_slice(b"PE\x00\x00");
        d
    }

    #[test]
    fn compile_and_scan_hits() {
        let rules = compile(RULES).unwrap();
        let data = pe_blob();
        let matches = scan(&rules, &data);
        let pe = matches.iter().find(|m| m.rule == "pe_header").unwrap_or_else(|| panic!("pe_header missing: {:?}", matches));
        // string ids are stored without the `$` prefix; `$mz` must hit at 0.
        assert!(pe.hits.iter().any(|(id, off)| id == "mz" && *off == 0));
    }

    #[test]
    fn wildcard_hex_matches() {
        let rules = compile("rule r { strings: $t = { 4D ?? 5A } condition: $t }").unwrap();
        let m = scan(&rules, b"\x41\x4D\x90\x5A\x42");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].hits[0].1, 1);
    }

    #[test]
    fn nocase_and_quantifier() {
        let rules = compile(RULES).unwrap();
        let m = scan(&rules, b"virtualalloc and createthread here");
        assert!(m.iter().any(|x| x.rule == "evil_import"), "got: {:?}", m);
    }

    #[test]
    fn at_constraint_offsets() {
        let rules = compile("rule anchored { strings: $s = \"MZ\" condition: $s at 0 }").unwrap();
        assert_eq!(scan(&rules, b"MZ....").len(), 1);
        assert_eq!(scan(&rules, b"..MZ..").len(), 0);
    }

    #[test]
    fn in_range_constraint() {
        let rules = compile("rule ranged { strings: $s = { 41 41 } condition: $s in (2..4) }").unwrap();
        assert_eq!(scan(&rules, b"xxAAyy").len(), 1, "offset 2 in range");
        let rules2 = compile("rule ranged2 { strings: $s = { 41 41 } condition: $s in (0..1) }").unwrap();
        assert_eq!(scan(&rules2, b"xxAAyy").len(), 0, "offset 2 outside range");
    }

    #[test]
    fn multi_rule_and_not() {
        let rules = compile(
            "rule a { strings: $x = \"aaa\" condition: $x }\n\
             rule b { strings: $y = \"bbb\" condition: not $y }",
        )
        .unwrap();
        let m = scan(&rules, b"aaa bbb");
        assert!(m.iter().any(|x| x.rule == "a"));
        assert!(!m.iter().any(|x| x.rule == "b"));
    }

    #[test]
    fn invalid_rules_error() {
        assert!(compile("not a rule").is_err());
        assert!(compile("rule x { condition: $a }").is_err()); // no strings
        assert!(compile("rule x { strings: $a = \"y\" }").is_err()); // no condition
        assert!(compile("rule x { strings: $a = 42 condition: $a }").is_err());
    }
}