# Rak

A programming language built for hackers and OSINT investigators.

English-readable syntax with first-class hexadecimal. Write network scanners, web scrapers, and forensic tools without boilerplate.

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

```rak
for port in scan_ports("127.0.0.1", { range: [0x0016, 0x0050] }) {
    dump fmt("Open: 0x{:04X}", port)
}
```

## Install

Download the latest release for Windows:

- **MSI installer**: `rak-ide_0.1.0_x64_en-US.msi`
- **NSIS installer**: `rak-ide_0.1.0_x64-setup.exe`

Or build from source:

```bash
git clone https://github.com/Louiml/Rak.git
cd Rak
cargo build --release
```

## The Rak IDE

A desktop IDE built with Tauri and Next.js:

- Custom syntax highlighting for Rak keywords, hex literals, byte strings
- File explorer with directory picker
- Multiple tabs
- Find and replace (Ctrl+F / Ctrl+H)
- Autocomplete for all builtins
- Right-click context menu
- Custom frameless title bar
- Dark hacker theme

### Run the IDE from source

```bash
cd ide
npm install
npx tauri dev
```

### Build the IDE

```bash
cd ide
npx tauri build
```

Output goes to `target/release/bundle/`.

## Language Syntax

### Variables

```rak
let target = "192.168.1.1"
let port = 0x0050
let mut counter = 0
counter = counter + 1
```

### Functions

```rak
fn add(a, b) {
    return a + b
}

let result = add(0x10, 0x20)
```

### Control Flow

```rak
if port == 0x0050 {
    dump "HTTP"
} else {
    dump "Other"
}

for x in [1, 2, 3] {
    dump x
}

while counter < 0x0A {
    counter = counter + 1
}

loop {
    break
}
```

### Hexadecimal

Hex is a first-class type. Arithmetic, bitwise ops, and formatting all work natively:

```rak
let sig = 0xDEADBEEF
let mask = 0xFF00FF00
let result = sig & mask
dump fmt("0x{:08X}", result)
```

### OSINT Statements

```rak
// Port scan with callback block
scan target {
    range: [0x0016, 0x01F4],
    timeout: 0x1388
} {
    if open {
        dump fmt("Port {} open", port)
        dump banner
    }
}

// HTTP fetch with response block
fetch "http://target.com" {
    method: "GET",
    headers: { "User-Agent": "Rak-OSINT/0.1" }
} {
    dump status
    dump headers["Server"]
    trace body
}

// Write to file
dump "results" , "output.txt"
```

### Types

| Type | Description | Example |
|------|-------------|---------|
| hex8-hex64 | Unsigned integers | 0xFF, 0xBEEF |
| int | Decimal integer | 42 |
| string | UTF-8 text | "hello" |
| bytes | Raw byte array | b"\x48\x65" |
| bool | Boolean | true, false |
| nil | Null | nil |
| [T] | Array | [1, 2, 3] |
| {K: V} | Map | {"key": "val"} |

## Standard Library

### Networking (net)

```rak
fetch "https://api.example.com" {
    method: "GET"
} {
    dump body
}
```

Real TCP scanning and HTTP requests via `ureq`.

### Recon

```rak
let ports = scan_ports("target.com", { range: [0x0016, 0x0050] })
let subs = scan_subdomains("example.com")
let ips = dns_lookup("example.com")
let ptr = reverse_dns("1.2.3.4")
```

### Crypto

```rak
dump md5("password")
dump sha1("data")
dump sha256("secret")
dump xor_encrypt(data, key)
dump rot13("text")
```

### Encoding

```rak
dump hex_encode("ABC")        // 414243
dump hex_decode("414243")     // bytes
dump base64_encode("hello")   // aGVsbG8=
dump url_encode("a b&c")      // a%20b%26c
```

### Web (HTML parsing)

```rak
let html = "<html><title>Page</title><a href='/link'>Link</a></html>"
dump html_title(html)           // "Page"
dump html_select(html, "a")     // "Link"
let links = html_links(html)    // ["/link"]
let imgs = html_images(html)
let forms = html_forms(html)
dump html_count(html, "a")      // 1
```

Uses the `scraper` crate with full CSS selector support.

### File operations

```rak
file_write("log.txt", "entry")
dump file_read("log.txt")
dump file_exists("log.txt")
dump file_size("log.txt")
let entries = file_list(".")
file_delete("log.txt")
file_mkdir("output")
file_copy("a.txt", "b.txt")
```

### JSON utilities

```rak
let json = "{\"user\": \"admin\"}"
dump json_get(json, "user")
dump json_path(json, "user")
dump json_keys(json)
let found = json_find_all(json, "user")
```

### String and array utilities

```rak
dump len("hello")           // 5
dump len([1, 2, 3])         // 3
dump split("a,b,c", ",")    // ["a", "b", "c"]
dump join(["a", "b"], "-")  // "a-b"
dump contains("hello", "lo") // true
dump upper("hello")        // HELLO
dump to_hex(0xFF)          // 0xFF
dump from_hex("DEAD")      // 0xDEAD
```

## CLI

```bash
rakc run script.rak      # Run a Rak script
rakc lex script.rak      # Print tokens
rakc parse script.rak    # Print AST
rakc --version           # Print version
rakc run -               # Read from stdin
```

## Project Structure

```
Rak/
├── rakc/              # Compiler crate (lexer, parser, interpreter)
│   └── src/
│       ├── lexer.rs    # Tokenizer with logos
│       ├── parser.rs   # Recursive descent parser
│       ├── ast.rs      # AST definitions
│       └── interpreter.rs  # Tree-walking interpreter
├── stdlib/            # Standard library
│   └── src/
│       ├── net.rs      # HTTP client, TCP scanning
│       ├── crypto.rs   # MD5, SHA1, SHA256, XOR, ROT13
│       ├── encoding.rs # Hex, Base64, URL encoding
│       ├── recon.rs    # DNS, subdomain enum, port scan
│       ├── web.rs      # HTML parsing with CSS selectors
│       ├── file.rs     # File system operations
│       └── js.rs       # JSON utilities
├── ide/               # IDE (Tauri + Next.js)
│   ├── src/
│   │   └── app/
│   │       ├── page.tsx              # Main IDE layout
│   │       └── components/
│   │           ├── TitleBar.tsx      # Custom window title bar
│   │           ├── FileExplorer.tsx  # File tree sidebar
│   │           ├── TabBar.tsx        # Multiple file tabs
│   │           ├── CodeEditor.tsx    # Editor with syntax highlighting
│   │           └── EditorContextMenu.tsx
│   └── src-tauri/     # Rust backend
│       └── src/lib.rs # Tauri commands
├── docs/             # HTML/CSS/JS documentation site
│   ├── index.html
│   ├── style.css
│   └── script.js
└── examples/         # Example scripts
    ├── hello.rak
    ├── quick.rak
    └── osint_scan.rak
```

## Tech Stack

- **Compiler**: Rust, `logos` for lexing, hand-written recursive descent parser
- **Standard library**: Rust, `ureq` for HTTP, `scraper` for HTML, `md-5`/`sha1`/`sha2` for hashing
- **IDE**: Tauri v2, Next.js 16, TypeScript, Tailwind CSS
- **Docs**: Static HTML/CSS/JS

## License

MIT

## Author

Louiml
