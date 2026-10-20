# Concurrency and async

Rak has threads, channels, and an async event loop. All three work on both
backends.

## Threads and channels

```rak
let h = spawn(fn() { return 42 })
dump thread_join(h)
```

`spawn` runs a function on an OS thread and returns a handle.
`thread_join` waits for it and returns its value.

```rak
let (tx, rx) = channel()
chan_send(tx, "hello")
dump chan_recv(rx)          // Some("hello")
```

`channel()` returns a `[sender, receiver]` pair. `chan_send` sends, and
`chan_recv` **blocks** until a value arrives — it returns `Some(value)`, or
`None` when the channel is closed and drained.

A common pattern is to send a result back from a worker:

```rak
let (tx, rx) = channel()
let h = spawn(fn() {
    chan_send(tx, compute())
    return 0
})
thread_join(h)
dump chan_recv(rx)
```

## Async functions and await

An `async fn` returns a future. `await` resolves it.

```rak
async fn probe(host, port) {
    let open = await tcp_probe(host, port, 200)
    return open
}

dump await probe("127.0.0.1", 80)
```

The async builtins are `async_sleep`, `async_yield`, `tcp_probe`,
`tcp_connect_async` and `http_get_async`. They need the `async` feature, and
a build without it does not register them at all — the call is an "Unknown
function" rather than a silent no-op.

`http_get_async` resolves to the body as a string. On failure it resolves to
the **string** `"error: …"` rather than to an error value, so check it before
you use it.

## Orchestration

| Builtin | Does |
|---|---|
| `await_all(futures)` | Resolve every future; the results are in input order. |
| `select(futures)` | `[index, value]` — the first to resolve. |
| `timeout(future, ms?)` | `Ok(value)`, or `Err` when the deadline passes. |
| `task_group(fns, limit?)` | Run functions concurrently; the results are in input order. |

```rak
let out = await_all([async_sleep(10), async_sleep(20)])
dump out

let (i, v) = select([async_sleep(10), async_sleep(400)])
dump i

let r = timeout(async_sleep(5), 5000)?
dump r
```

`timeout` gives a catchable error, not a crash:

```rak
try {
    let r = timeout(async_sleep(2000), 20)?
    dump "resolved"
} catch e {
    dump "timed-out"
}
```

`task_group` runs a list of functions with bounded parallelism, and fails
fast if one of them raises.

## The two backends differ here

On the interpreter, an `async fn` body is deferred to a worker, so it really
does run concurrently. On the bytecode VM, an `async fn` body runs
**synchronously** in place, and `await` resolves immediately. The result is
the same; the concurrency is not. This is the one place where the two
backends differ in behaviour rather than in error text, and it is a
difference of scheduling, not of semantics.

The `async` builtins themselves behave the same on both: they are backed by
the same Tokio runtime.

## Threads and futures are different things

`spawn` gives you a thread and a handle you can join. A future is a value
you await, and the work behind it is scheduled by the async runtime. They
compose — a `spawn`ed task can await futures — but they are not
interchangeable, and `thread_join` does not accept a future.
