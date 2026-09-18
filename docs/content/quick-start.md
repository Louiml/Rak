# Quick start

## Hello, World

```rak
dump "Hello, World"
```

`dump` prints a value to the console. That's the whole program — no `main`, no
imports, no boilerplate. (Need a `main`? See [Writing CLIs](cli.html#entry-point).)

Run it:

```text
rakc run hello.rak      # run on the interpreter
rakc vm hello.rak       # run on the bytecode VM (faster)
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

## Build a standalone executable

```bash
rakc build examples/hello.rak   # produces hello.exe on Windows, hello on Linux
./hello                          # [DUMP] Hello, World
```

The source is embedded in the binary; no Rak installation is needed on the
target machine.

## REPL

```text
rak> let x = 10
rak> dump x
[DUMP] 10
rak> fn add(a, b) { return a + b }
rak> dump add(3, 4)
[DUMP] 7
```

State persists between lines. Commands start with `:` — `:vars`, `:ast <expr>`,
`:bytecode <expr>`, `:clear`, `:quit`. Unclosed blocks continue on the next
line. See [Tooling](tooling.html#repl).
