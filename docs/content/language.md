# Language reference

## Variables

```rak
let target = "192.168.1.1"
let port = 0x0050
let mut counter = 0
counter = counter + 1
let pi = 3.14f64
let n = 42i32
```

`let` binds an immutable value; `let mut` allows reassignment. Literals carry
optional type suffixes (`42i32`, `3.14f64`).

## Functions

```rak
fn add(a, b) {
    return a + b
}
dump add(0x10, 0x20)   // 48
```

Closures are lexical: free variables resolve at the definition site, not the
call site. Recursive closures like
`let f = fn(n) { if n < 2 { return n } return f(n-1) + f(n-2) }` just work.

## Control flow

```rak
if port == 0x0050 {
    dump "HTTP"
} else {
    dump "Other"
}

for x in [1, 2, 3] { dump x }
while counter < 0x0A { counter = counter + 1 }
loop { break }
```

## Types

Signed and unsigned ints (i8 through i64, u8 through u64), floats (f32, f64),
typed literals like `42i32` and `3.14f64`, tuples, `Result`/`Option`, strings,
`bytes`, maps, arrays, structs, and enums. Hex is a first-class type
(`hex8`/`hex16`/`hex32`/`hex64`).

## Hex arithmetic

```rak
let sig = 0xDEADBEEF
let mask = 0xFF00FF00
dump fmt("0x{:08X}", sig & mask)
```

## String interpolation

```rak
let name = "Rak"
dump f"hello {name}!"
```

## Tuples and structs

```rak
let p = (1, 2)
dump p.1
struct Point { x: int, y: int }
let o = Point { x: 1, y: 2 }
dump o.x
```

## Pattern matching

```rak
match 2 {
    1 => { dump "one" },
    2 => { dump "two" },
    _ => { dump "other" }
}
```

Arms are comma-separated. Patterns can destructure arrays and tuples, match
literals, and use a `_` catch-all.

## Binary pattern matching

Match a `bytes` value against byte literals (hex, int, or char) with a trailing
`..` to match the rest. Useful for magic-number sniffing.

```rak
fn sniff(data: bytes) {
    match data {
        [0x89, 'P', 'N', 'G', ..] => { return "png" },
        [0xFF, 0xD8, 0xFF, ..] => { return "jpeg" },
        ['%', 'P', 'D', 'F', ..] => { return "pdf" },
        _ => { return "unknown" },
    }
}
dump sniff(b"\x89PNG\x0d\x0a\x1a\x0a")  // png
```

Without `..`, the pattern matches an exact length. Byte patterns are
interpreter-only (the VM errors clearly). Always terminate match arms with `,`.

## Pipeline operator

`x |> f` desugars to `f(x)`, and `x |> f(a, b)` to `f(x, a, b)`. Chaining is
left-associative, so `data |> parse |> load` is `load(parse(data))`. Works on
both the interpreter and the bytecode VM (parse-time desugar, no new opcode).

```rak
fn inc(n) { return n + 1 }
fn dbl(n) { return n * 2 }
dump 5 |> inc |> dbl          // 12
dump 3 |> add(10)             // 13
```

## Regex literals

`/pattern/flags` with flags `i` (case-insensitive), `m` (multi-line), `s`
(dotall), `x` (extended), `g` (accepted, no-op). A `/` after a value is
division; a `/` in operand position starts a regex.

```rak
let re = /\d+/g
dump re.is_match("abc123")             // true
dump re.find_all("a1 b22 c333")        // [1, 22, 333]
dump (/\s+/g).replace("a  b   c", "_") // a_b_c

// Free-function form (also works on the VM):
dump regex_find_all(/[a-z]+/g, "a1bc2def")   // [a, bc, def]
```

An invalid pattern is a runtime error, never a panic. A literal `/` outside a
character class must be escaped as `\/`; `/` inside `[...]` classes does not
terminate the literal.

## Traits (Display, Debug, Iterable, Index, IndexMut)

The receiver is passed as the first argument of each method. `Display::fmt`
drives `dump`, `Iterable::iter` drives `for x in target`, `Index::index` /
`IndexMut::set` drive `obj[key]` reads and writes.

| Trait | Method | Used by |
|-------|--------|---------|
| `Display` | `fmt(self) -> string` | `dump` |
| `Debug` | `fmt(self) -> string` | `trace` |
| `Iterable` | `iter(self) -> array` | `for x in target` |
| `Index` | `index(self, key)` | `obj[key]` (read) |
| `IndexMut` | `set(self, key, value)` | `obj[key] = value` (write) |

```rak
struct Point { x: int, y: int }
impl Display for Point {
    fn fmt(self) { return fmt("({}, {})", self.x, self.y) }
}
dump Point { x: 3, y: 4 }   // (3, 4)

struct Range { lo: int, hi: int }
impl Iterable for Range {
    fn iter(self) {
        let out = []; let i = self.lo
        while i <= self.hi { out = push(out, i); i = i + 1 }
        return out
    }
}
let s = 0
for n in Range { lo: 1, hi: 5 } { s = s + n }
dump s   // 15
```

Trait protocols and method-call dispatch are interpreter-only; on the VM,
`obj.method(...)` is limited to maps/structs holding callables.
