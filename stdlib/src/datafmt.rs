//! Data-format builtins for Rak: CSV (RFC-4180-style) and a documented
//! YAML subset ג€” pure Rust, no new dependencies.

// ---------------------------------------------------------------- CSV ----

/// Parse CSV text into rows of string cells. Handles quoted fields
/// (`""` escapes an embedded quote), embedded commas/newlines inside
/// quotes, `\r\n` line endings, and a trailing newline.
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut line = 1usize;
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        cell.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                '\n' => {
                    line += 1;
                    cell.push('\n');
                }
                _ => cell.push(c),
            }
            continue;
        }
        match c {
            '"' => in_quotes = true,
            ',' => {
                row.push(std::mem::take(&mut cell));
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                line += 1;
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                line += 1;
            }
            _ => cell.push(c),
        }
    }
    if in_quotes {
        return Err(format!("csv_parse: unterminated quote (line {})", line));
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}

/// Quote a CSV field when needed (comma, quote, or newline present).
fn csv_quote(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Serialize rows (cells already stringified by the caller) into CSV text.
pub fn write_csv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        let cells: Vec<String> = row.iter().map(|c| csv_quote(c)).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------- YAML ----

/// A parsed YAML-subset value (order-preserving maps).
#[derive(Debug, Clone, PartialEq)]
pub enum YamlV {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<YamlV>),
    Map(Vec<(String, YamlV)>),
}

/// Strip a trailing `# comment` (unless the `#` is inside quotes).
fn strip_comment(s: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) => {
                out.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '#' {
                    return out.trim_end().to_string();
                }
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    out
}

/// Parse a scalar: null/~/true/false/int/float/quoted/bare string.
pub fn parse_scalar(s: &str) -> YamlV {
    let t = s.trim();
    if t == "null" || t == "~" || t == "Null" || t == "NULL" || t.is_empty() {
        return YamlV::Null;
    }
    if let Some(inner) = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return YamlV::Str(unescape_basic(inner));
    }
    if let Some(inner) = t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return YamlV::Str(inner.replace("''", "'"));
    }
    match t {
        "true" | "True" | "TRUE" => return YamlV::Bool(true),
        "false" | "False" | "FALSE" => return YamlV::Bool(false),
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        return YamlV::Int(i);
    }
    if let Ok(f) = t.parse::<f64>() {
        return YamlV::Float(f);
    }
    YamlV::Str(t.to_string())
}

fn unescape_basic(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Split `key: value` (key may be quoted). `None` when there is no colon.
fn split_kv(s: &str) -> Option<(String, String)> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else if c == ':' {
                    let key = s[..i].trim();
                    let key = key
                        .strip_prefix('"')
                        .and_then(|x| x.strip_suffix('"'))
                        .map(|x| x.to_string())
                        .unwrap_or_else(|| key.to_string());
                    let val = s[i + 1..].trim().to_string();
                    return Some((key, val));
                }
            }
        }
    }
    None
}

/// Parse a block at `indent` starting from `idx`. Returns map or list.
fn parse_block(
    lines: &[(usize, String, usize)],
    idx: &mut usize,
    indent: usize,
    ctx: &str,
) -> Result<YamlV, String> {
    if *idx >= lines.len() {
        return Ok(YamlV::Null);
    }
    if lines[*idx].1.starts_with("- ") || lines[*idx].1 == "-" {
        parse_list(lines, idx, indent, ctx)
    } else {
        parse_map(lines, idx, indent, ctx)
    }
}

/// After consuming a `key: value` line, either the value was inline or a
/// nested block follows (deeper indent) — handle both.
fn push_kv_entry(
    lines: &[(usize, String, usize)],
    idx: &mut usize,
    indent: usize,
    k: &str,
    v: &str,
    entries: &mut Vec<(String, YamlV)>,
    ctx: &str,
) -> Result<(), String> {
    if v.is_empty() {
        if *idx < lines.len() && lines[*idx].0 > indent {
            let child_indent = lines[*idx].0;
            let child = parse_block(lines, idx, child_indent, ctx)?;
            entries.push((k.to_string(), child));
        } else {
            entries.push((k.to_string(), YamlV::Null));
        }
    } else {
        entries.push((k.to_string(), parse_scalar(v)));
    }
    Ok(())
}

fn parse_map(
    lines: &[(usize, String, usize)],
    idx: &mut usize,
    indent: usize,
    ctx: &str,
) -> Result<YamlV, String> {
    let mut entries = Vec::new();
    while *idx < lines.len() && lines[*idx].0 == indent {
        let content = lines[*idx].1.clone();
        let ln = lines[*idx].2;
        let (k, v) = split_kv(&content)
            .ok_or_else(|| format!("{}: line {}: expected 'key: value'", ctx, ln))?;
        *idx += 1;
        push_kv_entry(lines, idx, indent, &k, &v, &mut entries, ctx)?;
    }
    Ok(YamlV::Map(entries))
}

