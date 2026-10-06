# Rak

A programming language for hackers, OSINT investigators, and systems programmers. Build desktop GUI apps, backend SQL servers, shareable packages, and self-host the compiler. All in Rak.

Hex is a first-class type. The bytecode VM runs about 6x faster than the tree-walker. There's a SQL engine written in Rak itself, and a Rak interpreter written in Rak.

## What's new in 0.9.1

`v0.9.1` is a patch release on top of 0.9.0. One user-facing fix, but it was the noisy kind of kind.

- **Missing scripts now produce an error, not a crash.** `rakc run`, `rakc check` and
  `rakc verify` on a path that does not exist used to die with a Rust panic and, on
  Windows, an exit code with no meaning to shells and CI. They now print
  `Error reading <path>: <os reason>` and exit 1. A directory passed by mistake is
  reported as a directory rather than as a file that is not found.
- **The release pipeline checks the binary it publishes.** A smoke test runs the freshly
  built artifact before upload: version, `run`, `vm`, and `check` on a spread of
  programs that exercise the release's claims. The version it expects comes from the tag
  itself, so the next cut cannot silently advertise a different one.

The 0.9.0 picture behind the patch, since the feature list that used to live here was
several versions stale:

**A native installer.** `rak-setup` opens a window with no arguments: `eframe`/`egui` on
`glow`, with no WebView2 or `wgpu` to go missing. It shows progress, edits PATH, sets
`RAK_PATH`, creates shortcuts and `.rak` associations, and records every action in
`~/.rak/manifest.json` for clean uninstall. The Tauri NSIS/MSI/`.deb`/AppImage bundles
remain for IDE-only users.

**A trustworthy package manager.** Caret, tilde, comparators, wildcards, comma ranges,
prereleases and build metadata follow real semver rules. Every lockfile entry records a
SHA-256 over the whole vendored dependency tree, and `--offline` turns "existing checkout
is reused" into a guarantee instead of a lucky accident. `oyvey add` writes the package's
declared name instead of truncating it, and unknown flags are rejected rather than
silently ignored.

**A corrected compiler.** 0.9.0 hunted down the bugs that succeeded with the wrong
answer: `PartialEq` compared type discriminants, so distinct values compared equal;
type annotations were enforced on one backend only; integer arithmetic wrapped,
including `i64::MIN / -1`; numeric comparison lost precision above 2^53; same-valued
int and float constants aliased in the constant pool; casts returned 0 or nonsense;
struct/enum type declarations were parsed and discarded; malformed `\x` escapes ate
characters; `ord` and `chr` disagreed on what a character is; a negative `substr`
bound now surfaces a catchable error. Each one is pinned by a regression test on both
backends.

**A runtime that tells you the truth.** `fn main(argv)` works and its return value is
the process exit code. Errors raised inside `main` reach the user instead of being
folded into an exit code. `--sandbox` is enforced. Exceptions unwind from the right
place. Integer arithmetic now errors instead of wrapping, and integer division by zero
is a user-facing error; float division by zero follows IEEE.

**Written-down limits.** The bytecode VM still refuses a named list of constructs
(`async {}`, `test {}`, `assert`, `trace`, `scan`, `fetch`, `Lambda`, `Spawn`, `Raise`,
`Comprehension`, `as`, `TypedInt`, `Range`, `ensures`) instead of guessing, and several
language features (generics, trait bounds and `where`, binding patterns, tail-call
optimisation, doc comments, `while`/`for` `else`, closure capture of mutated locals)
are still roadmap rather than release promises. `substr` returns `""` when out of
range while `slice` clamps, and that difference is called out, not hidden. CI runs
the full suite on both Linux and Windows, and the release binaries for `rakc`,
`rak-setup` and `oyvey` ship behind the hardening verifier (ASLR, high-entropy VA,
DEP, CFG-compatible image, `LOAD_CONFIG`).

## Install

