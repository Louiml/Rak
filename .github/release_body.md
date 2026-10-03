# Rak v0.8.3

Cross-module variables that behave like Python's, byte-exact binary file I/O, a
`for` loop that survives a NUL byte, and `fmt` that understands a format spec.

## About the version number

This release is tagged **v0.8.3**. The tags before it read `v0.4.0` … `v0.7.2` and
then switched to `v8.0.0`, `v8.1.0`, `v8.1.1`, so v0.8.3 sits numerically below the
8.x line it follows.

That is deliberate, and it has one consequence worth stating plainly: **`cargo`
will read this as a downgrade from 8.1.1.** A `cargo update` will move the dependency
backwards rather than forwards, and a lockfile pinning `8.1.1` will not move to
`0.8.3` on its own. If you depend on Rak, take the commit rather than the tag, or
specify `=0.8.3` deliberately.

The two commits this release contains are titled `Rak v8.2.0`, which was the number
in use when they were written. Rewriting them would have meant force-pushing already
public history to make the subjects agree with a number decided afterwards, which
seemed the worse trade. The titles describe the work; the tag names the release.

## A `for` loop stopped at the first `0x00`

The headline fix, and it is worse than anything else in this release.

```rak
let buf = bytes([0, 15, 16, 255])
let mut n = 0
for b in buf { n = n + 1 }
dump n            // interpreter: 4    VM, before: 0
```

The VM's loop decided whether to continue by testing the **element's truthiness**
rather than the loop bound — `IndexGet`, then `JumpIfFalse`. Since `0`, `""` and
`false` are all falsy, iteration ended at the first of them. No diagnostic, and
silent, because the interpreter was fine and the divergence gate did not cover it.

`0x00` is the most common byte in a binary file, so `for b in buffer` walked a buffer
and then stopped dead at the first NUL. It was found by writing a hex editor on top
of Rak: the engine's first buffer walk died partway through a test file, and the
cause was a language bug rather than anything in the editor.

`Op::Len` now makes the bound `idx < len`. A loop bound has to be a comparison
against a length, not a test of the value being carried.

It survived review because the one test covering byte iteration used `bytes([1, 2])`,
which has no falsy byte in it.

## `fmt` takes real format specs

```rak
dump fmt("{:02X}", 5)     // 5, before.  05, now.
dump fmt("{:#x}", 255)    // 0xff
dump fmt("[{:>4}]", n)     // right-aligned, width 4
dump fmt("{:.2f}", ratio)  // two decimals, on both backends
```

The spec was matched by asking whether the text *contained* `04X`, `08X`, `x` or `X`.
Exactly two widths worked and no other type did, so `{:02X}` — the width a hex dump
wants — fell through to the default rendering.

The VM's copy had drifted further and had **no float branch at all**, so
`fmt("{:.2f}", x)` printed a rounded value on the interpreter and the raw float on
the VM. Two hand-written copies of the same chain is the underlying mistake; they are
now one shared parser (`rakc/src/fmt_spec.rs`), and `rakc/tests/fmt_specs.rs` runs a
table of specs through both backends and fails if they disagree.

The width counts the sign, matching Rust, Python and Go: `{:05}` of `-42` is `-0042`.

## Cross-module variables

`import m` used to bind a *snapshot* of the module's exports, so a `pub let mut` the
module reassigned never reached the importer. A module's own top-level `let mut`
reset on every call, because a module body ran in a scope on the importer's
environment and `Env::clone` deep-copies scopes — `bump(); bump()` gave `1, 1`.

```rak
import m
m.X = v         // writes the module's state, and only if it is `pub let mut`
from m import x as y   // a copy, on both backends
```

The interpreter already did this. Five of the six documented VM differences are now
closed: a private top-level is no longer visible to the importer, two modules may
export the same name, `from m import x` copies with or without an alias, `mod { }`
blocks no longer collide, and reading a private or misspelled name through a handle
reports it by name instead of returning `nil`.

The last one is not fixed, and `docs/V8-KNOWN-ISSUES.md` says what it needs: a
module body still sees a name it never *declared*. Closing it requires a module
scope in the compiler, and the compiler has no list of builtin globals — the VM
registers those at startup — so it cannot tell `len` from a leaked name. Guessing
would break every module that calls a builtin. The test that covers it fails loudly
with "convert this to agree_on" if it ever closes.

## Byte-exact binary file I/O

`file_read` is `fs::read_to_string`, so it **fails** on any file containing a byte
sequence that is not valid UTF-8, and `write` coerces through UTF-8, so a `0xFF`
came back as U+FFFD. There was no way to open a binary file at all.

```rak
let buf = file_read_bytes("firmware.bin")
buf[0] = 0xFF
file_write_bytes("patched.bin", buf)

let m = mmap_open("firmware.bin", "rw")
mmap_write(m, 0x100, 0x90)          // the mapping *is* the file
```

Plus `bytes([...])`, byte indexing, byte assignment, byte iteration, and buffer
concatenation. `mmap_write` refuses a read-only mapping and an out-of-range offset
*by name*, and a multi-byte write that would run past the end applies none of its
bytes.

## Smaller things

  * **`Hex` no longer renders padded.** `dump 0x00` printed
    `0x0000000000000000` on the VM. The value carried a digit width and `Display`
    used it, but nothing ever set that width from the source — the compiler
    hardcoded 64 — so all sixteen slots were used. The field had exactly one reader,
    so it is gone.
  * **A corrected claim.** The known-issues entry said the two backends *compared*
    `Hex` differently. They never did: `PartialEq` has a cross-representation numeric
    fallback and `Op::Eq` goes straight through it. Retracted, with the evidence,
    rather than left as a warning that would send someone hunting a bug that is not
    there.
  * **`:dis` desynced** by one byte after every `for` loop, because `IterItems` has
    a 1-byte operand and was missing from the width table.
  * **The VM's `len` did not accept a struct**, which the interpreter's did.
  * **The IDE version is pinned from the tag** by `scripts/pin-ide-version.sh`, which
    has been run against `v0.8.3` to confirm all three files take. v8.1.0 shipped
    four Tauri bundles named `8.0.0` because that check did not exist.

## Tests

505 workspace tests pass. `module_state.rs` is 33, `fmt_specs.rs` is 9,
`bytes_io.rs` is 11 — each running its cases on both backends and failing on any
disagreement. The tests that used to *pin* a divergence now assert agreement; the one
that is still real says so in its name.

The hex editor these primitives were built for is at
<https://github.com/Louiml/HexEditor>.