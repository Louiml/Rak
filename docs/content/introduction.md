# Rak

A programming language for hackers, OSINT investigators, and systems programmers. Build desktop GUI apps, backend SQL servers, shareable packages, and self-host the compiler. All in Rak.

Hex is a first-class type. The bytecode VM runs about 6x faster than the tree-walker. There's a SQL engine written in Rak itself, and a Rak interpreter written in Rak.

## Two backends, one language

- **Interpreter** (`rakc run`) — a tree-walker. Every language feature works there.
- **Bytecode VM** (`rakc vm`) — a stack machine, roughly 6x faster on compute-heavy code. It supports a deliberate subset and fails fast with a clear `VM does not support ...` error for anything else.

Both share one parser, and `rakc bench file.rak` compares them on the same script. `rakc build file.rak` produces a self-contained executable with no Rak installation needed on the target machine.

## What's new in 0.7.0

- **Structured errors** — `catch e` binds a first-class `Error` value with a
  `kind`, source span, cause, and context (`err_kind`/`err_message`/`err_line` …).
- **True async concurrency** — `await_all`, `select` (race), `timeout`,
  `task_group`, `async_sleep`/`async_yield`, and deferred `async fn` bodies run
  concurrently on a bounded worker pool (thousands of lightweight ops).
- **Streaming** — pull-based `Value::Stream` with `stream_map`/`filter`/`take`/
  `collect`, `read_lines`, and `tcp_stream`; lazily consumed by `for`.
- **CLI** — `fn main(argv) -> int` entry with real exit codes, `argv()`,
  stdin/stdout builtins, and a structured `parse_args(spec, argv)` flag parser.
- **Data processing** — lazy `stream_csv`/`stream_jsonl`, plus
  `gzip`/`gunzip`/`deflate`/`inflate` and `zip_archive`/`zip_list`/`zip_extract`.
- **rakpkg** — version/rev constraints, `rakpkg.lock` (rev + SHA-256 checksum),
  and `update`/`tree`/`audit`/`publish`.
- **Debugger** — `rakc debug program.rak` (break/continue/step/locals/stack/
  print/disassemble) powered by the bytecode VM with statement-to-bytecode
  line mapping.
- **Fuzzing** — 8 proptest harnesses (stable CI) plus a 13-target libFuzzer /
  cargo-fuzz pack (`fuzz/`) for the lexer, parser, DNS/TLS/JSON parsers,
  packet builders, WebSocket frames, tunnel, compression, and package manifests.

## Feature families

The language started as an OSINT scripting tool. It grew into something bigger:

- **Forensic structs and evidence provenance** — declare a binary wire format
  once with `binstruct` and get both a decoder and an encoder for free. Every
  decoded value is wrapped in an `evidence<T>` provenance tag, so `report(...)`
  emits a chain-of-custody-cited findings report. No other language bakes
  provenance into the value model.
- **Data pipelines** — a `|>` pipeline operator, regex literals (`/\d+/g`)
  with method syntax, and binary pattern matching over byte slices.
- **Type system** — signed and unsigned ints (i8..i64, u8..u64), floats
  (f32, f64), typed literals like `42i32` and `3.14f64`, tuples, Result/Option,
  modules, closures with lexical scoping, generics, traits with method
  dispatch, pattern matching, and string interpolation with `f"hello {name}"`.
- **Concurrency** — `spawn`/`thread_join`/`channel()` threads, an async event
  loop on Tokio, and v0.7's bounded-pool fan-out (`await_all`, `select`,
  `timeout`, `task_group`).
- **SQL server in Rak** — a full SQL engine, lexer through executor, written
  in the language itself. It serves queries over TCP.
- **Self-hosting** — `examples/self_host.rak` is a Rak interpreter written in
  Rak. It lexes, parses, and evaluates real Rak source code.

The full design and implementation plan for every systems-level feature is in
[`rak-features-spec.md`](../rak-features-spec.md).
