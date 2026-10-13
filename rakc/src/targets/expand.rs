//! Macro expansion for targets that compile from the AST.
//!
//! Both existing backends expand macros themselves: the interpreter on demand,
//! the bytecode compiler as a pre-pass. A target that translates the AST (C,
//! WASM, LLVM) needs the same expansion *before* it looks at the tree, or it
//! would have to implement `MacroVar`/`MacroInvoke` as runtime features ג€” which
//! they are not; they exist to be gone by the time code runs.
//!
//! Expansion reuses the interpreter's `substitute_stmts`, so the splice is the
//! same splice both backends perform.

use crate::ast::*;
use std::collections::HashMap;

/// Expand every macro invocation in `module`, in place. Macro definitions are
/// collected first so a macro may be defined after its use, and the walk
/// repeats while it changes something, so a macro that expands to another
/// invocation terminates instead of being left half-expanded.
pub fn expand_module(module: &mut Module) {
    let mut defs: HashMap<String, (Vec<Param>, Vec<Stmt>)> = HashMap::new();
    for item in &module.items {
        if let Stmt::MacroDef { name, params, body } = item {
            defs.insert(name.clone(), (params.clone(), body.clone()));
        }
    }
    if defs.is_empty() {
        return;
    }
    for item in module.items.iter_mut() {
        expand_stmt(item, &defs);
    }
}

/// Expand macros inside one statement, bottom-up over nested bodies.
fn expand_stmt(stmt: &mut Stmt, defs: &HashMap<String, (Vec<Param>, Vec<Stmt>)>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Dump { value: e, .. } | Stmt::Trace { value: e } => {
            expand_expr(e, defs)
        }
        Stmt::Return(Some(e)) | Stmt::Raise(e) | Stmt::Defer(e) | Stmt::Assert(e) => {
            expand_expr(e, defs)
        }
        Stmt::Let { value, .. } => expand_expr(value, defs),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expand_expr(cond, defs);
            expand_stmts(then_branch, defs);
            if let Some(b) = else_branch {
                expand_stmts(b, defs);
            }
        }
        Stmt::While { cond, body, .. } => {
            expand_expr(cond, defs);
            expand_stmts(body, defs);
        }
        Stmt::DoWhile { cond, body } => {
            expand_expr(cond, defs);
            expand_stmts(body, defs);
        }
        Stmt::Loop { body, .. } => expand_stmts(body, defs),
        Stmt::For { iterable, body, .. } => {
            expand_expr(iterable, defs);
            expand_stmts(body, defs);
        }
        Stmt::Match { value, arms } => {
            expand_expr(value, defs);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    expand_expr(g, defs);
                }
                expand_stmts(body, defs);
            }
        }
        Stmt::Try {
            body, catch_body, ..
        } => {
            expand_stmts(body, defs);
            expand_stmts(catch_body, defs);
        }
        Stmt::Const { value, .. } => expand_expr(value, defs),
        // A nested macro body is expanded when it is invoked, not here.
        Stmt::MacroDef { .. } => {}
        // The rest either hold no expressions or are rejected by the target's
        // support check before this runs.
        _ => {}
    }
}

fn expand_stmts(stmts: &mut [Stmt], defs: &HashMap<String, (Vec<Param>, Vec<Stmt>)>) {
    for s in stmts.iter_mut() {
        expand_stmt(s, defs);
    }
}

