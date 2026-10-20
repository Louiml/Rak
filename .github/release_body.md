## 1.1.0 - 2026-10-20

Rak runs in a browser, and its two backends are finally the same language.

### Rak in the browser

The documentation site has a playground, and it runs **Rak's own interpreter**
— the same lexer, parser and evaluator as `rakc run` — compiled to WebAssembly,
inside the page. Nothing is mocked and nothing is transcribed to JavaScript.

```
https://louiml.github.io/Rak/playground.html
```

The module is small and honest about what it cannot do. It imports exactly one
thing from the page: random bytes, which the browser's own cryptographic
generator supplies, so `rand_int` works in the browser. What it cannot have, it
says out loud. A page has no TLS stack, no event loop and no file system, so
the browser build has no network, no async and no filesystem — and a program
that reaches for one gets an error naming the missing capability, not a line of
output that scrolls past.

That last one was a bug until this release. `fetch` in a build with no HTTP
client reported a failed request and carried on, and the program exited 0. In
the browser build, where *every* request fails, that is exactly the wrong
answer: a program that asked for the network was told it had succeeded. It
raises now.

The browser build is not a toy branch. It is a configuration CI tests on every
push — the suite runs against it (`cargo test -p rakc --no-default-features`),
the wasm module is built, and a program is run through both the module that was
just built and the payload the site actually serves. The check is behavioural
rather than a comparison of file sizes, because a size comparison fails on
every toolchain bump, and a permanently red job is a job people stop reading.

### The two backends are one language

`rakc run` is a tree-walking interpreter. `rakc vm` is a stack machine. They
are two implementations of one language, and the release before this one closed
the gap between them: **31 builtins that existed only on the interpreter now
exist on the VM**, and the count that tracked the gap is gone. The gate that
compares the two backends' registration tables is a plain assertion now — a
builtin added to one side and forgotten on the other fails the build.

Most of that gap was never the coroutine problem it was written down as. A VM
native has the signature `fn(&[Value])`, so it cannot call a Rak function — but
it can block, and the interpreter's channels, sockets, DNS lookups and futures
block too. They are plain natives now. The three that genuinely need the
machine (`task_group`, `expect_error`, `gui_callback`) are intercepted where
the machine can hand a closure back.

Two more differences closed in the same release:

- **`ensures` postconditions are checked on both backends.** The VM used to
  refuse the program outright, which is honest but means a promise checked on
  one backend went unchecked on the other. The clauses are compiled into the
  callee's own body now, and the failure names the function and the clause in
  the interpreter's words.
- **A closure sees the function it was written in.** It used to resolve the
  name as a global on the VM. It now copies the enclosing frame's values out
  when the closure is made, which is the interpreter's semantics, so an
  assignment inside a closure lands in the closure's own copy.

Three things are still interpreter-only, and say so at compile time rather than
guessing: inline assembly (a bytecode VM has no instructions to escape into),
`exit` (the CLI owns the process), and the `fetch` statement, which is a
statement rather than a builtin and so is invisible to the registration gate.
The VM has no HTTP client yet. That is recorded in
`docs/V8-BACKEND-PARITY.md` rather than left to be discovered.

### The documentation site, rebuilt

The site was redesigned around a plain, fast system: a white canvas, one
accent, hairline rules, and 96 pixels of vertical rhythm. The pages are
static and served straight from this repository.

The new part is that the site and the compiler now meet: the playground on the
site *is* the compiler, running.

### CI that can fail

Two gates are hard now, and both of them caught things on the way to this
release:

- `cargo fmt --all --check` and `cargo clippy -- -D warnings` are failures
  rather than warnings. The tree is rustfmt-clean and clippy-clean under
  `-D warnings`, with two documented project-wide allowances.
- A new job builds and tests the *browser* configuration, which is the
  configuration every visitor to the site actually runs.

### The README, rewritten

It was 1,236 lines and had grown by accretion — the same package manager
twice, three vintages of release archaeology, and a project layout that had
drifted from the tree. It is now organised around what the language is, what
it is for, and what it cannot do yet, with the archaeology moved to the release
notes and the documentation site. Every code sample in it was run on both
backends before it was published.

### Upgrading

Nothing in the language changed. A program that ran on 1.0.1 runs on 1.1.0.

The one behaviour change is in builds without the `http` feature — the browser
build, and any build with `--no-default-features` — where `fetch` now raises
instead of printing a failed request and continuing.

### Upgrade notes

- The wasm payload in `docs/rak.wasm` is rebuilt for 1.1.0.
- `oyvey` is unchanged; `Rak.toml` and the legacy `package.rak` manifest both
  work as before.
