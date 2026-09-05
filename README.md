# Rak
## Work inn progress
A programming language built for hackers, OSINT investigators, and now general-purpose systems programming: build **backend SQL servers**, **desktop TCP services**, and **self-host** the compiler — all in Rak.

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
rakc run <file>     Run on the tree-walking interpreter
rakc vm <file>      Run on the bytecode VM
rakc bench <file>   Benchmark interpreter vs VM
rakc lex <file>     Print tokens
rakc parse <file>   Print AST
rakc --version
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
    └── bench.rak           # VM benchmark
```

## Tech Stack

- **Compiler**: Rust, `logos` for lexing, hand-written recursive descent parser, tree-walking interpreter + stack-based bytecode VM.
- **Standard library**: Rust, `ureq` (HTTP), `scraper` (HTML), `md-5`/`sha1`/`sha2` (hashing), `std::net` (TCP), `std::thread` + `mpsc` (concurrency).
- **IDE**: Tauri v2, Next.js, TypeScript, Tailwind CSS.

## License

MIT OR Apache-2.0

## Author

Louiml (Ryuzaki)
