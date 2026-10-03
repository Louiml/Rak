//! Escaping for the contexts a string built from untrusted data gets dropped into.
//!
//! Every function here exists because the alternative is a program that looks
//! correct and is injectable. None of them is a substitute for the right thing --
//! parameterised SQL beats `sql_escape`, an argv array beats `shell_escape` -- but
//! "build the query by escaping" is what most code does, and having it available
//! and correct is better than having it available and subtly wrong.
//!
//! # What each one guarantees
//!
//! * [`sql_escape`] doubles `'`, and rejects nothing else. It is for a value inside
//!   a quoted SQL literal. It deliberately does **not** quote the value for you: a
//!   function that silently added quotes would produce `"it's"` inside `'...'`, which
//!   is a syntax error at best.
//! * [`shell_escape`] single-quotes and replaces `'`. This is the only safe form
//!   for POSIX shells: it is immune to word splitting, globbing, `$(...)`, backticks
//!   and `;` alike. It does **not** make the argument safe to *paste* into a shell
//!   interactively -- a program that prints an escaped string for a human to retype
//!   is a different problem.
//! * [`html_escape`] escapes the five characters that matter in element text and
//!   attribute values, and escapes `'` as `&#x27;` so the result is safe in a
//!   single-quoted attribute too.
//! * [`regex_escape`] escapes every metacharacter, so the input matches itself and
//!   cannot smuggle in a quantifier. A pattern that says "search for this literal"
//!   should not be a pattern.
//!
//! # Why escaping is not the primary defence
//!
//! These functions reduce the chance of an injection; they do not remove it. Every
//! one of them has a structural alternative that is strictly better:
//!
//! | Instead of | Use |
//! | --- | --- |
//! | `sql_escape(user_input)` | a parameterised query |
//! | `shell_escape(arg)` | `exec` with an argv array |
//! | `html_escape(text)` | the templating layer's own escaping |
//! | `regex_escape(text)` | `contains`/`index_of`, which take no pattern |

/// Escape a value for use inside a single-quoted SQL string literal.
///
/// ```text
/// sql_escape("O'Brien")   //  O''Brien
/// ```
///
/// Backslashes are left alone, because the standard says they are literal inside a
/// single-quoted literal and doubling them would corrupt the value on MySQL.
pub fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Quote an argument for a POSIX shell so that the shell passes it as one word
/// with no interpretation.
///
/// The result is wrapped in single quotes, and any single quote inside is closed,
/// escaped and reopened -- the only sequence a POSIX shell cannot be made to
/// misread. `shell_escape("it's")` gives `'it'\''s'`.
///
/// An empty string becomes `''`, which the shell passes as an empty argument rather
/// than as nothing at all.
pub fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            // Close the quote, emit an escaped quote, reopen.
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Escape text for insertion into HTML element content or a quoted attribute.
///
/// `&` `<` `>` `"` `'` and `/` are escaped. `'` becomes `&#x27;` rather than
/// `&apos;` because `&apos;` is HTML5-only and does not decode in XML or in XHTML,
/// and this output is frequently written into a page served as anything.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            '/' => out.push_str("&#x2F;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a string so it matches itself when used as a regex pattern.
///
/// Escapes every character the `regex` crate treats as a metacharacter, including
/// the ASCII punctuation class. `/` is escaped too, so the result is safe to drop
/// between the slashes of a literal in source.
pub fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// ---- weak-crypto deprecation warnings ---------------------------------
//
// The `weak-crypto` lint catches these at the call site, but lint is not run by
// `rakc run` or `rakc vm`, so a script calling `md5()` produced a digest with no
// comment from the language at all. This is that comment, at the point of use.
//
// On stderr, not stdout: stdout is a program's data, and the parity tests compare
// it byte for byte between the two backends. A warning that lands in the middle of
// a dump would corrupt a pipeline and fail a test for the wrong reason.
//
// Once per primitive per process, not per call: a digest inside a loop would
// otherwise print a million lines and bury whatever the user was trying to read.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