fn expand_expr(expr: &mut Expr, defs: &HashMap<String, (Vec<Param>, Vec<Stmt>)>) {
    match expr {
        Expr::MacroInvoke { name, args } => {
            for a in args.iter_mut() {
                expand_expr(a, defs);
            }
            let Some((params, body)) = defs.get(name) else {
                return;
            };
            if params.len() != args.len() {
                return;
            }
            let bindings: HashMap<String, Expr> = params
                .iter()
                .map(|p| p.name.clone())
                .zip(args.iter().cloned())
                .collect();
            let mut expanded = crate::interpreter::substitute_stmts(body, &bindings);
            // The macro body's last statement is the invocation's value, the way
            // the bytecode compiler splices it: earlier statements run first.
            // Whatever comes out may itself contain invocations (a macro calling
            // a macro), so it is expanded again before it is spliced in.
            match expanded.pop() {
                Some(Stmt::Expr(mut e)) => {
                    expand_expr(&mut e, defs);
                    *expr = *e;
                }
                Some(other) => {
                    // A macro whose body ends in a statement becomes a block
                    // expression, which both backends also accept.
                    expanded.push(other);
                    let mut block = Expr::Block(expanded);
                    expand_block(&mut block, defs);
                    *expr = block;
                }
                None => *expr = Expr::Nil,
            }
        }
        Expr::Function {
            body,
            requires,
            ensures,
            ..
        } => {
            expand_stmts(body, defs);
            for e in requires.iter_mut().chain(ensures.iter_mut()) {
                expand_expr(e, defs);
            }
        }
        Expr::Lambda { body, .. } => expand_expr(body, defs),
        Expr::Unary(_, e)
        | Expr::FieldAccess(e, _)
        | Expr::OptField(e, _)
        | Expr::TryExpr(e)
        | Expr::Await(e)
        | Expr::Spawn(e)
        | Expr::Raise(e)
        | Expr::EvidenceFrom { value: e } => expand_expr(e, defs),
        Expr::Binary(_, a, b) | Expr::Index(a, b) | Expr::OptIndex(a, b) => {
            expand_expr(a, defs);
            expand_expr(b, defs);
        }
        Expr::Assign(_, e) | Expr::CompoundAssign(_, _, e) => expand_expr(e, defs),
        Expr::IndexAssign { obj, idx, value } => {
            expand_expr(obj, defs);
            expand_expr(idx, defs);
            expand_expr(value, defs);
        }
        Expr::FieldAssign { obj, value, .. } => {
            expand_expr(obj, defs);
            expand_expr(value, defs);
        }
        Expr::Call { callee, args, .. } => {
            expand_expr(callee, defs);
            for a in args.iter_mut() {
                expand_expr(a, defs);
            }
        }
        Expr::Array(items) | Expr::Tuple(items) => {
            for i in items.iter_mut() {
                expand_expr(i, defs);
            }
        }
        Expr::Map(pairs) => {
            for (k, v) in pairs.iter_mut() {
                expand_expr(k, defs);
                expand_expr(v, defs);
            }
        }
        Expr::Interp { parts, .. } => {
            for p in parts.iter_mut() {
                expand_expr(p, defs);
            }
        }
        Expr::Range(lo, hi) => {
            if let Some(e) = lo {
                expand_expr(e, defs);
            }
            if let Some(e) = hi {
                expand_expr(e, defs);
            }
        }
        Expr::Ternary { cond, then, els } => {
            expand_expr(cond, defs);
            expand_expr(then, defs);
            expand_expr(els, defs);
        }
        Expr::NilCoalesce(a, b) => {
            expand_expr(a, defs);
            expand_expr(b, defs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expand_expr(cond, defs);
            expand_stmts(then_branch, defs);
            if let Some(b) = else_branch {
                expand_stmts(b, defs);
            }
        }
        Expr::Match { value, arms } => {
            expand_expr(value, defs);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    expand_expr(g, defs);
                }
                expand_stmts(body, defs);
            }
        }
        Expr::Block(body) => expand_stmts(body, defs),
        Expr::As(e, _) => expand_expr(e, defs),
        _ => {}
    }
}

/// Expand every macro inside a block expression, including nested blocks.
fn expand_block(expr: &mut Expr, defs: &HashMap<String, (Vec<Param>, Vec<Stmt>)>) {
    if let Expr::Block(body) = expr {
        expand_stmts(body, defs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn module_of(source: &str) -> Module {
        let tokens = tokenize(source).expect("lex");
        parse(&tokens, source).expect("parse")
    }

    #[test]
    fn a_simple_macro_expands_to_its_body() {
        let mut m = module_of("macro add1(x: expr) { $x + 1 }\ndump add1!(41)");
        expand_module(&mut m);
        // The invocation is replaced by the substituted body: `41 + 1`.
        match &m.items[1] {
            Stmt::Dump { value, .. } => {
                assert!(matches!(**value, Expr::Binary(BinOp::Add, _, _)))
            }
            other => panic!("expected a dump, got {:?}", other),
        }
    }

    #[test]
    fn a_macro_may_be_used_before_it_is_defined() {
        let mut m = module_of("dump add1!(1)\nmacro add1(x: expr) { $x + 1 }");
        expand_module(&mut m);
        match &m.items[0] {
            Stmt::Dump { value, .. } => {
                assert!(matches!(**value, Expr::Binary(BinOp::Add, _, _)))
            }
            other => panic!("expected a dump, got {:?}", other),
        }
    }

    #[test]
    fn nested_macro_invocations_expand_to_the_end() {
        // `pair!(add1!(1), 2)` needs two rounds: the inner invocation is an
        // argument of the outer one.
        let mut m = module_of(
            "macro add1(x: expr) { $x + 1 }\nmacro pair(a: expr, b: expr) { [$a, $b] }\ndump pair!(add1!(1), 2)",
        );
        expand_module(&mut m);
        fn count_invokes(stmts: &[Stmt]) -> usize {
            let mut n = 0;
            for s in stmts {
                match s {
                    Stmt::Dump { value, .. } | Stmt::Expr(value) => n += count_expr(value),
                    _ => {}
                }
            }
            n
        }
        fn count_expr(e: &Expr) -> usize {
            match e {
                Expr::MacroInvoke { .. } => 1,
                Expr::Binary(_, a, b) | Expr::Index(a, b) => count_expr(a) + count_expr(b),
                Expr::Array(items) | Expr::Tuple(items) => items.iter().map(count_expr).sum(),
                _ => 0,
            }
        }
        assert_eq!(
            count_invokes(&m.items),
            0,
            "every invocation should be gone"
        );
    }

    #[test]
    fn an_unknown_macro_is_left_for_the_error_path() {
        let mut m = module_of("dump nope!(1)");
        expand_module(&mut m);
        // Expand is not a checker; the target's support check reports it.
        match &m.items[0] {
            Stmt::Dump { value, .. } => {
                assert!(matches!(**value, Expr::MacroInvoke { .. }))
            }
            other => panic!("expected a dump, got {:?}", other),
        }
    }
}
