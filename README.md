# Rak
## Work in progress
A programming language built for hackers, OSINT investigators, and general-purpose systems programming: build **desktop GUI apps**, **backend SQL servers**, **shareable packages**, and **self-host** the compiler — all in Rak.

English-readable syntax with first-class hexadecimal, a bytecode VM, real concurrency, and a SQL engine written in Rak itself.

## Quick Start

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

## What's New (v0.2)

Rak grew up into a general-purpose language:

- **Bytecode VM** — `rakc vm` runs bytecode; ~6x faster than the tree-walker (`rakc bench`).
- **Full type system** — signed ints (`i8`-`i64`), unsigned (`u8`-`u64`), floats (`f32`/`f64`), typed literals (`42i32`, `3.14f64`), tuples, `Result`/`Option`, real modules, closures, `async`/`spawn`/channels, generics, traits, pattern destructuring, string interpolation (`f"{}"`).
- **Concurrency** — `spawn`, `thread_join`, `channel()`, `chan_send`/`chan_recv`, `sleep`.
- **Networking** — `net_listen`, `net_accept`, `net_connect`, `tcp_read`/`tcp_write`/`tcp_read_line`. Build TCP servers in Rak.
- **SQL server written in Rak** — a complete SQL engine (lexer, parser, storage, executor) implemented in Rak, serving queries over TCP. See `examples/sql_server.rak`.
- **Self-hosting** — `examples/self_host.rak` is a Rak interpreter (lexer + parser + evaluator) written in Rak. Rak now writes Rak.
- **Error handling** — `try`/`catch`/`raise`, the `?` operator, `Result`/`Option`.
- **Map/array/field assignment** — `arr[i] = v`, `map[k] = v`, `obj.field = v`.

## What's New (v0.2.1)

- **Error reporting with line/column** — lexer and parser errors now print `at line L, col C` (and the offending source). New `rakc check <file>` prints diagnostics in the `file:L:C` format IDEs expect.
- **Lexical closures** — a function's free variables resolve where it was *defined*, not where it's called. Recursion now works for the `let f = fn(n){…f(n-1)…}` form (shared global), not just `fn` declarations.
- **VM honesty + more features** — `rakc vm` now **errors clearly** on unsupported features instead of silently returning `nil`, and supports **Map, Tuple, Index, FieldAccess, string interpolation, and `match`** in the bytecode VM.
- **Real JSON** — `json_parse(str)` returns native `Array`/`Map`/`Int`/`Float`/`Bool`/`Nil` (not a string); `json_stringify(value)` → JSON string.
- **String / array / math stdlib** — `replace`, `find`, `starts_with`, `ends_with`, `slice`, `repeat`, `trim_start`/`trim_end`, `reverse`, `min`, `max`, `sum`, `abs`, `sqrt`, `pow`, `clamp`, `env_set`, `to_string`.

## What's New (v0.3.0)

- **`rakc build`** — compile any `.rak` script into a **standalone .exe** that runs on any Windows machine (no Rak needed). The source is embedded as a payload in the binary.
- **GUI windows** (Windows-only, `--features gui`) — create native desktop windows with HTML/CSS/JS using `gui_open(title, html, width, height)`, `gui_update(id, html)`, `gui_wait()`, and a JS→Rak callback bridge via `window.rak_call(fn, args)`. Uses WebView2 (ships with Edge).
- **Package manager** (`rakpkg`) — a CLI for Git-based shareable Rak packages. `rakpkg init`, `rakpkg add user/repo`, `rakpkg install`, `rakpkg run`, `rakpkg build`, `rakpkg list`, `rakpkg remove`. Manifest is `package.rak` (Rak let-bindings).
- **Version bumped to 0.3.0.**

### Build a standalone .exe

```bash
rakc build examples/hello.rak   # → hello.exe (standalone, ~4 MB)
./hello.exe                      # → [DUMP] Hello, World
```

### Create a GUI window (requires `--features gui`)

```rak
let html = "<h1 style='color:#22c55e;text-align:center;margin-top:40px'>Hello from Rak!</h1>
<button onclick=\"window.rak_call('clicked')\" style='display:block;margin:20px auto;padding:10px 30px;font-size:18px;background:#22c55e;color:white;border:none;border-radius:8px;cursor:pointer'>Click me</button>"

let win = gui_open("Rak GUI Demo", html, 600, 400)
gui_wait()
```

### Package manager

```bash
rakpkg init mylib           # creates package.rak + lib.rak
rakpkg add user/repo        # Git-clone into .rak/packages/
rakpkg install              # install all deps from package.rak
rakpkg run                  # run the entry point (rakc run)
rakpkg build                # build to standalone .exe (rakc build)
rakpkg list                 # list installed packages
rakpkg remove mylib         # remove a package
```

**package.rak manifest:**
```rak
let name = "mylib"
let version = "0.1.0"
let deps = { net: "user/rak-net", crypto: "user/rak-crypto" }
let entry = "lib.rak"
```

