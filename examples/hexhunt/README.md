# hexhunt

A binary signature scanner for Rak — a small, complete project that shows what
a real Rak program looks like: a CLI entry point, modules, a record type with a
`Display` impl, contracts, closures, error handling, and **evidence
provenance** on every finding.

It scans a file for known signatures and reports each hit *cited against the
tool and the file it came from*, so the output is a chain-of-custody record
instead of a list of strings.

## Run it

```bash
# Interpreter (the default backend)
rakc run examples/hexhunt/src/main.rak <file>

# Bytecode VM — the same program, the same output
rakc vm examples/hexhunt/src/main.rak <file>

# With a hex dump of the first 256 bytes. The `--` is required: `rakc`'s own
# argument parser strips any `--flag` it does not recognise, so without it the
# script never sees `--hexdump`.
rakc run examples/hexhunt/src/main.rak <file> -- --hexdump

# Tests
rakc test examples/hexhunt/tests/main.rak
```

## What it does

```
$ rakc run examples/hexhunt/src/main.rak capture.bin -- --hexdump
[DUMP] hexhunt: capture.bin (19 bytes)
[DUMP]
[DUMP] 0x00000000  89 50 4E 47 0D 0A 1A 0A 41 42 43 FF D8 FF E0 25  |.PNG....ABC....%|
[DUMP] 0x00000010  50 44 46                                         |PDF|
[DUMP]
[DUMP] findings: 3
[DUMP] 0x00000000  PNG  image
[DUMP] 0x0000000B  JPEG  image
[DUMP] 0x0000000F  PDF  document
[DUMP]
[DUMP] 1. capture.bin
2. [Finding {kind: image, offset: 0, signature: PNG}, ...]

--- Sources ---
[1] tool=manual ts=1792341084
[2] tool=hexhunt target=capture.bin ts=1792341084

[DUMP]
[DUMP] by kind:
[DUMP]   image: 2
[DUMP]   document: 1
```

The `--- Sources ---` block is `report()`: every finding is numbered, and each
number carries the tool that produced it, the target it came from, and a
timestamp. That is the forensic-evidence model Rak has built into its value
type — `evidence<T>` — and it is the reason this language is interesting for
OSINT work.

## Structure

```
src/main.rak     CLI entry point: argv parsing, the signature table, the report
src/model.rak    `Signature` and `Finding` records, plus `impl Display`
src/sweep.rak    The engine: match_at, scan_file, count_by_kind, hexdump
tests/main.rak   Tests for the engine
```

## What this shows about Rak

**Records and traits.** `Finding` is a `struct`; `impl Display for Finding` is
the trait that `dump` consults, which is why a finding prints as
`0x00000000  PNG  image` instead of its field list.

**The same code on both backends.** `rakc run` is a tree-walking interpreter and
`rakc vm` is a bytecode machine. `hexhunt` runs unchanged on both and produces
identical output. Two contracts on that promise are pinned in
`rakc/tests/backend_parity.rs`: `impl Display` drives `dump` on both, and
importing a module that declares a struct and a trait impl compiles on both.

**Contracts.** `match_at` carries `requires len(pat) > 0`. `scan_file`'s
postcondition — `ensures len(result) <= len(data)` — is declared, and both
kinds of clause are checked on both backends: a precondition before the body,
a postcondition against the value the caller receives. The postcondition is
the one that earns its keep here, because "the result can never be longer than
the file" is a property of the scan that no test of the *output* would catch.

**Evidence provenance.** `evidence<T> from expr` tags a value with where it came
from; `cite(value, tool, target)` chains a provenance record onto it;
`report(...)` renders the whole chain. No other language has this in its value
model.

**A stdlib that is actually there.** `bytes`, `file_read_bytes`, `push`, `len`,
`fmt`, `chr`, `keys`, `string`, `has`, `parse_args` — all from Rak's own
standard library, with no native extensions.

## Known limits, stated

- The signature table is hard-coded. It is a list of `{ name, kind, pattern }`
  maps, so adding a format is one line.
- Scanning is `O(n · signatures)` with a byte-at-a-time compare. That is fine for
  a few megabytes and not a replacement for a real signature engine; it exists
  to demonstrate the portable subset rather than to be fast.
- No network, no FFI, no async — nothing that would make the two backends
  diverge.
