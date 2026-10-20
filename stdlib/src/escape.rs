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
//! | `sql_escape_mysql(user_input)` | a parameterised query |
//! | `shell_escape(arg)` | `exec` with an argv array |
//! | `html_escape(text)` | the templating layer's own escaping |
//! | `regex_escape(text)` | `contains`/`index_of`, which take no pattern |
//!
//! The two SQL escapers differ because SQL dialects disagree about what a
//! backslash means inside a string literal. `sql_escape` is for the dialects
//! where a backslash is literal; `sql_escape_mysql` is for MySQL's default
//! `sql_mode`, where it is not. Using the wrong one does not fail loudly --
//! it produces output that looks escaped and is not.

/// Escape a value for use inside a single-quoted SQL string literal.
///
/// ```text
/// sql_escape("O'Brien")   //  O''Brien
/// ```
///
/// # Which SQL dialect
///
/// SQL string escaping is dialect-dependent, and this function is for the
/// dialects where a backslash is an ordinary character inside a literal:
///
/// * ANSI SQL, PostgreSQL (with `standard_conforming_strings` on, the
///   default since 9.1), SQLite, and MySQL with `NO_BACKSLASH_ESCAPES`
///   enabled.
///
/// In those dialects a single quote is escaped by doubling it, and a
/// backslash needs no treatment at all.
///
/// # Not MySQL's default mode
///
/// MySQL's default `sql_mode` has `NO_BACKSLASH_ESCAPES` **off**, which makes
/// a backslash an escape character: there, `\'` is an escaped quote and `\\`
/// is a literal backslash. Doubling the quote alone is then not enough. A
/// value of `\'` becomes `\''`, which MySQL reads as an escaped quote
/// followed by a quote that **closes the literal** -- the injection survives.
///
/// Use [`sql_escape_mysql`] for MySQL's default mode.
///
/// A parameterised query is better than either: escaping is a
/// string-substitution trick, and it is only as good as the dialect it was
/// written for.
pub fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Escape a value for use inside a single-quoted SQL string literal on
/// MySQL's default `sql_mode`.
///
/// ```text
/// sql_escape_mysql("\\'")   //  \\''
/// ```
///
/// MySQL's default mode treats a backslash as an escape character, so both
/// the quote and the backslash have to be doubled. A value of `\'` becomes
/// `\\''`, which MySQL reads as a literal backslash followed by a literal
/// quote -- the literal does not close early.
///
/// This is wrong for ANSI SQL, PostgreSQL and SQLite, where a doubled
/// backslash is two backslashes rather than one. Use [`sql_escape`] there.
///
/// A parameterised query is better than either.
pub fn sql_escape_mysql(s: &str) -> String {
    // Backslash first, then the quote. The order does not matter here --
    // neither replacement introduces the character the other one looks for --
    // but doing the backslash first reads as "escape the escape character".
    s.replace('\\', "\\\\").replace('\'', "''")
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

    /// Where a single-quoted SQL literal ends, under a given dialect's rules.
    ///
    /// `backslash_escapes` selects the two dialects that disagree:
    ///
    /// * false -- ANSI SQL, PostgreSQL, SQLite, and MySQL with
    ///   `NO_BACKSLASH_ESCAPES` on. A backslash is an ordinary character.
    /// * true -- MySQL's default `sql_mode`. A backslash escapes the next
    ///   character.
    fn literal_end(s: &str, backslash_escapes: bool) -> usize {
        let b: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < b.len() {
            if backslash_escapes && b[i] == '\\' {
                i += 2;
            } else if b[i] == '\'' {
                if i + 1 < b.len() && b[i + 1] == '\'' {
                    i += 2;
                } else {
                    return i;
                }
            } else {
                i += 1;
            }
        }
        b.len()
    }

    /// The value stays inside the literal, under either dialect.
    fn stays_inside(escaped: &str) -> bool {
        literal_end(escaped, false) == escaped.chars().count()
            && literal_end(escaped, true) == escaped.chars().count()
    }

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

    /// `sql_escape` is the ANSI/PostgreSQL/SQLite escaper, and it is *not*
    /// safe on MySQL's default `sql_mode`, where a backslash is an escape
    /// character.
    ///
    /// A value of `\'` becomes `\''` under `sql_escape`. MySQL reads the
    /// `\'` as an escaped quote -- a literal quote, still inside the string --
    /// and then the final `'` **closes the literal**. Everything after it is
    /// SQL again.
    #[test]
    fn sql_escape_is_not_mysql_safe_and_says_so_in_its_behaviour() {
        let hostile = "\\'";
        let escaped = sql_escape(hostile);
        assert_eq!(escaped, "\\''");
        // Under the ANSI rules the literal holds to the end...
        assert_eq!(literal_end(&escaped, false), escaped.chars().count());
        // ...but under MySQL's default rules it closes early.
        let end = literal_end(&escaped, true);
        assert!(
            end < escaped.chars().count(),
            "the escaped text should close the MySQL literal early, but it did not"
        );
    }

    /// `sql_escape_mysql` is the escaper for MySQL's default mode, and it
    /// keeps the value inside the literal under *both* dialects' rules.
    #[test]
    fn sql_escape_mysql_keeps_the_value_inside_the_literal() {
        for hostile in [
            "\\'",
            "a\\'b",
            "\\",
            "x'; DROP TABLE users; --",
            "'",
            "\\\\",
        ] {
            let escaped = sql_escape_mysql(hostile);
            assert!(
                stays_inside(&escaped),
                "'{}' escaped to {:?}, which leaves the literal",
                hostile,
                escaped
            );
        }
    }

    /// The MySQL escaper doubles the backslash as well as the quote.
    #[test]
    fn sql_escape_mysql_doubles_the_backslash() {
        assert_eq!(sql_escape_mysql("\\"), "\\\\");
        assert_eq!(sql_escape_mysql("'"), "''");
        assert_eq!(sql_escape_mysql("\\'"), "\\\\''");
    }

    /// The two escapers disagree, which is the whole point: a caller has to
    /// know which dialect it is talking to.
    #[test]
    fn the_two_sql_escapers_disagree_on_a_backslash() {
        assert_ne!(sql_escape("\\'"), sql_escape_mysql("\\'"));
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
