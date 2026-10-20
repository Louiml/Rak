# Error handling

## try, catch, raise

```rak
try {
    raise "boom"
} catch e {
    dump e
}
```

`raise` propagates a value up to the nearest `catch`. `throw` is the same
keyword, spelled differently.

The catch clause is optional:

```rak
try {
    risky()
}
```

which propagates any error instead of catching it.

An error raised inside a `catch` block is **not** caught by the same `try`.

## The `?` operator

`expr?` unwraps a `Result` or an `Option`, and propagates the failure:

```rak
let v = Ok(42)?          // 42
let o = Some(7)?         // 7
let e = Err("bad")?      // raises "bad"
```

`?` is context-sensitive: when the token after it can start an expression, it
is the ternary instead. `x? { … }` is a ternary, because `{` can start an
expression.

## Structured errors

`catch e` binds a first-class error value, not a string. It carries a kind, a
message, a source position, a cause and a context:

```rak
try {
    raise "disk full"
} catch e {
    dump err_kind(e)      // user
    dump err_message(e)   // disk full
    dump err_line(e)      // 2
    dump err_col(e)
    dump err_file(e)
    dump err_cause(e)     // nil, or the error that caused this one
    dump err_context(e)   // a map
}
```

| Builtin | Does |
|---|---|
| `err_message(e)` | The message. |
| `err_kind(e)` | The kind. |
| `err_line(e)` / `err_col(e)` / `err_file(e)` | Where it happened. |
| `err_cause(e)` | The error that caused this one, or nil. |
| `err_context(e)` | The context map. |
| `err_with_context(e, key, value)` | Adds a context pair and returns the error. |

The kinds are `io`, `network`, `parse`, `compile`, `type`, `package`,
`permission`, `user`, `timeout`, `cancel`, and `runtime` for anything else. A
value raised with `raise` is a `user` error; a runtime fault is `runtime`.

You can build an error yourself:

```rak
let e = error("io", "could not open the file")
```

## Contracts

A function can state what it requires and what it promises:

```rak
fn divide(a, b) -> int {
    return a / b
} requires b != 0 ensures result * b == a
```

`requires` clauses are checked before the body runs; `ensures` clauses are
checked after it returns, with `result` bound to the value the caller
receives. A violation raises an error naming the function and the clause.

Both backends check them, and `rakc verify` reports them under a budget. The
deferred calls of the function run before an `ensures` clause is judged, so a
cleanup has already happened when the promise is checked.

## Assertions

```rak
assert add(2, 3) == 5
assert_eq(add(2, 3), 5)
assert_ne(a, b)
assert_true(flag)
assert_false(flag)
expect_error(fn() { raise "boom" })
```

`assert` reports the source of the expression it failed on, which is more
useful than a bare "assertion failed".

`expect_error` passes only when the function raises. It is the way to test
that a failure happens — a contract violation, a refused capability, a
missing file.

## Resource cleanup

`defer` runs a call when the current function exits, in LIFO order:

```rak
fn work() {
    defer close_handle()
    defer log "runs first"
}
```

Deferred calls run after the return value is computed and before an
`ensures` clause is checked. They run on the error path too, so a function
that fails halfway still cleans up.

## Panics

`panic(msg?)` raises a runtime error with the message. It is a plain raise,
not a different mechanism.

## What the error messages look like

Runtime errors are prefixed by their kind:

```text
Runtime error: Division by zero
Runtime error: type mismatch: expected int, found string
Runtime error: cannot assign to immutable variable 'x'
```

Lexer and parser errors carry a line and a column:

```text
Lexer error: Unexpected character at line 1, col 1
Parser error: Expected identifier at line 3, col 5
```

The bytecode VM prefixes its own:

```text
VM error: Runtime error: Division by zero
```
