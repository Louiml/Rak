# Standard library

Every builtin, grouped by what it does. Both backends register the same set;
the exceptions are noted at the bottom.

Results are written as `Ok(v)` / `Err(e)`, and options as `Some(v)` / `None`.
A builtin that *raises* says so.

## Core values and conversions

| Builtin | Does |
|---|---|
| `Some(v)` | Wraps a value in an `Option`. |
| `None` | The empty `Option`. A value, not a call. |
| `Ok(v)` / `Err(e)` | Wrap a value in a `Result`. |
| `int(v)` | Coerce to an integer: a hex literal, a float, a numeric string, a bool. |
| `float(v)` | Coerce to a float. |
| `string(v)` | Render as a string. |
| `bytes(v)` | Coerce to bytes. |
| `to_string(v)` | Render as a string. |
| `ord(c)` | The code point of a character. |
| `chr(n)` | The character for a code point, or nil. |
| `to_hex(n)` | `"0x7B"` — an uppercase hex string. |
| `from_hex(s)` | Parse a hex string into a `Hex` value. |
| `len(x)` | Length of a string (in characters), array, tuple, bytes, map or struct. |
| `contains(coll, item)` | Membership. The same as `item in coll`. |
| `slice(x, start?, end?)` | A sub-slice. Negative bounds count from the end; the range is clamped. |
| `fmt(spec, args...)` | Format with a spec string. |

`fmt` takes real format specs:

```rak
for b in row { dump fmt("{:02X}", b) }   // 0F, not 0
dump fmt("{:#x}", 255)                  // 0xff
dump fmt("[{:>4}]", n)                   // right-aligned, width 4
dump fmt("{:.2f}", ratio)                // two decimals
```

The grammar is `[[fill]align][+][#][0][width][.precision][type]`, with types
`x X b o d f e E`. The width counts the sign, so `{:05}` of `-42` is `-0042`.
One spec means the same thing on both backends.

## Strings

| Builtin | Does |
|---|---|
| `split(s, delim)` | Split into an array. |
| `join(delim, arr)` | Join an array. The delimiter comes **first**. |
| `replace(s, from, to)` | Replace every occurrence. |
| `find(s, needle)` | The byte offset, or -1. |
| `starts_with(s, p)` / `ends_with(s, p)` | Prefix and suffix tests. |
| `substr(s, start, length)` | A substring by **length**, not by end index. |
| `repeat(s, n)` | Repeat n times. |
| `upper(s)` / `lower(s)` | Case. |
| `trim(s)` / `trim_start(s)` / `trim_end(s)` | Whitespace. |

Note that `substr` and `slice` differ: `substr` takes a length, `slice` takes
an end index, and `substr` returns `""` when the range is out of bounds while
`slice` clamps.

## Arrays

| Builtin | Does |
|---|---|
| `push(arr, item)` | A **new** array with the item appended. |
| `sort(arr)` | A new array, sorted. Numbers numerically, anything else by rendered string. |
| `reverse(arr)` | A new array, reversed. Also works on a string. |
| `zip(a, b)` | Pairs, truncated to the shorter side. |
| `enumerate(arr)` | Index-and-value pairs. |
| `skip(arr, n)` | Drop the first n. |
| `fold(arr, init, f)` | `f(acc, item)`, left to right. |
| `reduce(arr, f)` | `f(acc, item)`, with the first element as the seed. `None` for an empty array. |
| `any(arr, f)` / `all(arr, f)` | `f(item)`. |
| `flat_map(arr, f)` | Map, then flatten one level. |
| `take_while(arr, f)` | The prefix where `f` holds. |

The higher-order builtins take a Rak function, so `fold(xs, 0, fn(a, b) { return a + b })`
sums an array.

## Maps

| Builtin | Does |
|---|---|
| `keys(m)` | The keys, as strings. |
| `values(m)` | The values. |
| `has(m, key)` | Whether the key is present. |
| `get(m, key, default?)` | The value, or the default (nil if none). |