### Using the new features

```rak
// line/col errors
rakc check my.rak          // e.g. "Parser error: Expected ... at line 3, col 11"

// real JSON
let j = json_parse("{\"a\": 1, \"b\": [2, 3]}")
dump j.a                   // 1
dump j.b[1]                // 3
dump json_stringify(j)     // {"a":1,"b":[2,3]}

// lexical closure / recursion
let fact = fn(n) { if n <= 1 { return 1 } return n * fact(n - 1) }
dump fact(5)               // 120

// match (runs on the VM too)
match 2 { 1 => { dump "one" }, 2 => { dump "two" }, _ => { dump "other" } }

// new string/array/math builtins
dump replace("hello", "l", "L")   // heLLo
dump slice("hello", 1, 4)         // ell
dump reverse("abc")              // cba
dump sum([1, 2, 3, 4])           // 10
dump max(3, 9, 2)                // 9
dump clamp(15, 0, 10)            // 10
```

## Run a SQL server (written in Rak)

```bash
rakc run examples/sql_server.rak   # listens on 127.0.0.1:18393
rakc run examples/sql_client.rak    # CREATE / INSERT / SELECT / UPDATE / DELETE
```

## Self-hosting

```bash
rakc run examples/self_host.rak    # a Rak interpreter written in Rak
```

## Language Syntax

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

### Control Flow

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

### Hexadecimal

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

### Tuples & structs

```rak
let p = (1, 2)
dump p.1
struct Point { x: int, y: int }
let o = Point { x: 1, y: 2 }
dump o.x
```

## Standard Library

### Networking (TCP server/client)

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

### SQL (engine written in Rak)

```rak
// see examples/sql_server.rak — full CREATE/INSERT/SELECT/UPDATE/DELETE
// over a TCP wire protocol, implemented entirely in Rak.
```

### Strings, arrays & math
```rak
dump replace("hello", "l", "L")      // heLLo
dump find("hello", "lo")             // 3  (-1 if not found)
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

### JSON (real values)
```rak
let j = json_parse("{\"user\": \"admin\", \"id\": 7}")
dump j.user                         // admin
dump j.id                           // 7
dump json_stringify(j)              // {"user":"admin","id":7}
```

### OSINT (original)

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
rakc run <file>     Run a Rak script (interpreter)
rakc vm <file>      Run on the bytecode VM (Map/Tuple/Index/Field/interp/match supported)
rakc build <file>   Build a standalone .exe from a Rak script
rakc bench <file>   Benchmark interpreter vs VM
rakc check <file>   Lex + parse, print diagnostics (with line/col)
rakc lex <file>     Print tokens
rakc parse <file>   Print AST
rakc --version

rakpkg init [name]       Create a new package
rakpkg add <user/repo>   Add a Git package
rakpkg install           Install all deps
rakpkg run               Run entry point (rakc run)
rakpkg build             Build to .exe (rakc build)
rakpkg list              List installed packages
rakpkg remove <name>     Remove a package
```

## Project Structure

```
Rak/
├── rakc/              # Compiler crate (lexer, parser, interpreter, VM, compiler)
│   └── src/
│       ├── lexer.rs        # Tokenizer (logos)
│       ├── parser.rs       # Recursive descent parser
│       ├── ast.rs          # AST definitions
│       ├── interpreter.rs  # Tree-walking interpreter + stdlib builtins
│       ├── bytecode.rs     # Opcode set + Chunk
│       ├── compiler.rs     # AST -> bytecode
│       └── vm.rs           # Stack-based bytecode VM
├── stdlib/            # Rust native stdlib (net, crypto, encoding, recon, web, file, js)
├── ide/               # Tauri + Next.js IDE
├── docs/              # HTML docs
└── examples/
    ├── hello.rak
    ├── quick.rak
    ├── osint_scan.rak
    ├── echo_server.rak     # TCP echo server
    ├── sql_server.rak      # SQL server (in Rak)
    ├── sql_client.rak
    ├── self_host.rak       # Rak interpreter in Rak
    ├── bench.rak           # VM benchmark
    ├── json_demo.rak       # real JSON parse/stringify
    ├── closures.rak        # lexical closures + recursion
    ├── vm_features.rak     # Map/Tuple/Index/interp/match on the VM
    └── stdlib_demo.rak     # new string/array/math builtins
```

## Tech Stack

- **Compiler**: Rust, `logos` for lexing, hand-written recursive descent parser, tree-walking interpreter + stack-based bytecode VM.
- **Standard library**: Rust, `ureq` (HTTP), `scraper` (HTML), `md-5`/`sha1`/`sha2` (hashing), `std::net` (TCP), `std::thread` + `mpsc` (concurrency).
- **IDE**: Tauri v2, Next.js, TypeScript, Tailwind CSS.

## License

MIT OR Apache-2.0

## Author

Louiml (Ryuzaki)
