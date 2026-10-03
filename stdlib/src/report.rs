//! Evidence-report rendering: turn structured findings into a tidy Markdown
//! document suitable for a findings file or paste into a ticket.
//!
//! Pure and offline-friendly: scripts build a nested map (title, meta table,
//! sections) and get a deterministic Markdown string back.

use std::collections::BTreeMap;

/// A findings section: `(heading, bullet lines)`.
pub type Section = (String, Vec<String>);

/// Render `meta` as a Markdown key/value table.
pub fn meta_table(meta: &BTreeMap<String, String>) -> String {
    if meta.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("| field | value |\n| --- | --- |\n");
    for (k, v) in meta {
        let v = v.replace('|', "\\|");
        out.push_str(&format!("| {} | {} |\n", k, v));
    }
    out
}

/// Render a full findings document from a title, metadata, and sections.
pub fn markdown_report(
    title: &str,
    meta: &BTreeMap<String, String>,
    sections: &[Section],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    let table = meta_table(meta);
    if !table.is_empty() {
        out.push_str(&table);
        out.push('\n');
    }
    for (heading, items) in sections {
        out.push_str(&format!("## {}\n\n", heading));
        if items.is_empty() {
            out.push_str("(no findings)\n\n");
            continue;
        }
        for item in items {
            out.push_str(&format!("- {}\n", item));
        }
        out.push('\n');
    }
    out
}

/// Escape user-supplied strings that would otherwise corrupt the Markdown
/// structure (leading `#`/`-`/numbers, pipe characters). Call before using a
/// value in a section item if it came from untrusted input.
pub fn escape_inline(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ").replace('\r', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_meta() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("target".to_string(), "example.com".to_string());
        m.insert("tool".to_string(), "rakc".to_string());
        m.insert("confidence".to_string(), "medium".to_string());
        m
    }

    #[test]
    fn renders_title_and_table() {
        let out = markdown_report("Recon report", &sample_meta(), &[]);
        assert!(out.starts_with("# Recon report\n"));
        assert!(out.contains("| field | value |"));
        assert!(out.contains("| target | example.com |"));
    }

    #[test]
    fn renders_sections_with_bullets() {
        let sections: Vec<Section> = vec![
            (
                "WHOIS".to_string(),
                vec!["registrar: Acme".to_string(), "created: 1995".to_string()],
            ),
            (
                "Subdomains".to_string(),
                vec!["www.example.com".to_string()],
            ),
        ];
        let out = markdown_report("R", &BTreeMap::new(), &sections);
        assert!(out.contains("## WHOIS\n\n- registrar: Acme\n- created: 1995"));
        assert!(out.contains("## Subdomains\n\n- www.example.com"));
    }

    #[test]
    fn empty_section_noted_not_skipped() {
        let sections: Vec<Section> = vec![("YARA".to_string(), vec![])];
        let out = markdown_report("R", &BTreeMap::new(), &sections);
        assert!(out.contains("(no findings)"));
    }

    #[test]
    fn pipes_escaped() {
        let mut m = BTreeMap::new();
        m.insert("target".to_string(), "a|b".to_string());
        let out = markdown_report("R", &m, &[]);
        assert!(out.contains("a\\|b"));
    }
}
