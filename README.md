# Rak

A programming language for hackers, OSINT investigators, and systems programmers. Build desktop GUI apps, backend SQL servers, shareable packages, and self-host the compiler. All in Rak.

Hex is a first-class type. The bytecode VM runs about 6x faster than the tree-walker. There's a SQL engine written in Rak itself, and a Rak interpreter written in Rak.

## Quick start

```rak
dump "Hello, World"
```

```rak
scan "127.0.0.1" {
    range: [0x0016, 0x0050]
} {
    if open {
        dump fmt("Port 0x{:04X}", port)
    }
}
```

## What you get

The language started as an OSINT scripting tool. It grew into something bigger.

**Bytecode VM.** `rakc vm` runs bytecode. Benchmarks show about 6x speedup over the tree-walking interpreter. Run `rakc bench file.rak` to see both.

**Data pipelines.** A `|>` pipeline operator, regex literals (`/\d+/g`) with method syntax, and binary pattern matching over byte slices — built for OSINT log and PCAP triage.

**Type system.** Signed and unsigned ints (i8 through i64, u8 through u64), floats (f32, f64), typed literals like `42i32` and `3.14f64`, tuples, Result/Option, modules, closures with lexical scoping, generics, traits with method dispatch, pattern matching (including binary byte-pattern matching), and string interpolation with `f"hello {name}"`.

**Concurrency.** `spawn` launches a thread, `thread_join` waits for it, `channel()` gives you a sender/receiver pair. TCP networking built in with `net_listen`, `net_accept`, `tcp_read`, `tcp_write`.

**SQL server in Rak.** A full SQL engine, lexer through executor, written in the language itself. It serves queries over TCP. See `examples/sql_server.rak`.

**Self-hosting.** `examples/self_host.rak` is a Rak interpreter written in Rak. It lexes, parses, and evaluates real Rak source code.

**Error handling.** `try`/`catch`/`raise`, the `?` operator, Result and Option types. Lexer and parser errors report line and column. `rakc check file.rak` prints diagnostics in the format IDEs expect.

**Lexical closures.** Free variables resolve at the definition site, not the call site. Recursive closures like `let f = fn(n) { if n < 2 { return n } return f(n-1) + f(n-2) }` just work.

**Real JSON.** `json_parse` returns native arrays, maps, ints, floats, bools, nil. `json_stringify` serializes back to a string. No more hand-rolling JSON in your SQL server.

**Build standalone executables.** `rakc build file.rak` produces a self-contained binary with the source embedded. On Windows it's a `.exe`. On Linux it gets `chmod 755`. No Rak installation needed on the target machine.

**GUI windows.** With `--features gui`, Rak scripts can open native desktop windows rendering HTML, CSS, and JS. Uses WebView2 on Windows, WebKitGTK on Linux. JavaScript inside the window can call back into Rak through `window.rak_call(fn, args)`.

**Package manager.** `rakpkg` is a CLI for Git-based shareable Rak packages. Initialize, add dependencies from GitHub repos, install, run, and build. The manifest is a Rak file with `let` bindings.

**Foreign Function Interface (FFI).** Call native C functions in `.so`/`.dll`/`.dylib` libraries. Declare bindings with `extern "C" { ... }` (resolved against the platform default C library, or an explicit `from "path"`) or load dynamically with `ffi_load` and `lib.call`. Marshal raw memory with `ffi_alloc`/`ffi_write`/`ffi_read`/`ffi_cstr_to_string`/`ffi_string_to_cstr`/`ffi_free`. Works on both the interpreter and the bytecode VM.

```rak
extern "C" {
    fn abs(n: i32) -> i32
}
dump abs(-42)                    // 42

let libc = ffi_load("libc.so.6") // or "ucrtbase.dll" / "libSystem.dylib"
dump libc.call("abs", [-9])      // 9
libc.close()

let buf = ffi_alloc(4)
ffi_write(buf, 0, 0x41)
dump ffi_cstr_to_string(buf)     // "A"
ffi_free(buf)
```

## Build a standalone executable

```bash
rakc build examples/hello.rak   # produces hello.exe on Windows, hello on Linux
./hello                          # [DUMP] Hello, World
```

