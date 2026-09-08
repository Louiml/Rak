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

### 1.5 Foreign Function Interface (FFI)  **[SPEC]**

#### Syntax
```rak
// 1. Declarative C bindings (preferred):
extern "C" {
    fn printf(fmt: *u8, ...) -> i32
    fn getpid() -> i32
}

// 2. Dynamic loader:
let libc = ffi_load("libc.so.6")           // "libc.dylib" / "msvcrt.dll"
let pid = libc.call("getpid", [])          // -> i64
libc.close()

// 3. Marshalling helpers:
let p = ffi_ptr(0xDEADBEEF)               // raw pointer value
let buf = ffi_alloc(256)                  // -> *u8  (ffi-managed, freed on GC)
ffi_write(buf, 0, 0x41)
let s = ffi_cstr_to_string(ptr)           // *i8 -> string (copies, NUL-terminated)
let cs = ffi_string_to_cstr(s)            // string -> *i8 (NUL-terminated, ffi-managed)
```

#### Architecture
- New crate dep `libffi` (for portable `ffi_call` with arbitrary signatures)
and `libc`/`windows-sys` for `dlopen`/`dlsym`/`LoadLibrary`/`GetProcAddress`.
- `ast.rs`: `Stmt::Extern { abi: String, decls: Vec<ForeignFn> }` and
  `Expr::FFICall { lib: Box<Expr>, symbol: String, args: Vec<Expr>, ret: Type }`.
  `ForeignFn { name, params: Vec<Param>, varargs: bool, return_type: Type }`.
- `lexer.rs`: `extern` keyword; `"C"` (and later `"stdcall"`) as a string
  literal after `extern`.
- `parser.rs`: `parse_extern` parses the ABI string and a block of `fn`
  signatures, including a trailing `...` for varargs.
- `interpreter.rs`: a `Value::ForeignLib(Arc<Mutex<LibHandle>>)` holding the
  OS handle, and `Value::ForeignPtr(usize)` for raw pointers. `extern` blocks
  register each symbol as a `NativeFn` that, on call:
  1. Marshals each `Value` arg to a C ABI slot via `libffi`'s `CType`:
     `i8..i64`/`u8..u64` → ints; `*u8`/`*i8` → pointer (from `ForeignPtr` or
     ffi-managed buffer); `string` → NUL-terminated `CString` (owned, freed
     after the call); `bytes` → pointer to the `Arc<[u8]>`'s data (valid for
     the call's duration).
  2. Calls `libffi::call` with the prepared CIF and a buffer for the return
     value.
  3. Unmarshals the return value to a Rak `Value` per `ret` type.
- `stdlib/ffi.rs` (new) wraps `dlopen`/`dlsym`/`dlclose` (Unix) and
  `LoadLibraryA`/`GetProcAddress`/`FreeLibrary` (Windows) behind one
  `LibHandle` enum.

#### Error handling & edge cases
- **Safety**: FFI is inherently `unsafe`. Every `extern` block requires the
  `--ffi` build flag (compile error otherwise) to gate it. A future `unsafe`
  keyword on the block is the planned UX.
- **Symbol not found** → `Runtime("ffi: symbol 'foo' not found in <lib>")`.
- **Library load failure** → `Runtime("ffi: cannot load 'libc.so.6': <os err>")`.
- **Marshalling failure** (e.g. passing a map where a pointer is expected) →
  `Runtime("ffi: cannot marshal <type> to <CType>")`. Never a panic; all
  `libffi` calls go through a `catch_unwind` boundary.
- **String lifetime**: strings passed to C are `CString`s owned by the call
  frame and freed when the call returns — the C side must not retain them.
  `ffi_string_to_cstr` returns an ffi-managed pointer with a deterministic
  free (`ffi_free`) to bridge to C APIs that retain buffers.
- **Varargs**: `printf`-style `...` is supported by `libffi`'s CIF builder
  with per-call argument types.
- **Pointers are opaque `usize`**: dereferencing is explicit via
  `ffi_read`/`ffi_write` with a `Type`, so the runtime never dereferences a
  raw pointer by accident.

---

### 1.6 Memory-mapped files & zero-copy I/O  **[SPEC]**

#### Syntax
```rak
let m = mmap_open("huge.pcap", "r")      // "r" | "rw" | "rw_new"
let slice = mmap_slice(m, 0x1000, 64)     // -> bytes (zero-copy view into the map)
let magic = slice[0..4]                   // bytes view, no copy
mmap_close(m)

// Zero-copy inspection helpers:
for line in mmap_lines(m, "\n") { ... }   // iterates without materialising
let off = mmap_find(m, b"\xff\xd8\xff")   // byte search, returns offset
let n = mmap_size(m)
```

