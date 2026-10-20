# Rak

A programming language for hackers, OSINT investigators, and systems programmers.

Hex is a first-class type. A bytecode VM runs about 6x faster than the
tree-walker. There is a SQL engine written in Rak itself, and a Rak interpreter
written in Rak.

- **Two backends, one language.** `rakc run` is a tree-walking interpreter;
  `rakc vm` is a stack machine. They register the same builtins, and a gate in
  the test suite fails the build if they ever drift apart.
- **Four compilation targets.** Bytecode, C11, a native binary, and a
  self-extracting executable — all from one shared front end, checked against
  each other by a conformance corpus.
- **Runs in a browser.** The docs site has a playground: the real interpreter,
  compiled to WebAssembly, running in the page.
  **[Try it](https://louiml.github.io/Rak/playground.html).**

## What's new in 1.1.0

- **Rak runs in a browser.** The docs site has a playground that runs Rak's own
  interpreter, compiled to WebAssembly, inside the page — the same lexer,
  parser and evaluator as `rakc run`. The browser build has no network, no
  async and no filesystem, and it says so rather than pretending: a program
  that reaches for one gets an error naming the missing capability. Randomness
  works, because the page lends it the browser's own cryptographic generator.
  That build is tested on every push, with its own suite.
- **The two backends are one language.** 31 builtins that existed only on the
  interpreter now exist on the VM, and the count that tracked the gap is gone.
  `ensures` postconditions are checked on both backends, and a closure sees the
  function it was written in on both backends. Three things remain
  interpreter-only *by design* — `asm`, `exit`, and the `fetch` statement —
  and they say so at compile time.
- **The documentation site was rebuilt**, and the README with it.
- **CI can fail now.** `cargo fmt --check` and `clippy -D warnings` are gates,
  and a new job tests the browser's build of the compiler.

The full notes are in the [1.1.0 release](https://github.com/Louiml/Rak/releases/tag/v1.1.0).

---

## Contents

- [What's new in 1.1.0](#whats-new-in-110)
- [Try it in the browser](#try-it-in-the-browser)
- [Install](#install)
- [Quick start](#quick-start)
- [The language](#the-language)
- [Two backends, one language](#two-backends-one-language)
- [Compile to anything](#compile-to-anything)
- [What Rak is for](#what-rak-is-for)
- [The standard library](#the-standard-library)
- [Native GUI windows](#native-gui-windows)
- [Packages](#packages)
- [Tooling](#tooling)
- [Security model](#security-model)
- [Known limits](#known-limits)
- [Project layout](#project-layout)
- [Build from source](#build-from-source)
- [Documentation](#documentation)
- [License](#license)

---

## Try it in the browser

The docs site carries a playground that runs **Rak's own interpreter**, compiled
to WebAssembly, inside the page — the same lexer, parser and evaluator as
`rakc run`. Nothing is mocked, and nothing is transcribed to JavaScript.

```
https://louiml.github.io/Rak/playground.html
```

Write a program, press `Run` (or `Ctrl+Enter`), and the output appears below.
The examples in the picker cover the language's range: hex parsing, traits,
structs, closures, channels, and the failure paths.

A browser has no TLS stack, no event loop and no file system, so the browser
build has no network, no async and no filesystem. Those builtins **say so**
rather than pretending to work: `fetch` reports that the build has no HTTP
client, and the async builtins are not registered at all. Randomness does work:
the module asks the page for entropy, and the page uses the browser's own
cryptographic generator.

That build is not a toy: it is the configuration CI tests on every push, with
its own suite (`cargo test -p rakc --no-default-features`), a wasm build, and a
check that runs a program through the payload the site actually serves.

## Install

The custom installer is an interactive wizard that installs `rakc`, `oyvey` and
the Rak IDE, edits PATH (append-only — your existing entries are never
touched), sets `RAK_PATH`, creates shortcuts and `.rak` associations, and
installs man pages and shell completions. Every action is recorded in
`~/.rak/manifest.json` for a clean uninstall.

**Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash
# non-interactive:
curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash -s -- --yes --install rakc,oyvey,ide --scope user
```

**Windows (PowerShell):**

```powershell
iwr -useb https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.ps1 | iex
```

Or download `rak-setup-linux-x86_64` / `rak-setup-windows-x86_64.exe` from the
[latest release](https://github.com/Louiml/Rak/releases/latest) and run it.

Components are a multi-select: `rakc`, `oyvey`, the IDE, `RAK_PATH`, shortcuts
and file associations, man pages and completions. Install scope is `user`
(default, no privileges) or `system`. The Tauri installers (NSIS/MSI on Windows,
`.deb`/AppImage on Linux) remain on the release page for IDE-only users.

## Quick start

```rak
dump "Hello, World"
```

```bash
rakc run hello.rak      # the interpreter
rakc vm hello.rak       # the bytecode VM (faster)
```

No `main`, no imports, no boilerplate. When you want a program with an exit
code, `fn main(argv) -> int` is the entry point and its return value is the
process exit code.

A port scan, which is where the language started:

```rak
scan "127.0.0.1" {
    range: [0x0016, 0x0050]
} {
    if open {
        dump fmt("Port 0x{:04X}", port)
    }
}
```

A standalone binary, with the source embedded — no Rak installation needed on
the target machine:

```bash
rakc build examples/hello.rak   # hello.exe on Windows, hello on Linux
./hello                          # [DUMP] Hello, World
```

## The language

### Variables and literals

```rak
let target = "192.168.1.1"
let port = 0x0050                // hex is a first-class literal
let mut counter = 0
counter = counter + 1

let pi = 3.14f64                 // typed literals: 42i32, 3.14f64
let c: char = 'a'
let bin = 0b1010                 // 10
let oct = 0o755                  // 493
dump 0xA == 10                   // true
```

Integers are `i8`..`i64` and `u8`..`u64`, floats are `f32` and `f64`, and
`Hex` is a distinct type with its own arithmetic. Integer overflow and integer
division by zero are errors, not wrapped values; float division by zero follows
IEEE.

### Functions, closures, and contracts

```rak
fn add(a, b) {
    return a + b
}
dump add(0x10, 0x20)             // 48
```

A closure sees the function it was written in, by value: free variables
resolve at the definition site, not the call site, and an assignment inside a
closure lands in the closure's own copy.

```rak
let f = fn(n) { if n < 2 { return n } return f(n-1) + f(n-2) }
dump f(10)                       // 55
```

`requires` and `ensures` are checked on **both** backends — a promise about
the value the caller receives, with `result` in scope:

```rak
fn double(x) -> int {
    return x * 2
} requires x > 0 ensures result == x * 2
```

`defer f()` runs in LIFO order when the current function exits.

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

### Error handling

```rak
let v = Ok(42)?                  // unwrap, propagate Err

try {
    raise "boom"
} catch e {
    dump e
}
```

Errors are structured: `catch e` binds a first-class error value with a `kind`,
a source span, a cause and context (`err_kind`, `err_message`, `err_line`, …).

### Tuples, structs, enums, and pattern matching

```rak
let p = (1, 2)
dump p.1

struct Point { x: int, y: int }
let o = Point { x: 1, y: 2 }
dump o.x

match 2 {
    1 => { dump "one" },
    2 => { dump "two" },
    _ => { dump "other" }
}
```

Binary pattern matching works on `bytes` values, with a trailing `..` to match
the rest — useful for magic-number sniffing:

```rak
fn sniff(data: bytes) {
    match data {
        [0x89, 'P', 'N', 'G', ..] => { return "png" },
        [0xFF, 0xD8, 0xFF, ..]    => { return "jpeg" },
        _                         => { return "unknown" },
    }
}
dump sniff(b"\x89PNG\x0d\x0a\x1a\x0a")   // png
```

### Traits

The receiver is the first argument of each method. `Display::fmt` drives `dump`,
`Iterable::iter` drives `for x in target`, `Index::index` / `IndexMut::set` drive
`obj[key]`.

```rak
struct Point { x: int, y: int }
impl Display for Point {
    fn fmt(self) { return fmt("({}, {})", self.x, self.y) }
}
dump Point { x: 3, y: 4 }        // (3, 4)
```

### Pipelines and regex

`x |> f` desugars to `f(x)`, and `x |> f(a, b)` to `f(x, a, b)`. Chaining is
left-associative.

```rak
fn inc(n) { return n + 1 }
fn dbl(n) { return n * 2 }
dump 5 |> inc |> dbl             // 12
```

`/pattern/flags` with flags `i`, `m`, `s`, `x`. A `/` after a value is
division; a `/` in operand position starts a regex.

```rak
let re = /\d+/g
dump re.is_match("abc123")       // true
dump re.find_all("a1 b22 c333")  // [1, 22, 333]
```

### Macros and constants

```rak
macro add1(x: expr) { $x + 1 }
dump add1!(41)                   // 42

const MAX_LEN = 256
dump MAX_LEN
```

Macros expand in the front end, so both backends see the expanded code.

### String interpolation and `fmt`

```rak
let name = "Rak"
dump f"hello {name}!"
```

`fmt` takes real format specs, so a hex column is a loop and a `fmt`:

```rak
for b in row { dump fmt("{:02X}", b) }   // 0F, not 0
dump fmt("{:#x}", 255)                  // 0xff
dump fmt("{:.2f}", ratio)               // two decimals, on both backends
```

## Two backends, one language

`rakc run` walks the AST. `rakc vm` runs bytecode: the same front end, a
compiler to chunks, and a stack machine. Benchmarks show roughly a 6x speedup
on compute-heavy code; `rakc bench file.rak` compares them on the same script.

They are two implementations of one language, so they are compared against each
other in the test suite rather than tested separately:

- a **structural gate** compares the two backends' builtin registration tables
  and fails the build if they diverge;
- **behavioural tests** run a program on both and require identical output,
  which catches cases where the two disagree rather than merely differ in what
  they implement.

The v8.0.0 release shipped nineteen builtins that existed only on the VM, and
the suite did not notice — a per-backend test suite cannot catch a per-backend
omission. The registration gap is now closed: 31 interpreter-only builtins
reached the VM, and the ratchet that tracked the count is gone.

Three things are still interpreter-only **by design**, and say so at compile
time rather than guessing: inline assembly (`asm` — a bytecode VM has no
instructions to escape into), `exit` (the CLI owns the process), and the `fetch`
statement (the VM has no HTTP client yet).

```bash
rakc run file.rak     # interpreter
rakc vm file.rak      # bytecode VM
rakc bench file.rak   # both, timed
```

`docs/V8-BACKEND-PARITY.md` has the full story, including the cases where a
structural gate would have passed and a behavioural one did not.

## Compile to anything

`rakc build` is a rustc-shaped multi-target front end. The front end — lex,
parse, type-check — is shared by every target; a target turns a checked module
into artifacts for a triple.

```bash
rakc build hello.rak                              # default: self-extracting exe
rakc build hello.rak --emit=c                     # emit C source
rakc build hello.rak --emit=link --release        # native binary via the C toolchain
rakc build hello.rak --emit=bytecode              # .rkbc container
rakc build --print target-list                    # the accepted triples
```

| `--emit` | Artifact | Runs with |
|---|---|---|
| `interp` | none | the interpreter (like `rakc run`) |
| `bytecode` | `.rkbc` container | `rakc vm program.rkbc` |
| `c` | `.c` source | any C compiler |
| `link` | native binary | directly |
| `exe` | self-extracting executable (the default) | directly |

Flags: `--target <triple>`, `-C opt-level=0..3|s|z`, `-C debuginfo=0..2`,
`-C panic=abort|unwind`, `--release`, `-o <path>`.

The C target is a real translation, not a wrapper: every Rak value becomes a
tagged `rak_val` in the emitted runtime, and the `Display` formatting, the
numeric tower, the `fmt` spec grammar and `raise`/`try` unwinding are carried
across, so a compiled binary agrees with the interpreter byte for byte. That
claim is checked rather than asserted — a conformance corpus runs the same
programs on the interpreter, the VM and the C toolchain and diffs their output.

The `.rkbc` container is the chunk the VM runs, written to a file: compile
once, ship it, run it anywhere. Baked function values and nested chunks
survive the round trip, and natives are stored by name and re-bound against the
running VM's registry, so a chunk compiled by a build with a feature the
target lacks says so rather than answering with a wrong value.

**The honest subset.** The C target compiles a single module, without the
runtime features whose implementation lives in Rust: no `spawn`/`await`, no
sockets, no FFI, no GUI, no `import`. Those are compile errors naming the
construct and the target, not silent behaviour changes. The bytecode and `exe`
targets keep the whole language.

## What Rak is for

The language started as an OSINT scripting tool. It grew into something
bigger, and these are the parts that are not generic.

### Forensic structs and evidence provenance

Declare a binary wire format once with `binstruct` and get both a **decoder**
and an **encoder** for free. Every decoded value is wrapped in an
`evidence<T>` provenance tag carrying where, when and how it was collected, so
`report(...)` emits a chain-of-custody-cited findings report.

```rak
binstruct DnsHeader {
    id:      u16be
    flags:   u16be
    qdcount: u16be
    ancount: u16be
}

let q = dns_build("example.com", "A")
let h = DnsHeader.decode(q)         // -> evidence<struct> with provenance
dump h.id                           // 0x1234
let back = DnsHeader.encode(h)      // round-trip back to bytes

let ip = evidence<string> from "93.184.216.34"
let answers = cite(dns_query("example.com", "A"), "dns_query", "example.com")
dump report(ip, answers)            // numbered assertions + cited sources
dump provenance(ip)                 // {tool, target, ts, raw_offset, raw_len, parent}
```

Field types are `u8`..`u64` and `i8`..`i64`, each with an optional `be`/`le`
endianness suffix, `bytes(n)` for a fixed run, `rest` for the remainder, and a
nested binstruct name for a `Ref` field.

### A SQL server, written in Rak

```bash
rakc run examples/sql_server.rak   # listens on 127.0.0.1:18393
rakc run examples/sql_client.rak    # CREATE, INSERT, SELECT, UPDATE, DELETE
```

The engine — lexer through executor — is in the language itself, and it serves
queries over TCP.

### Self-hosting

```bash
rakc run examples/self_host.rak    # a Rak interpreter written in Rak
```

It reads Rak source, lexes it, parses it, and evaluates it.

### Byte-exact file I/O

`read`/`file_read` are UTF-8, so `file_read` **fails** on any file containing a
byte sequence that is not valid UTF-8, and `write` turns a `0xFF` into U+FFFD on
the way out. Between them there was no way to open a binary file.

```rak
let buf = file_read_bytes("firmware.bin")     // bytes, any content
buf[0] = 0xFF                                  // mutate in place
file_write_bytes("patched.bin", buf)           // byte-exact write

let m = mmap_open("firmware.bin", "rw")        // no copy, no flush
mmap_write(m, 0x100, 0x90)                     // the mapping *is* the file
```

## The standard library

A survey; the full list is in [`docs/content/stdlib.md`](docs/content/stdlib.md).

```rak
// TCP
let listener = net_listen("127.0.0.1:18392")
let conn = net_accept(listener)
tcp_write(conn.0, "hello\n")
dump tcp_read_line(conn.0)

// Threads and channels
let h = spawn(fn() { return 42 })
dump thread_join(h)
let (tx, rx) = channel()
chan_send(tx, "hi")
dump chan_recv(rx)

// Async, on a bounded worker pool
async fn probe(host, port) {
    return await tcp_probe(host, port, 200)
}
dump await probe("127.0.0.1", 80)

// JSON
let j = json_parse("{\"user\": \"admin\", \"id\": 7}")
dump j.user                         // admin
dump json_stringify(j)              // {"user":"admin","id":7}

// Hashing and crypto, without FFI
dump md5("password")
dump sha256("secret")
dump hmac_sha256("key", "data")
let ct = aes_gcm_encrypt(key, nonce, b"payload")
dump aes_gcm_decrypt(key, nonce, ct)
```

Also in the box: DNS (`dns_query`, `dns_build`, `dns_parse`, `dns_resolve`,
`dns_reverse`, `dns_records`, `dns_walk`), TLS wire-format parsing
(`tls_parse_client_hello`, `tls_parse_cert_chain`), raw sockets and packet
forging (`net_raw_tcp_syn`, `net_raw_send`), an HTTP/1.1 server
(`http_server_start`, `http_server_poll`, `http_server_respond`), WebSockets
(`ws_connect`, `ws_send`, `ws_recv`), a pull-based HTTP client (`fetch`), CSV
and YAML, gzip and zip archives, structured JSON logging, a secrets store, the
process API, and WHOIS, certificate-transparency and YARA-lite lookups.

### Foreign Function Interface

Declare bindings against the platform C library, or load one dynamically:

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

## Native GUI windows

Requires `cargo build -p rakc --features gui`. Works on Windows and Linux.

The GUI is **native**: windows and widgets are drawn by `egui` through the
`glow` renderer. There is no HTML, no CSS, no JavaScript, and no WebView2 or
WebKitGTK anywhere in the pipeline, so a Rak GUI app does not need a browser
engine installed.

```rak
fn on_click(name, message) {
  print f"{name} says: {message}"
  return nil
}

gui_callback("on_widget", on_click)

let win = gui_window("Rak GUI Demo", 420, 240)
let heading = gui_add(win, "label", "Hello from Rak!", 20, 20, 380, 28)
let name = gui_add(win, "input", "world", 20, 70, 240, 28)
let button = gui_add(win, "button", "Click me", 20, 120, 140, 34)

gui_on_click(button, "on_widget")
gui_set_text(heading, "Native widgets, zero HTML")
gui_wait()
```

| Builtin | Behaviour |
|---|---|
| `gui_window(title, w, h)` | Opens a window, returns its id. Blocks until the window exists, so the id is real. |
| `gui_add(window, kind, text, x, y, w, h)` | Adds a widget at an absolute position; returns the widget id. |
| `gui_set_text(widget, text)` | Sets a caption, or an input's current text. |
| `gui_get_text(widget)` | Reads a widget back: the input's text, `"true"`/`"false"` for a checkbox, otherwise the caption. |
| `gui_set_visible(widget, bool)` | Shows or hides a widget without removing it. |
| `gui_on_click(widget, name)` | Binds a widget to a callback registered under `name`. |
| `gui_close(id)` | Closes one window. The process keeps running. |
| `gui_wait()` | Blocks until every window is closed. |
| `gui_quit(code)` | Stops the GUI with an exit code. |
| `gui_callback(name, fn)` | Registers a Rak function that `gui_on_click` dispatches to. |

Widget kinds are `label`, `button`, `input`, `checkbox`, `select` and
`textarea`. The IDE's **GUI Designer** draws a window visually and emits this
native code.

**One real limitation.** A callback does not share mutable state with the
script that registered it: it receives a copy of the environment as it stood
at `gui_callback` time. That follows from Rak having no reference types.

The webview-era builtins (`gui_open`, `gui_update`, `gui_title`) are gone;
`gui_open(title, html, w, h)` became `gui_window(title, w, h)` plus
`gui_add(...)` calls. **Scripts written against the old HTML API will not run
unchanged.**

## Packages

`oyvey` is the official Rak package manager and build system. It is
Cargo-inspired: a manifest declares the project, a lockfile records the exact
resolved revision and checksum of every dependency, and a global git cache
(`~/.oyvey`, override with `OYVEY_HOME`) keeps clones so builds are
reproducible.

```bash
oyvey new mylib              # generate a project
oyvey add user/repo          # add a dependency
oyvey install                # resolve + install (writes oyvey.lock)
oyvey run                    # run the entry point via rakc
oyvey build                  # build a standalone executable via rakc
oyvey test                   # run the project's tests
oyvey tree                   # the resolved dependency tree
oyvey audit                  # verify installed checksums against the lockfile
```

The manifest is `Rak.toml`, Cargo's table model:

```toml
[package]
name = "mylib"
version = "0.1.0"
edition = "2026"

[dependencies]
net = "1.2"                          # registry, semver requirement
local = { path = "../local" }        # sibling directory
gh = { git = "https://github.com/user/repo", branch = "main" }
```

The legacy `package.rak` manifest keeps working. Dependencies are vendored
into `packages/`, the directory `rakc` searches for a bare `import net`. A path
dependency resolves in place and is recorded in `oyvey.lock` with a `path+`
source and no checksum — a mutable directory is not a verifiable artifact.

`oyvey add` and `oyvey remove` edit the manifest in place with `toml_edit`, so
every comment in the file survives.

### Imports and exports

Python-style modules with explicit `pub`/`export`, name-based resolution,
`from ... import`, directory packages, import-once caching and circular-import
support.

```rak
import "./math.rak"          // file path -> binds "math"
import math                  // name: searches dir, ./packages/, RAK_PATH
import math as m             // alias
from math import add
from math import add as plus, mul as times
from math import *           // all exports; local bindings win on clash

import pkg                   // runs pkg/init.rak
import pkg.sub               // pkg/init.rak + pkg/sub.rak

pub let PI = 3.14
export fn add(a, b) { return a + b }
pub use math                 // re-export all of math
```

**One rule is worth knowing before writing a multi-file program.** `import m`
gives you the *module* — a live view of its top-level bindings, so `m.X = v`
writes the module's state. `from m import x` gives you a *value*, taken once
at import time.

```rak
// bank.rak
pub let mut BALANCE = 0
pub fn deposit(n) { BALANCE = BALANCE + n  return BALANCE }
```

```rak
import bank
from bank import BALANCE

bank.deposit(10)
bank.deposit(10)
dump bank.BALANCE              // 20. The view followed the module
```

A module's top-level `let mut` is real state: it persists across calls, not a
per-call copy. A name with no `pub` is unreachable through the handle, and
`pub let X` is readable but fixed — both backends enforce both rules.

## Tooling

### REPL

```text
rak> let x = 10
rak> dump x
[DUMP] 10
rak> fn add(a, b) { return a + b }
rak> dump add(3, 4)
[DUMP] 7
rak> :help
```

State persists between lines and unclosed blocks continue on the next line.
`:vars` shows defined variables, `:ast <expr>` prints the AST, `:bytecode
<expr>` prints the compiled chunk, `:clear` resets state, `:quit` exits.

### Language server, debugger, DAP

`rakc lsp` is a stdio language server: parser and lexer diagnostics, keyword
and builtin completion, hover text, and jumps to definitions. Build it with
`cargo build --release --features lsp`. The `vscode-rak/` directory has a VS
Code extension with a TextMate grammar to pair with it; for Neovim, point
`nvim-lspconfig` at `rakc lsp`.

`rakc debug program.rak` is a source debugger on the bytecode VM: breakpoints,
stepping, locals, stack and disassembly. `rakc dap program.rak` serves the
Debug Adapter Protocol over stdio, so VS Code and similar editors get
breakpoints, stepping and variable inspection.

### Tests, checks, and linting

```rak
test "addition works" {
    assert add(2, 3) == 5
    assert_eq(add(2, 3), 5)
    expect_error(fn() { raise "boom" })
}
```

```bash
rakc test tests/math.rak    # PASS/FAIL, exit non-zero on failure
rakc check file.rak         # static type check, diagnostics with line/column
rakc fmt file.rak           # canonical formatting (--write, --check)
rakc lint file.rak          # advisory style and security rules (--deny)
rakc verify file.rak        # run under limits and check requires/ensures
rakc fuzz all               # property-based fuzzing of the parsers
```

`rakc check` is opt-in — only that command runs the checker — and it is not
sound: it checks explicit annotations, call argument types, container element
types and non-exhaustive matches, and it does not enforce mutability.

### C header bindgen

`rakc bindgen header.h -o bindings.rak` reads a C header and generates a Rak
file with a function per C function and a struct per C struct. The generated
functions call `extern_call`, which errors until a Rust wrapper is linked into
the standard library; the wrapper is where the actual FFI linking happens.

## Security model

`rakc run untrusted.rak --sandbox --allow net,fs_write,secrets`

Eight capabilities: `net`, `fs_read`, `fs_write`, `process`, `ffi`, `raw`,
`gui`, `secrets`. With `--sandbox` and no `--allow`, all eight are denied and
only pure computation works. The gate sits at the call site on both backends,
and the interpreter and the VM enforce the same list.

`extern "C"`, `scan`, `fetch` and `tunnel` are gated, which is why a sandboxed
script cannot `dlopen` an attacker-named library or open a socket it was not
given. `--fs-root DIR` confines filesystem access to a directory.

`rakc lint` ships nine security rules on top of its style rules:
`hardcoded-secret`, `plaintext-url`, `weak-crypto`, `secret-compare`,
`ffi-raw-pointer`, `insecure-transport`, `unescaped-interpolation`,
`inline-asm`, `unsafe-thin-reason`. They are advisory; `--deny` makes them
fail the run.

## Known limits

Rak's policy is that a limit is stated where it is measured, not left to be
discovered.

- **`fetch` does not run on the VM.** It is a statement rather than a builtin,
  so the backend-parity gate — which compares builtin registrations — cannot
  see it. The VM refuses to compile such a program with a clear error rather
  than guessing.
- **The type checker is opt-in and not sound.** `rakc check` only, and it does
  not model generics.
- **Inline assembly and `exit` are interpreter-only, by design.**
- **`substr` returns `""` out of range while `slice` clamps.** The difference
  is deliberate and documented.
- **A function body without `return` evaluates to `nil`.**
- **A GUI callback cannot write to the script's state** (no reference types).
- **The `oyvey` manifest supports the tables Cargo does**, but features,
  dev-dependencies, profiles and workspaces are reported as unsupported rather
  than silently ignored.

`docs/V8-KNOWN-ISSUES.md` is the full list, including the symptoms of the VM's
flat global namespace; `docs/V8-ROADMAP.md` says what closing each one would
take; `docs/SECURITY-AUDIT-2026-10.md` is the security review.

## Project layout

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
│       ├── gui.rs         Native egui window + widget management
│       ├── lsp.rs         Language server
│       ├── typecheck.rs   Static type checker
│       ├── repl.rs        Interactive REPL
│       └── targets/       Multi-target compilation
│           ├── rkbc.rs      The .rkbc container (writer/reader)
│           ├── c/           The C target: translation + runtime
│           ├── wasm.rs      The WebAssembly target
│           └── exe.rs       The self-extracting executable target
├── stdlib/            Rust native stdlib (net, crypto, encoding, recon, web, file, json, log, process, secrets, http_server, websocket)
├── playground/        The interpreter, compiled to WebAssembly for the docs site
├── rak-setup/         Custom interactive installer (net-install + offline)
├── ide/               Tauri + Next.js IDE
├── docs/              The documentation site, served from this repository
├── dist/              One-liner bootstraps (install.sh / install.ps1), man pages, completions
├── examples/
│   ├── hello.rak, quick.rak
│   ├── osint_scan.rak        OSINT sweep
│   ├── echo_server.rak       TCP echo server
│   ├── sql_server.rak        SQL server in Rak
│   ├── self_host.rak         Rak interpreter in Rak
│   ├── gui_demo.rak          Native GUI window
│   ├── closures.rak          Lexical closures and recursion
│   ├── vm_features.rak       Map, tuple, index, interp, match on the VM
│   ├── pipeline.rak          Pipeline operator (|>)
│   ├── regex.rak             Regex literals and matching
│   ├── binary_patterns.rak   Binary byte-pattern matching
│   ├── traits.rak            Display/Iterable/Index trait protocols
│   ├── ffi.rak               FFI: extern bindings + raw memory
│   ├── mmap.rak              Memory-mapped files: zero-copy slice/search/lines
│   ├── async.rak             Async event loop: async fn / await / tcp_probe
│   ├── net_raw.rak           Raw sockets: forge IPv4/TCP/UDP packets
│   ├── parsers.rak           DNS / TLS / PCAP wire-format parsers
│   ├── macros.rak            Compile-time macros
│   ├── import_demo.rak       Python-style import / from / export
│   ├── forensic_structs.rak  binstruct round-trip + evidence provenance
│   └── hexhunt/              Multi-module binary signature scanner
└── .github/workflows/  CI (tests, a browser-build job, clippy and fmt) and the release pipeline
```

## Build from source

```bash
git clone https://github.com/Louiml/Rak.git
cd Rak
cargo build --release                        # rakc
cargo build --release -p rakc --features gui # rakc with native GUI support
cd ide && npm install && npx tauri build     # the IDE
```

The `gui` feature belongs to the `rakc` crate, so it needs `-p rakc`; a
workspace-wide `--features gui` does not enable it. Building `rakc --features
gui` needs no system packages, because the GUI is native. Building the *IDE* on
Linux does need `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev` and `librsvg2-dev` — that is a Tauri app.

The browser playground is built with:

```bash
cargo build -p rak-playground --profile playground --target wasm32-unknown-unknown
```

and the result is copied to `docs/rak.wasm`, which the docs site serves.

## Documentation

| Where | What |
|---|---|
| [louiml.github.io/Rak](https://louiml.github.io/Rak/) | The documentation site |
| [louiml.github.io/Rak/playground.html](https://louiml.github.io/Rak/playground.html) | The playground — Rak in your browser |
| [`docs/content/`](docs/content/) | The docs, as Markdown: language, CLI, stdlib, VM, tooling, safety |
| [`docs/rak-features-spec.md`](docs/rak-features-spec.md) | The design and implementation plan for every systems-level feature |
| [`docs/V8-BACKEND-PARITY.md`](docs/V8-BACKEND-PARITY.md) | What the two backends agree on, and how it is measured |
| [`docs/V8-KNOWN-ISSUES.md`](docs/V8-KNOWN-ISSUES.md) | Every known divergence, with the reason |
| [`docs/V8-ROADMAP.md`](docs/V8-ROADMAP.md) | What is left, and what closing it would take |
| [`docs/SECURITY-AUDIT-2026-10.md`](docs/SECURITY-AUDIT-2026-10.md) | The security review |
| [`SECURITY.md`](SECURITY.md) | Reporting a vulnerability |

## License

Apache-2.0

## Author

Louiml (Ryuzaki)
