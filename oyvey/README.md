# Oyvey

The package manager and build system for the [Rak](https://github.com/Louiml/Rak)
programming language.

Oyvey is Cargo-shaped and Rak-native: a `package.rak` manifest declares a
project and its dependencies, an `oyvey.lock` records the exact git revision
and checksum of every dependency, and a global cache keeps clones so builds are
reproducible and work offline once resolved. It drives the existing `rakc`
compiler for `build`, `run`, and `test` — it does not duplicate any of it.

```bash
oyvey new myproj        # generate a project (manifest, src/main.rak, tests)
oyvey add user/repo     # add a GitHub dependency
oyvey install           # resolve + install dependencies (writes oyvey.lock)
oyvey run               # run the entry point
oyvey test              # run the project's tests
oyvey build             # build a standalone executable
```

## Install

Oyvey ships with the Rak toolchain. The Rak installer puts it on your `PATH`:

```bash
curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash -s -- --yes --install rakc,oyvey
```

Or build it from this repository:

```bash
cargo build --release
./target/release/oyvey --version
```

Requirements: a Rust toolchain to build, and `git` on `PATH` at runtime (every
dependency is a git repository).

## The manifest

`package.rak` is a Rak file with `let` bindings:

```rak
let name = "mylib"
let version = "0.1.0"
let description = "A Rak package"
let license = "MIT"
let entry = "src/main.rak"
let deps = {
    net: "user/rak-net",
    crypto: "user/rak-crypto@^1.0"
}
```

`entry` is optional. When it is absent, Oyvey infers it from `init.rak`,
`lib.rak`, `main.rak`, `src/lib.rak`, then `src/main.rak`.

## Dependency specs

| Spec | Meaning |
|------|---------|
| `user/repo` | track the default branch |
| `user/repo@^1.2` | caret — compatible with `1.2` |
| `user/repo@~1.2` | tilde — compatible with `1.2.x` |
| `user/repo@1.2.3` | exact version |
| `user/repo@*` | any version |
| `user/repo#<rev>` | pin to a git revision (tag, branch, or commit) |
| `https://host/user/repo` | an explicit URL, for a self-hosted Git server |

A `#rev` may be combined with a constraint (`user/repo@^1.2#deadbeef`); the
revision wins. Dependencies of dependencies are resolved automatically. A
dependency cycle is fine; two different sources claiming the same package name
is reported as a conflict rather than resolved silently one way.

## The lockfile

`oyvey.lock` is TOML. Each package records the revision it was built from and a
SHA-256 of its manifest:

```toml
version = 1

[[package]]
name = "rak-net"
version = "0.2.1"
source = "git+https://github.com/user/rak-net#0123456789abcdef0123456789abcdef01234567"
checksum = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
```

`oyvey install` reuses the locked revision whenever it still satisfies the
manifest, which is what makes an install reproducible. `oyvey update`
re-resolves to the newest matching versions. `oyvey audit` checks the installed
packages against these checksums.

## How imports find a dependency

Installed packages are vendored into `<project>/packages/`, which is the
directory `rakc` already searches for a bare `import <name>`. When `run`,
`build`, and `test` invoke the compiler, they prepend that directory to
`RAK_PATH`, so imports resolve wherever the entry point lives:

```rak
import mylib
dump mylib.greet()
```

A package whose entry is a single file gets a generated `init.rak` that
re-exports it, which is what turns it into a directory package the compiler can
resolve by name.

## Cache

```text
~/.oyvey/git/db/<source>/             bare clones, one per source URL
~/.oyvey/git/checkouts/<source>/<rev>/ one working tree per locked revision
```

Set `OYVEY_HOME` to move it.

## Commands

```text
oyvey new <project>      Generate a new Rak project
oyvey init               Initialize an existing directory as a project
oyvey add <github-repo>  Add a GitHub package dependency
oyvey remove <package>   Remove a dependency
oyvey install            Resolve and install dependencies
oyvey update             Re-resolve dependencies within constraints
oyvey build              Compile the project through rakc
oyvey run                Run the entry point
oyvey test               Run the project's tests
oyvey clean              Remove build artifacts
oyvey list               List installed packages
oyvey tree               Print the dependency tree
oyvey audit              Verify packages against the lockfile
oyvey lock               Write oyvey.lock without installing
```

`oyvey help <command>` explains one command in detail.

## License

MIT OR Apache-2.0, matching Rak.