#### Architecture
- New crate dep `memmap2` (cross-platform `Mmap`/`MmapMut`).
- `value.rs` and `interpreter.rs`: `Value::Mmap(Arc<MmapHandle>)` where
  `MmapHandle { map: memmap2::Mmap, file: File, writable: bool }`.
- `stdlib/mmap.rs` (new): `open(path, mode)`, `slice(handle, off, len) ->
  &[u8]`, `close(handle)`, `size`, `find`, `lines`.
- `mmap_slice` returns a `Value::Bytes` whose `Arc<[u8]>` is an
  `Arc::from(&map[off..off+len])` — a **zero-copy** sub-slice of the mapped
  region (Rust's `Arc<[u8]>` from a `&[u8]` copies; to truly avoid copies we
  return a `Value::MmapSlice { map: Arc<MmapHandle>, off, len }` variant so the
  backing mapping is kept alive). Indexing/range over an `MmapSlice` reads
  directly from the mapped pages.

#### Error handling & edge cases
- **Unmapping**: `mmap_close` drops the `Arc<MmapHandle>`; slices derived from
  it keep their own `Arc` so they remain valid until they themselves drop
  (Rust's `Arc` refcount guarantees the mapping outlives all slices).
- **Out-of-bounds slice** → `Runtime("mmap_slice: [off,off+len) out of range")`.
- **File open / mmap failure** → `Runtime("mmap_open: <os err>")`.
- **Write to read-only map** → `Runtime("mmap: map is read-only")`.
- **RAM**: a 4 GiB PCAP is mapped, not loaded — peak RAM is the OS page cache
  for touched pages plus the slice metadata. Documented `Value::MmapSlice`
  keeps the `Mmap` alive, preventing use-after-unmap.
- **`mmap_lines`** yields owned `String`s per line (a copy is unavoidable for
  safe iteration); for true zero-copy line scanning, expose `mmap_lines_off`
  returning `(offset, len)` pairs.

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

### 2.1 Built-in async I/O & event loop  **[SPEC]**

#### Syntax
```rak
async fn fetch(host: string) -> string {
    let r = await http_get_async("https://" + host)
    return r.body
}

// Concurrent scan of tens of thousands of hosts:
let results = {}
await for host in targets {
    let banner = await tcp_probe(host, 80)
    results[host] = banner
}

// event-driven select
select {
    msg = rx.recv()    => { dump msg },
    _ = timeout(1000)  => { dump "timed out" },
}
```

#### Architecture
- New crate dep `tokio` (already a workspace dep) with `features =
  ["rt-multi-thread", "net", "io-util", "macros", "time"]`.
- A `Runtime` thread is started lazily on first `await`/`spawn` of an async
  block: a `tokio::runtime::Runtime` stored in a `once_cell::sync::Lazy` /
  `Arc<Runtime>`.
- `ast.rs`: `Expr::Function { ..., is_async: true }` already exists;
  `Expr::Await` already exists (currently a no-op in the interpreter). New
  `Stmt::AwaitFor`, `Expr::Select { arms: Vec<(Pattern, Expr, Vec<Stmt>)> }`.
- `value.rs`/`interpreter.rs`: `Value::Future(Arc<FutureHandle>)` where
  `FutureHandle` is a `Mutex<Option<tokio::sync::oneshot::Receiver<Value>>>`
  or a `tokio::task::JoinHandle`. `await` polls the runtime (blocking the
  interpreter thread via `tokio::runtime::Handle::block_on`) and resumes.
- Async networking lives in `stdlib/net_async.rs` (`http_get_async`,
  `tcp_probe`, `tcp_connect_async`) returning `Value::Future`.
- The event loop is **epoll** on Linux / **IOCP** on Windows via Tokio's
  `mio`/`windows` internals — Rak itself is unaware of the platform driver.

#### Error handling & edge cases
- **Blocking the runtime**: the tree-walker `await` uses `block_on` on the
  interpreter thread; async tasks run on the multi-thread Tokio runtime, so
  `await` does not stall the executor.
- **No thread-per-connection**: tens of thousands of concurrent TCP probes
  run as tasks on the Tokio runtime with ~2 KB stacks each, vs the current
  `std::thread::spawn` (~2 MB stacks).
- **`await` outside `async`** → `Runtime("await outside async context")`.
- **`select` fairness** → each branch is polled once per iteration; the first
  ready wins; ties resolve to the first arm (documented).
- **Cancellation**: dropping a `FutureHandle` aborts the Tokio task
  (`JoinHandle::abort`) to release resources.
- **Panic in a task** → propagated as `Runtime("task panicked: <msg>")` on
  `await`.

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
| FFI / mmap / macros / async / raw sockets / DNS / TLS / PCAP | spec | spec |

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
