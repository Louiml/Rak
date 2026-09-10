# Rak Language Features — Design & Implementation Spec

This document specifies advanced language features for **Rak**: syntax,
architecture, exact Rust changes, and error handling / edge cases.

Status legend: **[SHIPPED]** = implemented and tested in this pass;
**[SPEC]** = designed here, to be implemented in a follow-up.

The Rak compiler has two execution backends that share a common frontend
(`lexer.rs` → `parser.rs` → `ast.rs`):

- **Interpreter** (`interpreter.rs`) — a tree-walker with its own `Value` enum
  and the full stdlib bridge. This is the primary backend and supports every
  language feature.
- **Bytecode VM** (`compiler.rs` → `bytecode.rs` → `vm.rs`) — a stack machine
  using the `Value` enum in `value.rs`. It supports a deliberate subset (let,
  if, while, loop, for, functions, match, arrays/tuples/maps, indexing,
  interpolation, regex literals + `regex_*` builtins, and the `|>` desugar).
  Statements it does not implement return a clear `VM does not support ...`
  error rather than crashing.

---

## Part 1 — Language Mechanics & Systems Tooling

### 1.1 Pipeline operator `|>`  **[SHIPPED]**

#### Syntax
```rak
x |> f                  // f(x)
x |> f(a, b)            // f(x, a, b)
data |> parse |> load   // load(parse(data))   (left-associative)
```

#### Architecture
`|>` is implemented as a **parse-time desugar**, so both backends evaluate it
for free with no new opcode. `lexer.rs` gains a `|>` token (`PipeGt`).
`parser.rs` inserts a `parse_pipeline` precedence level above `parse_assignment`
(`parse_expr` → `parse_pipeline` → `parse_assignment`):

```rust
fn parse_pipeline(&mut self) -> Result<Expr> {
    let mut left = self.parse_assignment()?;
    while self.match_token(&Token::PipeGt) {
        let right = self.parse_assignment()?;
        left = desugar_pipe(left, right);
    }
    Ok(left)
}

fn desugar_pipe(left: Expr, right: Expr) -> Expr {
    match right {
        Expr::Call { callee, mut args } => {
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(left);
            new_args.append(&mut args);
            Expr::Call { callee, args: new_args }
        }
        other => Expr::Call { callee: Box::new(other), args: vec![left] },
    }
}
```

- `x |> f`        → `Call { callee: f, args: [x] }`
- `x |> f(a, b)`  → `Call { callee: f, args: [x, a, b] }` (left is prepended)
- Chaining is left-associative: `a |> f |> g` → `g(f(a))`.

No changes to `compiler.rs`, `interpreter.rs`, or `vm.rs` were needed beyond
the shared frontend — the resulting `Expr::Call` is already fully supported.
The latent `Value::Closure` `PartialEq` bug (see §1.1-Errata) was fixed so the
VM correctly distinguishes multiple top-level functions.

#### Error handling & edge cases
- **No AST bloat**: the operator exists only at parse time; `rakc parse`
  prints a `Call` node, which is the intended representation.
- **Receiver is the first argument**, matching Hack/Elixir-style pipelines. A
  function called via `|>` with the wrong arity reports the existing
  `"Undefined variable"` / arity-mismatch errors at runtime.
- **Precedence**: `|>` binds looser than assignment, so `let y = x |> f` parses
  the whole pipeline as the let value. The right-hand side of `|>` is a single
  `parse_assignment` expression; nested pipelines must be parenthesised.
- **Division vs pipe**: `|>` is a distinct 2-char token; logos longest-match
  keeps `|` (bitwise Or) and `|>` separate.

#### Errata (bug fixed in this pass)
`value.rs`'s `PartialEq` for `Value::Closure` and `Value::NativeFn` fell through
to `discriminant` equality, so `Chunk::add_const` deduplicated **all** closures
to the first one added. With two top-level functions, the VM would call the
wrong one (`dbl` actually invoked `inc`, yielding `inc(inc(5)) = 7` instead of
`12`). Fixed by:
```rust
(Value::Closure { code: a, .. }, Value::Closure { code: b, .. }) => Arc::ptr_eq(a, b),
(Value::NativeFn(na, _), Value::NativeFn(nb, _)) => na == nb,
```

---

### 1.2 Regex literals `/pattern/flags`  **[SHIPPED]**

#### Syntax
```rak
let re = /\d+/g
re.is_match("abc123")          // true   (method syntax, interpreter)
re.find_all("a1 b22 c333")    // [1, 22, 333]
re.find("no digits")          // nil
re.replace("a  b   c", "_")   // "a_b_c"

// Free-function form (works on both interpreter and VM):
regex_find_all(/[a-z]+/g, "a1bc2def")   // [a, bc, def]
regex_match(/rak/i, "RAK language")     // true
```

Flags: `i` (case-insensitive), `m` (multi-line), `s` (dotall), `x` (extended),
`g` (accepted, no-op — `find_all` is inherently global).

#### Architecture
The central difficulty is the `/` ambiguity (division vs regex). Logos cannot
express context-sensitive lexing, so the design is a **two-phase lexer**:

1. `lexer.rs` runs logos over the whole source, collecting tokens as
   `Option<Token>` where `None` marks a logos "gap" (an unmatchable span — e.g.
   a `\` inside a regex, which is not itself a valid token).
2. `postprocess_regexes` walks that stream. When it sees a `Token::Slash` in
   **operand context** (the previous significant token *cannot* end an
   expression — see `can_end_expr`), it scans the **raw source** from that
   offset with `scan_regex` to find the closing `/` (respecting `\/` escapes
   and `[...]` character classes) and the trailing flags, then emits a single
   `Token::Regex((pattern, flags))` and skips every token/gap whose offset is
   inside the literal.
3. Any `None` gap that survives (is not consumed by a regex) is reported as a
   genuine `Lexer("Unexpected character ...")` error.

`can_end_expr` returns true for identifiers, literals (Int/Float/Hex/Bytes/
String/Interp/Char/Regex), `true`/`false`/`nil`, closing delimiters `)` `]` `}`,
and the postfix `?`. A `/` after any of those is division; otherwise it begins
a regex.

AST: `Expr::Regex(String /*pattern*/, String /*flags*/)`.
Interpreter `Value::Regex(Arc<RegexValue>)` where `RegexValue { pattern, flags,
re: regex::Regex }`. The pattern is compiled once at evaluation time via
`build_regex` (which maps flags to `regex::RegexBuilder`).
VM `value.rs` mirrors this with its own `RegexValue`; `compiler.rs` compiles
the regex at **compile time** (`make_regex_value`) and stores a `Value::Regex`
constant, so the VM never pays for regex construction at runtime.

Method-call dispatch (`re.match(...)`, `re.find_all(...)`) is implemented in
the interpreter's `eval_call` (see §1.4). The VM, which has no method-dispatch
layer, exposes the same capability through the `regex_new` / `regex_match` /
`regex_find` / `regex_find_all` / `regex_replace` natives registered in
`vm.rs::register_natives`.

#### Error handling & edge cases
- **Invalid pattern** → `Runtime("invalid regex /pat/flags: <regex error>")`
  (interpreter) or a compile-time `Err` (VM) — never a panic.
- **Unknown flag** → `Runtime("unknown regex flag 'z'")`.
- **Unterminated regex** → the closing `/` is not found; `scan_regex` returns
  `None`, the `/` is treated as division, and the interior `\` gap surfaces as
  a `Lexer("Unexpected character ...")` error at the first invalid char.
- **`/` inside a character class** (`[a/b]`) does not terminate the literal
  (`scan_regex` tracks `in_class`). A literal `/` outside a class must be
  escaped as `\/`.
- **`//` inside a regex** is not supported (must be `\/\/`); logos would
  otherwise treat `//` as a line comment — this is documented and matches
  Rust/JS regex-literal behaviour.
