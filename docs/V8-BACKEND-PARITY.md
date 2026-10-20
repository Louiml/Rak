# Backend parity

Rak has two backends: a tree-walking interpreter (`rakc run`) and a bytecode VM
(`rakc vm`). They are two implementations of one language, so every builtin and
every language feature has to exist twice.

This document records what that means in practice, what is still missing, and
why the remaining gap is not a list of forgotten builtins.

## The measurement

`rakc::run_on_both` runs a program on both backends and classifies how they
agree. `tests/backend_parity.rs` uses it two ways:

- **`registrations_match`** is a structural gate. It reads the registration
  sites in both backends and requires the name sets to be identical - a plain
  assertion since the gap closed. A builtin added to one side and forgotten on
  the other fails here.
- **The `parity_*` tests** are behavioural. They run a program on both and
  require identical output, which catches not just missing builtins but any
  case where the two implementations disagree.
- **`parity_backlog_report`** is `#[ignore]`d and prints the registration gap,
  which is now empty. It is kept so a future regression shows up as a list of
  names rather than a bare assertion failure. Run it with
  `--ignored --nocapture`.

The structural scan is scoped to the interpreter's `eval_builtin` body, and
handles four registration shapes, because each was a false positive that would
have made the gate untrustworthy:

| Shape | Example | Why it was missed |
| --- | --- | --- |
| Alternation arm | `"regex_match" \| "regex_is_match" =>` | registers two names, scan saw one |
| `for name in [..]` loop | the GUI natives | one dispatcher, N names |
| `vm_natives()` table | `("time_now", vm_time_now),` | names live in the callee |
| Multi-line `insert_native` | `insert_native(\n    "http_server_poll", ..)` | the needle included the opening quote, so a name on the next line was invisible - the builtin ran on both backends while the gate counted it interpreter-only |

The whole-file interpreter scan also picked up `regex` *method* arms, which are
not builtins. Scoping to `eval_builtin` fixed that.

## Why this harness exists

v8.0.0 shipped nineteen builtins that existed only in the VM — `ct_eq`,
`rsa_keypair`, `ecdsa_sign`, `net_raw_arp_parse` and friends. `rakc run`, the
default backend, failed with `Unknown function` for every one of them. The test
suite passed, because it exercised each backend in isolation and nothing covered
the new builtins on the interpreter side at all. A per-backend test suite
cannot catch a per-backend omission; only a comparison can.

## What is closed

| Spec item | Status |
| --- | --- |
| 7A.11 sets | Both backends. `Value::Set`, ten builtins, `for x in set`, `x in set`. |
| 7A.5 tunnel / UDP | Both backends. `Value::UdpTransport`, four `udp_*` natives, `tunnel` lowering. |
| 7A.4 streams | Both backends. `Value::Stream`, ten natives, lazy `for`-over-stream lowering. |
| Call semantics | Both backends. Defaults (compiled into the callee prologue, keyed off a hidden `__argc`), rest parameters, optional parameters, arity errors with identical wording, and `fn main()` taking argv only when declared. Named arguments remain a VM compile error, not a silent difference. |
| 7A.6 `import pkg.sub` | Both backends. `Op::MergeModule`, `Op::LoadGlobalOrMap`. |
| Module namespaces | Both backends. `modns::ModuleNamespace`, `Op::MakeModule`/`ModulePublish`, `Op::StoreGlobal` republishing. `import m; m.X` is live and `pub` is enforced on both; `from m import x as y` copies on both. Three residual differences, all from the VM's flat global namespace, in [V8-KNOWN-ISSUES.md](V8-KNOWN-ISSUES.md). |
| GUI | Both backends. Nine natives (`gui_window`, `gui_add`, `gui_set_text`, `gui_get_text`, `gui_set_visible`, `gui_on_click`, `gui_close`, `gui_wait`, `gui_quit`), native egui windows, no webview. `gui_callback` is on both: it needs a Rak callback from a native, so the VM intercepts it in `Vm::call_value`, where the machine can hand the closure and a snapshot of the chunk globals to the GUI's registry. |
| ~100 stdlib builtins | Both backends, via `ext_stdlib.rs`. |
| `impl Display` drives `dump` | Both backends. `Op::Print` looks up `__method_<Type>_fmt` exactly as `Interpreter::display_value` does, so a struct with a `Display` impl prints the same on each. It worked for locally-declared structs only: the VM's module inliner skipped `struct`/`enum`/`impl`/`trait`/`type` when inlining an imported module, and the imported module's baked methods were never emitted into the running chunk. |
| Importing a module with a `struct` and an `impl` | Both backends. The inliner's skip list covered functions, macros, externs and binstructs but not type declarations, so `pub struct Finding` was `VM does not support statement: Impl { ... }`. |