## Sets

| Builtin | Does |
|---|---|
| `set_of(arr?)` | Build a set from an array. |
| `set_add(s, item)` | Add; true if it was new. |
| `set_discard(s, item)` | Remove; true if something was removed. |
| `set_has(s, item)` | Membership. |
| `set_len(s)` | Size. |
| `set_has_all(s, items)` | Superset test. |
| `set_union(a, b)` / `set_intersect(a, b)` / `set_diff(a, b)` | Set algebra. |
| `set_to_array(s)` | An array, in insertion order. |

## Numbers and math

| Builtin | Does |
|---|---|
| `abs(n)` | Absolute value. |
| `sqrt(n)` / `pow(base, exp)` | Roots and powers. `pow` returns a float. |
| `min(a, b, …)` / `max(a, b, …)` | Variadic over the **arguments**, not an array. |
| `sum(arr)` | Sum; promotes to a float if any element is not an integer. |
| `clamp(v, lo, hi)` | Constrain to a range. |

## Encoding and escaping

| Builtin | Does |
|---|---|
| `hex_encode(b)` / `hex_decode(s)` | Hex, lowercase. |
| `base64_encode(b)` / `base64_decode(s)` | Base64 with padding. |
| `url_encode(s)` / `url_decode(s)` | Percent-encoding. |
| `rot13(s)` | ROT13. An encoding, not encryption. |
| `xor(a, b)` | Repeating-key XOR. Not a cipher. |
| `sql_escape(s)` / `sql_escape_mysql(s)` | For SQL. |
| `shell_escape(s)` | For a shell command line. |
| `html_escape(s)` | For HTML. |
| `regex_escape(s)` | For a regex pattern. |

## Hashing and cryptography

| Builtin | Does |
|---|---|
| `sha256(b)` | SHA-256, hex. |
| `hmac_sha256(key, data)` | HMAC-SHA256, hex. |
| `md5(b)` / `sha1(b)` | Weak; they print a deprecation warning. |
| `aes_gcm_encrypt(key, nonce, plaintext)` / `aes_gcm_decrypt(key, nonce, ct)` | AES-256-GCM. 32-byte key, 12-byte nonce. |
| `chacha20_encrypt(key, nonce, aad, plaintext)` / `chacha20_decrypt(...)` | ChaCha20-Poly1305. |
| `ed25519_keypair(seed)` / `ed25519_sign(sk, msg)` / `ed25519_verify(pk, sig, msg)` | Ed25519. |
| `ecdsa_keypair()` / `ecdsa_sign(sk, msg)` / `ecdsa_verify(pk, sig, msg)` | NIST P-256. |
| `rsa_keypair(bits)` / `rsa_sign(sk, msg)` / `rsa_verify(pk, sig, msg)` | RSA PKCS#1 v1.5. |
| `rsa_encrypt(pk, plaintext, label?)` / `rsa_decrypt(sk, ct, label?)` | RSA OAEP. |
| `x25519_keypair(seed)` / `x25519_shared(sk, peer_pk)` | Key agreement. |
| `tunnel_preshared_key(password, salt, iters?, len?)` | PBKDF2. |
| `kdf_next(prev_key, counter?, len?)` | HKDF ratchet step. |
| `ct_eq(a, b)` / `ct_eq_hex(a, b)` / `ct_select(choice, a, b)` | Constant-time comparison and selection. |
| `zeroize(b)` | Wipe a buffer and return it. |

See [Safety](safety.html) for what each primitive is good for.

## Files

