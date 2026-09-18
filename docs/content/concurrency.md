# Concurrency & async

## Threads and channels

```rak
let h = spawn(fn() { return 42 })
dump thread_join(h)
let (tx, rx) = channel()
chan_send(tx, "hi")
dump chan_recv(rx)
```

`spawn` launches an OS thread, `thread_join` waits for it, and `channel()`
gives you a sender/receiver pair.

## Async functions and await

`async fn` returns a **deferred future** whose body runs on the first `await`.
`await` blocks until the future resolves.

```rak
async fn probe(host, port) {
    let open = await tcp_probe(host, port, 200)
    return open
}
dump await probe("127.0.0.1", 80)

let body = await http_get_async("https://example.com")
dump string(body)
```

`http_get_async` / `tcp_probe` / `tcp_connect_async` run on a lazily-started
multi-thread Tokio runtime and return a `Future`; a future can be stored and
awaited later:

```rak
let f = tcp_probe(host, 80, 200)
let open = await f
```

There is no special "async context" requirement — `await` outside `async fn`
just resolves the value. Works on both the interpreter (deferred `async fn`
bodies) and the bytecode VM (`Op::Await`; VM `async fn` bodies run
synchronously on call).

## Async concurrency (0.7)

Deferred `async fn` bodies and async I/O futures run concurrently on a bounded
worker pool (a non-Tokio counting semaphore over a shared Tokio runtime), so
thousands of lightweight operations run without one OS thread per op.

| Builtin | Signature | Notes |
|---------|-----------|-------|
| `await_all` | `await_all([futures]) -> [values]` | Join every future; returns all results. |
| `select` | `select([futures]) -> (index, value)` | Race; returns the first to resolve. |
| `timeout` | `timeout(future, ms) -> Result` | `Ok(value)` or `Err(...)` on expiry. |
| `task_group` | `task_group([fns], limit) -> [results]` | Bounded parallelism + error propagation. |
| `async_sleep` | `async_sleep(ms)` | Non-blocking timer future. |
| `async_yield` | `async_yield()` | Yield control to the pool. |

```rak
// Fan out a scan concurrently
let futures = hosts |> map(fn(h) { tcp_probe(h, 80, 500) })
let results = await_all(futures)
for (i, r) in results { dump f"{i}: {r}" }

// Race two mirrors; first response wins
let (idx, body) = select([http_get_async(url_a), http_get_async(url_b)])

// Bounded parallelism over a list of jobs
let out = task_group(jobs, 4)
```

A panic in a task surfaces as `Runtime("await: task failed: <join error>")`.
Dropping a pending future aborts nothing eagerly (the task completes in the
background); `await` is the only resolution point.

See `examples/async_orchestration.rak` for the end-to-end demo.

## GUI note

With `--features gui`, scripts can open native desktop windows rendering
HTML/CSS/JS (`gui_open`, `gui_update`, `gui_title`, `gui_close`, `gui_wait`,
`gui_callback`). JavaScript inside the window calls back into Rak via
`window.rak_call(fn, args)`.

```rak
let win = gui_open("Rak GUI Demo", "<h1>Hello from Rak!</h1>", 600, 400)
gui_wait()
```