fn parse_list(
    lines: &[(usize, String, usize)],
    idx: &mut usize,
    indent: usize,
    ctx: &str,
) -> Result<YamlV, String> {
    let mut items = Vec::new();
    while *idx < lines.len() && lines[*idx].0 == indent {
        let content = lines[*idx].1.clone();
        let rest = match content.strip_prefix("- ") {
            Some(r) => r.to_string(),
            None => {
                if content == "-" {
                    String::new()
                } else {
                    break; // not a list item at this indent — done
                }
            }
        };
        *idx += 1;
        if rest.is_empty() {
            // Nested block under the bare `-`.
            if *idx < lines.len() && lines[*idx].0 > indent {
                let child_indent = lines[*idx].0;
                items.push(parse_block(lines, idx, child_indent, ctx)?);
            } else {
                items.push(YamlV::Null);
            }
            continue;
        }
        // `- key: value` — inline first map entry, with more at indent + 2.
        if let Some((k, v)) = split_kv(&rest) {
            let mut entries = Vec::new();
            let child_indent = indent + 2;
            push_kv_entry(lines, idx, child_indent, &k, &v, &mut entries, ctx)?;
            while *idx < lines.len()
                && lines[*idx].0 == child_indent
                && !lines[*idx].1.starts_with("- ")
            {
                let (ck, cv) = split_kv(&lines[*idx].1).ok_or_else(|| {
                    format!("{}: line {}: expected 'key: value'", ctx, lines[*idx].2)
                })?;
                *idx += 1;
                push_kv_entry(lines, idx, child_indent, &ck, &cv, &mut entries, ctx)?;
            }
            items.push(YamlV::Map(entries));
        } else {
            items.push(parse_scalar(&rest));
        }
    }
    Ok(YamlV::List(items))
}

/// Parse the documented YAML subset: nested maps by indentation, `- ` list
/// items, inline scalars (int/float/bool/null/quoted/bare strings), and

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quoting_roundtrip() {
        let text = "name,note\nalice,\"says \"\"hi\"\", loudly\"\nbob,\"line1\nline2\"";
        let rows = parse_csv(text).unwrap();
        assert_eq!(rows[0], vec!["name", "note"]);
        assert_eq!(rows[1], vec!["alice", "says \"hi\", loudly"]);
        assert_eq!(rows[2], vec!["bob", "line1\nline2"]);
        let out = write_csv(&rows);
        let back = parse_csv(&out).unwrap();
        assert_eq!(rows, back);
    }

    #[test]
    fn csv_crlf_and_trailing_newline() {
        let rows = parse_csv("a,b\r\n1,2\r\n").unwrap();
        assert_eq!(rows, vec![vec!["a", "b"], vec!["1", "2"]]);
        assert!(parse_csv("\"unterminated").is_err());
    }

    #[test]
    fn yaml_subset_nested() {
        let y = parse_yaml(
            "# config\nhost: example.com\nport: 443\nretries: 2.5\nenabled: true\ntags:\n  - osint\n  - \"infra # not comment\"\nnested:\n  inner: null\n",
        )
        .unwrap();
        match y {
            YamlV::Map(m) => {
                assert_eq!(m[0], ("host".into(), YamlV::Str("example.com".into())));
                assert_eq!(m[1], ("port".into(), YamlV::Int(443)));
                assert_eq!(m[2], ("retries".into(), YamlV::Float(2.5)));
                assert_eq!(m[3], ("enabled".into(), YamlV::Bool(true)));
                match &m[4].1 {
                    YamlV::List(items) => {
                        assert_eq!(items[0], YamlV::Str("osint".into()));
                        assert_eq!(items[1], YamlV::Str("infra # not comment".into()));
                    }
                    o => panic!("tags should be a list, got {:?}", o),
                }
                match &m[5].1 {
                    YamlV::Map(inner) => assert_eq!(inner[0], ("inner".into(), YamlV::Null)),
                    o => panic!("nested should be a map, got {:?}", o),
                }
            }
            o => panic!("root should be a map, got {:?}", o),
        }
    }

    #[test]
    fn yaml_rejects_flow_style_with_line() {
        assert!(parse_yaml("a: {x: 1}").unwrap_err().contains("line 1"));
        assert!(parse_yaml("k: v\n  bad indent: 1").is_err());
    }

    #[test]
    fn yaml_list_of_maps() {
        let y = parse_yaml("findings:\n  - title: a\n    sev: high\n  - title: b\n    sev: low\n")
            .unwrap();
        match y {
            YamlV::Map(m) => match &m[0].1 {
                YamlV::List(items) => {
                    assert_eq!(items.len(), 2);
                    match &items[0] {
                        YamlV::Map(e) => {
                            assert_eq!(e[0], ("title".into(), YamlV::Str("a".into())));
                            assert_eq!(e[1], ("sev".into(), YamlV::Str("high".into())));
                        }
                        o => panic!("expected map, got {:?}", o),
                    }
                }
                o => panic!("expected list, got {:?}", o),
            },
            o => panic!("expected root map, got {:?}", o),
        }
    }
}

/// `#` comments. Flow style, anchors and aliases are errors naming the
/// line number.
pub fn parse_yaml(text: &str) -> Result<YamlV, String> {
    let mut lines: Vec<(usize, String, usize)> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let trimmed_end = raw.trim_end();
        if trimmed_end.trim().is_empty() {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        let content = strip_comment(trimmed_end.trim_start());
        if content.is_empty() {
            continue;
        }
        if content.contains('{')
            || content.contains('}')
            || content.contains('&')
            || content.starts_with('*')
        {
            return Err(format!(
                "yaml_parse: line {}: flow style, anchors and aliases are not supported in the YAML subset",
                i + 1
            ));
        }
        lines.push((indent, content.to_string(), i + 1));
    }
    if lines.is_empty() {
        return Ok(YamlV::Null);
    }
    let mut idx = 0usize;
    let v = parse_block(&lines, &mut idx, lines[0].0, "yaml_parse")?;
    if idx != lines.len() {
        return Err(format!(
            "yaml_parse: line {}: unexpected indentation structure",
            lines[idx].2
        ));
    }
    Ok(v)
}
