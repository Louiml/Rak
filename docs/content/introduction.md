# Rak

A programming language for hackers, OSINT investigators, and systems
programmers.

Hex is a first-class type. A bytecode VM runs about 6x faster than the
tree-walker. There is a SQL engine written in Rak itself, and a Rak
interpreter written in Rak.

## Two backends, one language

- **Interpreter** (`rakc run`) — a tree-walker. Every language feature works
  there.
- **Bytecode VM** (`rakc vm`) — a stack machine, roughly 6x faster on
  compute-heavy code.

They share one parser, and they register the same builtins. A gate in the
test suite compares the two registration tables and fails the build if they
ever drift apart, and a second family of tests runs a program on both and
requires identical output. The v8.0.0 release shipped nineteen builtins that
existed only on the VM and the suite did not notice; the registration gap is
now closed.

A few *statements* are still interpreter-only, and the VM says so with a
clear error rather than guessing. See [Bytecode VM](vm.html).

`rakc bench file.rak` compares the two on the same script, and
`rakc build file.rak` compiles to a standalone executable.

## It runs in a browser

The documentation site has a playground that runs **Rak's own interpreter**,
compiled to WebAssembly, inside the page — the same lexer, parser and
evaluator as `rakc run`.

```text
https://louiml.github.io/Rak/playground.html
```

A browser has no TLS stack, no event loop and no file system, so the browser
build has no network, no async and no filesystem, and it says so rather than
pretending to work. Randomness does work: the module asks the page for
entropy, and the page uses the browser's own cryptographic generator.

That build is a configuration CI tests on every push, with its own suite.

## Feature families

The language started as an OSINT scripting tool. It grew into something
bigger.

- **Forensic structs and evidence provenance** — declare a binary wire
  format once with `binstruct` and get both a decoder and an encoder for
  free. Every decoded value is wrapped in an `evidence<T>` provenance tag, so
  `report(...)` emits a chain-of-custody-cited findings report.
- **Data pipelines** — a `|>` pipeline operator, regex literals (`/\d+/g`)
  with method syntax, and binary pattern matching over byte slices.
- **A type system** — signed and unsigned ints (i8..i64, u8..u64), floats
  (f32, f64), typed literals like `42i32`, tuples, Result and Option,
  modules, closures with lexical scoping, traits with method dispatch,
  pattern matching, and string interpolation with `f"hello {name}"`.
- **Contracts** — `requires` and `ensures` on a function, checked on both
  backends, with `result` in scope.
- **Concurrency** — `spawn`/`thread_join`/`channel()` threads, an async event
  loop on Tokio, and bounded-pool fan-out (`await_all`, `select`, `timeout`,
  `task_group`).
- **A SQL server in Rak** — a full SQL engine, lexer through executor, written
  in the language itself. It serves queries over TCP.
- **Self-hosting** — `examples/self_host.rak` is a Rak interpreter written in
  Rak. It lexes, parses, and evaluates real Rak source code.
- **A native GUI** — windows and widgets drawn by `egui`, with no webview and
  no browser engine on either platform.
- **A package manager** — `oyvey`, with a Cargo-shaped manifest, a versioned
  lockfile with per-package checksums, path dependencies, and a lockfile that
  is verified rather than trusted.

The full design and implementation plan for every systems-level feature is in
[`rak-features-spec.md`](https://github.com/Louiml/Rak/blob/main/docs/rak-features-spec.md).