- Regex values are compared by `(pattern, flags)` for constant dedup, not by
  discriminant, so two different regexes never share a constant slot.

---

### 1.3 Binary pattern matching  **[SHIPPED]**

#### Syntax
```rak
match bytes {
    [0x89, 'P', 'N', 'G', ..]      => { dump "png" },
    [0xFF, 0xD8, 0xFF, ..]         => { dump "jpeg" },
    ['%', 'P', 'D', 'F', ..]       => { dump "pdf" },
    [0x00, 0x01]                   => { dump "exact 2 bytes" },   // no rest
    _                              => { dump "other" },
}
```

Byte literals in a pattern may be hex (`0x89`), decimal (`216`), or a char
(`'P'`). A trailing `..` matches the remainder.

#### Architecture
- `lexer.rs`: a char literal `'\x41'` / `'P'` / `'\n'` token (`Char(char)`)
  via the regex `r"'([^'\\]|\\x[0-9a-fA-F]{2}|\\.)'"`. In expression context a
  char literal evaluates to its codepoint as an `Int`; in pattern context it is
  a byte.
- `ast.rs`: new `Pattern::Byte(u8)` and `Pattern::Bytes(Vec<BytesPat>)` with
  `enum BytesPat { Byte(u8), Rest }`.
- `parser.rs`: `parse_single_pattern` for `LBracket` calls `peek_rest_in_brackets`
  — a lookahead that returns true if there is a top-level `DotDot` before the
  matching `]`. If so, `parse_bytes_pattern` reads a flat list of byte
  literals (hex/int/char, bounds-checked to `0..=255`) terminated by an
  optional `..` rest. Without a `..`, the existing `Pattern::Array` parser
  runs unchanged.
- `interpreter.rs::pattern_matches`:
  - `Pattern::Bytes` vs `Value::Bytes` walks the byte list; `Byte(b)` must
    equal `v[i]`, `Rest` matches everything left.
  - `Pattern::Array` vs `Value::Bytes` also matches exact-length byte slices
    (elements `Hex`/`Int`/`Byte`), so no-rest patterns work too.
  - `Pattern::Byte` matches a 1-byte `Bytes` or an equal `Int`.
- Bindings inside byte patterns are intentionally unsupported (binary
  matching is for inspection, not deconstruction).

#### Error handling & edge cases
- **Byte out of range**: `0x1FF` or `300` in a bytes pattern is a parser
  error (`"Byte value out of range in bytes pattern"`); a non-ASCII char
  (`'€'`) is `"Char out of byte range in bytes pattern"`.
- **Match arms require comma separators.** A missing comma lets
  `parse_postfix` treat the next arm's `[` as an index into the previous
  body — diagnosed as `Expected RBracket, found Comma`. Always terminate
  match arms with `,`.
- **Length mismatch without `..`** → the arm does not match (falls through to
  the next arm / `_`), no error.
- The VM's `compile_pattern` returns a clear `VM match does not support
  pattern` error for `Bytes`/`Byte`; binary matching is interpreter-only.

---

### 1.4 Standardized traits / protocols  **[SHIPPED] (interpreter)**

#### Syntax
```rak
struct Point { x: int, y: int }

impl Display for Point {
    fn fmt(self) { return fmt("({}, {})", self.x, self.y) }
}
dump p            // uses Display::fmt -> "(3, 4)"

impl Iterable for Range {
    fn iter(self) { ...; return out }
}
for n in my_range { ... }     // uses Iterable::iter

impl Index for Table {
    fn index(self, key) { return self.data[key] }
}
dump t["a"]                  // uses Index::index