| Builtin | Does |
|---|---|
| `read(path)` / `file_read(path)` | Read a file as UTF-8 text. |
| `write(path, content)` / `file_write(path, content)` | Write UTF-8 text. |
| `file_append(path, content)` | Append text. |
| `file_read_bytes(path)` | Read raw bytes. |
| `file_write_bytes(path, bytes)` | Write raw bytes. |
| `file_append_bytes(path, bytes)` | Append raw bytes. |
| `file_exists(path)` / `file_size(path)` | Existence and size. |
| `file_list(dir)` | Directory entries. |
| `file_delete(path)` / `file_mkdir(path)` | Remove a file, create a directory. |
| `file_copy(src, dst)` / `file_rename(src, dst)` | Copy and rename. |
| `file_ext(path)` / `file_basename(path)` / `file_dirname(path)` | Path parts. |

`file_basename` is the **stem** — the name without the extension.

`read` and `file_read` are UTF-8, so they fail on a file containing a byte
sequence that is not valid UTF-8, and `write` turns a `0xFF` into U+FFFD on
the way out. The `_bytes` variants are byte-exact. See
[FFI and memory mapping](ffi.html) for `mmap_*`, which maps a file with no copy
at all.

## Memory-mapped files

| Builtin | Does |
|---|---|
| `mmap_open(path, mode)` | Map a file. Mode `"r"` or `"rw"`. |
| `mmap_slice(m, off, len)` | A zero-copy view. |
| `mmap_size(m)` | The mapped length. |
| `mmap_write(m, off, byte)` | Write one byte through the mapping. |
| `mmap_find(m, needle)` | Byte offset of a needle, or -1. |
| `mmap_lines(m, delim?)` | Lines, as strings. |
| `mmap_lines_off(m, delim?)` | Line offsets and lengths, without copying. |
| `mmap_close(m)` | A no-op; the mapping drops with the value. |

```rak
let m = mmap_open("trace.pcap", "r")
let hdr = mmap_slice(m, 0, 8)
dump hdr[0]                       // first byte, zero-copy
dump mmap_find(m, "\xff\xd8\xff") // byte search
for (off, len) in mmap_lines_off(m, "\n") {
    dump string(mmap_slice(m, off, len))
}
```

`mmap_open(..., "rw")` needs the `fs_write` capability, and `mmap_write`
refuses a read-only mapping and an out-of-range offset by name.

## Processes

| Builtin | Does |
|---|---|
| `process_spawn(cmd, args?)` | Start a child; returns a handle. |
| `process_wait(pid)` | Wait; returns the exit code. |
| `process_stdout(pid)` / `process_stderr(pid)` | Captured output. |
| `process_kill(pid)` | Kill it. |

## Network — TCP

| Builtin | Does |
|---|---|
| `net_listen(addr)` | Bind; returns a listener. |
| `net_accept(listener)` | Accept; returns `[stream, peer_address]`. |
| `net_connect(addr)` | Connect; returns a stream. |
| `net_local_addr(listener)` | The bound address. |
| `tcp_write(stream, data)` | Write; returns the byte count. |
| `tcp_read(stream, n?)` | Read up to n bytes. |
| `tcp_read_line(stream)` | Read one line, without the newline. |
| `tcp_close(stream)` | Close. |

## Network — DNS

| Builtin | Does |
|---|---|
| `dns_query(name, rtype?, server?)` | A live query. Returns `Ok({answers, truncated})`. |
| `dns_build(name, rtype?)` | The raw query packet, built offline. |
| `dns_parse(msg)` | Parse a response packet. |
| `dns_resolve(name, server?)` | A and AAAA addresses. |
| `dns_reverse(ip, server?)` | PTR records. |
| `dns_records(name, server?)` | Every record type. |
| `dns_walk(domain, prefixes?, server?)` | Subdomain walk. |
| `dns_lookup(host)` | Addresses for a host. |
| `reverse_dns(ip)` | The PTR name for one address. |

`dns_query` returns a `Result`; it does not raise.

## Network — HTTP, WebSocket, raw sockets

