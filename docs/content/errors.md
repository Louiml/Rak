# Error handling

## try / catch / raise

```rak
try {
    raise "boom"
} catch e {
    dump e
}
```

`try`/`catch`/`raise` (and the alias `throw`) handle runtime errors. Lexer and
parser errors report line and column; `rakc check file.rak` prints diagnostics
in the format IDEs expect.

## The `?` operator

```rak
let v = Ok(42)?     // unwrap Ok, propagate Err to the caller
```

`Result` and `Option` values flow through the `?` operator and pattern
matching:

```rak
let r = dns_query("example.com", "A")
match r {
    Ok(resp) => { dump resp.answers },
    Err(e) => { dump e },
}
```

## Structured errors (0.7)

`catch e` binds a first-class `Error` value. It still stringifies to the
message (so existing `dump e` / `fmt("{}", e)` keep working) but exposes
structured fields:

- `error(kind, message)` — construct an error (`runtime`, `io`, `network`,
  `parse`, `compile`, `type`, `package`, `permission`, `user`, `timeout`,
  `cancel`).
- `err_message(e)` — the message string.
- `err_kind(e)` — the kind string.
- `err_line(e)` / `err_col(e)` / `err_file(e)` — source span.
- `err_cause(e)` — the wrapped cause, if any.
- `err_context(e)` — the context map.
- `err_with_context(e, map)` — a copy with extra context added.

```rak
try {
    raise "disk exploded"
} catch e {
    dump err_kind(e)      // e.g. "user"
    dump err_message(e)
    dump err_line(e)
    let e2 = err_with_context(e, { file: "data.csv" })
    dump err_context(e2)
}
```

Every error carries `message`, `kind`, `file:line:col` span, `cause`, a
context map, and a backtrace. Registered on both the interpreter and the
bytecode VM.

## Resource cleanup

Cleanup uses RAII: `Arc<MmapHandle>`, `Arc<Mutex<TcpStream>>`, and
`ForeignLib` drop handlers call `munmap`/`dlclose`/`shutdown` automatically.
The explicit `mmap_close`/`tcp_close`/`lib.close` calls are conveniences for
early release, not safety requirements.