The custom installer is an interactive TUI wizard that installs `rakc`, `oyvey`, and the Rak IDE, edits PATH, sets `RAK_PATH`, creates shortcuts + `.rak` associations, and installs man pages + shell completions. It runs in net-install (downloads the latest release) or offline-bundle mode, and supports `--uninstall` / `--list` / `--yes` for scripting.

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

Or download the setup binary directly from the [latest release](https://github.com/Louiml/Rak/releases/latest) (`rak-setup-linux-x86_64` or `rak-setup-windows-x86_64.exe`) and run it.

Menu items: `rakc → bin + PATH`, `oyvey → bin + PATH`, `IDE → portable dir / system location`, `Set RAK_PATH`, `Shortcuts + .rak association`, `Man pages + shell completions`. Install scope: `user` (default, no privileges) or `system`. A `~/.rak/manifest.json` records every action for clean uninstall/upgrade.

The Tauri installers (NSIS/MSI on Windows, `.deb`/AppImage on Linux) remain on the release page for IDE-only users who want the OS-native installer.

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

**Forensic Structs & evidence provenance.** Declare a binary wire format once with `binstruct` and get both a decoder and an encoder for free (round-trip). Every decoded value is wrapped in an `evidence<T>` provenance tag carrying where/when/how it was collected, so `report(...)` emits a chain-of-custody-cited findings report. No other language bakes provenance into the value model. This only makes sense in a hex-first, OSINT-first language.

**Bytecode VM.** `rakc vm` runs bytecode. Benchmarks show about 6x speedup over the tree-walking interpreter. Run `rakc bench file.rak` to see both.

**Data pipelines.** A `|>` pipeline operator, regex literals (`/\d+/g`) with method syntax, and binary pattern matching over byte slices, built for OSINT log and PCAP triage.

**Type system.** Signed and unsigned ints (i8 through u64), floats (f32, f64),
typed literals like `42i32` and `3.14f64`, a first-class `char`, base literals
(`0b1010`/`0o755`/`0xFF`), tuples, Result/Option, modules, closures with lexical
scoping, traits with method dispatch, pattern matching (including
binary byte-pattern matching, plus user enum variants), and string interpolation
with `f"hello {name}"`. A static type checker behind `rakc check` catches bad
annotations and non-exhaustive matches before you run anything.

**Concurrency.** `spawn` launches a thread, `thread_join` waits for it, `channel()` gives you a sender/receiver pair. TCP networking built in with `net_listen`, `net_accept`, `tcp_read`, `tcp_write`. Async I/O via a Tokio runtime: `async fn`/`await`, `http_get_async`, `tcp_probe`.

**SQL server in Rak.** A full SQL engine, lexer through executor, written in the language itself. It serves queries over TCP. See `examples/sql_server.rak`.

**Self-hosting.** `examples/self_host.rak` is a Rak interpreter written in Rak. It lexes, parses, and evaluates real Rak source code.

**Error handling.** `try`/`catch`/`raise`, the `?` operator, Result and Option types. Lexer and parser errors report line and column. `rakc check file.rak` prints diagnostics in the format IDEs expect.

**Lexical closures.** Free variables resolve at the definition site, not the call site. Recursive closures like `let f = fn(n) { if n < 2 { return n } return f(n-1) + f(n-2) }` just work.

**Real JSON.** `json_parse` returns native arrays, maps, ints, floats, bools, nil. `json_stringify` serializes back to a string. No more hand-rolling JSON in your SQL server.

**Build standalone executables.** `rakc build file.rak` produces a self-contained binary with the source embedded. On Windows it's a `.exe`. On Linux it gets `chmod 755`. No Rak installation needed on the target machine.

**GUI windows.** With `--features gui`, Rak scripts open native desktop windows rendering HTML, CSS and JS, on Windows and Linux. Windows can be updated, retitled and closed; JavaScript calls back into Rak through `rak_call`, and a callback's return value reaches the page. Closing a window no longer ends the process. See [GUI windows](#gui-windows).

**Package manager and build system.** `oyvey` is the official Rak package manager: a Cargo-style toolchain with a `package.rak` manifest, an `oyvey.lock` recording the exact revision and checksum of every dependency, transitive resolution, a global git cache, and `build` / `run` / `test` driven through `rakc`.

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

**Memory-mapped files.** Map huge PCAP / log files into memory and inspect them zero-copy. `mmap_open` maps a file; `mmap_slice` returns a view that keeps the mapping alive and reads directly from the mapped pages. Single-byte indexing, byte search, and line scanning work on both the interpreter and the bytecode VM; range slicing and binary pattern matching over slices work on the interpreter.

```rak
let m = mmap_open("trace.pcap", "r")
dump mmap_size(m)
let hdr = mmap_slice(m, 0, 8)
dump hdr[0]                       // first byte (zero-copy)
dump mmap_find(m, "\xff\xd8\xff") // byte search -> offset
for (off, len) in mmap_lines_off(m, "\n") {
    dump string(mmap_slice(m, off, len))
}
```

**Async event loop.** `async fn`, `await`, and Tokio-backed async I/O. `http_get_async` / `tcp_probe` / `tcp_connect_async` run on a lazily-started multi-thread Tokio runtime and return a `Future`; `await` blocks until it resolves. Works on both the interpreter (deferred `async fn` bodies) and the bytecode VM (`Op::Await`).

```rak
async fn probe(host, port) {
    let open = await tcp_probe(host, port, 200)
    return open
}
dump await probe("127.0.0.1", 80)

let body = await http_get_async("https://example.com")
dump string(body)
```

**Raw sockets & packet forging.** Build IPv4/TCP/UDP headers with correct ones-complement checksums and send them on a raw socket. The packet builders are pure computation and run anywhere; `net_raw_send`/`recv` need `CAP_NET_RAW`/Administrator (unix) and return a `Result`.

```rak
let pkt = net_raw_tcp_syn("10.0.0.5", "10.0.0.10", 12345, 80)
dump len(pkt)          // 40 bytes
dump pkt[0]            // 0x45 (IPv4)
dump pkt[33]           // 0x02 (SYN flag)
dump net_raw_send(pkt) // Ok(40) on a privileged unix box, Err(...) otherwise
```

**Protocol parsers (DNS / TLS / PCAP).** Hand-rolled DNS wire-format builder/parser + UDP query (no external DNS crate, works air-gapped). TLS ClientHello SNI extraction and DER cert-chain parsing via `x509-parser`. PCAP offline capture behind the `pcap` cargo feature (libpcap/Npcap). All return `Result`s so they degrade gracefully offline / without the feature.

```rak
dump dns_query("example.com", "A")      // Ok({answers: [{name, type, ttl, rdata}, ...], truncated})
let q = dns_build("example.com", "A")   // raw query bytes (offline)
let info = tls_parse_client_hello(bytes) // {sni, ciphers}
let certs = tls_parse_cert_chain(der)    // [{subject, issuer}, ...]
dump pcap_open("capture.pcap")           // Ok(<pcap>) or Err(...)
```

**Compile-time macros.** `macro name($params) { body }` defines an AST-expanding template; `name!(args)` splices the argument expressions into the body's `$param` placeholders before evaluation. Expanded in the frontend, so both backends see the expanded code. `const NAME = expr` binds a compile-time constant.

```rak
macro add1(x: expr) { $x + 1 }
dump add1!(41)            // 42

macro pair(a: expr, b: expr) { [$a, $b] }
dump pair!(1, 2)          // [1, 2]

const MAX_LEN = 256
dump MAX_LEN
```

## Forensic Structs & evidence provenance

A declarative wire-format DSL (`binstruct`) plus a provenance-typed value
(`evidence<T>`), the OSINT differentiator. Declare a binary layout once and
get both a **decoder** and an **encoder** for free (round-trip); every decoded
value is wrapped in an evidence tag carrying where/when/how it was collected, so
`report(...)` emits a chain-of-custody-cited findings report. No other language
bakes provenance into the value model. This only makes sense in a hex-first,
OSINT-first language.

```rak
binstruct DnsHeader {
    id:      u16be
    flags:   u16be
    qdcount: u16be
    ancount: u16be
    nscount: u16be
    arcount: u16be
}

let q = dns_build("example.com", "A")
let h = DnsHeader.decode(q)        // -> evidence<struct> with provenance
dump h.id                          // 0x1234
dump h.qdcount                     // 1
let back = DnsHeader.encode(h)     // round-trip back to bytes
```

Field types: `u8`/`u16`/`u32`/`u64` and signed `i8`..`i64`, each with an optional
`be`/`le` endianness suffix (default big-endian); `bytes(n)` for a fixed run of
raw bytes; `rest` for the trailing remainder; and a nested binstruct name for a
`Ref` field. Works on both the interpreter and the bytecode VM (compile-time
codegen to per-struct native-fn globals, no new VM opcodes).

Evidence provenance:

```rak
let ip = evidence<string> from "93.184.216.34"          // root tag
let answers = cite(dns_query("example.com", "A"), "dns_query", "example.com")
dump report(ip, answers)   // numbered assertions + cited sources (tool/target/ts)
dump provenance(ip)        // {tool, target, ts, raw_offset, raw_len, parent}
dump strip_evidence(ip)    // 93.184.216.34
```

- `evidence<T> from expr` wraps a value in a root provenance tag.
- `cite(value, tool?, target?)` wraps a value, chaining `parent` to any
  existing evidence so provenance merges transitively.
- `report(evidence, ...)` renders a Markdown-style report with numbered,
  source-cited assertions (the chain-of-custody output an investigator needs).
- `provenance(value)` returns the provenance chain as a map.
- `strip_evidence(value)` drops the provenance wrapper and returns the inner value.

Field access and indexing transparently unwrap evidence, so
`evidence<struct>.field` reads through to the inner struct.

## Build a standalone executable

```bash
rakc build examples/hello.rak   # produces hello.exe on Windows, hello on Linux
./hello                          # [DUMP] Hello, World
```

## GUI windows

Requires `cargo build -p rakc --features gui`. Works on Windows and Linux.

```rak
fn on_click(msg) { return "you said: " + msg }

let html = r#"<h1 style='color:#22c55e;text-align:center;margin-top:40px'>Hello from Rak!</h1>
<button onclick="rak_call('on_click', 'hi'); rak_on('on_click', show)">Click me</button>
<script>
  function show(v) { document.body.insertAdjacentHTML('beforeend', '<p>' + v + '</p>') }
</script>"#

gui_callback("on_click", on_click)

let win = gui_open("Rak GUI Demo", html, 600, 400)
gui_title(win, "Still here")
gui_update(win, "<h1>replaced</h1>")
gui_wait()
```

| Builtin | Behaviour |
|---|---|
| `gui_open(title, html, w, h)` | Opens a window, returns its id. Blocks until it exists, so the id is real. |
| `gui_update(id, html)` | Replaces the document. The window keeps its position, size and z-order. |
| `gui_title(id, title)` | Sets the window title. |
| `gui_close(id)` | Closes one window. The process keeps running. |
| `gui_wait()` | Blocks until every window is closed. |
| `gui_quit(code)` | Stops the GUI with an exit code. |
| `gui_callback(name, fn)` | Registers a Rak function callable from page JavaScript as `rak_call(name, ...)`. |

The page calls Rak with `rak_call(name, ...args)`, and a callback's return value
reaches the page as `rak_result(name, value)`. Register a page-side handler with
`rak_on(name, fn)`.

**One real limitation.** A callback does not share mutable state with the script
that registered it. It receives a copy of the environment as it stood at
`gui_callback` time, so it can read what existed then and return a value to the
page, but it cannot write to a variable the main script reads afterwards. This
follows from Rak having no reference types. Values are shared rather than moved.
See [docs/V8-KNOWN-ISSUES.md](docs/V8-KNOWN-ISSUES.md).

Also worth knowing: a function body without an explicit `return` evaluates to
`nil`, which matters here because `gui_callback` takes a function.

## Package manager

`oyvey` is the official Rak package manager and build system.

```bash
oyvey new mylib              # generate a project (manifest, src/main.rak, tests)
oyvey add user/repo          # add a GitHub dependency (vendored into packages/)
oyvey install                # resolve + install deps from package.rak
oyvey run                    # run the entry point via rakc run
oyvey build                  # build a standalone executable via rakc build
oyvey test                   # run the project's tests
oyvey list                   # list installed packages
oyvey remove mylib           # remove a dependency
```

The manifest is a Rak file:

```rak
let name = "mylib"
let version = "0.1.0"
let description = "A Rak package"
let license = "MIT"
let entry = "src/main.rak"
let deps = {
    net: "user/rak-net",
    crypto: "user/rak-crypto@^1.0"
}
```

Import installed packages by file path or by name (Python-style). See the
Imports & exports section below for the full syntax.

```rak
use "./packages/mylib/lib.rak"   // file-path (back-compat)
import mylib                     // name -> searches dir, ./packages/, RAK_PATH
```

## Imports & exports

Rak has a Python-style module system with explicit `pub`/`export`, name-based
resolution, `from ... import`, directory packages, import-once caching, and
circular-import support. Works on both the interpreter and the bytecode VM.

```rak
// Whole-module import (binds the module; access via m.x)
import "./math.rak"          // file path -> binds "math"
import math                  // name: searches dir, ./packages/, RAK_PATH for math.rak
import math as m             // alias
use math                     // back-compat: same as import

// from-import (binds names directly)
from math import add
from math import add as plus, mul as times
from math import *           // all exports; local bindings win on clash

// Directory packages: `import pkg` runs pkg/init.rak; `import pkg.sub`
// runs pkg/init.rak + pkg/sub.rak, so both `pkg.f` and `pkg.sub.f` resolve.
import pkg
import pkg.sub

// Exports. Both `pub` and `export` mark an item as exported (let/fn/const/
// struct/enum/macro):
pub let PI = 3.14
export fn add(a, b) { return a + b }
pub const MAX = 256

// Re-exports
pub use math                 // re-export all of math from this module
pub use {add, mul} from math // re-export named
```

Module resolution for a name `m`: the importing file's directory, then
`./packages/`, then the `RAK_PATH` env var (`;` on Windows, `:` on Unix),
trying `m.rak` then `m/init.rak`. Modules are imported once (cached); a
circular import returns the partially-initialized module (Python semantics).

### `import m` gives you the module; `from m import x` gives you a value

This is the one rule worth knowing before writing a multi-file program.

```rak
// bank.rak
pub let mut BALANCE = 0
pub fn deposit(n) { BALANCE = BALANCE + n  return BALANCE }
```

```rak
import bank                        // <- a live view of the module's state
from bank import BALANCE           // <- a copy, taken once, at import time

bank.deposit(10)
bank.deposit(10)
dump bank.BALANCE                  // 20. The view followed the module
```

`m` is a handle on the module's own top-level bindings, so `m.X` reads what the
module last stored there and `m.X = v` writes the module's state. It survives
aliases, function arguments, and being stored in a map. A module's top-level
`let mut` is therefore real state: it persists across calls and across a call
chain, not a per-call copy.

`m.X = v` writes the module's own state, and is bounded by the module's own
declarations: `pub let mut X` is assignable through the handle, `pub let X` is
readable but fixed, and a name with no `pub` is unreachable. Both backends enforce
both rules.

`from m import x` binds the value as it stood at import time, Python's rule,
and worth keeping because a live binding there would be a second invisible way
for a module to mutate a caller's variables. Alias it (`from bank import
BALANCE as opening`) when you want a snapshot under a name of your choosing;
that copies on both backends.

`pub use` copies too, for the same reason. See
[docs/content/modules.md](docs/content/modules.md) for the full rules, and
[docs/V8-KNOWN-ISSUES.md](docs/V8-KNOWN-ISSUES.md) for the six symptoms of the
VM's flat global namespace, which is where the two backends still differ.

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

### Characters and base literals

```rak
let c: char = 'a'
let hebrew: char = 'א'
let alpha = '\u{03B1}'   // α
let bin = 0b1010          // 10
let oct = 0o755           // 493
dump 0xA == 10            // true
```

### Generic functions

```rak
fn identity<T>(value: T) -> T { return value }
dump identity<int>(42)        // 42
dump identity<string>("hi")   // hi
dump identity(99)             // inferred: 99
```

Both backends run this by erasing the type parameter, but `rakc check` does not yet
model generics, so it flags the calls above. It is syntax on the road to real
checking, not a finished promise.             

### Defer

`defer f()` runs in LIFO order when the current function exits. The last one
registered runs first. Same on the interpreter and the VM.

```rak
fn work() {
    defer dump "cleanup-last"
    defer dump "cleanup-first"
}
work()   // cleanup-first, then cleanup-last
```

### Tests and type checking

```rak
test "addition works" {
    assert add(2, 3) == 5
    assert_eq(add(2, 3), 5)
    expect_error(fn() { raise "boom" })
}
```

```bash
rakc test tests/math.rak    # PASS/FAIL, exit non-zero on failure
rakc check file.rak         # type mismatches, non-exhaustive matches
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

### Byte-exact file I/O

`read`/`file_read` are UTF-8, which means `file_read` **fails** on any file
containing a byte sequence that is not valid UTF-8, and `write` turns a `0xFF`
into U+FFFD on the way out. Between them there was no way to open a binary file.

```rak
let buf = file_read_bytes("firmware.bin")     // bytes, any content
buf[0] = 0xFF                                  // mutate in place
dump hex_encode(buf)
file_write_bytes("patched.bin", buf)           // byte-exact write
file_append_bytes("log.bin", bytes([0xDE, 0xAD]))

let m = mmap_open("firmware.bin", "rw")        // no copy, no flush
mmap_write(m, 0x100, 0x90)                     // the mapping *is* the file
dump m[0x100]
```

`bytes([...])` builds a buffer from numbers, `buf[i] = v` writes a byte, `for b in
buf` yields the bytes as ints, and `b1 + b2` joins buffers. `mmap_write` refuses a
read-only mapping and an out-of-range offset by name, and a rejected multi-byte
write applies none of its bytes.

`fmt` takes real format specs, so a hex column is a loop and a `fmt`:

```rak
for b in row { dump fmt("{:02X}", b) }   // 0F, not 0
dump fmt("{:#x}", 255)                  // 0xff
dump fmt("[{:>4}]", n)                   // right-aligned, width 4
dump fmt("{:.2f}", ratio)                // two decimals, on both backends
```

`[[fill]align][+][#][0][width][.precision][type]`, with `x X b o d` and `f e E`.
The width counts the sign, so `{:05}` of `-42` is `-0042`. One spec means the same
thing on both backends.

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

### Cryptography without FFI

`hmac_sha256`, AES-256-GCM (`aes_gcm_encrypt` / `aes_gcm_decrypt`), and Ed25519
(`ed25519_keypair`, `ed25519_sign`, `ed25519_verify`). All take and return
`bytes`/`string` and are available on both the interpreter and the VM.

```rak
dump hmac_sha256("key", "data")
let key = b"\x00\x01...\x1f"         // 32 bytes
let nonce = b"\x00\x01...\x0b"       // 12 bytes
let ct = aes_gcm_encrypt(key, nonce, b"payload")
dump aes_gcm_decrypt(key, nonce, ct) // b"payload"

let pair = ed25519_keypair(seed)
let sig = ed25519_sign(pair.1, b"msg")
dump ed25519_verify(pair.0, sig, b"msg")   // true
```

### DNS toolkit

Beyond `dns_query`/`dns_build`/`dns_parse`, the investigation API:
`dns_resolve(host)`, `dns_reverse(ip)` (real PTR, v4 + v6), `dns_records(host)`
(all record types), and `dns_walk(domain, prefixes)`.

```rak
dump dns_resolve("example.com")          // all A + AAAA addresses
dump dns_reverse("8.8.8.8")              // [dns.google]
for r in dns_records("example.com") { dump r.type }
dump dns_walk("example.com", ["www", "mail", "api"])
```

### Process API

`process_spawn(cmd, args)`, `process_wait(pid)`, `process_stdout(pid)`,
`process_stderr(pid)`, `process_kill(pid)`.

```rak
let pid = process_spawn("cmd", ["/c", "echo", "hi"])
process_wait(pid)
dump process_stdout(pid)
```

### HTTP server (pull-based)

A minimal HTTP/1.1 server. Requests are queued and read on the main thread, so
handlers can use arbitrary Rak logic (closures, etc.). Works on both backends.

```rak
http_server_start("127.0.0.1", 8080)
loop {
    let req = http_server_poll()          // nil when idle; never blocks
    if req == nil { sleep(10) }
    else {
        if req.path == "/users" {
            http_server_respond(req.id, 200, { "Content-Type": "application/json" }, "{\"ok\":true}")
        } else {
            http_server_respond(req.id, 404, {}, "not found")
        }
    }
}
```

`http_server_start(addr, port)` binds and queues parsed requests (returns the
bound port); `http_server_poll()` returns
`{id, method, path, query, headers, body}` or `nil`; `http_server_respond(id,
status, headers, body)` writes the response; `http_server_stop()` cleans up.
String-keyed map literals (`{ "Content-Type": "application/json" }`) are now
supported for JSON-style maps.

### WebSocket

RFC 6455 framing + handshake built on the existing TCP stream handle.
`ws_connect(url)` performs the client handshake; `ws_handshake(stream)` replies
to a client Upgrade request on a `net_accept` connection; `ws_send(stream, data,
mask)` sends a text frame (client→server frames must be masked, servers pass
`false`); `ws_recv(stream)` returns `{opcode, payload}` or `nil`; `ws_close(stream)`
sends a close frame. Interpreter-only (the VM has no TCP layer).

```rak
// server: accept + handshake + echo
let list = net_listen("127.0.0.1:19001")
let (stream, _) = net_accept(list)
ws_handshake(stream)
loop {
    let m = ws_recv(stream)
    if m == nil { break }
    if m.opcode == 1 { ws_send(stream, m.payload, false) }
}

// client
let ws = ws_connect("ws://127.0.0.1:19001/echo")
ws_send(ws, "hello", true)
dump string(ws_recv(ws).payload)   // hello
```

### Structured logging (machine-readable)

`log_level(level)`, `log_init(path?)`, and `log_info` / `log_warn` / `log_error`
/ `log_debug`. Each call emits one JSON line (default stdout, or append-only
file), greppable with jq.

```rak
log_level("debug")
log_info("scan_start", { host: "127.0.0.1", ports: [80, 443] })
```

### Secrets API

`secret_get(name)`, `secret_set(name, value)`, `secret_persist(name, value)`
(0600 file), `secret_delete(name)`, `secret_ls()`. Resolution: session → env
var → durable store. Never logged.

```rak
secret_set("API_KEY", "abc123")
dump secret_get("API_KEY")
```

## CLI

```bash
rakc run <file>     Run a Rak script on the interpreter
rakc vm <file>      Run on the bytecode VM
rakc build <file>   Build a standalone executable
rakc bench <file>   Benchmark interpreter vs VM
rakc check <file>   Static type check; print diagnostics with line/column
rakc test [file]    Run test blocks (--filter NAME, --verbose)
rakc lex <file>     Print tokens
rakc parse <file>   Print AST
rakc repl           Start an interactive REPL
rakc lsp            Start the language server (stdio)
rakc bindgen <h>    Generate Rak bindings from a C header
rakc --version

oyvey new <project>      Generate a new Rak project
oyvey init               Initialize an existing directory as a project
oyvey add <user/repo>    Add a GitHub package dependency
oyvey remove <package>   Remove a dependency
oyvey install            Resolve and install dependencies
oyvey update             Re-resolve dependencies within constraints
oyvey run                Run the entry point
oyvey build              Build a standalone executable
oyvey test               Run the project's tests
oyvey clean              Remove build artifacts
oyvey list               List installed packages
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

Windows and Linux. On Windows the GUI uses WebView2 (ships with Edge); on Linux it uses WebKitGTK. The feature is per-crate, so it needs `-p rakc --features gui`, not a workspace-wide `--features gui`. `rakc build` produces `.exe` on Windows and an executable with `chmod 755` on Linux. The IDE ships as NSIS/MSI on Windows and `.deb`/AppImage on Linux via GitHub Actions CI, alongside the custom `rak-setup` installer (net-install + offline bundle) on both.

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
├── stdlib/            Rust native stdlib (net, crypto, encoding, recon, web, file, json, log, process, secrets, http_server, websocket)
├── oyvey/             Package manager + build system
├── rak-setup/        Custom interactive installer (TUI wizard: net-install + offline)
├── dist/              One-liner bootstraps (install.sh / install.ps1), man pages, completions
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
│   ├── mmap.rak            Memory-mapped files: zero-copy slice/search/lines
│   ├── async.rak           Async event loop: async fn / await / tcp_probe
│   ├── net_raw.rak         Raw sockets: forge IPv4/TCP/UDP packets
│   ├── parsers.rak         DNS / TLS / PCAP wire-format parsers
│   ├── macros.rak          Compile-time macros: macro / name! / const
│   ├── import_demo.rak    Python-style import / from / export (with mymod/ package)
│   ├── modstate/           Cross-module variable semantics: live `import m` vs
│   │   ├── module_state.rak  copying `from m import x`, module-level `let mut`,
│   │   ├── bank.rak          `pub` as the visibility boundary, `pub use` copying
│   │   └── ledger.rak
│   ├── forensic_structs.rak  binstruct decode/encode round-trip + evidence provenance
│   └── stdlib_demo.rak     String, array, and math builtins
```

## Build from source

```bash
git clone https://github.com/Louiml/Rak.git
cd Rak
cargo build --release                    # rakc + oyvey
cargo build --release -p rakc --features gui   # rakc with GUI support
cd ide && npm install && npx tauri build # IDE
```

Linux requires `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, and `librsvg2-dev`.

## Tech stack

The compiler is Rust. Logos handles lexing. The parser is hand-written recursive descent. Two backends: a tree-walking interpreter and a stack-based bytecode VM.

The standard library uses `ureq` for HTTP, `scraper` for HTML parsing, `md-5`/`sha1`/`sha2` for hashing, `std::net` for TCP, `std::thread` and `mpsc` for concurrency, `libloading` for FFI, `memmap2` for memory-mapped files, and `tokio` for the async event loop. Real JSON via `serde_json`.

The IDE is Tauri v2, Next.js, TypeScript, and Tailwind CSS. It runs Rak scripts as child processes, streams output, and has an integrated terminal.

## License

Apache-2.0

## Author

Louiml (Ryuzaki)