## GUI windows

Requires `cargo build --features gui`.

```rak
let html = "<h1 style='color:#22c55e;text-align:center;margin-top:40px'>Hello from Rak!</h1>
<button onclick=\"window.rak_call('clicked')\" style='display:block;margin:20px auto;padding:10px 30px;font-size:18px;background:#22c55e;color:white;border:none;border-radius:8px;cursor:pointer'>Click me</button>"

let win = gui_open("Rak GUI Demo", html, 600, 400)
gui_wait()
```

`gui_open` returns a window ID. `gui_update(id, html)` changes the content. `gui_title(id, title)` changes the title. `gui_close(id)` closes it. `gui_wait()` blocks until all windows close. `gui_callback(name)` registers a Rak function as callable from JS.

## Package manager

```bash
rakpkg init mylib           # creates package.rak + lib.rak
rakpkg add user/repo        # git clone into .rak/packages/
rakpkg install              # install all deps from package.rak
rakpkg run                  # run the entry point via rakc run
rakpkg build                # build to standalone executable via rakc build
rakpkg list                 # list installed packages
rakpkg remove mylib         # remove a package
```

The manifest is a Rak file:

```rak
let name = "mylib"
let version = "0.1.0"
let deps = { net: "user/rak-net", crypto: "user/rak-crypto" }
let entry = "lib.rak"
```

Import installed packages by file path. No new syntax needed:

```rak
use "./packages/mylib/lib.rak"
```

## SQL server

```bash
rakc run examples/sql_server.rak   # listens on 127.0.0.1:18393
rakc run examples/sql_client.rak    # CREATE, INSERT, SELECT, UPDATE, DELETE
```

The engine supports CREATE TABLE, INSERT, SELECT with WHERE and column projection, UPDATE, DELETE, and DROP. Storage is file-backed with a simple line format. The server handles each connection inline, parsing SQL and returning JSON.

## Self-hosting

```bash
rakc run examples/self_host.rak    # a Rak interpreter written in Rak
```

It reads Rak source, lexes it, parses it, and evaluates it. The output of `1 + 2 * 3` is 7. The output of `let x = 5; let y = 7; dump x * y - 1` is 34.

## Language syntax

### Variables

```rak
let target = "192.168.1.1"
let port = 0x0050
let mut counter = 0
counter = counter + 1
let pi = 3.14f64
let n = 42i32
```

### Functions

```rak
fn add(a, b) {
    return a + b
}
dump add(0x10, 0x20)
```

### Control flow

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

### Hex arithmetic

```rak
let sig = 0xDEADBEEF
let mask = 0xFF00FF00
dump fmt("0x{:08X}", sig & mask)
```

### Error handling

```rak
let v = Ok(42)?     // unwrap, propagate Err
try {
    raise "boom"
} catch e {
    dump e
}
```

### String interpolation

```rak
let name = "Rak"
dump f"hello {name}!"
```

### Tuples and structs

```rak
let p = (1, 2)
dump p.1
struct Point { x: int, y: int }
let o = Point { x: 1, y: 2 }
dump o.x
```

### Pattern matching

```rak
match 2 {
    1 => { dump "one" },
    2 => { dump "two" },
    _ => { dump "other" }
}
```

### Pipeline operator

`x |> f` desugars to `f(x)`, and `x |> f(a, b)` to `f(x, a, b)`. Chaining is
left-associative, so `data |> parse |> load` is `load(parse(data))`. Works on
both the interpreter and the bytecode VM.

```rak
fn inc(n) { return n + 1 }
fn dbl(n) { return n * 2 }
dump 5 |> inc |> dbl          // 12
dump 3 |> add(10)             // 13
```

### Regex literals

`/pattern/flags` with flags `i` (case-insensitive), `m` (multi-line), `s`
(dotall), `x` (extended). A `/` after a value is division; a `/` in operand
position starts a regex, so `a / b` and `let r = /\d+/g` both work.

```rak
let re = /\d+/g
dump re.is_match("abc123")            // true
dump re.find_all("a1 b22 c333")       // [1, 22, 333]
dump (/\s+/g).replace("a  b   c", "_") // a_b_c

// Free-function form (also works on the VM):
dump regex_find_all(/[a-z]+/g, "a1bc2def")   // [a, bc, def]
```

