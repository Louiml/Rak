# Macros & compile-time constants

## Macros

`macro name($params) { body }` defines an AST-expanding template;
`name!(args)` splices the argument expressions into the body's `$param`
placeholders before evaluation. Expanded in the frontend, so both backends see
the expanded code.

```rak
macro add1(x: expr) { $x + 1 }
dump add1!(41)            // 42

macro pair(a: expr, b: expr) { [$a, $b] }
dump pair!(1, 2)          // [1, 2]

macro swap(a: expr, b: expr) {
    let t = $a
    t + $b
}
dump swap!(10, 20)        // 30
```

## Compile-time constants

```rak
const MAX_LEN = 256
dump MAX_LEN
```

`const` is eagerly evaluated and bound (immutable by convention) on both
backends.

## Semantics and limits

- Macro bodies see `expr` fragments (the kind annotation is accepted; all
  params are expr fragments).
- Expansion is **non-hygienic**: `let t = $a` in a macro body introduces `t`
  in the caller's scope.
- Arity errors: `macro 'foo!' expects N args, got M`; unknown macros:
  `undefined macro 'foo!'`.
- `$name` outside a macro body is a clear error.
- Macro bodies that expand to `raise` work on the interpreter but not the VM
  (VM-compatible macro bodies avoid `raise`).
- Macro expansion is not depth-capped; avoid self-referential bodies.
