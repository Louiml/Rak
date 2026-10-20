# Quick start

## Hello, World

```rak
dump "Hello, World"
```

`dump` prints a value to the console. That is the whole program — no `main`,
no imports, no boilerplate. (Need a `main`? See
[Writing a CLI in Rak](cli.html#writing-a-cli-in-rak).)

```bash
rakc run hello.rak      # the interpreter
rakc vm hello.rak       # the bytecode VM (faster)
```

## A quick port scan

```rak
scan "127.0.0.1" {
    range: [0x0016, 0x0050]
} {
    if open {
        dump fmt("Port 0x{:04X}", port)
    }
}
```

`scan` takes an options group and then a body group, in that order. The body
binds `port`, `open` and `banner`. It is an interpreter-only statement; the
bytecode VM refuses to compile a program that uses it.

## Build a standalone executable

```bash
rakc build examples/hello.rak   # hello.exe on Windows, hello on Linux
./hello                          # [DUMP] Hello, World
```

The source is embedded in the binary, so no Rak installation is needed on the
target machine. For a genuinely compiled artifact, see
[Building and targets](build.html).

## The REPL

```text
rak> let x = 10
rak> dump x
[DUMP] 10
rak> fn add(a, b) { return a + b }
rak> dump add(3, 4)
[DUMP] 7
```

State persists between lines, and an unfinished block continues on the next
line. Commands start with `:` — `:vars`, `:ast <expr>`, `:bytecode <expr>`,
`:history`, `:clear`, `:help`, `:quit`. See [Tooling](tooling.html#the-repl).

## In a browser

The documentation site has a playground that runs Rak's own interpreter,
compiled to WebAssembly, in the page:

```text
https://louiml.github.io/Rak/playground.html
```

Write a program, press `Run` or `Ctrl+Enter`, and the output appears below.
