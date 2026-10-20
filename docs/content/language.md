# Language reference

Rak is dynamically typed with optional annotations, and it has two backends
that agree. This page is the syntax and semantics; the [standard
library](stdlib.html) is the builtins.

## Comments and whitespace

```rak
// a line comment
/* a block comment */
```

`///` and `//!` are **not** doc comments — they are ordinary comments and are
discarded. Rak has no doc comments and no attributes: `#![allow(dead_code)]`
is a lexer error.

## Literals

### Integers

```rak
42          // decimal
1_000_000   // underscores between digits
0xDEADBEEF  // hex
0b1010      // binary
0o755       // octal
```

Hex, binary and octal literals are unsigned. A typed suffix may be applied to
a **decimal** literal only:

```rak
42i32
10u8
1_0i64
```

The suffixes are `i8 i16 i32 i64 u8 u16 u32 u64`. `0x1Au8` is not a literal —
the suffix is a separate identifier.

### Floats

```rak
3.14
1_000.5
1.5e3
3.14f32
```

A fractional part is required: `.5` is a syntax error, and `1e10` is an
identifier, not a float. The only float suffix is `f32`.

### Characters

```rak
'a'
'א'
'\u{03B1}'    // α
'\x41'        // A
'\n' '\'' '\\'
```

A character is a code point, and `ord`/`chr` agree with that.

### Strings

```rak
"plain"
"escapes: \" \\ \n \t \r \0 \x41"
r"raw: \n is two characters"
"""multi
line"""
f"interpolated {name}, {{literal braces}}"
```

An unknown escape in a normal string is kept verbatim rather than dropped, and
a malformed `\x` keeps its digits.

### Bytes

```rak
b"\x89PNG\r\n"
```

Bytes are a distinct type from strings. Indexing yields an integer.

### Regular expressions

```rak
/\d+/g
/[a-z]+/i
```

A `/` after a value is division; a `/` in operand position starts a regex.
Flags are `i` (case-insensitive), `m` (multi-line), `s` (dotall) and `x`
(extended). A regex value has methods (`is_match`, `find_all`, `replace`) and
there are free-function forms (`regex_find_all(/…/g, text)`) that work the
same way on both backends.

### Collections

```rak
[1, 2, 3]             // array
[]                     // empty array
(1, 2)                // tuple
(1,)                   // one-element tuple
()                     // empty tuple
{ name: "rak", n: 1 } // map
{ "name": "rak" }      // string keys work too
```

A map literal's **first** key must be an identifier or a string; later keys
may be any expression. `{1: 2}` is not a map — it is parsed as a block, and
fails.

There are no set literals. Build one with `set_of([...])`.

## Variables

```rak
let target = "192.168.1.1"
let mut counter = 0
let pi: f64 = 3.14
let (host, port) = ("10.0.0.5", 443)      // destructuring
let [first, second] = [10, 20]
let Point { x, y } = Point { x: 1, y: 2 }
```

`mut` goes before the name; the type annotation goes after it and before `=`.
A `let` binding is immutable, and assigning to one is an error on both
backends.

Destructuring supports tuple, array and struct patterns.

## Assignment

Assignment is an expression, and `=` is right-associative.

```rak
counter = counter + 1
counter += 1
xs[0] = 9
obj.field = 9
a, b = b, a
```

Compound assignment (`+= -= *= /= %= &= |= ^= <<= >>=`) takes a bare
identifier only — not an index or a field.

## Functions

```rak
fn add(a, b) {
    return a + b
}

fn add(a, b) -> int {
    return a + b
}

fn identity<T>(value: T) -> T {
    return value
}
```

A `fn` declaration is a `let` binding of a function value, which is why
`pub fn` and `pub let` behave alike. Generic parameters are parsed and
**discarded**: the language is dynamically typed, and `identity<int>(42)` is a
dynamic call with a type argument the runtime ignores.

A function body without an explicit `return` evaluates to `nil`.

### Closures

```rak
let f = fn(n) { if n < 2 { return n } return f(n-1) + f(n-2) }
```

Free variables resolve at the **definition** site: a closure sees the function
it was written in, by value. An assignment inside a closure lands in the
closure's own copy, and the enclosing function never sees it. A *global* is
not copied — a closure reads and writes the program's globals on both
backends.

