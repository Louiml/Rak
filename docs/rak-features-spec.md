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

### 1.7 Metaprogramming & macros  **[SPEC]**

#### Syntax
```rak
// Hygienic AST macros, macro_rules!-style:
macro_rules! json_obj {
    ($($k:literal : $v:expr),* $(,)?) => {
        map([$($k, $v),*])
    }
}
let m = json_obj! { "a": 1, "b": 2 }

// Compile-time `const` evaluation for schema validation:
const MAX_LEN = 256
fn validate(buf: bytes) {
    if len(buf) > MAX_LEN { raise "too long" }
}

// AST transformation functions (proc-macro analogue):
macro assert_nonneg(x: expr) {
    if $x < 0 { raise fmt("negative: {}", $x) }
}
assert_nonneg!(port)
```

#### Architecture
- `ast.rs`: `Stmt::MacroRules { name, arms: Vec<MacroArm> }` with
  `MacroArm { pat: TokenTree, body: TokenTree }`, and `Expr::MacroInvoke {
  name, tokens: TokenTree }`.
- `lexer.rs`/`parser.rs`: a `TokenTree` type (delimited groups + tokens) is
  captured raw during parsing so macro bodies can be re-parsed with
  substitutions. `macro_rules!` is parsed into `Stmt::MacroRules` and stored in
  a compile-time `MacroRegistry`. `name!(...)` / `name!{...}` invocations
  expand **before** the rest of the AST is finalised.
- `compiler.rs` (new `macro_expand.rs` pass): pattern-matches the invocation's
  `TokenTree` against each arm, binds fragment variables (`:expr`, `:literal`,
  `:ident`, `:tt`), substitutes into the body, and re-parses the result into
  an `Expr`/`Stmt` which replaces the invocation node.
- `const` is a compile-time `let` evaluated by the interpreter at build time;
  the resulting `Value` is inlined wherever the const name appears.
- `macro` (proc-macro analogue) is a Rak function tagged `#[macro]` that
  receives the AST of its arguments as `Value::Struct` nodes and returns an
  AST node to splice in.

#### Error handling & edge cases
- **Macro not found** → `Compile("undefined macro 'foo!'")`.
- **No matching arm** → `Compile("macro 'foo!' has no arm matching ...")` with
  the offending token slice printed.
- **Hygiene**: macro-introduced identifiers are renamed with a per-expansion
  suffix to avoid capturing user bindings, matching `macro_rules!` hygiene.
- **Recursion limit**: macro expansion is capped (default 64) to prevent
  infinite self-expansion → `Compile("macro expansion depth exceeded")`.
- **`const` evaluation failure** → compile error with the runtime message.
- Macros are expanded in the frontend, so both backends see the expanded AST.

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

### 2.2 Raw sockets & packet forging  **[SPEC]**

#### Syntax
```rak
use net_raw

// SYN scan a target:
let pkt = net_raw.tcp_syn(src="10.0.0.5", dst="10.0.0.10", dport=80)
net_raw.send(pkt)

// Custom IPv4 + TCP headers:
let ip = net_raw.ipv4(src="10.0.0.5", dst="10.0.0.10", proto=6, payload=tcp_bytes)
let tcp = net_raw.tcp(src=12345, dst=80, flags="S", seq=0x1A2B3C4D, payload=b"")
net_raw.send(ip + tcp)

// Capture flags from a response:
let resp = net_raw.recv(4096)
match resp {
    [0x45, ..] => { dump "IPv4 reply" },
    _ => { dump "other" },
}
```

#### Architecture
- New crate deps `socket2` (raw socket creation, `SOCK_RAW`,
  `IPPROTO_TCP`/`IPPROTO_RAW`), `libc` for `setsockopt(IP_HDRINCL)`.
