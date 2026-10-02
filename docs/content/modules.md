# Modules (imports & exports)

Rak has a Python-style module system with explicit `pub`/`export`, name-based
resolution, `from ... import`, directory packages, import-once caching, and
circular-import support. Works on both the interpreter and the bytecode VM.

## Syntax

```rak
// Whole-module import (binds the module; access via m.x)
import "./math.rak"          // file path -> binds "math"
import math                  // name: searches dir, ./packages/, RAK_PATH for math.rak
import math as m             // alias
use math                     // back-compat: same as import

// from-import (binds names directly)
from math import add
from math import add as plus, mul as times
from math import *           // all exports; local bindings win on clash

// Directory packages: `import pkg` runs pkg/init.rak; `import pkg.sub`
// runs pkg/init.rak + pkg/sub.rak, so `pkg.sub` and `pkg`'s own exports are
// both reachable through `pkg`.
import pkg
import pkg.sub

// Exports — both `pub` and `export` mark an item as exported (let/fn/const/
// struct/enum/macro):
pub let PI = 3.14
export fn add(a, b) { return a + b }
pub const MAX = 256

// Re-exports
pub use math                 // re-export all of math from this module
pub use {add, mul} from math // re-export named
```

## The two access rules

This is the part worth internalising, because it is the only place in Rak where
binding something gives you a *view* rather than a *value*.

```rak
// bank.rak
pub let mut BALANCE = 0
pub fn deposit(n) { BALANCE = BALANCE + n  return BALANCE }
```

```rak
import bank                   // <- a view of the module's state
from bank import BALANCE      // <- a copy, taken once, right now

bank.deposit(10)
bank.deposit(10)
dump bank.BALANCE             // 20 — the view followed the module's writes
```

### `import m` gives you the module

`m` is a live handle on the module's own top-level bindings. Every read and
write through it goes to the module:

```rak
import bank
import bank as b              // the same module, not a second copy

bank.deposit(5)
dump b.BALANCE                // 5 — one module, two handles

bank.BALANCE = 100            // writes the module's state, not a copy
dump bank.BALANCE             // 100

dump bank.deposit(1)          // functions too
```

This is what makes a module usable as shared state: a counter, a registry, a
cache. It is the same rule Python has, and it works through aliases and through
function arguments (`fn total(m, n) { return m.deposit(n) }`), which is the usual
way to reach a module's state from somewhere else.

Two rules bound the write side, and both come from the module's own declarations
rather than the importer's:

```rak
// bank.rak
pub let NAME = "fixed"      // reachable, not assignable
pub let mut RATE = 0        // reachable and assignable
```

```rak
bank.NAME = "x"             // refused: declared `let` in its module
bank.RATE = 1               // allowed
```

`m.X = v` writes the module's real state, so `pub` and `let mut` are what stand
between an importer and a module's internals.

### `from m import x` gives you a value

The name is bound to the value the module held at import time. If the module
later rebinds it, your copy does not follow.

```rak
from bank import BALANCE, deposit
deposit(10)
dump BALANCE                  // 0 — the snapshot you asked for
```

Python behaves the same way, and the reason is worth keeping: a live binding for
a `from`-imported name would be a second, invisible route by which a module could
mutate a caller's variables, and "who changed this?" would stop having an answer.

**Use `import m` when you want to follow the module's state. Use `from m import
x` when you want a value you own.**

If you want a snapshot *and* a name of your choosing, alias it — `from bank
import BALANCE as opening_balance` — which copies on both backends.

### `pub use` copies too

A re-export hands out values, not a second live view:

```rak
// outer.rak
pub use {deposit, BALANCE} from bank
```

`outer`'s namespace gets the values `bank` exported at that moment. Importing
`outer` does not give you a live window onto `bank`; importing `bank` does.

## Resolution

For a name `m`, the importer searches: the importing file's directory, then
`./packages/`, then the `RAK_PATH` env var (`;` on Windows, `:` on Unix),
trying `m.rak` then `m/init.rak`.

- Modules are imported once (cached by canonical path).
- A circular import returns the partially-initialized module (Python
  semantics).
- `from m import *` never overwrites an existing local binding.
- Unmarked top-level names are private to the module: `m.private` and
  `m.nosuchname` are indistinguishable from outside.
- A module's code resolves names against **its own file**, not the importing
  one, so a name in a module means what that file says it means.

## Module state is real state

A module's top-level `let mut` is module-level state, not a local. It persists
across calls, across a call chain, and across threads:

```rak
// bank.rak
pub let mut TOTAL = 0
pub fn deposit(n) { TOTAL = TOTAL + n  return TOTAL }

// main.rak
import bank
fn deposit_twice(n) { bank.deposit(n)  return bank.deposit(n) }
deposit_twice(5)
deposit_twice(5)
dump bank.TOTAL                // 20, not 10
```

## `mod { }` — a module without a file

The in-file spelling, with the same rules:

```rak
mod counter {
    pub let mut COUNT = 0
    pub fn bump() { COUNT = COUNT + 1  return COUNT }
}
dump counter.bump()
dump counter.COUNT            // 1
```

Inside a `mod` block `pub` is optional: every top-level name is exported, since
the block already marks the boundary explicitly. `export` works too.

## Errors

- Missing module: `import: cannot find module 'm' (searched: dir, packages, RAK_PATH)`.
- Missing or private name: `from m import x: 'x' is not exported (module exports:
  a, b, c)`. The list is there because a misspelling and a missing `pub` look
  identical from the importing file.

## VM differences

Everything above holds on both backends *except* what follows, and all of it comes
from one cause: the VM compiles a module's body into the same chunk, so a module's
top-level bindings are ordinary globals in a flat namespace rather than a cell of
their own.

- A module can read the *importer's* globals, and a module's private top-level is
  visible to the importer.
- Reading a private name through the handle reports an error on the interpreter
  and reads as `nil` on the VM. Writes are reported on both.
- Two modules exporting the same name share one binding on the VM, so `a1.shared`
  can report `b1`'s value. Nothing reports it.
- `from m import x` without an alias, and `from m import *`, copy on the
  interpreter but stay live on the VM. Aliasing (`from m import x as y`) copies on
  both.
- `mod { }` blocks are inlined into the enclosing namespace on the VM, so their
  private names leak and two blocks declaring one name collide.
- A module stored in a map or array can be read through on both, but assigning
  through it (`d.m.N = 1`) is a compile error on the VM.

The full list, with reproductions, is in
[docs/V8-KNOWN-ISSUES.md](../V8-KNOWN-ISSUES.md) under "The VM's module globals
are not namespaced". None of it affects a program written as one package with
distinct names per module.

Also interpreter-only for cross-module types: `pub struct` / `pub enum` export.
On the VM use `pub let` / `fn` / `const` / `macro` for cross-module exports.
