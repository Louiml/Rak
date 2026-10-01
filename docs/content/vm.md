# Bytecode VM

## Overview

`rakc vm` runs bytecode. The compiler (`compiler.rs`) lowers the shared AST to
bytecode (`bytecode.rs` chunks) executed by a stack-based VM (`vm.rs`).
Benchmarks show about 6x speedup over the tree-walking interpreter; run
`rakc bench file.rak` to compare both on the same script.

The VM never produces wrong results for unsupported features: the compiler's
`compile_stmt`/`compile_expr`/`compile_pattern` return
`Err("VM does not support ...")`, so interpreter-only programs fail fast with a
clear message.

## Coverage matrix

| Feature | Interpreter | VM |
|---------|:-----------:|:--:|
| Pipeline `\|>` | yes | yes (desugar) |
| Regex literals | yes (methods + builtins) | yes (builtins) |
| Binary pattern matching | yes | yes (`Op::MatchPat` descriptors) |
| Trait protocols | yes | no (graceful error) |
| Method-call dispatch | yes | yes (`Op::CallMethod`) |
| User enum patterns (`Event::Variant(..)`) | yes | yes (`Op::MatchPat`) |
| Generic functions (`fn f<T>`) | yes | yes |
| `defer` (LIFO cleanup) | yes | yes (`Op::DeferCall`) |
| `char` type | yes | yes |
| Base literals (`0b`/`0o`) | yes | yes |
| Cross-numeric equality (`0xA == 10`) | yes | yes |
| Mutability (`let mut`) | yes | yes (compile-time check) |
| FFI | yes | yes (natives + `Op::FFICall`/`Op::FFIClose`) |
| Memory-mapped files | yes (slice/range/pattern) | yes (natives + single-byte index) |
| Async (`async fn`/`await`/I/O futures) | yes (deferred bodies + I/O) | yes (`Op::Await` + I/O natives; `async fn` runs sync) |
| Raw sockets (packet forging) | yes (builders + unix send/recv) | yes (builders + `Result` send/recv) |
| DNS / TLS / PCAP | yes | yes (same builtins; `Result` for query/open) |
| Compile-time macros (`macro`/`name!`/`const`) | yes (expand + exec) | yes (expand at compile time) |
| Imports & exports (`import`/`from`/`pub`) | yes (whole/from/star/pkg/cycles) | yes (whole/from/star/pkg/cycles) |
| Forensic structs (`binstruct`/`.decode`/`.encode`) | yes (registry + codec) | yes (compile-time baked natives, no new opcodes) |
| Evidence provenance (`evidence<T>`/`cite`/`report`) | yes | yes (mirrored value + natives) |
| Structured errors (`err_*`) | yes | yes |
| Streaming (`stream_*`) | yes | yes (`Value::Stream`; lazy `for` lowering) |
| Sets (`set_*`, `for s in set`) | yes | yes (`Value::Set`, insertion-ordered) |
| GUI (`gui_*`, `rak_call` from JS) | yes | yes (same process-wide manager) |
| CLI (`argv()`, `parse_args`) | yes | yes |
| Tunnel / UDP transport | yes | yes (`Value::UdpTransport`, `tunnel` lowering) |
| Inline assembly (`asm`) | yes | **no**, by design (see below) |
| WebSocket | yes | no (no TCP layer) |
| Channels & threads (`channel`, `select`, `spawn`, `timeout`) | yes | no (needs coroutines) |

The `yes`/`no` column is the authoritative backend-parity list, and it is
checked rather than maintained by hand: `tests/backend_parity.rs` compares the
two backends' registration tables and fails when they diverge, so a builtin
cannot be added to one side and forgotten on the other. The v8.0.0 release
shipped nineteen builtins that existed only in the VM, and the suite did not
notice.

**Remaining gaps as of 8.1.0.** 34 builtins, all blocked on one thing: a VM
native has signature `fn(&[Value])`, so it cannot call a Rak function, suspend or
resume. That rules out the channel/thread family, the socket family, `spawn`
and FFI trampolines until the VM has coroutines. `asm` has no VM counterpart
because a bytecode VM has no instructions to escape into.

`docs/V8-BACKEND-PARITY.md` has the full list and the reasoning;
`docs/V8-ROADMAP.md` has what closing it would take.

## Error model

- **Lexer** errors carry line/column; `rakc check` emits them in an
  IDE-friendly form.
- **Parser** errors carry line/column.
- **Runtime** errors are structured (`message`, `kind`, span, cause, context,
  backtrace — see [Error handling](errors.html)); `try`/`catch`/`raise` and
  the `?` operator propagate them. FFI/mmap/net_raw failures convert OS errors
  into runtime errors, never panicking the VM.

## The debugger (0.7)

`rakc debug program.rak` is a bytecode-VM source debugger with
statement-to-bytecode line mapping: breakpoints (`break <line>`), stepping
(`step`, `next`, `finish`), inspection (`locals`, `stack`, `print`,
`frame`), and `disassemble` for the full line-annotated bytecode listing. See
[CLI reference](cli.html#rakc-debug-07).

## Debug Adapter Protocol (0.7.2)

`rakc dap program.rak` speaks DAP over stdio (VS Code / any LSP-based editor):

```
rakc dap program.rak
```

The server implements the standard session flow — `initialize`,
`setBreakpoints` (verified per-line), `configurationDone`, then drives the VM
on a worker thread. While the program is paused you can:

- **continue** / **next** — breakpoint-triggered pauses resume with either
  run-to-completion or single-step-over semantics (line-granular; repeated
  opcodes of the same source line don't re-trigger).
- **stackTrace / scopes / variables** — the current source line plus `Locals`
  and `Globals` snapshots rendered at each stop.
- **evaluate** — look up a local/global by name while paused.
- **pause / disconnect** — interrupt a running program or terminate cleanly.

Program output (`dump` etc.) streams as `output` events; the session ends with
`terminated`. Because the hook runs at statement boundaries in the VM, only
the bytecode backend is supported (`rakc dap`), the same one `rakc debug`
uses.

## Implementation notes

- Regex values are compiled at **compile time** on the VM (a `Value::Regex`
  constant), so the VM never pays for regex construction at runtime.
- `binstruct` decode/encode are baked at compile time into self-contained
  native-fn globals (`__bin_decode_<Name>` / `__bin_encode_<Name>`) — no new
  opcodes.
- `|>` is a parse-time desugar, so the VM evaluates it with no new opcode.
- `evidence<T> from expr` lowers to a call to the `__evidence_from` native.
- `defer f(args)` compiles to `Op::DeferCall` with the pre-evaluated callee and
  args. Each frame holds a defer stack; `Op::Return` runs it LIFO before
  populating the caller's result, so the return value survives deferred calls.