| Builtin | Does |
|---|---|
| `http_server_start(addr, port?)` | Bind; returns the bound port. |
| `http_server_poll()` | The next request, or nil. Never blocks. |
| `http_server_respond(id, status?, headers?, body)` | Answer it. |
| `http_server_stop()` | Stop the server. |
| `http_get_async(url)` | An async GET. Resolves to the body, or to the string `"error: …"`. |
| `ws_connect(url)` | A WebSocket client handshake. |
| `ws_handshake(stream)` | The server-side handshake reply. |
| `ws_send(stream, data, mask?)` | Send a text frame. |
| `ws_recv(stream)` | `{opcode, payload}` or nil. |
| `ws_close(stream)` | Send a close frame. |
| `net_raw_csum(b)` | A one's-complement checksum. |
| `net_raw_ipv4(src, dst, proto?, payload)` | An IPv4 header and payload. |
| `net_raw_tcp(src, dst, sport, dport, flags?, seq, ack, payload)` | A TCP header and payload. |
| `net_raw_udp(src, dst, sport, dport, payload)` | A UDP header and payload. |
| `net_raw_tcp_syn(src, dst, sport?, dport?)` | A whole SYN packet. |
| `net_raw_icmp(src, dst, id, seq, payload?)` | An ICMP echo request. |
| `net_raw_icmp_ping(src, dst, id, seq)` | The same, without a payload. |
| `net_raw_icmp_echo_reply(src, dst, id, seq, payload?)` | An echo reply. |
| `net_raw_arp_request(src_mac, src_ip, target_ip)` | An ARP request frame. |
| `net_raw_arp_reply(src_mac, src_ip, target_mac, target_ip)` | An ARP reply frame. |
| `net_raw_arp_parse(frame)` | The parsed frame, or nil. |
| `net_raw_send(packet)` | Send. Returns `Ok(bytes_sent)`. |
| `net_raw_recv(max?)` | Receive. Returns `Ok(bytes)`. |

