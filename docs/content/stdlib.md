# Standard library

## Strings, arrays, and math

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

Also: `sort`, `upper`, `lower`, `trim`, `trim_end`, `substr`, `ord`, `chr`,
`push`, `keys`, `values`, `has`, `get`, `split`, `join`, `contains`, `len`,
`to_hex`, `from_hex`, `fmt`.

## HTML scraping (OSINT)

`html_title`, `html_select`, `html_select_all`, `html_attr`, `html_links`,
`html_images`, `html_scripts`, `html_forms`, `html_inputs`, `html_meta`,
`html_count`, `html_headers`.

```rak
let html = fetch("https://example.com")
dump html_links(html)
```

Also `scan_ports(host, { range: [a, b] })`, `scan_subdomains(domain)`,
`dns_lookup`, `reverse_dns`.

## Hashing & encoding

```rak
dump md5("password")
dump sha256("secret")
dump hex_encode("ABC")
dump base64_encode(b"hi")
dump url_encode("a b")
```

## Cryptography (native — no FFI)

`hmac_sha256`, AES-256-GCM (`aes_gcm_encrypt` / `aes_gcm_decrypt`), and
Ed25519 (`ed25519_keypair`, `ed25519_sign`, `ed25519_verify`). All take and
return `bytes`/`string` and are available on both the interpreter and the VM.

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

The VPN toolkit (X25519 / ChaCha20-Poly1305 / HKDF / UDP transport) is covered
in [VPN & tunneling](vpn.html).

## DNS toolkit

Beyond `dns_query`/`dns_build`/`dns_parse`, the investigation API:

```rak
dump dns_resolve("example.com")          // all A + AAAA addresses
dump dns_reverse("8.8.8.8")              // [dns.google]
for r in dns_records("example.com") { dump r.type }
dump dns_walk("example.com", ["www", "mail", "api"])
```

## Process API

```rak
let pid = process_spawn("cmd", ["/c", "echo", "hi"])
process_wait(pid)
dump process_stdout(pid)
```

`process_spawn(cmd, args)`, `process_wait(pid)`, `process_stdout(pid)`,
`process_stderr(pid)`, `process_kill(pid)`.

## Structured logging (machine-readable)

Each call emits one JSON line (default stdout, or append-only file) —
greppable with jq.

```rak
log_level("debug")
log_init("app.log")
log_info("scan_start", { host: "127.0.0.1", ports: [80, 443] })
```

`log_level(level)`, `log_init(path?)`, `log_info`, `log_warn`, `log_error`,
`log_debug`.

## Secrets API

`secret_get(name)`, `secret_set(name, value)`, `secret_persist(name, value)`
(0600 file), `secret_delete(name)`, `secret_ls()`. Resolution: session → env
var → durable store. Never logged.

```rak
secret_set("API_KEY", "abc123")
dump secret_get("API_KEY")
```

## Files

`file_read`, `file_write`, `file_append`, `file_exists`, `file_size`,
`file_list`, `file_delete`, `file_mkdir`, `file_copy`, `file_rename`,
`file_ext`, `file_basename`, `file_dirname`.

## Misc

`sleep`, `now_ms`, `args`/`argv`, `env_get`, `env_set`, `ord`, `chr`,
`substr`, `print`, `dbg`, `exit`, `eprint` (stderr, 0.7), GUI builtins
(`gui_open`, `gui_update`, `gui_title`, `gui_close`, `gui_wait`,
`gui_callback`).
