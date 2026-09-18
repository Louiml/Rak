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
// runs pkg/init.rak + pkg/sub.rak (interpreter; use `from pkg.sub import x`
// on the VM).
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

## Resolution

For a name `m`, the importer searches: the importing file's directory, then
`./packages/`, then the `RAK_PATH` env var (`;` on Windows, `:` on Unix),
trying `m.rak` then `m/init.rak`.

- Modules are imported once (cached by canonical path).
- A circular import returns the partially-initialized module (Python
  semantics).
- `from m import *` never overwrites an existing local binding.
- Unmarked top-level names are private to the module.

## Errors

- Missing module: `import: cannot find module 'm' (searched: dir, packages, RAK_PATH)`.
- Missing name: `from m import x: 'x' is not exported`.

## VM subset

- `import pkg.sub` (nested directory access) is interpreter-only; use
  `from pkg.sub import x` on the VM.
- `pub struct`/`pub enum` export on the interpreter; on the VM use
  `pub let`/`fn`/`const`/`macro` for cross-module exports.