## What is not, and why

**Nothing. The registration gap reached zero**, and the ratchet this document
used to track it was replaced with the plain assertion its own comment
prescribed: `registrations_match` now fails on *any* name the two backends do
not share, and the `MAX_KNOWN_INTERP_ONLY` constant is gone. The backlog
report (`parity_backlog_report`, still `#[ignore]`d) prints an empty list.

Getting there disproved this document's own central claim. The gap was recorded
as "31 builtins blocked on coroutines in the VM", and coroutines were described
as "a project, not a list of one-line registrations". That was wrong in a useful
way: the interpreter has no coroutines either. Its `spawn` is an OS thread, its
`channel` is a `std::sync::mpsc` pair, and its `net_*` family blocks the same
thread a VM native would. Almost the whole gap was *blocking* work, and a
blocking native needs no `&mut Vm` at all:

| Family | Names | How it closed |
| --- | --- | --- |
| Channels | `channel`, `chan_send`, `chan_recv` | Plain natives over `Value::Channel`, which now holds the real `mpsc` endpoints. A blocking `chan_recv` is correct for the same reason it is on the interpreter: the sender runs on another thread. |
| Future orchestration | `await_all`, `select`, `timeout` | Plain natives that resolve VM futures - the same resolution `Op::Await` performs, factored out. `select` races one thread per future into an `mpsc` and takes the first; `timeout` polls a resolving thread against a deadline. |
| Concurrency | `task_group`, `expect_error` | Intercepted in `Vm::call_value`, because both must *call a Rak function*, which a `fn(&[Value])` native cannot. `task_group` runs each member on a fresh `Vm` over an OS thread (the `spawn` pattern); `expect_error` calls, catches, and clears `pending_error` so the caller's `try` does not see it. |
| TCP / WebSocket | `net_listen`, `net_accept`, `net_connect`, `net_local_addr`, `tcp_*`, `ws_*` | Plain natives over two new `Value` variants, `TcpListener` and `TcpStream`. `tcp_connect_async` is the one that wanted suspension, and it gets the same treatment `http_get_async` already had: `spawn_blocking` producing a `Future`. |
| DNS / recon | `dns_lookup`, `reverse_dns`, `scan_ports`, `scan_subdomains`, `subdomain_enum` | `ext_stdlib.rs` wrappers over the same `rak_stdlib::recon` calls the interpreter makes. |
| Misc | `argv`, `extern_call` | `argv` reads the `script_argv` thread-local `run_cli` already sets. `extern_call` mirrors the interpreter's stub message exactly - on *both* backends it errors until a Rust wrapper is linked, so the two agree by agreeing on the refusal. |
| GUI | `gui_callback` | Intercepted in `Vm::call_value`, where the machine can hand the closure and a snapshot of the chunk globals to the GUI's registry; dispatch runs it on a fresh `Vm` on the event-loop thread. |

One of the 31 was not a gap at all: the parity scanner missed
`http_server_poll` because its registration is split across two lines. The
scan now skips whitespace after `insert_native(` before expecting the name -
and requires the quote as the first non-whitespace character, so it cannot
scrape strings out of the `insert_native` *definition* either.