### Binary pattern matching

Match a `bytes` value against byte literals (hex, int, or char) with a
trailing `..` to match the rest. Useful for magic-number sniffing.

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

### Traits (Display, Iterable, Index, IndexMut)

The receiver is passed as the first argument of each method. `Display::fmt`
drives `dump`, `Iterable::iter` drives `for x in target`, `Index::index` /
`IndexMut::set` drive `obj[key]` reads and writes.

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

The full design and implementation plan for these plus FFI, memory-mapped
files, macros, an async event loop, raw sockets, and DNS/TLS/PCAP parsers is
in [`docs/rak-features-spec.md`](docs/rak-features-spec.md).

## Standard library

### TCP networking

```rak
let listener = net_listen("127.0.0.1:18392")
let conn = net_accept(listener)
let stream = conn.0
tcp_write(stream, "hello\n")
dump tcp_read_line(stream)
tcp_close(stream)
```

### Concurrency

```rak
let h = spawn(fn() { return 42 })
dump thread_join(h)
let (tx, rx) = channel()
chan_send(tx, "hi")
dump chan_recv(rx)
```

### Strings, arrays, and math

```rak
dump replace("hello", "l", "L")      // heLLo
dump find("hello", "lo")             // 3, or -1 if not found
dump starts_with("hello", "he")      // true
dump ends_with("hello", "lo")        // true
dump slice("hello", 1, 4)            // ell
dump repeat("ab", 3)                 // ababab
dump trim_start("  hi")              // hi
dump reverse("abc")                  // cba
dump reverse([1, 2, 3])             // [3, 2, 1]
dump sum([1, 2, 3, 4])              // 10
dump max(3, 9, 2)                   // 9
dump min(3, 9, 2)                   // 2
dump abs(-5)                        // 5
dump sqrt(16)                       // 4
dump pow(2, 10)                     // 1024
dump clamp(15, 0, 10)              // 10
```

### JSON

```rak
let j = json_parse("{\"user\": \"admin\", \"id\": 7}")
dump j.user                         // admin
dump j.id                           // 7
dump json_stringify(j)              // {"user":"admin","id":7}
```

### OSINT (where it started)

```rak
for port in scan_ports("127.0.0.1", { range: [0x0016, 0x0050] }) {
    dump fmt("Open: 0x{:04X}", port)
}
dump md5("password")
dump sha256("secret")
dump hex_encode("ABC")
dump html_links(html)
```

## CLI

```bash
rakc run <file>     Run a Rak script on the interpreter
rakc vm <file>      Run on the bytecode VM
rakc build <file>   Build a standalone executable
rakc bench <file>   Benchmark interpreter vs VM
rakc check <file>   Lex and parse, print diagnostics with line/column
rakc lex <file>     Print tokens
rakc parse <file>   Print AST
rakc repl           Start an interactive REPL
rakc lsp            Start the language server (stdio)
rakc bindgen <h>    Generate Rak bindings from a C header
rakc --version

rakpkg init [name]       Create a new package
rakpkg add <user/repo>   Add a package from GitHub
rakpkg install           Install all dependencies
rakpkg run               Run the entry point
rakpkg build             Build to a standalone executable
rakpkg list              List installed packages
rakpkg remove <name>     Remove a package
```

## Tooling

### REPL

`rakc repl` starts an interactive session. State persists between lines, so variables and functions stay defined. Unclosed blocks continue on the next line. Commands start with `:`.

```
rak> let x = 10
rak> dump x
[DUMP] 10
rak> fn add(a, b) { return a + b }
rak> dump add(3, 4)
[DUMP] 7
rak> :ast 1 + 2
rak> :help
```

`:vars` shows defined variables. `:ast <expr>` prints the AST. `:bytecode <expr>` prints the compiled chunk. `:clear` resets state. `:quit` exits.

### Language server

`rakc lsp` is a stdio language server. It reports parser and lexer diagnostics, offers keyword, type, and builtin completion, shows hover text for identifiers, and jumps to `fn`, `let`, `struct`, and `enum` definitions.