impl IndexMut for Table {
    fn set(self, key, value) { self.entries[key] = value; return self }
}
t["c"] = 99                   // uses IndexMut::set
```

The built-in trait hooks are:
| Trait | Method | Used by |
|-------|--------|---------|
| `Display` | `fmt(self) -> string` | `dump`, `dump x, "file"` |
| `Debug`   | `fmt(self) -> string` | `trace` |
| `Iterable`| `iter(self) -> array`  | `for x in target` |
| `Index`   | `index(self, key)`     | `obj[key]` (read) |
| `IndexMut`| `set(self, key, value)`| `obj[key] = value` (write) |

The receiver is passed as the **first argument** of every method; the user
names it (conventionally `self`, though `self` is not a keyword).

#### Architecture
- `Interpreter` gains two registries:
  - `trait_impls: HashMap<(String /*trait*/, String /*type*/, String /*method*/), Value>`
  - `methods: HashMap<(String /*type*/, String /*method*/), Value>` (for
    `obj.method(...)` call syntax, populated from both inherent and trait impls).
- `Stmt::Impl` is reinterpreted: the parser stores `impl <A> for <B>` as
  `target=A` (trait), `trait_name=B` (type); an inherent `impl <Type>` has
  `trait_name=None`. The interpreter computes `(trait_str, type_name)` and
  registers each method in both maps (and keeps the legacy `Type.method` env
  binding for backward compatibility).
- `eval_call` was extended to handle `Expr::FieldAccess` callees (method
  dispatch):
  1. Native methods on `Value::Regex` (`is_match`, `find`, `find_all`,
     `replace`).
  2. `methods.get(&(type, method))` → call with the receiver prepended.
  3. Fallback: a field that itself holds a callable (preserves
     `module.function(...)` and struct-field-as-function calls).
- Built-in hooks call `call_method_with_values` with the receiver as arg 0:
  - `Stmt::Dump`/`Stmt::Trace` use `display_value`/`debug_value`.
  - `Stmt::For` uses `Iterable::iter` when present, else falls back to
    array/tuple/string/map/Option iteration.
  - `Expr::Index` (read) uses `Index::index`.
  - `Expr::IndexAssign` uses `IndexMut::set`, which must **return the updated
    container** (functional-update semantics); the result is stored back via
    `store_back`. This matches Rak's clone-on-read value semantics.

#### Error handling & edge cases
- **No `self` keyword**: the receiver is bound to whatever the user names the
  first parameter. A method declared with zero params silently ignores the
  receiver — declare a first param to use it.
- **`IndexMut::set` must return the updated container.** Because Rak values
  are cloned on read, mutation is functional: `set` returns the new container
  and the interpreter writes it back to the target expression.
- **Ambiguity**: if a struct has a field and a method with the same name, the
  method wins for `obj.name(...)` call syntax; field read `obj.name` still
  returns the field.
- **`Iterable::iter` must return an array**; otherwise `Runtime("Iterable::iter
  must return an array, got <type>")`.
- Trait protocols are **interpreter-only**. The VM has no method-dispatch
  layer; `obj.method(...)` on the VM resolves `obj.method` via `FieldGet`
  (which yields `nil` for non-map/struct/tuple) and errors
  `"cannot call non-function"`. This is the documented VM subset.

---

### 1.5 Foreign Function Interface (FFI)  **[SHIPPED]**

#### Syntax
```rak
// 1. Declarative C bindings (preferred):
extern "C" {
    fn printf(fmt: *u8, ...) -> i32
    fn getpid() -> i32
}

// Optional explicit library:
extern "C" from "libc.so.6" {
    fn abs(n: i32) -> i32
}

// 2. Dynamic loader:
let libc = ffi_load("libc.so.6")           // "libc.dylib" / "msvcrt.dll"
let pid = libc.call("getpid", [])          // -> i64
libc.close()

// 3. Marshalling helpers:
let p = ffi_ptr(0xDEADBEEF)               // raw pointer value
let buf = ffi_alloc(256)                  // -> *u8  (ffi-managed, freed on GC)
ffi_write(buf, 0, 0x41)
let b = ffi_read(buf, 0)                  // -> i64 (one byte)
let s = ffi_cstr_to_string(p)             // *i8 -> string (copies, NUL-terminated)
let cs = ffi_string_to_cstr(s)            // string -> *i8 (NUL-terminated, ffi-managed)
ffi_free(buf)                             // release an ffi_alloc/ffi_string_to_cstr buffer
```

> `lib.call(symbol, args)` takes the **args as a single Rak array** (the array
> elements are marshalled to C; `[]` means no arguments). The return is an
> `i64` (untyped dynamic call). The declarative `extern "C"` form uses the
> declared return type for proper unmarshalling.

#### Architecture
- New crate dep `libloading` (cross-platform `dlopen`/`LoadLibrary`).
  `stdlib/ffi.rs` wraps it in a `LibHandle` and exposes `load`, `load_default`,
  `sym_addr`, plus two low-level call trampolines.
- `ast.rs`: `Stmt::Extern { abi, lib: Option<String>, decls: Vec<ForeignFn> }`,
  `ForeignFn { name, params, varargs, return_type }`, `Type::Ptr(Box<Type>)`,
  `Type::Void`.
- `lexer.rs`: `extern` keyword (`Token::Extern`), `...` (`Token::Ellipsis`).
- `parser.rs`: `parse_extern` parses the ABI string, optional `from "path"`,
  and a block of `fn` signatures (trailing `...` for varargs; `*T` pointer
  types; `void` return type). Optional `;`/`,` between declarations.
- `interpreter.rs`: `Value::ForeignLib(Arc<Mutex<LibHandle>>)` and
  `Value::ForeignPtr(u64)`. `Stmt::Extern` registers each declaration in a
  `foreign_fns` registry (lazily loading the platform default C library or the
  named `from` library once). Calls marshal each `Value` arg to a `u64` bit
  pattern (ints → two's-complement bits; `string` → NUL-terminated `CString`
  kept alive for the call; `bytes` → pointer into a kept-alive buffer; `ptr`
  → raw address) and unmarshal the return word per the declared `Type`.
  `lib.call`/`lib.sym`/`lib.close` method dispatch on `ForeignLib`, plus the
  `ffi_*` free-function builtins.
- VM (`value.rs` + `vm.rs` + `compiler.rs`): mirrored `Value::ForeignLib` /
  `Value::ForeignPtr` variants, `ffi_*` natives registered in
  `register_natives`, two new opcodes `Op::FFICall` / `Op::FFIClose`, and
  `make_foreign_native` which bakes each `extern` declaration into a
  `Value::NativeFn` global (lazy library cache + typed marshalling) so the VM
  runs `extern`-declared calls via `Op::Call` and `lib.call` via `Op::FFICall`.

#### Error handling & edge cases
- **Safety**: FFI is inherently `unsafe`. Every foreign call goes through a
  single `unsafe` trampoline; the runtime never dereferences a raw pointer by
  accident — `ffi_read`/`ffi_write` require an explicit offset and operate one
  byte at a time.
- **Symbol not found** → `Runtime("ffi: symbol 'foo' not found: <os err>")`.
- **Library load failure** → `Runtime("ffi: cannot load 'libc.so.6': <os err>")`.
- **Marshalling failure** (e.g. passing a map where a pointer is expected) →
  `Runtime("ffi: cannot marshal <type> to C")`. Never a panic.
- **String lifetime**: strings passed to C are `CString`s owned by the call
  frame and freed when the call returns — the C side must not retain them.
  `ffi_string_to_cstr` returns an ffi-managed pointer freed explicitly with
  `ffi_free`.
- **Varargs**: `printf`-style `...` is accepted by the parser and marshalled
  best-effort by runtime type after the fixed params.
- **Pointers are opaque `u64`**: dereferencing is explicit via `ffi_read`/
  `ffi_write` / `ffi_cstr_to_string`.
- **`ffi_alloc`/`ffi_string_to_cstr` allocations are tracked** (pointer →
  byte length) so `ffi_free` releases them with the correct
  `Vec::from_raw_parts` layout; freeing an untracked pointer is an error.

#### Errata (deviations from the original [SPEC])
- **No `libffi` dependency.** Instead of the `libffi`-based CIF builder, FFI
  uses a pair of `extern "C" fn(u64×8) -> u64` / `-> f64` trampolines that
  pass integer/pointer arguments in the integer register class. This builds
  cleanly on Windows MSVC (where `libffi-sys` is fragile) and covers the
  OSINT/FFI use cases (`getpid`, `abs`, `strlen`, pointer passing). **Float
  arguments** are not supported by the trampoline (float *return* is, via the
  `-> f64` trampoline); full float-by-value argument support is deferred to a
  future `libffi`-backed backend.
- **`lib.call(symbol, args)` takes an array**, not a spread argument list,
  matching the spec's `libc.call("getpid", [])` example. The compiler emits a
  dedicated `Op::FFICall` (no operand) that unpacks the array.
- **VM `extern` support** uses `Value::NativeFn` globals (consistent with the
  existing regex-builtin precedent) rather than dedicated per-declaration
  opcodes; this is the idiomatic VM path and reuses `Op::Call`.

---

### 1.6 Memory-mapped files & zero-copy I/O  **[SHIPPED]**

#### Syntax
```rak
let m = mmap_open("huge.pcap", "r")      // "r" | "rw"
let slice = mmap_slice(m, 0x1000, 64)     // -> mmap-slice (zero-copy view)
let magic = slice[0..4]                   // bytes view, no copy (interpreter)
dump magic[0]                             // single byte (both backends)
mmap_close(m)

// Zero-copy inspection helpers:
for line in mmap_lines(m, "\n") { ... }   // iterates without materialising
let off = mmap_find(m, "\xff\xd8\xff")    // byte search, returns offset (or -1)
let n = mmap_size(m)
let offs = mmap_lines_off(m, "\n")        // -> [(offset, length), ...] zero-copy
```

> `mmap_slice` returns a `Value::MmapSlice(Arc<MmapHandle>, off, len)` — a
> zero-copy view that keeps the mapping alive. Single-byte indexing (`s[i]`)
> reads directly from the mapped page on both backends; range slicing
> (`s[a..b]`) and binary pattern matching over slices work on the interpreter.
> `string(mmap_slice)` / `val_to_bytes(mmap_slice)` copy the slice once when
> crossing into string/regex builtins.

#### Architecture
- New crate dep `memmap2` (cross-platform `Mmap`/`MmapMut`).
- `stdlib/mmap.rs`: `MmapHandle { inner: MmapInner { Ro(Mmap), Rw(MmapMut) },
  _file: File }` with `open(path, mode)`, `size`, `find`, `lines`, `lines_off`.
  `open` returns an `Arc<MmapHandle>` so slices share ownership.
- `value.rs` and `interpreter.rs`: `Value::Mmap(Arc<MmapHandle>)` and
  `Value::MmapSlice(Arc<MmapHandle>, usize, usize)`. `mmap_slice` returns the
  slice variant — a true zero-copy view (the `Arc` keeps the `Mmap` alive, so
  slices outlive `mmap_close`). Indexing/range over an `MmapSlice` reads
  directly from the mapped pages.
- `interpreter.rs`: zero-copy `Expr::Index` handling for `Mmap`/`MmapSlice`
  (single index → byte; `Expr::Range` → sub-slice), `pattern_matches` arms for
  `Pattern::Bytes`/`Array`/`Byte` against `MmapSlice` (reads mapped pages),
  and `val_to_bytes`/`val_to_string` MmapSlice arms (copy-on-use). Builtins:
  `mmap_open`, `mmap_slice`, `mmap_size`, `mmap_close`, `mmap_find`,
  `mmap_lines`, `mmap_lines_off`.
- VM (`value.rs` + `vm.rs`): mirrored `Value::Mmap`/`MmapSlice` variants,
  `mmap_*` natives registered in `register_natives`, and `Op::IndexGet` cases
  for `Mmap`/`MmapSlice` single-byte reads. (`string()` native special-cases
  `Mmap`/`MmapSlice` to return content.)

#### Error handling & edge cases
- **Unmapping**: `mmap_close` is a no-op convenience — the `Arc<MmapHandle>`
  keeps the mapping alive until all slices drop (Rust's refcount guarantees the
  mapping outlives all views), preventing use-after-unmap.
- **Out-of-bounds slice** → `Runtime("mmap_slice: [off, off+len) = [...] out of
  range (len ...)")`.
- **File open / mmap failure** → `Runtime("mmap_open: cannot open/map '...':
  <os err>")`.
- **RAM**: a 4 GiB PCAP is mapped, not loaded — peak RAM is the OS page cache
  for touched pages plus the slice metadata.
- **`mmap_lines`** yields owned `String`s per line (a copy is unavoidable for
  safe iteration); for true zero-copy line scanning use `mmap_lines_off`
  returning `(offset, length)` pairs.

#### Errata (deviations from the original [SPEC])
- **VM coverage**: range slicing (`s[a..b]`) and binary pattern matching over
  `MmapSlice` are interpreter-only on the VM (the VM has no range-index or
  binary-pattern opcodes); single-byte indexing, `mmap_find`, `mmap_lines*`,
  and `mmap_size` work on both backends.
- **Bug fix**: the VM `for x in <array> { ... }` loop had an inverted
  `IndexGet` operand order (idx pushed before obj) and an unbalanced
  `Pop`/`StoreLocal` sequence, so the loop body never executed. Fixed in this
  pass (`compile_for` array arm) — `for x in [1,2,3] { dump x }` now works on
  the VM. New tests `test_vm_for_array` / `test_vm_for_array_dump` cover it.

---

### 1.7 Metaprogramming & macros  **[SHIPPED]**

#### Syntax
```rak
// AST-expanding macros with $param placeholders, macro_rules!-style:
macro add1(x: expr) {
    $x + 1
}
dump add1!(41)         // 42 — `add1!(41)` splices `41` into `$x` before eval

macro swap(a: expr, b: expr) {
    let t = $a
    t + $b
}
dump swap!(10, 20)     // 30

macro pair(a: expr, b: expr) {
    [$a, $b]
}
dump pair!(1, 2)       // [1, 2]

// Compile-time constants (eagerly evaluated, inlined):
const MAX_LEN = 256
dump MAX_LEN
```

> `macro name(params) { body }` defines a macro; `name!(args)` splices each
> argument's AST into the matching `$param` placeholder in the body, then
> evaluates/compiles the expanded body in place. Macros are expanded in the
> frontend, so both backends see the expanded code.

#### Architecture
- `ast.rs`: `Stmt::MacroDef { name, params, body }`, `Expr::MacroVar(String)`
  (a `$name` placeholder), `Expr::MacroInvoke { name, args }`, and
  `Stmt::Const { name, value }`.
- `lexer.rs`: `macro` and `const` keywords, and a `$ident` regex →
  `Token::MacroVar(String)`.
- `parser.rs`: `parse_macro` (`macro name(params) { body }`, reusing
  `parse_params` — the kind is accepted but treated as an expr fragment),
  `parse_const`, and `name!(args)` invocation in `parse_postfix` (when the
  base is an `Expr::Ident` followed by `!`).
- Expansion: `interpreter.rs::substitute_stmts` / `substitute_expr` walk the
  body AST and replace each `Expr::MacroVar(name)` with the bound argument
  AST. The interpreter registers `MacroDef`s in a `macros` map and evaluates
  `MacroInvoke` by substituting + executing the body in a pushed scope
  (returning the last expression's value).
- VM (`compiler.rs`): `MacroDef`s are collected in a `compile()` pre-pass
  (and cloned into sub-compilers) so `Expr::MacroInvoke` is expanded at
  **compile time** — the substituted body is compiled in place (last stmt's
  value is the result). `Expr::MacroVar` outside a macro body is a compile
  error.
- `Stmt::Const` compiles like a `let` (eagerly evaluated and bound; immutable
  by convention) on both backends.

#### Error handling & edge cases
- **Macro not found** → `Runtime/Compile("undefined macro 'foo!'")`.
- **Arity mismatch** → `Runtime/Compile("macro 'foo!' expects N args, got M")`.
- **`$name` outside a macro body** → a clear runtime/compile error.
- **Hygiene**: this v1 is **non-hygienic** — `let t = $a` in a macro body
  introduces `t` in the caller's scope (matching `Expr::Block` semantics).
  Per-expansion identifier renaming (true hygiene) is a follow-up.
- **Recursion limit**: macro expansion is not currently depth-capped; a
  self-referential macro body would recurse at parse/eval time and is the
  user's responsibility (a depth cap is a planned follow-up).

#### Errata (deviations from the original [SPEC])
- **Named-param macros** (`macro name($params) { body }` + `name!(args)`)
  instead of `macro_rules!` with `TokenTree` capture, fragment specifiers
  (`:expr`/`:literal`/`:ident`/`:tt`), and `$(...)*` repetition. The kind
  annotation is accepted but all params are treated as expr fragments. Full
  `macro_rules!` with repetition is a follow-up.
- **`const`** is eager-eval-and-bind (like `let`), not a true compile-time
  constant-folding pass that inlines the value at every use site.
- **`macro` proc-macro analogue** (`#[macro]`-tagged fn receiving AST nodes)
  is not implemented; the `macro ... { body }` template form covers the
  common cases.
- **VM `Stmt::Raise`** is not supported, so macro bodies that expand to
  `raise` (e.g. an `assert_nonneg!` that raises) work on the interpreter but
  not the VM; VM-compatible macro bodies avoid `raise`.

---

## Part 2 — OSINT & Security Capabilities

### 2.1 Built-in async I/O & event loop  **[SHIPPED]**

#### Syntax
```rak
async fn fetch(host: string) -> string {
    let r = await http_get_async("https://" + host)
    return r.body
}

// A future can be stored and awaited later:
let f = tcp_probe(host, 80, 200)
let open = await f

// http_get_async and tcp_probe run on the Tokio runtime and return a Future;
// `await` blocks until the I/O completes.
let body = await http_get_async("https://example.com")
```

> `async fn` returns a **deferred future** whose body runs on the first `await`.
> `http_get_async` / `tcp_probe` / `tcp_connect_async` spawn real Tokio tasks
> (`spawn_blocking` for the sync `ureq`/`std::net` calls) and return a
> pending `Future`; `await` resolves them via the runtime's `block_on`.

#### Architecture
- New crate dep `tokio` (always on) with `rt-multi-thread`, `net`, `io-util`,
  `macros`, `time`, `sync`. A lazily-started `tokio::runtime::Runtime` lives in
  `async_rt.rs` (`OnceLock`), shared by the interpreter and the VM.
- `interpreter.rs`: `Value::Future(Arc<FutureHandle>)` where `FutureHandle`
  holds `Mutex<FutureState>` and `FutureState` is `Ready(Value)` |
  `Pending(tokio::task::JoinHandle<Value>)` | `Deferred { params, body,
  closure, args }` | `Polled`. `async fn` calls return a `Deferred` future (the
  body is not run yet). `Expr::Await` resolves a future: `Ready` → value,
  `Pending` → `runtime.block_on(handle.await)`, `Deferred` → run the body on
  the interpreter thread (via `call_function_values` with `is_async = false`)
  and cache as `Ready`. `Expr::Spawn` runs a function on a native thread
  (legacy `spawn`) and passes a `Future` through (async I/O is already
  concurrent).
- VM (`value.rs` + `vm.rs` + `compiler.rs` + `bytecode.rs`): mirrored
  `Value::Future(Arc<FutureHandle>)` with `VmFutureState { Ready, Pending,
  Polled }`, async I/O natives (`http_get_async`, `tcp_probe`) registered in
  `register_natives`, a new `Op::Await` opcode (compiled from `Expr::Await`)
  that resolves a future via `runtime.block_on`. `async fn` on the VM compiles
  to a `Closure` whose body runs synchronously on call (so `await` on the
  result is a no-op) — see Errata.
- Parser: `async fn` is now recognised at statement level (`async` followed by
  `fn` dispatches to `parse_fn`, which consumes `async` as `is_async`); `async
  { ... }` remains an async block.

#### Error handling & edge cases
- **`await` outside an async context** just resolves the value (or returns a
  non-future unchanged) — Rak has no special "async context" requirement; the
  interpreter thread blocks at each `await`.
- **Panic in a task** → `Runtime("await: task failed: <join error>")`.
- **`block_on` on the interpreter thread** does not stall the Tokio runtime:
  async I/O tasks run on the multi-thread runtime's worker / blocking-pool
  threads; `block_on` only parks the interpreter thread.
- **Cancellation**: dropping a pending future aborts nothing eagerly (the
  Tokio task completes in the background); `await` is the only resolution
  point.

#### Errata (deviations from the original [SPEC])
- **`await for` and `select` are not implemented in this pass.** Concurrent
  fan-out (tens of thousands of scans) would require `join_all` / `select`
  primitives; deferred to a follow-up. The shipped subset covers `async fn`,
  `await`, `spawn`, `http_get_async`, `tcp_probe`, `tcp_connect_async`.
- **VM `async fn` is not deferred.** The bytecode VM has no interpreter to
  drive a deferred body, so `async fn` compiles to a plain `Closure` whose
  body runs synchronously on call; `await` on the (non-future) result returns
  it. Real async I/O (`http_get_async` / `tcp_probe`) does return a
  `Value::Future` on the VM and `Op::Await` blocks on it. Documented VM
  subset.
- **No `#[tokio::main]`**: the runtime is started lazily by the first
  `await`/async-builtin call, so `rakc run`/`rakc vm` need no special entry
  point.

---

### 2.2 Raw sockets & packet forging  **[SHIPPED]**

#### Syntax
```rak
// Build a SYN packet (IPv4 + TCP, SYN flag) — pure computation, no privileges.
let pkt = net_raw_tcp_syn("10.0.0.5", "10.0.0.10", 12345, 80)
dump len(pkt)                 // 40 bytes (20 IP + 20 TCP)
dump pkt[0]                   // 0x45 (IPv4 ver/ihl)
dump pkt[33]                  // 0x02 (TCP SYN flag)

// Custom IPv4 + TCP headers with payload.
let tcp_seg = net_raw_tcp("10.0.0.5", "10.0.0.10", 12345, 80, "SA", 0x1A2B3C4D, 0, b"hello")
let ip_pkt = net_raw_ipv4("10.0.0.5", "10.0.0.10", 6, tcp_seg)

// UDP segment.
let udp_seg = net_raw_udp("10.0.0.5", "10.0.0.10", 1234, 53, b"\x00")

// Ones-complement checksum helper.
dump net_raw_csum(pkt[0..20])  // 0 (the IP header is self-checking)

// Send / receive on a raw socket (returns a Result; needs CAP_NET_RAW).
dump net_raw_send(pkt)
let resp = net_raw_recv(4096)
```

> Builtins use the `net_raw_` prefix (`net_raw_ipv4`, `net_raw_tcp`,
> `net_raw_udp`, `net_raw_tcp_syn`, `net_raw_csum`, `net_raw_send`,
> `net_raw_recv`). Send/recv return a `Result` so callers handle the permission
> error without `try`/`catch`.

#### Architecture
- New crate deps `socket2` (raw socket creation) and `libc` (for `IP_HDRINCL`
  on unix).
- `stdlib/net_raw.rs`: `csum16` (ones-complement sum); `ipv4`, `tcp`, `udp`,
  `tcp_syn` builders that compute correct IP-header and TCP/UDP-over-IPv4-
  pseudo-header checksums; `send`/`recv` open a `SOCK_RAW` / `IPPROTO_RAW`
  socket (`IP_HDRINCL` on unix) and send/receive.
- `interpreter.rs` + `vm.rs`: `net_raw_*` builtins registered in both
  backends. Send/recv return `Value::Result`; on a permission failure the
  `Err` arm carries the OS message.
- VM: `Expr::Bytes` literals are now compiled to `Value::Bytes` constants,
  and `Op::IndexGet` handles `Value::Bytes` single-byte reads (and the
  interpreter adds `Bytes` range slicing → sub-`bytes`).

#### Error handling & edge cases
- **Permission denied** → `Err("net_raw: open raw socket failed (need
  CAP_NET_RAW/Administrator): <os err>")`, surfaced as the `Err` of a `Result`.
- **Checksums** are computed by the stdlib, not the kernel (`IP_HDRINCL`);
  an invalid checksum is *not* an error (forging malformed packets is
  intentional).
- **Platform**: send/recv are unix-only in this build (raw-socket I/O on
  Windows needs Npcap/Administrator and is not wired up); the packet
  builders are cross-platform. On Windows, `net_raw_send`/`recv` return
  `Err("... Windows raw-socket I/O not in this build")`.
- **Bad IPv4 address** → `Err("net_raw: bad IPv4 address '...'")`.

#### Errata (deviations from the original [SPEC])
- **Free-function `net_raw_*` builtins** instead of a `use net_raw` module
  with `net_raw.tcp_syn(...)` method syntax — Rak modules are file-based, so
  the prefix-builtin form is used.
- **Windows raw-socket I/O** (`WSAIoctl(SIO_RCVALL)` / Npcap) is not
  implemented; only the unix `SOCK_RAW` path is wired up. Packet builders
  work on all platforms.
- **Send/recv return a `Result`** (not a raw `int`/`bytes`) so the permission
  error is handleable without VM `try`/`catch` (which the VM subset does not
  implement).

---

### 2.3 Native protocol parsers  **[SHIPPED]**

#### Syntax
```rak
// DNS: build a query (offline) and do a real lookup (Result; Err offline).
let q = dns_build("example.com", "A")
dump len(q)
dump dns_query("example.com", "A")          // Ok({answers: [...], truncated: bool})
dump dns_parse(q)                             // parse a raw response

// TLS: parse a ClientHello's SNI / ciphers from raw bytes (offline).
let info = tls_parse_client_hello(captured_bytes)
dump info.sni
let certs = tls_parse_cert_chain(der_bytes)   // [{subject, issuer}, ...]

// PCAP: open an offline capture (needs --features pcap + libpcap/Npcap).
dump pcap_open("capture.pcap")                 // Ok(<pcap>) or Err(...)
let h = pcap_open("capture.pcap")?
let pkt = pcap_next(h)                        // {timestamp, linktype, payload} or nil
```

> Builtins use the `dns_*` / `tls_*` / `pcap_*` prefix. `dns_query` and
> `pcap_open` return a `Result` so they degrade gracefully (offline /
> feature-off).

#### Architecture
- **DNS** (`stdlib/dns.rs`): hand-written wire-format builder (`build_query`)
  and parser (`parse_response`) with compression-pointer decoding. Records:
  A/AAAA/MX/TXT/CNAME/NS/PTR/SOA. `query(name, rtype, server?)` sends a UDP
  datagram to `8.8.8.8:53` (default) via `std::net::UdpSocket` — no external
  DNS crate, works air-gapped with a local resolver.
- **TLS** (`stdlib/tls.rs`): `parse_client_hello(bytes)` decodes the TLS
  record + handshake layers to extract SNI (extension 0) and the cipher
  suite list; `parse_cert_chain(der)` walks a concatenated DER chain via
  `x509-parser` → `{subject, issuer}` per cert. Pure inspection (no TLS
  termination).
- **PCAP** (`stdlib/pcap.rs`): gated behind the `pcap` Cargo feature (the
  `pcap` crate needs libpcap/Npcap at build time). When the feature is off,
  `pcap_open` returns `Err("pcap: not built ...")`; when on, `open(path)`
  reads an offline capture and `next(handle)` yields
  `{timestamp, linktype, payload}`.
- `interpreter.rs` + `vm.rs`: `dns_*`, `tls_*`, `pcap_*` builtins registered
  in both backends; a `Value::Pcap(Arc<Mutex<PcapHandle>>)` variant holds the
  handle.

#### Error handling & edge cases
- **DNS truncation** (TC flag) → `truncated: true` in the response map.
- **Malformed wire bytes** → `Runtime("dns: ...")` / `"tls: ..."`, never a
  panic; all parsers bounds-check every slice.
- **DNS / PCAP failure** → the `Err` arm of a `Result` carries the OS / parse
  message, so callers handle it without `try`/`catch` (which the VM subset
  does not implement).
- **PCAP feature off** → `pcap_open` returns
  `Err("pcap: not built (build rak-stdlib with --features pcap; needs
  libpcap/Npcap)")`.

#### Errata (deviations from the original [SPEC])
- **Free-function `dns_*` / `tls_*` / `pcap_*` builtins** instead of `use
  net.dns` / `use tls.handshake` / `use pcap` module syntax — Rak modules are
  file-based, so prefix builtins are used.
- **Live `tls.inspect(host:port)`** (send a ClientHello, read ServerHello +
  cert chain) is **not implemented**; the offline `tls_parse_client_hello` /
  `tls_parse_cert_chain` parsers are. Live inspect is a follow-up.
- **PCAP `pcap_listen`** (live capture with a BPF filter) is not wired up;
  only offline `pcap_open` + `pcap_next`. The `pcap` crate dep is optional
  (default-off) because it requires libpcap/Npcap installed to build.
- **`dns_query` / `pcap_open` return a `Result`** (not a raw value) so the
  offline / feature-off / parse-failure cases are handleable on both
  backends.

---

### 2.4 Data processing pipelines

#### 2.4.1 Regex literals — see §1.2  **[SHIPPED]**

#### 2.4.2 Binary pattern matching — see §1.3  **[SHIPPED]**

#### 2.4.3 Pipeline operator — see §1.1  **[SHIPPED]**

The three pipeline-oriented data-processing features are implemented as
specified above. A representative end-to-end flow:

```rak
// Read a PCAP-magic file, slice it zero-copy, match the header, scan payload:
let m = mmap_open("trace.pcap", "r")
let hdr = mmap_slice(m, 0, 24)
match hdr {
    [0xD4, 0xC3, 0xB2, 0xA1, ..] => { dump "pcap little-endian" },
    [0xA1, 0xB2, 0xC3, 0xD4, ..] => { dump "pcap big-endian" },
    _ => { dump "not pcap" },
}
let payload = mmap_slice(m, 0x1000, 64)
let hits = payload |> regex_find_all(/\d{3}\.\d{3}\.\d{3}\.\d{3}/g)  // [..]
dump hits
mmap_close(m)
```

---

## Part 3 — Module system (imports & exports)

### 3.1 Imports & exports  **[SHIPPED]**

Python-style modules with explicit `pub`/`export`, name-based resolution,
`from ... import`, directory packages, import-once caching, and circular-import
support. Works on both the interpreter and the bytecode VM.

#### Syntax
```rak
// Whole-module import (binds the module; access via m.x)
import "./math.rak"          // file path -> binds "math"
import math                  // name: dir -> ./packages/ -> RAK_PATH, m.rak or m/init.rak
import math as m             // alias
use math                     // back-compat: same as import

// from-import (binds names directly)
from math import add
from math import add as plus, mul as times
from math import *           // all exports; local bindings win on clash

// Directory packages
import pkg                   // runs pkg/init.rak
import pkg.sub               // runs pkg/init.rak + pkg/sub.rak (interpreter)

// Exports (pub and export are equivalent)
pub let PI = 3.14
export fn add(a, b) { return a + b }
pub const MAX = 256
pub struct Vec3 { x: int, y: int, z: int }

// Re-exports
pub use math                 // re-export all of math from this module
pub use {add, mul} from math // re-export named
```

#### Architecture
- `ast.rs`: `Import` extended with `kind: ImportKind { Whole, From }`,
  `from_names: Vec<(String, Option<String>)>`, `star`, `reexport`.
- `lexer.rs`: `import`, `from`, `export` keywords (alongside `pub`/`use`).
- `parser.rs`: `parse_module` collects `import`/`from`/`use`/`pub use ...`/
  `pub from ... import`; `parse_stmt` treats `export` like `pub` (wraps in
  `Stmt::Export`).
- `modules.rs` (new): `resolve_dotted(importer_dir, parts)` searches
  `importer_dir`, `./packages/`, `RAK_PATH` (in order) for `<name>.rak` then
  `<name>/init.rak`. Shared by both backends.
- Interpreter: a `module_cache: HashMap<PathBuf, ModuleEntry>` (exports +
  macros) + `loading_modules` set. `load_module_file` inserts an empty entry
  before executing (so circular imports see a partial), runs the module's own
  imports + items, collecting every `Export(<kind>)` into the entry. `load_import`
  handles Whole/From/Reexport, directory packages (`pkg` bound as a Module
  containing `sub`), `import *` (locals win), and macro import (registers in
  `self.macros`).
- VM (`compiler.rs` + `vm.rs` + `bytecode.rs`): a compile-time `module_cache`
  + `compiling` set; `inline_module` compiles each imported module's exported
  items as globals in the current chunk (recursively, cached, cycle-aware). A
  new `Op::BuildModule` builds a `Value::Module` from a list of exported global
  names at run time; `from m import x` copies/aliases globals; `from m import *`
  is a no-op (already inlined); re-exports add to the module's export list.
  `compile_module_in(module, base_dir)` resolves name-based imports relative to
  the file.

#### Error handling & edge cases
- **Missing module** → `Runtime("import: cannot find module 'm' (searched: dir, packages, RAK_PATH)")`.
- **Missing name** → `Runtime("from m import x: 'x' is not exported")`.
- **Circular imports** → return the partially-built entry (Python semantics);
  a name not yet defined at the cycle point is a `Undefined variable` error.
- **Import-once** → modules are cached by canonical path; re-importing returns
  the cached entry without re-running.
- **`from m import *`** → never overwrites an existing local binding.
- **`pub`/`export`** mark items exported; unmarked top-level names are private
  to the module.

#### Errata (VM subset)
- **`import pkg.sub`** (directory-package nested access) is interpreter-only;
  the VM's flat-globals architecture can't isolate per-module scopes. Use
  `from pkg.sub import x` on the VM.
- **`pub struct`/`pub enum`** are exported on the interpreter; the VM (which
  has no `Value::StructDef`/`EnumDef`) errors on struct/enum export — use
  `pub let`/`fn`/`const`/`macro` for cross-VM modules.
- **Non-pub top-level** of an imported module is inlined as globals on the VM
  (visible to the importer); the interpreter keeps them in the module's
  private scope.

---

## Cross-cutting concerns

### Error model
- **Lexer** errors carry line/column (`offset_to_line_col`); `rakc check`
  emits them in an IDE-friendly form.
- **Parser** errors carry line/column via `perr`.
- **Runtime** errors are `RakError::Runtime`/`RakError::Raise`; `try`/`catch`/
  `raise` and the `?` operator propagate them. Native FFI / mmap / net_raw
  failures convert `std::io::Error`/OS errors into `RakError::Runtime`, never
  panicking the VM.
- **Resource cleanup** uses RAII: `Arc<MmapHandle>`, `Arc<Mutex<TcpStream>>`,
  `ForeignLib` drop handlers call `munmap`/`dlclose`/`FreeLibrary`/`shutdown`
  automatically. There are no manual `close` calls required for safety — the
  explicit `mmap_close`/`tcp_close`/`lib.close` are convenience + early
  release.

### VM vs interpreter coverage
| Feature | Interpreter | VM |
|---------|:-----------:|:--:|
| Pipeline `\|>` | yes | yes (desugar) |
| Regex literals | yes (methods + builtins) | yes (builtins) |
| Binary pattern matching | yes | no (graceful error) |
| Trait protocols | yes | no (graceful error) |
| Method-call dispatch | yes | no (no dispatch layer) |
| FFI | yes | yes (natives + `Op::FFICall`/`Op::FFIClose`) |
| Memory-mapped files | yes (slice/range/pattern) | yes (natives + single-byte index) |
| Async (`async fn`/`await`/I/O futures) | yes (deferred bodies + I/O) | yes (`Op::Await` + I/O natives; `async fn` runs sync) |
| Raw sockets (packet forging) | yes (builders + unix send/recv) | yes (builders + `Result` send/recv) |
| DNS / TLS / PCAP | yes (dns_query/build/parse, tls parse, pcap open/next) | yes (same builtins; `Result` for query/open) |
| Compile-time macros (`macro`/`name!`/`const`) | yes (expand + exec) | yes (expand at compile time) |
| Imports & exports (`import`/`from`/`pub`) | yes (whole/from/star/pkg/cycles) | yes (whole/from/star; no `pkg.sub` nesting) |
| Forensic Structs (`binstruct`/`.decode`/`.encode`) | yes (registry + codec) | yes (compile-time baked natives, no new opcodes) |
| Evidence provenance (`evidence<T>`/`cite`/`report`) | yes (Value::Evidence + builtins) | yes (mirrored Value::Evidence + natives) |

The VM's `compile_stmt`/`compile_expr`/`compile_pattern` return
`Err("VM does not support ...")` for unsupported nodes, so running an
interpreter-only program on the VM fails fast with a clear message rather
than producing wrong results.

### Verification
- `cargo test -p rakc` — 68 unit tests pass, including new tests for the
  pipeline desugar, regex lexing/matching, char literals, binary patterns,
  and all five trait protocols (Display, Debug, Iterable, Index, IndexMut).
- Example programs in `examples/pipeline.rak`, `examples/regex.rak`,
  `examples/binary_patterns.rak`, `examples/traits.rak` run on `rakc run`.

---

## Part 4 — Forensic Structs & evidence provenance  **[SHIPPED]**

Two fused features, neither of which exists in any production language, and
both of which only make sense in a hex-first, OSINT-first language:

1. **`binstruct`** — a declarative wire-format DSL that compiles to both a
   decoder and an encoder (round-trip), the Kaitai-Struct / Zig-comptime dream
   as a native language construct.
2. **`evidence<T>`** — provenance as a first-class type-system layer: every
   collected value carries where/when/how it was collected, merging
   transitively through `cite`/pipelines, so `report(...)` emits a defensible,
   chain-of-custody-cited findings report.

### Syntax

```rak
binstruct DnsHeader {
    id:      u16be
    flags:   u16be
    qdcount: u16be
    ancount: u16be
    nscount: u16be
    arcount: u16be
}

let h = DnsHeader.decode(bytes)     // -> evidence<struct> (auto-provenance)
dump h.id                            // 0x1234
let raw = DnsHeader.encode(h)        // round-trip back to bytes

let ip = evidence<string> from "93.184.216.34"
let cited = cite(dns_query("example.com", "A"), "dns_query", "example.com")
dump report(ip, cited)               // numbered assertions + cited sources
```

Field types: `u8`/`u16`/`u32`/`u64` and signed `i8`..`i64`, each with an
optional `be`/`le` endianness suffix (default big-endian); `bytes(n)` for a
fixed run of raw bytes; `rest` for the trailing remainder; and a nested
binstruct name for a `Ref` field.

### Architecture

- `lexer.rs`: `binstruct` and `evidence` keywords.
- `ast.rs`: `Stmt::BinStructDef { name, fields: Vec<BinField> }`, `BinField`
  `{ name, kind, repeat }`, `BinKind { Uint{bits,endian}, Int{bits,endian},
  Bytes(usize), Rest, Ref(String) }`, `Endian { Big, Little }`,
  `Type::Evidence(Box<Type>)`, `Expr::EvidenceFrom { value }`.
- `parser.rs`: `parse_binstruct` (field list with `name: type` + optional
  `repeat: <expr>`), `parse_bin_kind` (width+endianness, `bytes(n)`, `rest`,
  nested name), `parse_evidence` (`evidence<T> from expr`), and `evidence<T>`
  in `parse_type`.
- Interpreter (`interpreter.rs`): a `binstructs: HashMap<String, Vec<BinField>>`
  registry populated by `Stmt::BinStructDef`; `Name.decode(bytes)` /
  `Name.encode(value)` dispatched in `eval_call`'s method-call branch when the
  receiver `Ident` is a registered binstruct. `Value::Evidence { inner,
  provenance }` with a `Provenance { tool, target, ts, raw_offset, raw_len,
  parent }` chain. Builtins `report`, `cite`, `strip_evidence`, `provenance`.
  Field access / indexing transparently unwrap evidence.
- VM (`compiler.rs` + `value.rs` + `vm.rs`): **compile-time codegen to VM — no
  new opcodes.** The compiler pre-pass bakes a self-contained
  `Value::NativeFn` for each binstruct's decode and encode (with `Ref` fields
  resolved recursively at bake time into a `ResolvedBinField` tree), registered
  as globals `__bin_decode_<Name>` / `__bin_encode_<Name>`. `Name.decode(...)`
  lowers to `Op::LoadGlobal; Op::Call` on that native. `evidence<T> from expr`
  lowers to a call to the `__evidence_from` native. `Value::Evidence` is
  mirrored in `value.rs` with its own `Provenance`; `FieldGet`/`IndexGet`
  unwrap evidence; `report`/`cite`/`strip_evidence`/`provenance` are registered
  as VM natives.

### Error handling & edge cases
- **Unknown binstruct** in `.decode`/`.encode` or a nested `Ref` →
  `Runtime/Compile("unknown binstruct '...'")`.
- **Field outruns buffer** → `Runtime("binstruct: field '...' outruns buffer
  (off+n > len)")`; never a panic, all reads are bounds-checked.
- **Circular binstruct ref** → `Compile("binstruct '...': circular ref")`
  (detected at resolve time).
- **Non-byte-aligned widths** (`u4`, `u6`) fall through to a nested-`Ref`
  lookup and error as unknown — nibble/bitfield support is future work; the
  shipped subset covers all byte-aligned wire formats (DNS, TCP, IPv4, UDP,
  TLS records).
- **`encode` on a non-struct/map** → `Runtime("encode expects struct/map,
  got <type>")`.
- **Evidence is observational, not a barrier**: `evidence<struct>.field`
  reads through to the inner struct; `==` compares inner values, ignoring
  provenance; `is_truthy`/`as_i64`/display unwrap.

### Errata (deviations from the original [SPEC])
- **Auto-wrapping collectors** was deliberately NOT done — it would change the
  return shape of every collector and break the existing 121 tests. Instead,
  provenance is **explicit** via `evidence<T> from` (root tag) and `cite(value,
  tool, target)` (chained tag), which is non-magic and keeps existing
  behaviour intact. Auto-propagation through `|>` remains future work.
- **Dogfooding** is demonstrated by `examples/forensic_structs.rak`, which
  decodes real DNS queries produced by the stdlib `dns_build()` and verifies
  the binstruct layout matches the Rust builder's wire bytes. The hand-rolled
  Rust builders in `stdlib/dns.rs` / `stdlib/net_raw.rs` are kept (the
  packet-forging path needs ones-complement checksums and platform sockets
  that binstruct encoding doesn't express); the binstruct path is the
  declarative *inspection* layer alongside them.
- **Nibble/bitfield fields** (`u4`, `u6`) are not implemented — only
  byte-aligned widths (multiples of 8, 8..64) are accepted. This covers all
  shipped wire formats; bit-level packing is a follow-up.

### Verification
- `cargo test -p rakc` — 129 unit tests pass, including new tests for
  binstruct decode/encode round-trip, nested `Ref`, DNS-header-against-stdlib
  dogfood, evidence `from`/`report`, and `cite` chain provenance — on both the
  interpreter and the VM.
- `examples/forensic_structs.rak` runs on both `rakc run` and `rakc vm`,
  decoding real DNS queries, round-tripping, and emitting a cited report.