The `fetch` **statement** is the pull-style HTTP client; see
[Language reference](language.html#statements-that-are-not-expressions).

## Reconnaissance

| Builtin | Does |
|---|---|
| `scan_ports(target, opts?)` | Open ports. `opts` is `{range: [a, b], timeout: ms}`. |
| `scan_subdomains(domain)` | Subdomains that resolve. |
| `subdomain_enum(domain)` | Candidate subdomains, generated from a word list. |
| `whois_lookup(domain)` | `Ok(text)`. |
| `whois_parse(text)` | Parse a WHOIS response. Pure text, no network. |
| `ct_subdomains(domain)` | `Ok(names)` from certificate transparency logs. |

## Protocol parsers

| Builtin | Does |
|---|---|
| `tls_parse_client_hello(bytes)` | `{sni, ciphers}` from a ClientHello. |
| `tls_parse_cert_chain(der)` | `[{subject, issuer}]`. |
| `pcap_open(path)` | Open a capture. `Ok(handle)`. |
| `pcap_next(handle)` | The next packet, or nil at the end. |

`pcap_open` needs the `pcap` feature of `rak-stdlib`, which `rakc` does not
enable, so it returns `Err("pcap: not built …")` in a normal build.

## Concurrency

| Builtin | Does |
|---|---|
| `spawn(fn)` | Run a function on a thread; returns a handle. |
| `thread_join(handle)` | Wait for it; returns its value. |
| `channel()` | A `[sender, receiver]` pair. |
| `chan_send(tx, value)` | Send. |
| `chan_recv(rx)` | Receive. **Blocking**; `None` when the channel closes. |
| `await_all(futures)` | Every future, in input order. |
| `select(futures)` | `[index, value]` — the first to resolve. |
| `timeout(future, ms?)` | `Ok(value)`, or `Err` on expiry. |
| `task_group(fns, limit?)` | Run functions concurrently, results in input order. |
| `async_sleep(ms)` / `async_yield()` | A future that waits, or yields. |
| `tcp_probe(host, port?, timeout?)` | A future that resolves to a bool. |
| `tcp_connect_async(addr)` | A future that resolves to a stream. |

The async family needs the `async` feature. See
[Concurrency and async](concurrency.html).

## Data and documents

| Builtin | Does |
|---|---|
| `json_parse(text)` | Native arrays, maps, ints, floats, bools, nil. |
| `json_stringify(value)` | Back to a string. |
| `json_get(text, key)` / `json_path(text, path)` | Read a value. |
| `json_keys(text)` / `json_len(text)` | Keys and length. |
| `json_find_all(text, needle)` | Every match, recursively. |
| `csv_parse(text, opts?)` | Rows as maps, or as arrays with `{header: false}`. |
| `csv_stringify(rows)` | Back to CSV. |
| `yaml_parse(text)` | A YAML subset: block maps and lists, scalars, comments. |
| `gzip(b, level?)` / `gunzip(b)` | gzip. |
| `deflate(b, level?)` / `inflate(b)` | Raw deflate. |
| `zip_archive(files)` | Build a zip in memory. |
| `zip_extract(zip, name)` | Read one entry. |
| `zip_list(path)` / `zip_read(path, name)` / `zip_write(path, entries)` | Zip files on disk. `Ok(...)` values. |

`json_stringify` pretty-prints. The `zip_*` file functions return `Result`
values.

## Streams

| Builtin | Does |
|---|---|
| `stream_from_array(arr)` | A stream over an array. |
| `stream_map(s, f)` / `filter(s, f)` / `take(s, n)` | Lazily transform. |
| `stream_next(s)` | `Some(value)` or `None`. |
| `collect(s)` | Drain into an array. |
| `read_lines(path)` | A stream of lines from a file. |
| `tcp_stream(addr)` | A stream of lines from a socket. |
| `stream_csv(path, opts?)` | A lazy CSV stream. |
| `stream_jsonl(path)` | A lazy JSON-lines stream. |

`filter` and `take` take a **stream**, not an array. See
[Streams and data](streams.html).

## HTML scraping

| Builtin | Does |
|---|---|
| `html_title(html)` | The document title. |
| `html_select(html, css)` / `html_select_all(html, css)` | Text of the matches. |
| `html_attr(html, css, attr)` | An attribute. |
| `html_links(html)` / `html_images(html)` / `html_scripts(html)` | Every `href`, `src`, script. |
| `html_forms(html)` / `html_inputs(html)` | Forms and inputs. |
| `html_meta(html, name)` | A meta tag. |
| `html_count(html, css)` | How many matches. |
| `html_headers(html)` | The heading texts. |

The `html` feature. A build without it registers these builtins but they
return nothing, which is a silent wrong answer — see
[Standard library batteries](batteries.html).

## Evidence and provenance

| Builtin | Does |
|---|---|
| `cite(value, tool?, target?)` | Wrap a value in a provenance tag, chaining to any existing one. |
| `strip_evidence(value)` | Unwrap it. |
| `provenance(value)` | The provenance chain, as a map. |
| `report(v1, v2, …)` | A Markdown findings report with cited sources. |
| `report_markdown(report_map)` | Render a report map. |
| `yara_scan(rules, data)` | Match a YARA-lite rule set. |

See [Forensics](forensics.html).

## Time and randomness

| Builtin | Does |
|---|---|
| `time_now()` / `time_now_millis()` | Unix time. |
| `time_fmt(ts, spec)` | Format a timestamp. |
| `time_parse(s)` | `Ok(ts)`. Accepts `YYYY-MM-DD` and `YYYY-MM-DD HH:MM:SS`. |
| `time_parts(ts)` | The calendar fields, as a map. |
| `time_add(ts, secs)` / `time_diff(a, b)` | Arithmetic on time. |
| `date_today()` | `YYYY-MM-DD`. |
| `rand_seed(n)` | Reseed the generator. |
| `rand_int(lo, hi)` / `rand_int(hi)` | A random integer in `[lo, hi)`. |
| `rand_float()` | A random float in `[0.0, 1.0)`. |
| `rand_bytes(n)` / `rand_hex(n)` | Random bytes, random hex. |
| `rand_choice(arr)` | `Ok(item)`. |
| `rand_shuffle(arr)` | Shuffle in place. |

The `rand` feature. See [Standard library batteries](batteries.html).

## Logging

| Builtin | Does |
|---|---|
| `log_level(level)` | Set the level: `debug`, `info`, `warn`, `error`. |
| `log_init(path?)` | Log to a file, append-only. |
| `log_info(key, fields?)` / `log_warn` / `log_error` / `log_debug` | One JSON line each. |

## Secrets

| Builtin | Does |
|---|---|
| `secret_get(name)` | Session, then environment, then the on-disk store. |
| `secret_set(name, value)` | The session store. |
| `secret_persist(name, value)` | The on-disk store. |
| `secret_delete(name)` | Remove it, and shred the file. |
| `secret_ls()` | The names. |
| `secret_delete_all()` | Remove everything, and shred the file. |

Values are never logged.

## Diagnostics

| Builtin | Does |
|---|---|
| `assert_eq(a, b)` / `assert_ne(a, b)` | Assert, with a message on failure. |
| `assert_true(a)` / `assert_false(a)` | Assert a boolean. |
| `expect_error(fn)` | Passes only if the function raises. |
| `panic(msg?)` | Raise. |
| `error(kind?, message?)` | Build a structured error. |
| `err_message(e)` / `err_kind(e)` | The message and the kind. |
| `err_line(e)` / `err_col(e)` / `err_file(e)` | Where it happened. |
| `err_cause(e)` / `err_context(e)` | Why it happened. |
| `err_with_context(e, key, value)` | Add a context pair. |

See [Error handling](errors.html).

## CLI helpers

| Builtin | Does |
|---|---|
| `print(v)` | To stdout. |
| `eprint(v)` | To stderr. |
| `dbg(v)` | To stderr, prefixed `[dbg] `. |
| `argv()` | The script's arguments. |
| `args()` | The process's arguments. |
| `stdin_read_line()` / `stdin_read_all()` | Standard input. |
| `parse_args(spec, argv?)` | A flag parser. |
| `env_get(name)` / `env_set(name, value)` | Environment variables. Gated by `secrets`. |
| `sleep(ms)` | Sleep. |
| `now_ms()` | Unix milliseconds. |

## GUI

`gui_window`, `gui_add`, `gui_set_text`, `gui_get_text`, `gui_set_visible`,
`gui_on_click`, `gui_close`, `gui_wait`, `gui_quit` and `gui_callback`. The
`gui` feature. See [Native GUI](gui.html).

## FFI

`ffi_load`, `ffi_ptr`, `ffi_alloc`, `ffi_free`, `ffi_write`, `ffi_read`,
`ffi_read_i32`, `ffi_cstr_to_string`, `ffi_string_to_cstr`, `ffi_trust`,
`ffi_call` and `extern_call`. See [FFI and memory mapping](ffi.html).

`extern_call` is an honest stub on both backends: it errors, saying that the
generated Rust wrapper has to be linked in.

## Inline assembly

`asm(instruction, value?)` — interpreter-only, by design. It needs the `asm`
capability and an `unsafe` block, and it supports a fixed set of templates
(`cpuid_sse2`, `cpuid_pclmulqdq`, `cpuid_aes`, `cpuid_rdrand`, `rdtsc`,
`rdtscp_aux`, `tsc_invariant_hz`, `add`).

## Not on both backends

Three things are interpreter-only, and say so rather than guessing:

- `asm` — a bytecode VM has no instructions to escape into.
- `exit` — the CLI owns the process.
- The `scan` and `fetch` **statements** — the VM has no HTTP client yet.

`array(...)` and `map(...)` are interpreter-only special forms as well.
`string(bytes)` renders differently on the two backends: the interpreter
prints the debug form, the VM prints the text.
