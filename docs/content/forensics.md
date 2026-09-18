# Forensic structs & evidence provenance

Two fused features, neither of which exists in any production language, and
both of which only make sense in a hex-first, OSINT-first language:

1. **`binstruct`** — a declarative wire-format DSL that compiles to both a
   decoder and an encoder (round-trip).
2. **`evidence<T>`** — provenance as a first-class type-system layer: every
   collected value carries where/when/how it was collected, merging
   transitively, so `report(...)` emits a defensible, chain-of-custody-cited
   findings report.

## binstruct

Declare a binary layout once and get both a **decoder** and an **encoder** for
free:

```rak
binstruct DnsHeader {
    id:      u16be
    flags:   u16be
    qdcount: u16be
    ancount: u16be
    nscount: u16be
    arcount: u16be
}

let h = DnsHeader.decode(q)        // -> evidence<struct> with provenance
dump h.id                          // 0x1234
dump h.qdcount                     // 1
let back = DnsHeader.encode(h)     // round-trip back to bytes
```

Field types: `u8`/`u16`/`u32`/`u64` and signed `i8`..`i64`, each with an
optional `be`/`le` endianness suffix (default big-endian); `bytes(n)` for a
fixed run of raw bytes; `rest` for the trailing remainder; and a nested
binstruct name for a `Ref` field. Only byte-aligned widths (multiples of 8,
8..64) are accepted.

Works on both the interpreter and the bytecode VM (compile-time codegen to
per-struct native-fn globals — no new VM opcodes).

## Evidence provenance

```rak
let ip = evidence<string> from "93.184.216.34"          // root tag
let answers = cite(dns_query("example.com", "A"), "dns_query", "example.com")
dump report(ip, answers)   // numbered assertions + cited sources (tool/target/ts)
dump provenance(ip)        // {tool, target, ts, raw_offset, raw_len, parent}
dump strip_evidence(ip)    // 93.184.216.34
```

- `evidence<T> from expr` — wrap a value in a root provenance tag.
- `cite(value, tool?, target?)` — wrap a value, chaining `parent` to any
  existing evidence so provenance merges transitively.
- `report(evidence, ...)` — render a Markdown-style report with numbered,
  source-cited assertions.
- `provenance(value)` — the provenance chain as a map.
- `strip_evidence(value)` — drop the wrapper, return the inner value.

Evidence is **observational, not a barrier**: field access and indexing
transparently unwrap it (`evidence<struct>.field` reads through), `==` compares
inner values ignoring provenance, and display/truthiness unwrap.

## Errors and limits

- Unknown binstruct in `.decode`/`.encode` or a nested `Ref`:
  `unknown binstruct '...'`.
- Field outruns buffer: `binstruct: field '...' outruns buffer (off+n > len)` —
  never a panic; all reads are bounds-checked.
- Circular binstruct ref: detected at resolve time.
- Provenance is explicit (no auto-wrapping collectors) — non-magic, and keeps
  existing behaviour intact.

## Dogfooding

`examples/forensic_structs.rak` decodes real DNS queries produced by the
stdlib `dns_build()` and verifies the binstruct layout matches the Rust
builder's wire bytes — on both `rakc run` and `rakc vm`.