### What the structural gate cannot see

`registrations_match` compares *name sets*. It says nothing about whether the
two implementations of a name agree, which is where the last divergences
lived. Each was found by running the same program on both, not by reading the
table:

- **`string(bytes([65, 66]))`** returned `"AB"` on the interpreter and
  `b"\x41\x42"` on the VM - the VM's `string` native fell through to `Display`,
  which for `Value::Bytes` is a debug form, so a program that built a printable
  glyph got a five-character string where it expected two. Not a missing name:
  a different answer, with no diagnostic.
- **`impl Display`** drove `dump` on the interpreter and was ignored by the VM's
  `Op::Print`, so `dump Point { x: 3, y: 4 }` printed `Point (3, 4)` and
  `Point {x: 3, y: 4}` depending on how the program was launched.
- **Importing a `struct` with an `impl`** was a hard compile error on the VM.
- **`let (a, b) = ...` at the top level** bound locals on the VM and globals on
  the interpreter, so a destructured binding was invisible to every closure and
  spawned task - which turned a `chan_recv` into a hang, not an error.
- **`await_all([42])`** rejected the non-future on the interpreter and silently
  passed it through on the VM.

All of these are pinned by `parity_*` tests, because a structural gate would
have passed every one of them.

### What is still open

The registration sets now match; the *behavioural* gaps that remain are
recorded where they belong rather than here as a count:

- `extern_call` is an honest stub on both backends, as above.

- **`fetch` is a statement, and the VM does not have it.** The registration
  gate compares *builtin* names, and `fetch` is not a builtin: it is a
  statement with a body block, so the gate cannot see it. The interpreter
  runs it, and the VM refuses to compile the program at all -- "VM does not
  support statement: Fetch". That is loud rather than silent, and a program
  that uses `fetch` therefore runs under `rakc run` and not under `rakc vm`.

  The C target refuses it too, and says why: "the C target has no HTTP
  client". The VM's refusal has no such reason attached, and it should: the
  honest version of this gap is "the VM has no HTTP client yet", not "the VM
  does not support statements".

  This is a *statement* gap and not a builtin gap, which is why the count in
  this document reached zero without closing it. The measurement that would
  catch it is a comparison of the statement forms the two backends accept,
  and there is no such gate yet.

### A build with less language in it

The browser build of the playground -- the interpreter compiled to
WebAssembly -- turns `http`, `async` and `html` off, because what they need
does not compile to a wasm module and a page has neither a TLS stack nor an
event loop. `rand` used to be off for the same reason, and is not any more:
the module asks the page for entropy through the one import it already had,
so the browser's own cryptographic generator is the source.

A build with less language in it has to *say* so, or a program that works
under `rakc run` fails in the browser for a reason the page cannot explain.
The rule the browser build follows is that a missing capability is an error
and never a silent answer:

| Missing | What the program sees |
| --- | --- |
| HTTP client | `fetch: this build has no HTTP client (the `http` feature is off)` -- raised, not printed and passed over |
| Random number generator | `rand: this build has no random number generator (the `rand` feature is off)` -- raised |
| Async runtime | the builtins are not registered, so the call is an "Unknown function" |

The first of those was a bug until recently: `fetch` reported a failed
request and the program carried on as if the request had come back empty.
The browser build is exactly the build where that matters, because it is the
build where every request fails.

The configuration is tested. `cargo test -p rakc --no-default-features` runs
the suite against the build the browser uses, and CI runs it on every push
along with a build of the wasm module and a check that the committed
`docs/rak.wasm` -- the file the site actually serves -- runs a program.

`docs/rak.wasm` is a build artifact in the tree, so it can fall behind the
interpreter it was built from. Rebuild it with:

```
cargo build -p rak-playground --profile playground --target wasm32-unknown-unknown
```

and copy the result to `docs/rak.wasm`. CI reports when the committed file
is older than the interpreter, rather than failing the run: the rebuild
takes a minute and a half and produces a three-megabyte diff, which is a tax
on every commit that touches the interpreter, including the ones that change
nothing the playground can run.

