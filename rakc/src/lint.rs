//! `rakc lint` — advisory static checks for Rak (Part 7B).
//!
//! Style rules (each reported as `warning[<rule>]: ...`):
//! - `unused-var` — a `let` binding that is never read afterwards.
//! - `shadowed` — the same name bound twice in the same block.
//! - `unreachable` — statements after a `return` in the same block.
//! - `missing-ret-type` — an exported (`pub`) `fn` without a return type.
//! - `duplicate-import` — the same module imported more than once.
//!
//! Security rules (the reason to reach for this on someone else's script):
//! - `hardcoded-secret` — a literal assigned to a name that looks like a
//!   credential, or a literal that matches a known token format.
//! - `plaintext-url` — an `http://` URL, which leaks whatever it carries.
//! - `weak-crypto` — MD5, SHA-1, ROT13, or XOR used as if they were secure.
//! - `secret-compare` — `==` between values that look secret-bearing. Use
//!   `ct_eq` / `ct_eq_hex` so the comparison does not leak by timing.
//! - `ffi-raw-pointer` — raw-pointer FFI calls. These are unchecked; run the
//!   script under `--sandbox` without `ffi`.
//! - `insecure-transport` — a WebSocket or plain TCP connection that carries
//!   no TLS.
//!
//! Advisory by default (exit 0). `--deny` exits 1 when any warning fires, for
//! CI use. Names starting with `_` silence `unused-var`. A secret-shaped name
//! in a `let` whose value is *not* a literal (a call, a variable) is fine and
//! is not reported.

use crate::ast::*;
use std::collections::HashSet;

#[derive(Debug)]
pub struct LintFinding {
    pub rule: &'static str,
    pub message: String,
}

/// The string value of a literal expression, if it is one. Used by the
/// security rules to read an argument without recursing into the whole tree.
fn literal_str(e: &Expr) -> Option<String> {
    match e {
        Expr::String(s) => Some(s.clone()),
        _ => None,
    }
}

pub fn lint_source(source: &str) -> Result<Vec<LintFinding>, String> {
    lint_source_full(source).map(|r| r.findings)
}

/// Render the `unsafe` audit as a review artifact. This is the output a
/// security reviewer reads before running someone else's script: the complete
/// list of safety exemptions the program claims, each with the justification
/// its author wrote down.
pub fn format_audit(file: &str, sites: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("\nsafety audit: {} unsafe block(s) in {}\n", sites.len(), file));
    if sites.is_empty() {
        out.push_str("  no unsafe blocks. nothing to review.\n");
        return out;
    }
    for (i, reason) in sites.iter().enumerate() {
        out.push_str(&format!("  {:>2}. {}\n", i + 1, reason));
    }
    out.push_str(
        "  each of these bypasses the safety lint rules. check the justification holds,\n  \
         then check the block does not do what the justification does not cover.\n",
    );
    out
}

/// Findings plus the audit trail: every `unsafe` block reason found, in source
/// order. `rakc lint --audit` prints the second part, which is the artifact a
/// reviewer actually wants: the complete list of safety exemptions a program
/// claims, each with its justification.
pub struct LintReport {
    pub findings: Vec<LintFinding>,
    pub unsafe_sites: Vec<String>,
}

pub fn lint_source_full(source: &str) -> Result<LintReport, String> {
    let tokens = crate::lexer::tokenize(source).map_err(|e| e.to_string())?;
    let module = crate::parser::parse(&tokens, source).map_err(|e| e.to_string())?;
    let mut l = Linter::new();
    l.module(&module);
    l.finish();
    Ok(LintReport {
        findings: l.findings,
        unsafe_sites: l.unsafe_sites,
    })
}

struct Linter {
    findings: Vec<LintFinding>,
    /// `(name, mutable)` bindings from plain `let`/`const` statements.
    declared: Vec<(String, bool)>,
    /// Top-level names this file marks `pub`/`export`. Read by other files, so
    /// they are never "unused" here.
    exported: HashSet<String>,
    /// All names ever bound (patterns included) — never "unused".
    defined: HashSet<String>,
    /// Names read anywhere in the program.
    reads: HashSet<String>,
    /// Names that appear on the left of `=`/`+=` (assigned, not just declared).
    assigned: HashSet<String>,
    saw_return: bool,
    /// Import targets seen, as (form, module) so a rom-import and a whole
    /// import of the same module are not counted as duplicates of each other.
    imported: HashSet<(&'static str, String)>,
    /// Reason strings from every `unsafe` block, for the `--audit` report.
    unsafe_sites: Vec<String>,
}

impl Linter {
    fn new() -> Self {
        Linter {
            findings: Vec::new(),
            declared: Vec::new(),
            exported: HashSet::new(),
            defined: HashSet::new(),
            reads: HashSet::new(),
            assigned: HashSet::new(),
            saw_return: false,
            imported: HashSet::new(),
            unsafe_sites: Vec::new(),
        }
    }