An expression-bodied lambda parses but returns `nil`:

```rak
fn(x) x + 1        // parses, evaluates to nil
fn(x) { return x + 1 }   // this is what you want
```

### Contracts

`requires` and `ensures` come after the body. `ensures` clauses see `result`
bound to the value the caller receives.

```rak
fn double(x) -> int {
    return x * 2
} requires x > 0 ensures result == x * 2
```

Both backends check them, and `rakc verify` reports them under a budget. A
violation names the function and the clause.

### `defer`

```rak
fn work() {
    defer cleanup()
    defer dump "runs first"
}
```

Deferred calls run in LIFO order when the current function exits — after the
return value is computed, and before an `ensures` clause is judged, so a
cleanup has already happened when the promise is checked.

## Control flow

```rak
if cond { … } else if other { … } else { … }

if let PATTERN = expr { … } else { … }

while cond { … }
while let PATTERN = expr { … }
do { … } while cond;
loop { … }

for x in [1, 2, 3] { … }
for i, x in pairs { … }
```

`if` is an expression, so `let x = if cond { 1 } else { 2 }` works. `if let`
is a statement.

`for` iterates arrays, tuples, strings (yielding one-character strings), maps
(yielding `[key, value]` tuples), `Some` values, sets, and anything with an
`Iterable` implementation.

### Labels

A label has **no closing quote**:

```rak
'outer: for x in rows {
    for y in x {
        if y == 0 { break 'outer }
    }
}
```

`break 2` breaks out of two enclosing loops. `continue` works the same way.

## Pattern matching

```rak
match value {
    1 => { dump "one" },
    2 => { dump "two" },
    x if x > 100 => { dump "big" },
    _ => { dump "other" }
}
```

Arms are separated by commas or newlines — the comma is optional. An arm body
is a single expression; `{ … }` is a block expression.

Or-patterns need a leading pipe: `| 1 | 2 => { … }`.

## Operators and precedence

Loosest to tightest:

| Level | Operators |
|---|---|
| 1 | `\|>` pipeline (left) |
| 2 | `=`, `+=`, `a, b = c, d` (right) |
| 3 | `??` (right) |
| 4 | `? :` ternary, postfix `?` (right) |
| 5 | `\|\|` (left) |
| 6 | `&&` (left) |
| 7 | `==` `!=` (left) |
| 8 | `<` `>` `<=` `>=` `in` (left) |
| 9 | `..` range |
| 10 | `&` `\|` `^` — all three at the **same** level (left) |
| 11 | `<<` `>>` (left) |
| 12 | `+` `-` (left) |
| 13 | `*` `/` `%` (left) |
| 14 | unary `-` `!` `~` `await` `spawn` `raise` |
| 15 | postfix `()` `[]` `.` `::` `?.` `?[` `?` `as` `!()` |

`as` binds tighter than every binary operator, so `x as int + 1` is
`(x as int) + 1`.

Comparisons are not chainable: `1 < 2 < 3` is an error, because it evaluates
`(1 < 2) < 3`.

`+` concatenates strings and arrays as well as adding numbers. `/` on two
integers is integer division.

### The `?` operator

`?` is context-sensitive. `expr?` unwraps a `Result` or `Option` and raises
on `Err`/`None`. `cond ? a : b` is a ternary. Which one you get depends on
whether the token after `?` can start an expression:

```rak
let v = Ok(42)?          // unwrap
let name = flag ? "yes" : "no"   // ternary
```

So `x? { … }` is a ternary, because `{` can start an expression.

`??` is nil-coalescing: `a ?? b` is `b` when `a` is `nil` or `None`.

### Ranges

`..` is **inclusive** as a value and **exclusive** as a slice bound. That
difference is real and deliberate:

```rak
dump 1..5              // [1, 2, 3, 4, 5]
dump [1, 2, 3, 4][1..3] // [2, 3]
dump [1, 2, 3, 4][..2]  // [1, 2]
```

Negative indices count from the end: `xs[-1]` is the last element. Slicing
clamps; out-of-range indices are an error.

### Casts

```rak
dump 65 as char       // 'A'
dump 300 as u8        // error: 300 is out of range
```