Build it with `cargo build --release --features lsp`. The `vscode-rak/` directory has a VS Code extension with a TextMate grammar that pairs with it. For Neovim, point `nvim-lspconfig` at `rakc lsp` for `.rak` files.

### C header bindgen

`rakc bindgen header.h -o bindings.rak` reads a C header and generates a Rak file with a function per C function (calling `extern_call`) and a struct per C struct. Pointers map to `u64`, `char*` maps to `string`, numeric types map to `i8` through `u64` and `f32`/`f64`.

```bash
cargo build --release --features bindgen
rakc bindgen sdl.h -o sdl.rak
```

The generated functions call `extern_call`, which returns an error until a Rust wrapper is linked into the standard library. The wrapper is where the actual FFI linking happens.

## Platform support

Windows and Linux. On Windows, the GUI uses WebView2 (ships with Edge). On Linux, it uses WebKitGTK. `rakc build` produces `.exe` on Windows and an executable with `chmod 755` on Linux. The IDE ships as NSIS/MSI on Windows and `.deb`/AppImage on Linux via GitHub Actions CI.

## Project structure

```
Rak/
├── rakc/              Compiler, interpreter, VM, GUI
│   └── src/
│       ├── lexer.rs       Tokenizer (logos)
│       ├── parser.rs      Recursive descent parser
│       ├── ast.rs         AST definitions
│       ├── interpreter.rs Tree-walking interpreter + stdlib builtins
│       ├── bytecode.rs    Opcode set and chunk
│       ├── compiler.rs    AST to bytecode
│       ├── vm.rs          Stack-based bytecode VM
│       ├── gui.rs         WebView2/WebKitGTK window management
│       ├── repl.rs        Interactive REPL
│       ├── lsp.rs         Language server
│       └── bindgen.rs     C header bindgen
├── stdlib/            Rust native stdlib (net, crypto, encoding, recon, web, file, json)
├── rakpkg/            Package manager CLI
├── ide/               Tauri + Next.js IDE
├── .github/workflows/  CI (Linux .deb + AppImage builds)
├── examples/
│   ├── hello.rak
│   ├── quick.rak
│   ├── osint_scan.rak
│   ├── echo_server.rak     TCP echo server
│   ├── sql_server.rak      SQL server in Rak
│   ├── sql_client.rak
│   ├── self_host.rak       Rak interpreter in Rak
│   ├── bench.rak           VM benchmark
│   ├── gui_demo.rak        GUI window demo
│   ├── json_demo.rak       JSON parse and stringify
│   ├── closures.rak        Lexical closures and recursion
│   ├── vm_features.rak     Map, tuple, index, interp, match on the VM
│   ├── pipeline.rak        Pipeline operator (|>)
│   ├── regex.rak           Regex literals and matching
│   ├── binary_patterns.rak Binary byte-pattern matching
│   ├── traits.rak          Display/Iterable/Index trait protocols
│   ├── ffi.rak             FFI: extern bindings + raw memory (interpreter + VM)
│   ├── ffi_dynamic.rak     FFI dynamic loader (lib.call/lib.sym/lib.close)
│   └── stdlib_demo.rak     String, array, and math builtins
```

## Build from source

```bash
git clone https://github.com/Louiml/Rak.git
cd Rak
cargo build --release                    # rakc + rakpkg
cargo build --release --features gui     # rakc with GUI support
cd ide && npm install && npx tauri build # IDE
```

Linux requires `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, and `librsvg2-dev`.

## Tech stack

The compiler is Rust. Logos handles lexing. The parser is hand-written recursive descent. Two backends: a tree-walking interpreter and a stack-based bytecode VM.

The standard library uses `ureq` for HTTP, `scraper` for HTML parsing, `md-5`/`sha1`/`sha2` for hashing, `std::net` for TCP, `std::thread` and `mpsc` for concurrency. Real JSON via `serde_json`.

The IDE is Tauri v2, Next.js, TypeScript, and Tailwind CSS. It runs Rak scripts as child processes, streams output, and has an integrated terminal.

## License

Apache-2.0

## Author

Louiml (Ryuzaki)
