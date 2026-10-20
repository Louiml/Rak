# Bytecode VM

`rakc vm` runs bytecode. The compiler lowers the shared AST to chunks, and a
stack machine executes them. Benchmarks show about a 6x speedup over the
tree-walking interpreter on compute-heavy code; `rakc bench file.rak` compares
them on the same script.

```bash
rakc run file.rak     # the interpreter
rakc vm file.rak      # the bytecode VM
rakc bench file.rak   # both, timed
```

The VM never produces a wrong result for a feature it does not support: the
compiler refuses, with a message naming the construct.

## The two backends

They are two implementations of one language, so they are compared against
each other in the test suite rather than tested separately.

- A **structural gate** reads the registration sites in both backends and
  requires the name sets to be identical. A builtin added to one side and
  forgotten on the other fails the build.
- **Behavioural tests** run a program on both and require identical output.
  This is the kind that catches a *different answer* rather than a missing
  name.

The v8.0.0 release shipped nineteen builtins that existed only on the VM, and
the suite did not notice: a per-backend test suite cannot catch a per-backend
omission. The registration gap is now closed — 31 interpreter-only builtins
reached the VM, and the count that tracked the gap is gone.

## Coverage

| Feature | Interpreter | VM |
|---------|:-----------:|:--:|
| Pipeline `\|>` | yes | yes (desugared in the front end) |
| Regex literals | yes (methods + builtins) | yes (builtins) |
| Binary pattern matching | yes | yes |
| Trait protocols | yes | yes |
| Method-call dispatch | yes | yes |
| User enum patterns | yes | yes |
| Generic functions (`fn f<T>`) | yes | yes (the type argument is discarded) |
| `defer` (LIFO cleanup) | yes | yes |
| `char`, base literals, cross-numeric equality | yes | yes |
| Mutability (`let mut`) | yes | yes (compile-time check) |
| FFI | yes | yes |
| Memory-mapped files | yes (slice, range slice, pattern match) | yes (natives; single-byte index only) |
| Async (`async fn` / `await` / I/O futures) | yes (deferred bodies, I/O) | yes (`async fn` bodies run synchronously) |
| Raw sockets (forging and sending) | yes | yes |
| DNS / TLS / PCAP | yes | yes |
| Compile-time macros | yes | yes (expanded in the front end) |
| Imports and exports | yes | yes (whole, from, star, package, cycles) |
| Forensic structs (`binstruct`) | yes | yes (baked natives, no new opcodes) |
| Evidence provenance | yes | yes |
| Structured errors (`err_*`) | yes | yes |
| Streaming (`stream_*`) | yes | yes |
| Sets (`set_*`) | yes | yes |
| Threads, channels, `select`, `task_group` | yes | yes |
| GUI | yes | yes (the same process-wide manager) |
| CLI (`argv()`, `parse_args`) | yes | yes |
| Tunnel and UDP transport | yes | yes |
| WebSocket | yes | yes |
| `scan` statement | yes | **no** |
| `fetch` statement | yes | **no** |
| `trace` statement | yes | **no** |
| `assert` statement | yes | **no** |
| `async { }` block | yes | **no** |
| Inline assembly (`asm`) | yes | **no**, by design |
| `exit` | yes | **no**, by design |

The last four are *statements* rather than builtins, so the registration gate
cannot see them: it compares names, and these are not names. The VM's
refusal is loud — "VM does not support statement: Fetch" — but a program that
uses `fetch` runs under `rakc run` and not under `rakc vm`, and that is a
real difference. `docs/V8-BACKEND-PARITY.md` records it.

Two more differ by design: `asm`, because a bytecode VM has no instructions to
escape into, and `exit`, because the CLI owns the process.

## Two differences worth knowing

**`async fn` runs synchronously on the VM.** The interpreter defers an
`async fn` body to a worker; the VM runs it in place and `await` resolves
immediately. The result is the same, the concurrency is not.

**`string(bytes)` renders differently.** The interpreter prints the debug
form (`b"\x41\x42"`), the VM prints the text (`"AB"`). A program that builds a
printable glyph gets different bytes depending on the backend, which is the
worst shape of divergence — but it is a rendering difference, not a semantic
one.

## Implementation notes

- Regex values are compiled at **compile time** on the VM, so it never pays
  for regex construction at run time.
- `binstruct` decode and encode are baked at compile time into native
  functions — no new opcodes.
- `|>` is a parse-time desugar, so the VM evaluates it with no new opcode.
- `evidence<T> from expr` lowers to a call to a native.
- `defer f(args)` compiles to a defer op with the pre-evaluated callee and
  arguments. Each frame holds a defer stack, and the return runs it LIFO
  before the caller's result is populated, so the return value survives the
  deferred calls.
- A function with postconditions does not return directly: the value is parked
  in a local and a trampoline at the end of the body runs the deferred calls,
  checks the clauses, and returns. The failure names the function and the
  clause in the interpreter's words.

## The error model

- **Lexer** errors carry line and column.
- **Parser** errors carry line and column.
- **Runtime** errors are structured — message, kind, span, cause, context —
  and `try`/`catch`/`raise` and the `?` operator propagate them. FFI, mmap
  and raw-socket failures become runtime errors; the VM does not panic.

## The debugger

`rakc debug program.rak` is a source debugger on the bytecode VM, with
statement-to-bytecode line mapping.

```text
dbg> break 12
dbg> c
[stopped] line 12
dbg> locals
  count = 3
dbg> print count
  3
```

The commands are `break <line|file:line>`, `continue`/`c`, `step`/`s`,
`next`/`n`, `finish`, `locals`, `stack`/`bt`/`backtrace`, `print <name>`,
`disassemble`, `frame` and `quit`. An empty line prints the locals. There is
no watchpoint, no conditional breakpoint, and no distinction between stepping
into and stepping over: `s` and `n` are the same single-line step.

## The Debug Adapter Protocol

`rakc dap program.rak` speaks DAP over stdio, so VS Code and similar editors
get breakpoints, stepping and variable inspection.

```bash
rakc dap program.rak
```

The server implements the standard session flow — `initialize`,
`setBreakpoints`, `configurationDone` — and then drives the VM on a worker
thread. While the program is paused you can continue, step, inspect the stack,
the scopes and the variables, and evaluate a local or global by name. Program
output streams as DAP `output` events, and the session ends with
`terminated`.

**What it does not do.** `stackTrace` returns exactly one frame — there is no
real call stack in the DAP yet. `setVariable` and `restart` are advertised as
unsupported. Exception breakpoints are accepted and ignored. `evaluate`
resolves a bare identifier; it does not evaluate an expression. The "stopped"
reason is always `breakpoint`, even for a step.

Only the bytecode backend is supported, because the hook runs at statement
boundaries in the VM.