- `stdlib/net_raw.rs` (new):
  - `tcp_syn`, `tcp`, `ipv4`, `udp` builders returning `Value::Bytes` with
    correct checksums (IP header checksum, TCP/UDP checksum over the IPv4
    pseudo-header). A `csum16` helper computes the ones-complement sum.
  - `send(buf)`, `recv(max)`, `open_iface(name)`.
- Requires `CAP_NET_RAW` / Administrator / `SO_RAW` — surfaced as a clear
  runtime error if missing.

#### Error handling & edge cases
- **Permission denied** → `Runtime("net_raw: CAP_NET_RAW required (run as
  root/Administrator)")`.
- **Checksums** are computed by the stdlib, not the kernel, when
  `IP_HDRINCL` is set; an invalid checksum is *not* an error (forging
  malformed packets is intentional) but is logged via `trace`.
- **Platform**: raw sockets on Windows use `WSAIoctl(SIO_RCVALL)` on a
  `SOCK_RAW` bound to an interface; the same `net_raw` API is exposed.
- **Buffer sizes** are bounds-checked; oversized payloads are truncated with
  a warning, not a panic.

---

### 2.3 Native protocol parsers  **[SPEC]**

#### Syntax
```rak
use net.dns
let resp = dns.query("example.com", "A")          // -> DnsResponse
for a in resp.answers { dump a.rdata }             // 93.184.216.34
let pkt = dns.build("example.com", "MX", recurse=true)
dns.send("8.8.8.8", pkt)

use tls.handshake
let info = tls.inspect("example.com:443")         // -> { sni, cipher, cert_chain }
dump info.sni
for cert in info.cert_chain { dump cert.subject; dump cert.issuer }

use pcap
let cap = pcap.pcap_listen("eth0", "tcp port 80")  // live capture
for pkt in cap { dump pkt.timestamp; dump pkt.payload }
let f = pcap.pcap_open("capture.pcap")            // offline
for pkt in f { ... }
```

#### Architecture
- **DNS** (`stdlib/dns.rs`): a hand-written wire-format
  builder/`parser`. Header (id, flags, qdcount, ...) + question/answer
  records. `Value::Struct` for `DnsResponse { answers: array<DnsRecord> }`.
  Record types A/AAAA/MX/TXT/CNAME/PTR/NS/SOA. No external DNS crate — direct
  UDP `socket2` to port 53, so it works in air-gapped OSINT setups.
- **TLS** (`stdlib/tls.rs`): a `ClientHello`/`ServerHello` parser that reads
  the raw handshake bytes from a `tcp_connect` socket (no TLS termination —
  pure inspection). Extracts SNI from the ClientHello extension, the cipher
  suite list, and parses the certificate chain (DER→spki/issuer/subject via
  `x509-parser` crate). Useful for passive SNI enumeration and JA3/JA4
  fingerprinting.
- **PCAP** (`stdlib/pcap.rs`): wraps the `pcap` crate (libpcap/Npcap). Live
  `pcap_listen(iface, bpf)` and offline `pcap_open(path)`. Each packet is a
  `Value::Struct { timestamp: i64, linktype: int, payload: bytes }`. The
  iterator is `Iterable`-compatible (works with `for x in cap`).

#### Error handling & edge cases
- **DNS truncation** (TC flag) → the parser returns the partial answers plus
  `Runtime("dns: truncated response, retry over TCP")` guidance.
- **Malformed wire bytes** → `Runtime("dns: bad offset / truncated record")`,
  never a panic; all parsers bounds-check every slice.
- **TLS inspection** stops at the first unparseable record and returns what
  was decoded plus a `partial: true` flag, so a broken server doesn't lose
  the SNI that was already observed.
- **libpcap/Npcap missing** → `Runtime("pcap: libpcap not found ...")` with
  platform install hints. The feature is gated behind `--features pcap`.
- **BPF compile error** → `Runtime("pcap: bad filter '...': <msg>")`.

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
| raw sockets / DNS / TLS / PCAP / macros | spec | spec |

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
