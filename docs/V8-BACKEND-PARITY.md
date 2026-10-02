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
  sites in both backends and requires the name sets to be identical. A builtin
  added to one side and forgotten on the other fails here.
- **The `parity_*` tests** are behavioural. They run a program on both and
  require identical output, which catches not just missing builtins but any
  case where the two implementations disagree.
- **`parity_backlog_report`** is `#[ignore]`d and prints the current gap, so
  the backlog is measurable rather than remembered. Run it with
  `--ignored --nocapture`.

The structural scan is scoped to the interpreter's `eval_builtin` body, and
handles three registration shapes, because each was a false positive that would
have made the gate untrustworthy:

| Shape | Example | Why it was missed |
| --- | --- | --- |
| Alternation arm | `"regex_match" \| "regex_is_match" =>` | registers two names, scan saw one |
| `for name in [..]` loop | the GUI natives | one dispatcher, N names |
| `vm_natives()` table | `("time_now", vm_time_now),` | names live in the callee |

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
| 7A.6 `import pkg.sub` | Both backends. `Op::MergeModule`, `Op::LoadGlobalOrMap`. |
| Module namespaces | Both backends. `modns::ModuleNamespace`, `Op::MakeModule`/`ModulePublish`, `Op::StoreGlobal` republishing. `import m; m.X` is live and `pub` is enforced on both; `from m import x as y` copies on both. Three residual differences, all from the VM's flat global namespace, in [V8-KNOWN-ISSUES.md](V8-KNOWN-ISSUES.md). |
| GUI | Both backends. Six natives, real window handles, JS→Rak IPC. |
| ~100 stdlib builtins | Both backends, via `ext_stdlib.rs`. |

## What is not, and why

**34 builtins exist only on the interpreter**, and every one of them is blocked
on the same thing.

A VM native has the signature:

```rust
fn(&[Value]) -> Result<Value, String> + Send + Sync
```

No `&mut Vm`. No frame. So a native cannot invoke a Rak function, cannot
suspend, and cannot resume. Anything that needs to do one of those is not
expressible as a native:

| Family | Count | What it needs |
| --- | --- | --- |
| `channel`, `chan_send`, `chan_recv`, `select`, `timeout`, `await_all`, `task_group`, `thread_join` | 9 | Suspend and resume |
| `net_listen`, `net_accept`, `net_connect`, `net_local_addr`, `tcp_*`, `ws_*` | 17 | Blocking I/O, and `tcp_connect_async` needs suspension |
| `spawn`, `dns_lookup`, `reverse_dns`, `scan_ports`, `scan_subdomains`, `subdomain_enum` | 6 | Threads or async I/O |
| `extern_call`, `ffi_read_i32` | 2 | FFI trampolines through VM frames |
| `bytes`, `argv`, `expect_error` | 3 | Reachable, but not yet swept |
| `gui_callback` | 1 | Needs a Rak callback from a native |

The interpreter does not have this problem because `eval_builtin` takes
`&mut self`. It can call `call_function_with_values`, and it can hand back out
a `Future` that the runtime resolves later.

**The fix is coroutines in the VM**, and that is a real project, not a list of
one-line registrations. It is not being done in this release. The gate fails on
these 34 by design, so the gap cannot be forgotten, and the backlog test prints
them so the number cannot drift upward unnoticed.

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

`VM_ONLY` lists four names the *scan* cannot see rather than real gaps: `Ok`,
`Some`, `Err` and `fmt` are special-cased in the interpreter's `eval_call` rather
than `eval_builtin`, and `__evidence_from` is VM-internal.

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

## A syntax limitation worth knowing

`compiler.rs::iterable_is_stream` decides whether a `for` loop is over a stream
by looking at the syntax — a call to `stream_from_array` and friends, or an
identifier whose name looks like one. This avoids emitting a runtime probe, but
it means a stream held in a variable under an unrecognised name falls back to
`Op::IterItems`, which materialises the whole stream. Correct, but no longer
lazy: a `for` over an unbounded stream would not terminate.

The right fix is a runtime type check, which means either a new opcode or
inspecting the value at the top of the loop body. Both are small; neither is
done.