Casts work between numbers, from a number to a string, and between a number
and a `char` (the code point). Casting a string to a number is an error.

### Pipelines

`x |> f` is `f(x)`, and `x |> f(a, b)` is `f(x, a, b)`. The desugaring
happens in the front end, so both backends see the same call.

## Structs, enums, traits

```rak
struct Point { x: int, y: int }
struct Box<T> { value: T }

enum Color { Red, Green }
enum Shape { Circle(i32), Rect(i32, i32) }
```

A struct literal needs `field: value` — there is no `Point { x }` shorthand.
Field access is `p.x`; tuple access is `p.0`.

Traits are protocols, and the receiver is the first parameter:

```rak
trait Display {
    fn fmt(self) -> string
}

impl Display for Point {
    fn fmt(self) { return fmt("({}, {})", self.x, self.y) }
}
```

`Display::fmt` drives `dump`, `Iterable::iter` drives `for x in target`, and
`Index::index` / `IndexMut::set` drive `obj[key]`.

`impl Type { … }` without a `for` is an inherent implementation.

## Sets

```rak
let s = set_of(["a", "b", "a"])
dump set_len(s)         // 2
dump set_has(s, "a")    // true
for item in s { dump item }
```

Sets keep insertion order. `set_add`, `set_discard`, `set_has_all`,
`set_union`, `set_intersect`, `set_diff` and `set_to_array` complete the
family.

## Statements that are not expressions

Four things are statements rather than calls, and read differently:

```rak
dump value                // print, using Display
dump value, "path.txt"    // print, and also write to a file
trace value               // print, using Debug
```

```rak
scan "127.0.0.1" { range: [0x0016, 0x0050] } {
    if open { dump fmt("Port 0x{:04X}", port) }
}
```

`scan` takes an **options** group and then a **body** group, in that order.
The body binds `port`, `open` and `banner`.

```rak
fetch "https://example.com" { method: "GET" } {
    dump status
    dump body
}
```

`fetch` takes the same two groups; the body binds `status`, `headers` and
`body`. The options are `method`, `headers` and `body`.

Both `scan` and `fetch` are **interpreter-only**: the bytecode VM refuses to
compile a program that uses them, with a clear error.

## `unsafe` blocks

```rak
unsafe "the pointer comes from ffi_alloc and is 4 bytes long" {
    asm("cpuid_sse2")
}
```

The reason string is mandatory and must be a real justification — `rakc lint`
rejects a short one, or one containing "trust me". Inline assembly is only
legal inside an `unsafe` block; the parser enforces that.

## `evidence`

```rak
let ip = evidence<string> from "93.184.216.34"
```

`evidence<T> from expr` tags a value with where it came from. The `<T>` is
parsed and discarded. See [Forensics](forensics.html).

## Tests

```rak
test "addition works" {
    assert add(2, 3) == 5
    assert_eq(add(2, 3), 5)
    expect_error(fn() { raise "boom" })
}
```

Test blocks are collected, not run, during a normal run. `rakc test` runs
them.

## Static type checking

`rakc check` type-checks a file. The diagnostics look like this:

```text
error[E0308]: type mismatch
  --> demo.rak:1:19
1 | let count: i32 = "hello"
  |                   ^

expected: int
found:    string
help: an explicit `let x: T = value` requires `value` to have type `T`
```

It checks explicit annotations, call argument types and arity, container
element types, struct fields, enum variants, and casts. It does **not** model
generics, enforce mutability, or check across modules. Annotations are
enforced at runtime regardless of whether you run `rakc check`.

## Things that are not in the language

So that you do not go looking for them:

- No attributes (`#![…]`) and no doc comments.
- No `mod { }` without a name — it is `mod name { … }`.
- No set literals, no `**` power operator, no comparison chaining.
- No `.5` floats, no `1e10` floats, no `f64` literal suffix, no typed suffix
  on a hex literal.
- No `+=` on an index or a field target.
- No general method dispatch on builtins: `"abc".len()` is an error. Methods
  come from an `impl` block.
- `use`/`import`/`from` are only valid at module top level, not inside a
  function or a `mod` block.
- `raise` and `throw` are the same keyword.
- `use` and `import` are the same statement; `pub` and `export` are the same
  prefix.