### Two gaps that closed

- **`ensures` postconditions.** The VM used to refuse the program outright --
  "VM does not support postconditions" -- which is honest but means a promise
  checked on one backend goes unchecked on the other, and a program using
  `ensures` ran under `rakc run` and not at all under `rakc vm`. The VM now
  compiles the clauses into the callee's own body: every `return` parks the
  value in a local and jumps to a trampoline that runs the deferred calls,
  checks the clauses and returns. A body that falls off the end returns nil,
  and the clauses are checked against that nil. The failure names the function
  and the clause, in the interpreter's words, so a diagnostic reads the same on
  either backend.

- **Closure captures.** A closure that mentioned an enclosing function's local
  used to resolve the name as a *global* on the VM: it read a global of that
  name if there was one, and failed with "Undefined variable" when there was
  not. The interpreter has always given a closure a copy of the environment it
  was written in, and the VM does the same now -- the values are copied out of
  the enclosing frame when the closure is made, so an assignment inside a
  closure lands in the closure's own copy and the enclosing function never
  sees it. A *global* is not copied: a closure reads and writes the program's
  globals on both backends.

  The interpreter had a bug here that the VM would have inherited: the
  scope-local immutability sets were shared while the scopes were not, and the
  two are consulted by index, so a closure that pushed a scope of its own read
  the wrong set -- an immutable binding captured by a closure was silently
  treated as mutable. `Env::clone` now copies the two together.

### Two entries that are allowed to differ

`INTERP_ONLY` in `tests/backend_parity.rs` lists two names with reasons:

- **`asm`** — inline assembly has no VM counterpart *by design*. A bytecode VM
  has no instructions to escape into, so a VM version could only be a lookup
  table pretending to be one. Registering the name on both sides would have
  made the gate quiet while the feature stopped touching hardware under
  `rakc vm`.
- **`exit`** — a native could call `std::process::exit`, but that would skip
  the VM's frame teardown, its deferred calls, and the interpreter's buffered
  output. There is no correct place to put it.

`VM_ONLY` lists six names the *scan* cannot see rather than real gaps: `Ok`,
`Some`, `Err` and `fmt` are special-cased in the interpreter's `eval_call` rather
than `eval_builtin`, `__evidence_from` is VM-internal, and `whois_parse` is
dispatched from `try_interp` — it exists on both backends (the runtime report
counts it on neither side of the gap; `vm-only: 0`), the structural scan just
cannot reach it from inside `eval_builtin`.

## The `fn(&[Value])` boundary in practice

Three features shipped this release by working *around* the boundary rather than
removing it, and each says so at the call site:

- **`set_add` / `set_discard`** are intercepted in `Vm::call_value`. A set is a
  shared value behind an `Arc`, and a native cannot mutate one. They are
  registered as placeholders so the gate and the LSP can see the names.
- **`stream_next` / `collect`** are intercepted for the same reason: pulling
  from a stream produced by `stream_map` or `filter` means calling a Rak
  function.
- **`gui_*`** resolve the manager through a process-wide `OnceLock`, because a
  native cannot reach the interpreter that owns it.

The pattern generalises: any builtin that needs the machine belongs in
`Vm::call_value`'s interception, not in the native table.

## Two places where the backends are separate code

Sets and streams have shared storage (`SetRepr`, `ext_streams`), but:

- **Sets** share `setrepr::SetRepr` and the identity rule, so iteration order and
  membership semantics are identical by construction. The value type and the
  mutation path are per-backend.
- **Streams** are two separate implementations. The interpreter's `RakStream`
  trait takes `&mut Interpreter`, which is exactly why it could not be reused.
  `ext_streams.rs` takes an explicit callback instead. What is shared is the
  design — pull-based, lazy, backpressure-preserving — and the builtin names,
  not the types.

Both are covered by parity tests, which is the point: separate code can diverge,
so it is compared.