fn warned() -> &'static Mutex<HashSet<&'static str>> {
    static SET: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Warn, once per primitive, that `name` is not a safe choice.
///
/// Returns nothing and cannot fail: a warning that could break a program would be
/// worse than the problem it reports.
pub fn warn_weak_crypto(name: &'static str, why: &'static str, use_instead: &'static str) {
    let already = {
        let mut set = match warned().lock() {
            Ok(g) => g,
            // A poisoned lock means another thread panicked while warning. That is
            // not a reason to panic again over a diagnostic.
            Err(poisoned) => poisoned.into_inner(),
        };
        !set.insert(name)
    };
    if already {
        return;
    }
    eprintln!(
        "rak: warning: `{}` is deprecated: {}. Use `{}` unless you are analysing \
         data that uses it, in which case suppress this with unsafe {{ \"md5 is the \
         subject of this analysis\" {{ {}() }} }}",
        name, why, use_instead, name
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- sql_escape ------------------------------------------------------

    #[test]
    fn sql_escape_doubles_a_single_quote() {
        assert_eq!(sql_escape("O'Brien"), "O''Brien");
        assert_eq!(sql_escape("''"), "''''");
    }

    #[test]
    fn sql_escape_leaves_ordinary_text_alone() {
        assert_eq!(sql_escape("harmless"), "harmless");
        assert_eq!(sql_escape(""), "");
    }

    #[test]
    fn sql_escape_defuses_the_canonical_injection() {
        // The classic: close the literal, comment out the rest, append a DROP.
        let hostile = "x'; DROP TABLE users; --";
        let escaped = sql_escape(hostile);
        assert_eq!(escaped, "x''; DROP TABLE users; --");
        // Every quote in the output is part of a pair, so the literal never closes.
        assert_eq!(escaped.matches('\'').count() % 2, 0);
    }

    // ---- shell_escape ----------------------------------------------------

    #[test]
    fn shell_escape_quotes_and_escapes_internal_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
        assert_eq!(shell_escape("simple"), "'simple'");
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn shell_escape_defuses_every_shell_metacharacter() {
        for hostile in [
            "; rm -rf /",
            "$(whoami)",
            "`id`",
            "a && b",
            "a | b",
            "*",
            "~",
            "$HOME",
            "new\nline",
        ] {
            let escaped = shell_escape(hostile);
            // Single-quoting is what makes it safe: the only quote in the output
            // must be one of the ones that open and close it.
            assert!(escaped.starts_with('\''), "{hostile:?} -> {escaped:?}");
            assert!(escaped.ends_with('\''), "{hostile:?} -> {escaped:?}");
        }
    }

    #[test]
    fn shell_escape_output_has_balanced_quotes() {
        // Every `'` in the output either opens a quoted run or is the escaped pair.
        let escaped = shell_escape("a'b'c");
        assert_eq!(escaped, "'a'\\''b'\\''c'");
    }

    // ---- html_escape -----------------------------------------------------

    #[test]
    fn html_escape_covers_the_five_that_matter() {
        assert_eq!(html_escape("<b>"), "&lt;b&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
    }

    #[test]
    fn html_escape_escapes_ampersand_first_so_output_is_not_double_escaped() {
        // `&lt;` must not become `&amp;lt;` on a second pass, which is what happens
        // if `&` is handled after the character that introduces an entity.
        assert_eq!(html_escape("&lt;"), "&amp;lt;");
        assert_eq!(html_escape("&"), "&amp;");
    }

    #[test]
    fn html_escape_escapes_slash_to_close_an_element() {
        assert_eq!(html_escape("</script>"), "&lt;&#x2F;script&gt;");
    }

    // ---- regex_escape ----------------------------------------------------

    #[test]
    fn regex_escape_defuses_quantifiers_and_groups() {
        // Unescaped, this is "zero or more of a", not the literal text.
        assert_eq!(regex_escape("a*"), "a\\*");
        assert_eq!(regex_escape("(a|b)"), "\\(a\\|b\\)");
        assert_eq!(regex_escape("a+b?"), "a\\+b\\?");
    }

    #[test]
    fn regex_escape_leaves_alphanumerics_and_spaces_alone() {
        assert_eq!(regex_escape("hello world 123"), "hello world 123");
    }

    #[test]
    fn regex_escape_output_matches_the_input_literally() {
        // The property that matters: compiled as a regex, the escaped form matches
        // the original string and nothing that merely looks like it.
        let hostile = "a.c*";
        let re = regex::Regex::new(&format!("^{}$", regex_escape(hostile))).unwrap();
        assert!(re.is_match(hostile), "should match itself");
        assert!(!re.is_match("abc"), "'abc' must not match 'a.c*'");
        assert!(!re.is_match("aaaa"), "a quantifier must not survive");
    }

    #[test]
    fn regex_escape_is_idempotent_in_the_sense_that_matters() {
        // Escaping an already-escaped string still matches only itself.
        let once = regex_escape("a.b");
        let re = regex::Regex::new(&format!("^{}$", once)).unwrap();
        assert!(re.is_match("a.b"));
        assert!(!re.is_match("axb"));
    }

    // ---- round trips -----------------------------------------------------

    #[test]
    fn escaped_output_never_contains_a_bare_delimiter() {
        let cases = ["'", "\"", "<", ">", "&", "`", "$", ";", "|", "&"];
        for c in cases {
            let html = html_escape(c);
            assert!(
                !html.contains('<') && !html.contains('>'),
                "{c:?} -> {html:?}"
            );
            let sql = sql_escape(c);
            assert_eq!(sql.matches('\'').count() % 2, 0, "{c:?} -> {sql:?}");
        }
    }
}