    fn module(&mut self, m: &Module) {
        for imp in &m.imports {
            self.import(imp);
        }
        for s in &m.items {
            self.stmt(s);
        }
        // Anything this file exports is read by whoever imports it, so it cannot
        // be unused merely because this file does not mention it. Collected after
        // the walk rather than during it, so an `export` appearing before its
        // declaration still counts. The linter parses one file at a time and has
        // no view of the importers, which is exactly why this is needed.
        for s in &m.items {
            let Stmt::Export(inner) = s else { continue };
            match inner.as_ref() {
                // `pub fn name(..)` is an exported `let` whose value is a
                // function, so this one arm covers functions and values.
                Stmt::Let { name, .. } | Stmt::Const { name, .. } => {
                    self.exported.insert(name.clone());
                }
                _ => {}
            }
        }
    }

    fn import(&mut self, imp: &Import) {
        let target = if imp.is_file {
            imp.path.first().cloned().unwrap_or_default()
        } else {
            imp.path.join(".")
        };
        // Keyed by form as well as target. `import m` and `from m import x` are
        // not a redundant pair: the first binds a live view of the module and the
        // second copies a value out of it, and a program that wants both writes
        // both. Only the same *form* twice is worth a second look — and even then
        // `import m` plus `import m as k` is a deliberate way to hold two names
        // for one module, so the warning names what it saw rather than claiming
        // the second import is pointless.
        let key = match imp.kind {
            crate::ast::ImportKind::Whole => ("whole", target.clone()),
            crate::ast::ImportKind::From => ("from", target.clone()),
        };
        if !self.imported.insert(key) {
            self.findings.push(LintFinding {
                rule: "duplicate-import",
                message: format!("module '{}' is imported more than once in the same form", target),
            });
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Unsafe { reason, body } => {
                // The parser already refuses a missing or empty reason, so by
                // the time we get here the question is whether the reason says
                // anything. A placeholder justification is worse than none,
                // because it reads as if someone had thought about it.
                self.unsafe_sites.push(reason.clone());
                let r = reason.trim();
                let placeholder = r.len() < 15
                    || ["todo", "fixme", "xxx", "because", "trust me", "hack", "n/a"]
                        .iter()
                        .any(|p| r.to_ascii_lowercase().contains(p));
                if placeholder {
                    self.findings.push(LintFinding {
                        rule: "unsafe-thin-reason",
                        message: format!(
                            "unsafe block reason is not a real justification: \"{}\". Say what invariant holds and who checked it.",
                            r
                        ),
                    });
                }
                for st in body {
                    self.stmt(st);
                }
            }
            Stmt::Let { name, value, pattern, .. } => {
                // A credential assigned directly from a literal is the classic
                // leak. Assigned from a call or a variable, it is fine.
                if pattern.is_none() && Self::is_secret_name(name) {
                    if let Expr::String(s) = value.as_ref() {
                        self.findings.push(LintFinding {
                            rule: "hardcoded-secret",
                            message: format!("'{}' is assigned a literal credential; use secret_set(\"{}\", env_get(\"...\")) instead", name, name),
                        });
                    }
                }
                match pattern {
                    Some(p) => self.binds_from_pattern(p),
                    None => {
                        self.declared.push((name.clone(), matches!(s, Stmt::Let { mutable: true, .. })));
                        self.defined.insert(name.clone());
                    }
                }
                self.expr(value);
            }
            Stmt::Const { name, value } => {
                self.declared.push((name.clone(), false));
                self.defined.insert(name.clone());
                self.expr(value);
            }
            Stmt::Expr(e) => {
                if let Expr::Assign(name, _) = e.as_ref() {
                    self.assigned.insert(name.clone());
                }
                if let Expr::IndexAssign { obj, .. } = e.as_ref() {
                    if let Expr::Ident(n) = obj.as_ref() {
                        self.assigned.insert(n.clone());
                    }
                }
                if let Expr::FieldAssign { obj, .. } = e.as_ref() {
                    if let Expr::Ident(n) = obj.as_ref() {
                        self.assigned.insert(n.clone());
                    }
                }
                if let Expr::MultiAssign { targets, .. } = e.as_ref() {
                    for t in targets {
                        if let Expr::Ident(n) = t {
                            self.assigned.insert(n.clone());
                        }
                    }
                }
                self.expr(e)
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
                self.saw_return = true;
            }
            Stmt::Dump { value, .. } => self.expr(value),
            Stmt::Trace { value } => self.expr(value),
            Stmt::Assert(e) => self.expr(e),
            Stmt::Defer(e) => self.expr(e),
            Stmt::If { cond, then_branch, else_branch } => {
                self.expr(cond);
                self.block(then_branch);
                if let Some(els) = else_branch {
                    self.block(els);
                }
            }
            Stmt::IfLet { pattern, value, then_branch, else_branch } => {
                self.binds_from_pattern(pattern);
                self.expr(value);
                self.block(then_branch);
                if let Some(els) = else_branch {
                    self.block(els);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::WhileLet { pattern, value, body } => {
                self.binds_from_pattern(pattern);
                self.expr(value);
                self.block(body);
            }
            Stmt::DoWhile { cond, body } => {
                self.block(body);
                self.expr(cond);
            }
            Stmt::Loop { body, .. } => self.block(body),
            Stmt::For { pattern, iterable, body, .. } => {
                self.binds_from_pattern(pattern);
                self.expr(iterable);
                self.block(body);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Scan { target, options, body } => {
                self.expr(target);
                for (_, e) in options {
                    self.expr(e);
                }
                if let Some(b) = body {
                    self.block(b);
                }
            }
            Stmt::Fetch { target, options, body } => {
                self.expr(target);
                for (_, e) in options {
                    self.expr(e);
                }
                if let Some(b) = body {
                    self.block(b);
                }
            }
            Stmt::Match { value, arms } => {
                self.expr(value);
                for (pattern, guard, body) in arms {
                    self.binds_from_pattern(pattern);
                    if let Some(g) = guard {
                        self.expr(g);
                    }
                    self.block(body);
                }
            }
            Stmt::Try { body, catch_name, catch_body } => {
                self.block(body);
                if let Some(n) = catch_name {
                    self.defined.insert(n.clone());
                }
                self.block(catch_body);
            }
            Stmt::Raise(e) => self.expr(e),
            Stmt::Struct { fields, .. } => {
                for f in fields {
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                }
            }
            Stmt::Enum { .. } => {}
            Stmt::Impl { methods, .. } => {
                for m in methods {
                    self.stmt(m);
                }
            }
            Stmt::Trait { .. } => {}
            Stmt::Test { body, .. } => self.block(body),
            Stmt::Mod { items, .. } => {
                // Every top-level name in a `mod` block is exported - the block is
                // already the boundary - so `counter.bump()` from outside means
                // `bump` is used, and reporting it as unused would be wrong.
                //
                // The names are read off the block's own items rather than by
                // diffing `declared` across the block. Diffing also caught
                // function-locals declared inside the block's functions, because
                // `declared` is only drained in `finish` and so never resets at a
                // function boundary - which silently stopped `unused-var`
                // working for every function in a `mod` block. It was also
                // quadratic in the file's size.
                self.block(items);
                for stmt in items {
                    // pub fn f() is an exported let whose value is a function,
                    // so both spellings have to be unwrapped.
                    let decl = match stmt {
                        Stmt::Export(inner) => inner.as_ref(),
                        other => other,
                    };
                    match decl {
                        Stmt::Let { name, .. } | Stmt::Const { name, .. } => {
                            self.exported.insert(name.clone());
                        }
                        _ => {}
                    }
                }
            }
            Stmt::Use { .. } | Stmt::TypeAlias { .. } => {}
            Stmt::Async(body) => self.block(body),
            Stmt::Export(inner) => {
                if let Stmt::Let { name, value, .. } = inner.as_ref() {
                    if let Expr::Function { return_type: None, .. } = value.as_ref() {
                        self.findings.push(LintFinding {
                            rule: "missing-ret-type",
                            message: format!("exported fn '{}' has no return type annotation", name),
                        });
                    }
                }
                self.stmt(inner);
            }
            Stmt::Extern { .. } => {}
            Stmt::MacroDef { body, .. } => self.block(body),
            Stmt::Tunnel { body, .. } => self.block(body),
            Stmt::BinStructDef { .. } => {}
        }
    }

    fn block(&mut self, stmts: &[Stmt]) {
        let mut seen: HashSet<String> = HashSet::new();
        self.saw_return = false;
        for s in stmts {
            if self.saw_return && !matches!(s, Stmt::Return(_) | Stmt::Test { .. }) {
                self.findings.push(LintFinding {
                    rule: "unreachable",
                    message: "statement is unreachable (a `return` precedes it in this block)".to_string(),
                });
                self.saw_return = false;
            }
            if let Stmt::Let { name, pattern: None, .. } = s {
                if !seen.insert(name.clone()) {
                    self.findings.push(LintFinding {
                        rule: "shadowed",
                        message: format!("'{}' is bound again in the same block", name),
                    });
                }
            }
            self.stmt(s);
        }
        self.saw_return = false;
    }

    /// Record pattern-bound names as definitions (never "unused").
    fn binds_from_pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Wild => {}
            Pattern::Ident(n) => {
                self.defined.insert(n.clone());
            }
            Pattern::Tuple(ps) | Pattern::Array(ps) | Pattern::Or(ps) => {
                for x in ps {
                    self.binds_from_pattern(x);
                }
            }
            Pattern::Struct(_, fields) => {
                for (_, sub) in fields {
                    self.binds_from_pattern(sub);
                }
            }
            Pattern::EnumVariant(_, _, subs) => {
                for x in subs {
                    self.binds_from_pattern(x);
                }
            }
            Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => self.binds_from_pattern(i),
            Pattern::Range(lo, hi) => {
                self.binds_from_pattern(lo);
                self.binds_from_pattern(hi);
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------
    // Security rules
    // -----------------------------------------------------------------

    /// True when a binding name reads like it holds a credential. Deliberately
    /// broad: a false positive costs one `let _ =` rename, a false negative
    /// ships a live key to a repository.
    fn is_secret_name(name: &str) -> bool {
        let n = name.to_ascii_lowercase();
        const NEEDLES: &[&str] = &[
            "secret", "password", "passwd", "pwd", "token", "apikey", "api_key",
            "credential", "cred", "private_key", "privatekey", "priv_key",
            "passphrase", "auth", "session_key", "signing_key", "client_secret",
        ];
        NEEDLES.iter().any(|k| n.contains(k))
    }

    /// True when an expression plausibly yields a secret: a secret-named
    /// identifier, or a call to one of the key-producing builtins.
    fn looks_secret_expr(e: &Expr) -> bool {
        match e {
            Expr::Ident(n) => Self::is_secret_name(n),
            Expr::FieldAccess(o, f) => Self::is_secret_name(f) || Self::looks_secret_expr(o),
            Expr::Index(o, i) => {
                Self::is_secret_name(&literal_str(i).unwrap_or_default()) || Self::looks_secret_expr(o)
            }
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::Ident(n) => matches!(
                    n.as_str(),
                    "secret_get" | "hmac_sha256" | "ed25519_keypair" | "rsa_keypair"
                        | "ecdsa_keypair" | "x25519_keypair" | "tunnel_preshared_key"
                        | "psk_derive" | "hkdf_derive" | "sha256" | "md5"
                ),
                _ => false,
            },
            _ => false,
        }
    }

    /// Inspect one string literal for a hardcoded credential or a plaintext
    /// URL. Called from the `Expr::String` arm.
    fn check_string_literal(&mut self, s: &str) {
        if let Some(rest) = s.strip_prefix("http://") {
            // localhost and bare IPs are fine in a dev script.
            let host = rest.split('/').next().unwrap_or("");
            if !host.is_empty() && !host.starts_with("localhost") && !host.starts_with("127.") {
                self.findings.push(LintFinding {
                    rule: "plaintext-url",
                    message: format!("'http://{}' sends in the clear; use https:// unless this is localhost", host),
                });
            }
        }
        if Self::looks_like_token(s) {
            self.findings.push(LintFinding {
                rule: "hardcoded-secret",
                message: format!("literal looks like a credential ({} chars); move it to secret_set / an env var", s.len()),
            });
        }
    }

    /// Heuristic credential detection on a literal. Requires an explicit
    /// vendor prefix or a high-entropy body, so ordinary long strings (paths,
    /// SQL, HTML) do not trip it.
    fn looks_like_token(s: &str) -> bool {
        const PREFIXES: &[&str] = &[
            "sk-", "sk_live_", "sk_test_", "pk_live_", "ghp_", "gho_", "github_pat_",
            "xoxb-", "xoxp-", "xoxa-", "AKIA", "ASIA", "AIza", "ya29.", "eyJ",
            "-----BEGIN", "ssh-rsa", "ssh-ed25519",
        ];
        if PREFIXES.iter().any(|p| s.starts_with(p)) {
            return true;
        }
        // Assignment form inside a string, e.g. an .env line pasted inline.
        for sep in ["password=", "passwd=", "secret=", "token=", "api_key=", "apikey="] {
            if let Some(i) = s.to_ascii_lowercase().find(sep) {
                let tail = &s[i + sep.len()..];
                if tail.len() >= 8 && tail.chars().all(|c| !c.is_whitespace()) {
                    return true;
                }
            }
        }
        // High-entropy hex of a key-like length: 32, 40, 64, or 128 nibbles.
        if matches!(s.len(), 32 | 40 | 64 | 128) && s.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
        false
    }

    /// Flag weak primitives and raw-pointer FFI at the call site.
    fn check_builtin_call(&mut self, callee: &Expr, args: &[Expr]) {
        let Expr::Ident(name) = callee else { return };        let weak = match name.as_str() {
            "md5" => Some("MD5 is broken for anything adversarial; use sha256"),
            "sha1" => Some("SHA-1 is broken for collision resistance; use sha256"),
            "rot13" => Some("ROT13 is an encoding, not encryption"),
            "xor" => Some("repeating-key XOR is not a cipher; use aes_gcm_encrypt or chacha20_encrypt"),
            "file_hash" => {
                // Signature is file_hash(path, algorithm); the algorithm is
                // the second argument.
                let alg = args.get(1).and_then(literal_str).unwrap_or_default();
                match alg.as_str() {
                    "md5" => Some("MD5 is broken for anything adversarial; use sha256"),
                    "sha1" => Some("SHA-1 is broken for collision resistance; use sha256"),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(msg) = weak {
            self.findings.push(LintFinding {
                rule: "weak-crypto",
                message: format!("{}: {}", name, msg),
            });
        }
        // Raw-pointer FFI is unchecked memory access. Name it explicitly so a
        // reviewer can find it, and point at the sandbox switch.
        if name.starts_with("ffi_") && !matches!(name.as_str(), "ffi_load" | "ffi_cstr_to_string" | "ffi_string_to_cstr") {
            self.findings.push(LintFinding {
                rule: "ffi-raw-pointer",
                message: format!("{}() is unchecked memory access; run with --sandbox and without the ffi capability, or review the pointer arithmetic", name),
            });
        }
          // A ws_ or bare tcp_ connection carries no TLS.
          if name == "ws_connect" || name == "net_connect" {
              self.findings.push(LintFinding {
                  rule: "insecure-transport",
                  message: format!("{}() opens an unencrypted connection; anything sent over it is readable in transit", name),
              });
          }
          // Inline assembly, under its own rule rather than `weak-crypto` or
          // `ffi-raw-pointer`, because it is a distinct kind of finding: those
          // two are about choosing a weak primitive or unchecked memory, while
          // this is about the sandbox not being able to see the call at all.
          if name == "asm" {
              self.findings.push(LintFinding {
                  rule: "inline-asm",
                  message: format!(
                      "{}() is not mediated by the capability sandbox: the sandbox \
                       gates builtins, and the machine executes the instruction \
                       directly. It is behind the `asm` capability and requires an \
                       `unsafe` block, so treat every use as a review boundary",
                      name
                  ),
              });
          }
      }

    fn expr(&mut self, e: &Expr) {
        match e {
            // String literals used to fall through the `_ => {}` arm, so the
            // linter never looked at a single byte of user text. Every
            // security rule below depends on this arm existing.
            Expr::String(s) => self.check_string_literal(s),
            Expr::Ident(n) => {
                self.reads.insert(n.clone());
            }
            Expr::Binary(op, l, r) => {
                // `==` between two secret-looking values leaks the length of
                // the common prefix through its exit timing.
                if matches!(op, BinOp::Eq | BinOp::NotEq) {
                    if Self::looks_secret_expr(l) || Self::looks_secret_expr(r) {
                        self.findings.push(LintFinding {
                            rule: "secret-compare",
                            message: "comparing a secret-looking value with == leaks its common-prefix length through timing; use ct_eq / ct_eq_hex".to_string(),
                        });
                    }
                }
                self.expr(l);
                self.expr(r);
            }
            Expr::Assign(name, value) => {
                self.assigned.insert(name.clone());
                self.expr(value);
            }
            Expr::CompoundAssign(_, name, value) => {
                self.assigned.insert(name.clone());
                self.expr(value);
            }
            Expr::Ternary { cond, then, els } => {
                self.expr(cond);
                self.expr(then);
                self.expr(els);
            }
            Expr::Unary(_, i)
            | Expr::TryExpr(i)
            | Expr::Await(i)
            | Expr::Spawn(i)
            | Expr::Raise(i)
            | Expr::As(i, _) => self.expr(i),
            Expr::NilCoalesce(l, r) => {
                self.expr(l);
                self.expr(r);
            }
            Expr::Index(o, i) | Expr::OptIndex(o, i) => {
                self.expr(o);
                self.expr(i);
            }
            Expr::OptField(o, _) | Expr::FieldAccess(o, _) => self.expr(o),
            Expr::FieldAssign { obj, value, .. } => {
                self.expr(obj);
                self.expr(value);
            }
            Expr::IndexAssign { obj, idx, value } => {
                self.expr(obj);
                self.expr(idx);
                self.expr(value);
            }
            Expr::MultiAssign { targets, values } => {
                for t in targets {
                    if let Expr::Ident(n) = t {
                        self.assigned.insert(n.clone());
                    }
                    self.expr(t);
                }
                for v in values {
                    self.expr(v);
                }
            }
            Expr::Call { callee, args, named } => {
                self.check_builtin_call(callee, args);
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
                for (_, v) in named {
                    self.expr(v);
                }
            }
            Expr::Array(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::Tuple(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::Map(pairs) => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            Expr::Function { params, body, .. } => {
                for p in params {
                    if let Some(d) = &p.default {
                        self.expr(d);
                    }
                }
                self.block(body);
            }
            Expr::Lambda { params, body, .. } => {
                for p in params {
                    if let Some(d) = &p.default {
                        self.expr(d);
                    }
                }
                self.expr(body);
            }
            Expr::If { cond, then_branch, else_branch } => {
                self.expr(cond);
                self.block(then_branch);
                if let Some(els) = else_branch {
                    self.block(els);
                }
            }
            Expr::Match { value, arms } => {
                self.expr(value);
                for (pattern, guard, body) in arms {
                    self.binds_from_pattern(pattern);
                    if let Some(g) = guard {
                        self.expr(g);
                    }
                    self.block(body);
                }
            }
            Expr::Block(stmts) => self.block(stmts),
            Expr::Comprehension { iterable, cond, elem, value, .. } => {
                self.expr(iterable);
                self.expr(elem);
                if let Some(c) = cond {
                    self.expr(c);
                }
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            Expr::MacroInvoke { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            Expr::EvidenceFrom { value } => self.expr(value),
            _ => {}
        }
    }

    /// Report `let` bindings that were never read.
    fn finish(&mut self) {
        let declared = std::mem::take(&mut self.declared);
        for (name, _mutable) in declared {
            if name.starts_with('_') {
                continue; // convention: `_name` silences the lint
            }
            if self.exported.contains(&name) {
                continue; // read by the files that import this one
            }
            if !self.reads.contains(&name) {
                self.findings.push(LintFinding {
                    rule: "unused-var",
                    message: format!("variable '{}' is never read", name),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_flags_unused_variable() {
        let findings = lint_source("let x = 10\ndump 1").unwrap();
        assert!(findings.iter().any(|f| f.rule == "unused-var" && f.message.contains("'x'")));
    }

    #[test]
    fn lint_accepts_read_variable() {
        let findings = lint_source("let x = 10\ndump x").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "unused-var"));
    }

    #[test]
    fn lint_underscore_silences() {
        let findings = lint_source("let _ignored = 10\ndump 1").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "unused-var"));
    }

    #[test]
    fn lint_flags_unreachable_after_return() {
        let findings = lint_source("fn f() {\n    return 1\n    dump 2\n}\ndump f()").unwrap();
        assert!(findings.iter().any(|f| f.rule == "unreachable"));
    }

    #[test]
    fn lint_flags_shadowed_binding() {
        let findings = lint_source("fn f() {\n    let a = 1\n    let a = 2\n    dump a\n}\ndump f()").unwrap();
        assert!(findings.iter().any(|f| f.rule == "shadowed" && f.message.contains("'a'")));
    }

    #[test]
    fn lint_flags_missing_pub_ret_type() {
        let findings = lint_source("pub fn go() {\n    return 1\n}\ndump go()").unwrap();
        assert!(findings.iter().any(|f| f.rule == "missing-ret-type"));
    }

    // --- security rules ---

    #[test]
    fn flags_hardcoded_secret_by_binding_name() {
        let findings =
            lint_source("let api_key = \"hunter2hunter2\"\ndump api_key").unwrap();
        assert!(
            findings.iter().any(|f| f.rule == "hardcoded-secret"),
            "expected hardcoded-secret, got {:?}",
            findings
        );
    }

    #[test]
    fn does_not_flag_secret_bound_from_a_call() {
        // secret_set is the correct way to do this.
        let findings =
            lint_source("let api_key = secret_get(\"K\")\ndump api_key").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "hardcoded-secret"));
    }

    #[test]
    fn flags_known_token_formats_anywhere() {
        for lit in [
            "\"sk-live-abc123def456\"",
            "\"ghp_0123456789abcdef0123\"",
            "\"AKIAIOSFODNN7EXAMPLE\"",
        ] {
            let src = format!("let x = {}\ndump x", lit);
            let findings = lint_source(&src).unwrap();
            assert!(
                findings.iter().any(|f| f.rule == "hardcoded-secret"),
                "expected hardcoded-secret for {}, got {:?}",
                lit,
                findings
            );
        }
    }

    #[test]
    fn flags_key_length_hex_without_a_secret_name() {
        // 64 hex chars is a plausible key length; the name gives it away.
        let findings = lint_source("let blob = \"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff\"\ndump blob").unwrap();
        assert!(findings.iter().any(|f| f.rule == "hardcoded-secret"));
    }

    #[test]
    fn does_not_flag_ordinary_long_strings() {
        // A long path or HTML fragment must not trip the entropy heuristic.
        for lit in [
            "\"/usr/local/share/some/deeply/nested/directory/path/file.txt\"",
            "\"<!DOCTYPE html><html><body><h1>hello</h1></body></html>\"",
            "\"SELECT id, name, created_at FROM users WHERE active = 1\"",
        ] {
            let src = format!("let page = {}\ndump page", lit);
            let findings = lint_source(&src).unwrap();
            assert!(
                !findings.iter().any(|f| f.rule == "hardcoded-secret"),
                "false positive for {}: {:?}",
                lit,
                findings
            );
        }
    }

    #[test]
    fn flags_plaintext_url_but_not_localhost() {
        let bad = lint_source("let u = \"http://example.com/x\"\ndump u").unwrap();
        assert!(bad.iter().any(|f| f.rule == "plaintext-url"));
        for ok in ["\"http://localhost:8080/x\"", "\"http://127.0.0.1:9000\""] {
            let src = format!("let u = {}\ndump u", ok);
            let findings = lint_source(&src).unwrap();
            assert!(!findings.iter().any(|f| f.rule == "plaintext-url"), "false positive for {}", ok);
        }
    }

    #[test]
    fn flags_weak_crypto() {
        for call in ["md5(\"x\")", "sha1(\"x\")", "rot13(\"x\")", "xor(b\"a\", b\"b\")"] {
            let src = format!("let h = {}\ndump h", call);
            let findings = lint_source(&src).unwrap();
            assert!(
                findings.iter().any(|f| f.rule == "weak-crypto"),
                "expected weak-crypto for {}, got {:?}",
                call,
                findings
            );
        }
    }

    #[test]
    fn flags_weak_hash_via_file_hash_argument() {
        let findings = lint_source("let h = file_hash(\"f.bin\", \"md5\")\ndump h").unwrap();
        assert!(findings.iter().any(|f| f.rule == "weak-crypto"), "{:?}", findings);
    }
    #[test]
    fn does_not_flag_strong_crypto() {
        let findings =
            lint_source("let h = sha256(\"x\")\ndump hmac_sha256(\"k\", \"m\")\ndump h").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "weak-crypto"));
    }

    #[test]
    fn flags_secret_comparison_and_not_constant_time() {
        // The rule keys off the *name*, so the bindings have to look
        // secret-bearing for it to say anything.
        let bad = lint_source(
            "let token_a = secret_get(\"k\")\nlet token_b = secret_get(\"j\")\nif token_a == token_b { dump 1 }",
        )
        .unwrap();
        assert!(bad.iter().any(|f| f.rule == "secret-compare"), "{:?}", bad);
    }

    #[test]
    fn does_not_flag_comparison_of_ordinary_values() {
        let findings = lint_source("let a = 1\nlet b = 2\nif a == b { dump 1 }").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "secret-compare"));
    }

    #[test]
    fn flags_ffi_raw_pointer_calls() {
        let findings = lint_source("let p = ffi_load(\"m\")\nlet q = ffi_ptr(p, 0)\ndump q").unwrap();
        assert!(findings.iter().any(|f| f.rule == "ffi-raw-pointer"));
    }

    #[test]
    fn does_not_flag_ffi_load_itself() {
        // ffi_load only names a library; it does not dereference anything.
        let findings = lint_source("let p = ffi_load(\"m\")\ndump p").unwrap();
        assert!(!findings.iter().any(|f| f.rule == "ffi-raw-pointer"));
    }

    #[test]
    fn flags_unencrypted_transport() {
        let findings = lint_source("let s = ws_connect(\"host\", 80)\ndump s").unwrap();
        assert!(findings.iter().any(|f| f.rule == "insecure-transport"));
    }

    #[test]
    fn string_literals_are_actually_inspected() {
        // Regression guard for the original bug: `Expr::String` fell through
        // to `_ => {}`, so no rule could ever see a literal.
        let findings = lint_source("let x = \"http://evil.test/a\"\ndump x").unwrap();
        assert!(findings.iter().any(|f| f.rule == "plaintext-url"));
    }

    // --- unsafe blocks ---

    #[test]
    fn unsafe_requires_a_reason_at_parse_time() {
        // The point of the construct is that every exemption is justified, so
        // this has to be a parse error, not a lint warning.
        let err = crate::parser::parse(
            &crate::lexer::tokenize("unsafe {\n dump 1\n}").unwrap(),
            "unsafe {\n dump 1\n}",
        )
        .unwrap_err();
        assert!(err.to_string().contains("requires a reason"), "{}", err);
    }

    #[test]
    fn unsafe_rejects_an_empty_reason() {
        let src = "unsafe \"\" {\n dump 1\n}";
        let err = crate::parser::parse(&crate::lexer::tokenize(src).unwrap(), src).unwrap_err();
        assert!(err.to_string().contains("cannot be empty"), "{}", err);
    }

    #[test]
    fn unsafe_block_executes_its_body_normally() {
        // An unsafe block gets no special runtime treatment: normal scoping,
        // normal mutability enforcement, normal control flow. If it quietly
        // relaxed those, the marker would be hiding real behaviour.
        let out = crate::eval(
            "unsafe \"a real justification for this block\" {\n let mut x = 41\n x = x + 1\n dump x\n}",
        )
        .unwrap();
        assert!(out.iter().any(|l| l.contains("42")), "{:?}", out);

        // ...and mutability is still enforced inside it.
        let err = crate::eval(
            "unsafe \"a real justification for this block\" {\n let y = 1\n y = 2\n}",
        )
        .unwrap_err();
        assert!(err.to_string().contains("immutable"), "{}", err);
    }

    #[test]
    fn audit_lists_every_unsafe_block_with_its_reason() {
        let src = "unsafe \"first reason that is long enough\" {\n dump 1\n}\nunsafe \"second reason that is also long\" {\n dump 2\n}";
        let report = lint_source_full(src).unwrap();
        assert_eq!(report.unsafe_sites.len(), 2);
        assert!(report.unsafe_sites[0].contains("first"));
        assert!(report.unsafe_sites[1].contains("second"));
    }

    #[test]
    fn audit_finds_unsafe_blocks_nested_inside_functions() {
        let src = "fn f() {\n unsafe \"nested reason long enough here\" {\n dump 1\n }\n}\ndump f()";
        let report = lint_source_full(src).unwrap();
        assert_eq!(report.unsafe_sites.len(), 1, "{:?}", report.unsafe_sites);
    }

    #[test]
    fn thin_reason_is_flagged() {
        for reason in ["TODO", "because", "trust me", "hack", "short"] {
            let src = format!("unsafe \"{}\" {{\n dump 1\n}}", reason);
            let report = lint_source_full(&src).unwrap();
            assert!(
                report.findings.iter().any(|f| f.rule == "unsafe-thin-reason"),
                "expected a thin-reason finding for {:?}, got {:?}",
                reason,
                report.findings
            );
        }
    }

    #[test]
    fn a_real_justification_is_not_flagged() {
        let src = "unsafe \"the binstruct decoder already bounds-checked this length\" {\n dump 1\n}";
        let report = lint_source_full(src).unwrap();
        assert!(
            !report.findings.iter().any(|f| f.rule == "unsafe-thin-reason"),
            "{:?}",
            report.findings
        );
        assert_eq!(report.unsafe_sites.len(), 1);
    }

    #[test]
    fn audit_formatter_handles_the_empty_case() {
        let out = format_audit("x.rak", &[]);
        assert!(out.contains("nothing to review"), "{}", out);
    }
}
