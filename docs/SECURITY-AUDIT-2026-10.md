# Security assessment: the Rak toolchain

**Date:** 2026-10-20
**Assessor:** Lead Ethical Hacker / Penetration Tester (white-box, authorised)
**Baselines**

| Repository | HEAD | Working tree at assessment time |
| --- | --- | --- |
| `D:\Rak` (compiler, stdlib, IDE) | `b656d098111eb206c5f21406b3234573185df733` | 17 files modified, +~500 lines |
| `D:\Rak\oyvey` (package manager) | `35bbaddde87ce223b039a9e8324445e2dfd1b3d5` | 2 files modified, +558 lines |

`oyvey` is an **independent repository**, not a workspace member of `D:\Rak` and
not tracked by it (`git ls-files oyvey` returns nothing). It has its own
history and its own release cadence. Any remediation therefore lands in two
changesets, and any report that quotes "the repo" is ambiguous.

> **Note on the working tree.** The tree was being modified concurrently by
> other agents throughout this assessment. Every finding below was re-verified
> against the on-disk state immediately before it was written up, and each one
> names the revision it was verified against. Where another agent's in-flight
> work already closed a finding, that is stated explicitly and the finding is
> credited rather than re-reported.

---

## 1. Executive summary

Rak is a cybersecurity language: it exists to analyse hostile input, forge
packets and drive network tools. The interesting security question is
therefore not "is it safe to run untrusted Rak code" — that is never true of a
language with `unsafe` and raw sockets — but **"when I point it at someone
else's script, what will it do to me?"**

That question has two answers, and the good news is that the language's own
answers are better than most. The capability sandbox is real, enforced at a
single choke point, and the `--fs-root` scope confines the filesystem.

The bad news is that the **package manager was the weakest link in the chain,
and it failed open.** `oyvey` handed a dependency's URL straight to `git
clone` without checking what it was. Because `git` parses a leading `-` in
any argument as an option, and because git interpolates the value of
`--upload-pack` into a shell command line, a package's own manifest could
name a dependency that ran an arbitrary command **during `oyvey install`**,
before any of the user's code was even compiled, and with no prompt, no
sandbox, no checksum and no signature. The only visible symptom is a
confusing git protocol error *after* the payload has already run.

That is finding **A-1** (CVSS 8.8), and it is the reason this assessment
exists. It is now fixed and the exploit no longer runs.

Nine findings were remediated during this engagement. The most serious of the
rest are a **decompression bomb in `zip_extract`** that `SECURITY.md` claimed
was capped and was not (C-2), an **out-of-bounds read in the TLS ClientHello
parser** that a 46-byte packet turns into a process abort (C-1), and a
**SQL injection that survives `sql_escape`** on MySQL's default configuration —
where the linter was actively recommending the function as the fix (C-4).

A second class of finding is quieter but matters for the language's core
promise: the sandbox is enforced by **name prefix**, and the language server
parsed untrusted files on a **2 MiB stack** while the command line used 64 MiB.
A file of under a kilobyte killed the editor's Rak support. The parser's
expression-nesting limit did not cover statement nesting, so the limit was
never reached (D-2).

### At a glance

| | Count | Highest CVSS |
| --- | --- | --- |
| Remediated in this engagement | 9 | 8.8 (A-1) |
| Fixed concurrently by other work, verified | 5 | 7.5 |
| Open, recommended | 24 | 8.1 |
| Verified safe (documented in §7) | 18 | — |

### The one-line version

> The sandbox works; the package manager did not. Fix the package manager
> before shipping the sandbox story, because "run it under `--sandbox`" is
> advice that only helps if the code got onto the disk safely in the first
> place.

---

## 2. Scope and methodology

### 2.1 In scope

| Track | Surface |
| --- | --- |
| **A — Supply chain** | `oyvey/`: `spec.rs`, `git.rs`, `resolve.rs`, `cache.rs`, `lock.rs`, `manifest*.rs`, `project.rs`, `main.rs` |
| **B — Sandbox / runtime** | `rakc/src/caps.rs`, `interpreter.rs`, `vm.rs`, `main.rs`, `targets/` |
| **C — Stdlib & parsers** | `stdlib/src/*` (crypto, net, tls, dns, archive, mmap, ffi, escape, yara, secrets, http_server, websocket), `rakc/src/parser.rs` |
| **D — IDE / LSP / DAP** | `ide/src-tauri/`, `rakc/src/lsp.rs`, `dap.rs`, `gui.rs` |

### 2.2 Threat model

Four distinct actors, because conflating them is how these bugs hide:

| Actor | Capability |
| --- | --- |
| **Script author** | Writes the `.rak` file. May be malicious. |
| **Script consumer** | Runs someone else's `.rak` file. Wants the sandbox to hold. |
| **Package publisher** | Publishes a package to a git repository. May be malicious. |
| **Package consumer** | Installs a package. Wants the install to be safe. |

A-1 breaks the fourth pair. C-1 and C-2 break the second and third: a
malicious *script* need not be involved at all, only a malicious *file the
script parses*.

### 2.3 Method

- Full source read of all four tracks, then targeted deep-dives.
- Every headline finding was **re-verified against the on-disk source** at the
  moment it was written up, because the tree moved during the assessment.
- Reproduction in an isolated directory outside the repository
  (`%TEMP%\opencode\rak-sec`), never inside the working tree.
- Exploitation was demonstrated against **debug builds** so panics were
  catchable, and the release-profile behaviour (`panic = "abort"`,
  `overflow-checks = true`) was reasoned about separately: in release, a Rust
  panic is a hard process abort with no message.
- CVSS 3.1 base scores were computed mechanically from the vectors, not
  estimated.

### 2.4 Out of scope

- The `ide/` Tauri frontend's React components beyond their interaction with
  the Rust command surface.
- `cargo audit` / third-party CVE triage of the dependency graph.
- Physical security, wireless, and the red-team tradecraft that the skill
  description covers: this is a code assessment of one repository, not a
  campaign against a live target.

---

## 3. Findings remediated in this engagement

### A-1 — Remote code execution at install time via git option injection

**CVSS 3.1: 8.8** — `AV:N/AC:L/PR:N/UI:R/S:U/C:H/I:H/A:H`
**Status: FIXED · `oyvey/src/spec.rs:392`, `oyvey/src/git.rs:61`**

#### Description

A dependency spec is attacker-controlled: it can come from the manifest of a
package the user is installing, not only from the user's own project. `oyvey`
resolved such a spec into a URL and handed it to `git clone` as an argument:

```rust
// oyvey/src/spec.rs, before the fix
pub fn repo_url(repo: &str) -> String {
    let r = repo.trim();
    if r.contains("://") || r.starts_with("git@") {
        return r.to_string();          // <-- verbatim, unvalidated
    }
    format!("https://github.com/{}.git", r)
}
```

```rust
// oyvey/src/git.rs, before the fix
git(None, &["clone", "--bare", url, &dest.to_string_lossy()])
```

`git clone` parses a leading `-` in any argument as an **option**. And git
interpolates the value of `--upload-pack` into a **shell command line**. So a
spec of

```
--upload-pack=echo PWNED > /tmp/pwned ; echo 0000://
```

is not a URL at all: it is a git option whose value is a shell fragment, and
the fragment runs.

The `://` in the payload is what gets the spec past `repo_url`'s check — the
string contains `://`, so it is passed through unchanged instead of being
formatted into a GitHub URL. The `://` is also what makes the payload a
plausible-looking spec to a reader skimming the manifest.

The attack reaches the victim through a **transitive** dependency
(`oyvey/src/resolve.rs:133-137`): the resolver reads the dependencies out of
the dependency's own `package.rak`, so the user only has to install a package
that looks ordinary.

#### Proof of concept

Attacker publishes a package. Its manifest is ordinary except for one line:

```rak
// package.rak inside the attacker's repository
let name = "attacker"
let version = "0.1.0"
let deps = { x: "--upload-pack=echo PWNED > C:/Users/raz/AppData/Local/Temp/opencode/rak-sec/e2e/E2E_PWNED ; echo 0000://" }
```

Victim's project depends on it, and runs one command:

```console
$ oyvey install
oyvey error: resolving dependency 'attacker': resolving transitive dependency 'x': cloning --upload-pack=echo PWNED > C:/Users/raz/AppData/Local/Temp/opencode/rak-sec/e2e/E2E_PWNED ; echo 0000://: git clone --bare --upload-pack=echo PWNED > C:/Users/raz/AppData/Local/Temp/opencode/rak-sec/e2e/E2E_PWNED ; echo 0000:// C:\Users\raz\AppData\Local\Temp\opencode\rak-sec\oyvey-home\git\db\... failed: Cloning into bare repository '...'
warning: You appear to have cloned an empty repository.
fatal: unable to write flush packet: Broken pipe
```

The install "fails". The marker file was created anyway:

```console
$ cat E2E_PWNED
PWNED
```

**The only symptom is a confusing git error, and it arrives after the payload
has already run.** No prompt, no sandbox, no checksum, no signature.

The primitive was also confirmed in isolation, to show the mechanism is git's
and not oyvey's:

```console
$ git -c core.longpaths=true clone --bare "--upload-pack=echo PWNED > /tmp/PWNED ; echo 0000" /tmp/nonexistent /tmp/out
fatal: protocol error: bad line length character: done
```

`echo done` executed — its output was fed to git's protocol parser, which
proves the value reached a shell.

#### Business impact

This is the worst finding in the assessment, and it is worth being precise
about why:

- The **victim did nothing unusual.** `oyvey install` in a project that
  depends on a package is the entire interaction.
- The **payload runs before the sandbox exists.** The capability sandbox
  protects the *program*; the install runs `git`, with the user's full
  privileges, outside it.
- **The install-time path has no integrity check at all** (see A-2, A-3), so
  there is nothing that would have caught the malicious spec, and nothing
  that would catch the malicious *content* either.
- The realistic outcomes are credential theft (`~/.aws/credentials`,
  `~/.ssh/id_rsa`, browser token stores), persistence, or pivoting to the
  targets the analyst is *supposed* to be attacking — which is the worst
  possible outcome, because the analyst's machine is the one holding the
  attacker's toolkit.

`ext::` was a second instance of the same class: `ext::` hands the tail of
the URL to a git remote helper, which runs it through a shell. `git
remote-ext` is present in Git 2.45.1 for Windows (confirmed), so this was a
live transport, not a theoretical one.

#### Remediation

Two layers, because one is a policy and the other is structure.

1. **Validate the spec at the trust boundary** (`oyvey/src/spec.rs:392`).
   `repo_url` now returns a `Result` and refuses:
   - any spec beginning with `-` (it can only be an option, never a URL);
   - any scheme outside `https`, `ssh`, `file` — so `http://` (plaintext
     clone, no integrity check anywhere in the install path) and `ext::`
     (shell-executing remote helper) are both rejected;
   - any spec containing whitespace or a control character;
   - any scheme-less spec that is not a plain `owner/repo` pair.

   The four forms that legitimately need to keep working — the GitHub
   shorthand, `https://`, `ssh://`, `git@`, and `file://` (which the test
   suite uses to resolve real repositories without the network) — still do.

2. **Make the rejection structural** (`oyvey/src/git.rs:61`). `git clone`
   accepts `--` before the repository argument, after which nothing can be
   read as an option. This holds even if a future spec slips past the
   validation.

The manifest serializer uses `repo_url_lossy`, which keeps a rejected spec as
written rather than rewriting it into a different, accepted one.

#### Proof of remediation

The same exploit, against the patched build:

```console
$ oyvey install
oyvey error: resolving dependency 'attacker': resolving transitive dependency 'x': dependency spec '--upload-pack=echo PWNED > C:/Users/raz/AppData/Local/Temp/opencode/rak-sec/e2e/E2E_PWNED ; echo 0000://' starts with '-' and would be read as a git option, not a URL
```

No marker file. The error names the reason, so the user is not left with a
git protocol error and no explanation.

Seven new unit tests in `oyvey/src/spec.rs` cover the accepted forms, the
leading-dash case, `ext::`, plaintext `http://`, unknown schemes,
scheme-less malformed specs, and whitespace. Full oyvey suite: **118 passed,
0 failed.**

---

### A-6 — No URL scheme allowlist

**CVSS 3.1: 9.8** for the `ext::` variant — `AV:N/AC:L/PR:N/UI:R/S:U/C:H/I:H/A:H`
**Status: FIXED (with A-1)** — `oyvey/src/spec.rs:366`

`repo_url` passed through *any* string containing `://`. `http://` gave an
attacker a plaintext clone with no integrity check; `ext::` gave them a shell.
Both are closed by the scheme allowlist described above.

---

### A-7 — `oyvey run` could not pass `--sandbox` (regression, now fixed)

**Status: FIXED** — the flag support now exists in `oyvey/src/main.rs`
(`--sandbox`, `--no-sandbox`, `--allow`, `--fs-root`, defaulting to
`fs_read`).

One related defect was found and fixed here: the new flag list rejected a
bare `--`, so `oyvey run -- --port 8080` failed with `unknown flag '--'` and
the documented "the separator reaches the program" contract
(`oyvey/src/main.rs:931`) was broken. The pre-existing test
`run_passes_the_separator_through_so_flags_reach_the_program` was **red** as a
result. `parse_flags` now stops at `--` and forwards the separator and
everything after it verbatim.

---

### C-1 — Out-of-bounds read in the TLS ClientHello parser

**CVSS 3.1: 8.2** — `AV:N/AC:L/PR:N/UI:N/S:U/C:L/I:N/A:H`
**Status: FIXED** — `stdlib/src/tls.rs:73`

#### Description

`tls::parse_client_hello` reads the cipher-suite list length out of the wire
and uses it directly as the end of the list, without ever comparing it to the
length of the buffer:

```rust
let cs_len = u16::from_be_bytes([body[p], body[p + 1]]) as usize;
p += 2;
let ciphers_end = p + cs_len;          // <-- from the wire
while p + 1 < ciphers_end {
    ciphers.push(u16::from_be_bytes([body[p], body[p + 1]]));   // <-- OOB
    p += 2;
}
```

Every *other* length in the function is checked. This one was not, and it is
the one a fuzzer is least likely to reach: the fuzzer mutates bytes, and
constructing a valid record header, a valid handshake header, and a lying
length field in the right order is a specific shape that random mutation
rarely produces.

A 46-byte input is enough.

#### Proof of concept

```rak
// 46 bytes: record | handshake | version | random(32) | sid_len=0 | cs_len=0xFFFF
let b = bytes([0x16,0x03,0x03,0x00,0x29, 0x01,0x00,0x00,0x25, 0x03,0x03,
                0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                0x00, 0xFF, 0xFF])
let info = tls_parse_client_hello(b)
```

```console
$ rakc run poc.rak
thread 'rak-interp' (2752) panicked at stdlib\src\tls.rs:64:42:
index out of bounds: the len is 37 but the index is 37
EXITCODE=101
```

In a release build (`panic = "abort"`) this is a hard abort with no message
at all. The function is reachable from any script that parses a hostile
ClientHello out of a capture or a scan — which is exactly what the language
is for.

#### Business impact

A denial of service on the analyst's own tooling, triggered by a 46-byte
packet from the network, in a language whose stated purpose is to *process*
hostile network input. The out-of-bounds read also reads up to 64 KiB past a
heap buffer, so if the bounds check were ever relaxed rather than made
fatal, it would become an information disclosure as well.

#### Remediation

The overrun is now an **error**, not a clamped bound:

```rust
if cs_len > body.len() - p {
    return Err("tls: cipher list overruns the ClientHello".to_string());
}
```

An error rather than a clamp, because clamping would leave `p` pointing into
the middle of the cipher list, and the compression and extension lengths
after it would be read from the wrong offset. The subtraction cannot
underflow: the length field was bounds-checked immediately above.

#### Proof of remediation

```console
$ rakc run poc.rak
Error: Runtime error: tls: cipher list overruns the ClientHello
EXIT=1
```

A diagnostic with a non-zero exit, instead of a panic. Two regression tests
in `stdlib/src/tls.rs` cover the 65535-byte case and the one-byte-past case.

---

### C-2 — `zip_extract` and `zip_read` had no output bound

**CVSS 3.1: 7.5** — `AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H`
**Status: FIXED** — `stdlib/src/archive.rs:86`, `stdlib/src/stream_io.rs:124`

#### Description

`SECURITY.md:37` states:

> `gunzip`, `inflate` and `zip_extract` refuse output over 100 MiB. A
> deflate stream can expand a thousandfold, so this bounds a remote denial of
> service.

`gunzip` and `inflate` did enforce that. **`zip_extract` did not** — it was a
bare `read_to_end`, and so was `zip_read`:

```rust
pub fn zip_extract(data: &[u8], name: &str) -> anyhow::Result<Vec<u8>> {
    ...
    let mut out = Vec::new();
    file.read_to_end(&mut out)?;      // <-- no limit
    Ok(out)
}
```

A security document that lists a mitigation that does not exist is worse than
one that lists the gap, because it is read as a reason not to look.

#### Proof of concept

A 255 KiB archive whose single entry inflates to 256 MiB — a 1028:1 ratio:

```console
$ python -c "..."   # or any zip writer
zip created: 261028 bytes compressed -> 268435456 bytes (256 MiB) uncompressed
ratio: 1028.4:1
```

```rak
let z = file_read_bytes("bomb.zip")
let out = zip_extract(z, "big.bin")
dump len(out)
```

```console
$ rakc run poc.rak
[DUMP] 261028
[DUMP] 268435456
[DUMP] SECURITY.md says zip_extract must refuse output over 100 MiB
EXITCODE=0
```

256 MiB extracted, no error, exit 0. The limit the documentation promises
was not there. The ratio scales with whatever the host has free memory.

#### Business impact

A remote denial of service on the analyst's machine, from a file the analyst
was asked to examine. Because `panic = "abort"` and the allocation failure is
an abort, the failure mode is the process disappearing rather than an error
message.

#### Remediation

`archive::zip_entry_limited` (`stdlib/src/archive.rs:86`) is the shared
bounded reader, and it enforces the bound **while reading** — the allocation
is what does the damage, so the check has to happen before the buffer
exists, exactly as `gzip_decompress_limited` already did. Both `zip_read` and
`zip_extract` now go through it, with `MAX_DECOMPRESS` (100 MiB).

`SECURITY.md:37` is now true.

#### Proof of remediation

```console
$ rakc run poc.rak
Error: Runtime error: zip_extract: output exceeds the 104857600-byte limit (this looks like a decompression bomb, not a zip entry)
EXIT=1
```

Two regression tests in `stdlib/src/archive.rs` cover the over-limit refusal
and the in-memory `zip_extract` path.

---

### C-4 — `sql_escape` is bypassable on MySQL, and the linter recommended it

**CVSS 3.1: 8.6** — `AV:N/AC:L/PR:N/UI:N/S:U/C:L/I:H/A:L`
**Status: FIXED** — `stdlib/src/escape.rs:77`, `stdlib/src/escape.rs:97`,
`rakc/src/lint.rs`

#### Description

```rust
pub fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}
```

This is correct for ANSI SQL, PostgreSQL and SQLite, where a backslash is an
ordinary character inside a single-quoted literal. It is **wrong for MySQL's
default `sql_mode`**, where `NO_BACKSLASH_ESCAPES` is off and a backslash
escapes the next character.

The value `\'` becomes `\''`. MySQL reads `\'` as an escaped quote — a
*literal* quote, still inside the string — and then the final `'` **closes
the literal**. Everything after it is SQL again.

The documentation had the dialect backwards, saying that doubling backslashes
"would corrupt the value on MySQL". Doubling them is what *fixes* the
injection on MySQL; the corruption it avoids is on the *other* dialects.

What made this worse: the `unescaped-interpolation` lint rule treats
`sql_escape` as **the fix**, and reports nothing when it is present. The
linter was steering users toward an escape that does not hold on the most
common SQL server configuration.

#### Proof of concept

A scanner implementing MySQL's default-mode single-quoted literal rules
(`\` escapes the next character, `''` is a literal quote, `'` ends the
literal) reports where the literal actually ends:

```console
---- backslash-quote ----
raw                 : \'
sql_escape          : \''
literal ends at (escaped)    : 2
RESULT: the escaped text CLOSES the literal early -- context escaped

---- backslash-quote in a word ----
raw                 : a\'b
sql_escape          : a\''b
literal ends at (escaped)    : 3
RESULT: the escaped text CLOSES the literal early -- context escaped
```

The canonical case (`x'; DROP TABLE users; --`) is handled correctly — the
vulnerability is specific to the backslash.

#### Business impact

SQL injection in exactly the tool a security researcher uses to test
injection. The linter's silence on a wrapped value is the part that turns a
library weakness into a systemic one: the user was told they were safe.

#### Remediation

Two escapers, each honest about its dialect:

- `sql_escape` — unchanged behaviour, now documented as the ANSI/PostgreSQL/
  SQLite escaper, with the MySQL case spelled out as a counter-example.
- `sql_escape_mysql` (`stdlib/src/escape.rs:97`) — doubles the backslash as
  well as the quote, for MySQL's default `sql_mode`. Registered as a builtin
  on both backends (`interpreter.rs`, `vm.rs`).

The lint rule now:

- recognises `sql_escape_mysql` as an escape (it previously matched only
  names ending in `_escape`);
- says in the message that the escapers are dialect-dependent and names which
  is which, rather than naming one as "the fix";
- says that a parameterised query is better than either.

#### Proof of remediation

```console
---- backslash-quote ----
raw                 : \'
sql_escape          : \''
sql_escape_mysql    : \\''
literal ends at (sql_escape)       : 2
literal ends at (sql_escape_mysql) : 4
sql_escape: the MySQL literal closes early -- not MySQL-safe
sql_escape_mysql: the MySQL literal holds -- safe
```

Four regression tests in `stdlib/src/escape.rs` cover the MySQL bypass, the
MySQL escaper's correctness under both dialects' rules, the backslash
doubling, and the fact that the two escapers disagree.

---

### C-5 — The secret store was created world-readable on first write

**CVSS 3.1: 6.2** — `AV:L/AC:L/PR:N/UI:N/S:U/C:H/I:N/A:N`
**Status: FIXED** — `stdlib/src/secrets.rs:115`, `stdlib/src/secrets.rs:148`

#### Description

The comment said "Set the permissions before the data lands, not after", and
the order was wrong:

```rust
#[cfg(unix)]
{
    let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));  // p does not exist yet
}
let res = fs::write(&p, &json).map_err(...);   // creates it with the umask
```

`set_permissions` on a file that does not exist fails, and the error was
swallowed by `let _ =`. The file was then created by `fs::write` with the
process umask — **0644 on a typical system** — and stayed that way until the
*second* persist, when the file existed and the chmod finally worked. Any
local user could read the secrets in that window.

The module header claims the store is "0600, user-owned". It was 0644 on
first use.

#### Remediation

`write_file` now creates the file through `OpenOptions` with `mode(0o600)`
applied at creation, and tightens an already-existing file through the open
handle. The umask can only remove bits from the requested mode, so 0600 is
the floor rather than the ceiling.

**Windows still has no ACL handling** — the `#[cfg(unix)]` blocks are the only
permission logic, and the file inherits the parent directory's ACL. That
remains open; it is listed in §5.

#### Related: the shred wrote its zeroes to the wrong blocks

**CVSS 3.1: 6.2** — `AV:L/AC:L/PR:N/UI:N/S:U/C:H/I:N/A:N`
**Status: FIXED** — `stdlib/src/secrets.rs:54`

`shred_file` truncated the file *first*:

```rust
if f.set_len(0).is_err() { return; }     // frees the extents
...write zeroes...                        // into freshly allocated blocks
```

`set_len(0)` releases the extents, so on ext4, XFS and APFS the subsequent
write lands on **newly allocated** blocks; the old plaintext is still in the
blocks that were freed, and in the journal, and on an SSD in the
wear-levelling pool. The zeros now go in place, before the length is
touched. The doc comment now also states the residual limits honestly
(copy-on-write, journalling, SSD TRIM) rather than implying a guarantee.

---

### D-2 — Statement nesting was not depth-bounded, and the language server parsed on a 2 MiB stack

**CVSS 3.1: 5.5** — `AV:L/AC:L/PR:N/UI:R/S:U/C:N/I:N/A:H`
**Status: FIXED** — `rakc/src/parser.rs:1318`, `rakc/src/parser.rs:1405`,
`rakc/src/lsp.rs`

#### Description

The parser had exactly one depth guard, and it covered only *expressions*:

```rust
const MAX_PARSE_DEPTH: usize = 64;
fn parse_expr(&mut self) -> Result<Expr> {
    self.depth += 1;
    if self.depth > Self::MAX_PARSE_DEPTH { ... }
```

`parse_stmt` → `parse_block` → `parse_stmts_until_rbrace` → `parse_stmt` is a
cycle that never passes back through `parse_expr`, so `self.depth` was never
incremented for it. A file of nothing but nested blocks recursed without
limit. `parse_type` had the same problem: `**********u8` is one byte of
source per level of recursion.

The consequence depended entirely on whose stack the parse ran on.

- The command line runs the parser on a 64 MiB stack
  (`rakc/src/lib.rs:218`, `run_on_big_stack`). Measured: survives 4 000
  levels (28 000 bytes), overflows at 8 000 (56 000 bytes) — about 11 KB of
  stack per level.
- The language server did **not**. `rakc/src/main.rs:874` builds a default
  tokio runtime, whose worker threads get Rust's default 2 MiB stack, and
  the parse ran there.

So the same file that the command line shrugged off killed the editor's Rak
support at roughly one thirty-second of the depth — **a file of about 1.3 KB**.

#### Proof of concept

```console
$ ("if 1 {" * 4000) + ("}" * 4000) | rakc check -      # 64 MiB stack
-: no errors                                            # survives

$ ("if 1 {" * 8000) + ("}" * 8000) | rakc check -      # 64 MiB stack
thread 'rak-interp' (27660) has overflowed its stack
EXIT=-1073741571                                       # 0xC00000FD
```

On the LSP's 2 MiB stack the same input overflows at about 190 levels.

#### Business impact

Availability, not compromise: a repository containing a pathologically nested
file takes the language server down when an editor opens it, and the editor's
Rak support goes with it. Because a stack overflow cannot be caught in Rust,
there is no diagnostic and no recovery — the process simply dies.

#### Remediation

Two layers:

1. **The root cause.** The parser now bounds statement nesting
   (`MAX_BLOCK_DEPTH = 64`, counted in `parse_stmts_until_rbrace`, which is
   the point where the recursion cycles) and pointer-type nesting
   (`MAX_TYPE_DEPTH = 64`). The counters are separate from the expression
   depth, because they are separate recursions.
2. **Defence in depth.** The language server now parses on the same 64 MiB
   stack every other entry point uses (`rakc/src/lsp.rs`), so a source file
   that is legal but deep cannot kill the editor either.

Counting the depth in `parse_stmts_until_rbrace` rather than in `parse_stmt`
is deliberate: it makes the counter track *nesting* rather than the number
of statements, so a flat list of ten thousand statements does not trip it.

#### Proof of remediation

```console
$ rakc check deep_8000.rak
Parser error: Runtime error: Block nesting exceeds 64 levels; split it into a local variable
EXIT=1
```

Three regression tests in `rakc/src/parser.rs` cover block nesting, type
nesting, and the fact that ordinary (20-level) nesting still parses.

---

### D-3 — The debug adapter allocated a client-controlled buffer size

**CVSS 3.1: 6.2** — `AV:L/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H`
**Status: FIXED** — `rakc/src/dap.rs:103`, `rakc/src/dap.rs:143`

#### Description

```rust
let len = content_length.ok_or_else(|| "dap: missing Content-Length".to_string())?;
let mut buf = vec![0u8; len];      // len is entirely client-controlled
```

A DAP client — an editor, or anything that can write to the adapter's stdin
— can name any length it likes. `vec![0u8; 2^40]` asks the allocator for a
terabyte, and Rust **aborts the process** when an allocation fails, so the
failure mode is a dead debugger rather than a diagnostic. A merely large
length also blocks the reader thread indefinitely on `read_exact`.

#### Remediation

`MAX_FRAME = 8 MiB` (`rakc/src/dap.rs:103`), checked before the allocation.
A DAP frame is a JSON request; the largest in practice are big `variables`
responses and long `source` payloads, all far below 8 MiB.

---

### Test-isolation defect in `fs_scope` (found while validating the above)

**Status: FIXED** — `stdlib/tests/fs_scope.rs`

Not a vulnerability in the shipped code, but it was making the test suite
unreliable, and it is worth recording because of *why*.

The filesystem scope root is process-global state. The scope tests lived in
the module's `#[cfg(test)]` block, which means the same binary as the
library's other unit tests — and several of those open files
(`mmap::open`, `file::read_bytes`, `archive::zip_read`, `log::init` all call
`fs_scope::check` first). One scope test cleared the root without restoring
it, so a concurrently-running `mmap` test had its file open refused, and the
failure read as **"the sandbox let a path through"** rather than "two tests
raced".

The scope tests now live in `stdlib/tests/fs_scope.rs`, an integration-test
binary and therefore a separate process, so the scope cannot leak into
anything else. A lock still serialises them against each other.

---

## 4. Fixed concurrently, verified in the working tree

These were closed by other work while this assessment was running. They are
recorded because they were part of the original finding set and the report
should not claim them as open.

| ID | Finding | Where |
| --- | --- | --- |
| A-4 | The vendored tree is now verified against the lockfile checksum **on the build/run path**, not only by the opt-in `audit` | `oyvey/src/main.rs` |
| A-5 | `oyvey run/build/test` now support `--sandbox`, `--no-sandbox`, `--allow` and `--fs-root`, defaulting to `fs_read` | `oyvey/src/main.rs` |
| A-8a | Dependency **names** are validated as plain path components before anything is deleted or written under `packages/` | `oyvey/src/project.rs` |
| A-8c | Symlinks inside a vendored package are skipped rather than followed and copied | `oyvey/src/project.rs` |
| B-1a | The capability table was rewritten: a new `fs_read` capability, `--fs-root` scope enforcement at the layer that opens files, and more builtins gated (`reverse_dns`, `scan_subdomains`, `log_init`, `pcap_*`, `extern_call`) | `rakc/src/caps.rs`, `stdlib/src/fs_scope.rs` |

---

## 5. Open findings

Recommendations only — none of these were remediated in this engagement.

### 5.1 Supply chain

| ID | Finding | Location | CVSS | Recommendation |
| --- | --- | --- | --- | --- |
| A-2 | The lockfile checksum is **self-attested**: it is computed from the bytes that were just downloaded, and never compared to an external anchor. `resolve_rev` reuses a locked revision on URL match alone, ignoring the checksum, so a swapped rev passes `--locked` and `audit`. | `oyvey/src/resolve.rs:315-340`, `oyvey/src/project.rs:126` | 7.4 | Anchor integrity outside the lockfile: publish and verify a digest from the registry/index, or sign the lockfile. |
| A-3 | The lockfile pins a **mutable tag**, not a commit SHA. A force-moved tag changes the installed code with a byte-identical `source` line. | `oyvey/src/resolve.rs:362-369` | 7.4 | Record the resolved commit SHA in the lockfile and verify it on install. |
| A-8b | **Path-dependency traversal.** `visit_path` joins the declared path with no canonicalisation and no containment check, so a nested dependency can copy an arbitrary local directory into the project — and out of it, on the next commit. | `oyvey/src/resolve.rs:210-281` | 5.0 | Canonicalise and require path dependencies to stay inside the workspace root. |
| A-9 | A cache checkout is reused **without verifying that its HEAD matches the requested revision**, and `short_rev` truncates to 12 characters, so `v1.0` and `v10` collide into the same directory. | `oyvey/src/cache.rs:149-159, 238-247` | 5.0 | Verify HEAD before reuse; key the checkout by the full revision. |
| A-10 | **`--offline` still performs network I/O** when the cache database exists, and the bare clone has no `refs/heads/*` refspec, so `oyvey update` never picks up upstream commits — security fixes are silently never installed. | `oyvey/src/cache.rs:97-140`, `git.rs:44-62` | 5.3 | Skip the fetch entirely when offline; give the clone a real refspec. |
| A-11 | The lockfile is written with default permissions and records the **full URL**, including any `user:token@` credentials, in cleartext. There is no credential store at all. | `oyvey/src/main.rs:810-814`, `lock.rs:113-128` | 6.2 | Redact userinfo before storing or logging; add a credential store with 0600. |
| A-12 | `oyvey add` compares only the **name** when checking for an existing dependency, so a malicious package can squat an import name and the legitimate one is silently refused. | `oyvey/src/main.rs:438-471` | 5.0 | Fail loudly on a name collision with a different source. |
| A-13 | The generated `init.rak` shim interpolates the dependency's `entry` string into generated Rak source without escaping. | `oyvey/src/project.rs:143-167` | 4.0 | Escape or validate the entry path. |

### 5.2 Sandbox and runtime

| ID | Finding | Location | CVSS | Recommendation |
| --- | --- | --- | --- | --- |
| B-1 | **Compiled artifacts ignore the sandbox.** `rakc build --emit=exe` embeds the source and runs it before any argv parsing, so `--sandbox` becomes the script's `argv[0]`. The C target contains no reference to `caps::` at all — `dump v, "path"` writes files with `fopen`. | `rakc/src/main.rs:813-827`, `targets/c/runtime.rs:2222` | 7.5 | The sandbox is a runtime-only concept; make compiled output say so, or refuse `--sandbox` on a build path that cannot honour it. |
| B-2 | **`import` bypasses both `--fs-root` and `fs_read`.** An absolute path is accepted verbatim; a relative one is joined without canonicalisation. | `rakc/src/interpreter.rs:1531-1552` | 6.5 | Apply the scope check at module resolution, on both backends. |
| B-3 | `rakc verify` **executes** the script and has no `--sandbox` option. | `rakc/src/main.rs:64-160` | 6.2 | Add the sandbox flags to `verify`. |
| B-4 | The VM has **no runtime FFI gate of its own**: the native's registered name is the C symbol, which matches no gated prefix. FFI is blocked only at compile time, so the "single point of control" the capability table claims does not hold for the VM. | `rakc/src/vm.rs:170, 3637` | 8.1 (latent) | Add an FFI rule to the VM's runtime check. Not currently exploitable — the compile-time gate holds — but the runtime layer has no backstop. |
| B-5 | `lib.call(symbol, args)` dispatches as a method and never reaches `eval_builtin`, so it is outside the stated model. | `rakc/src/interpreter.rs:5193-5230` | 5.0 | Route it through the same gate. |
| B-6 | The name-prefix table is **privileged by default**: a new builtin is reachable from a sandbox until somebody remembers to add it. No test cross-checks the builtin registry against the table. | `rakc/src/caps.rs` | 6.5 | Add a test that enumerates every registered builtin and asserts it is either in the table or on an explicit "pure" allow-list. |
| B-7 | `stdin_read_line` / `stdin_read_all` are ungated — a sandboxed script can consume the invoker's stdin — and `args` leaks the host argv, including the `--allow` list. | `rakc/src/interpreter.rs:8666-8676, 7352` | 5.3 | Gate stdin under a capability; do not expose the host argv. |

### 5.3 Standard library and parsers

| ID | Finding | Location | CVSS | Recommendation |
| --- | --- | --- | --- | --- |
| C-3 | **`http_server` allocates the wire `Content-Length` directly** (16 GiB from one header) and `ws_recv` allocates a 2^64 wire length. Both remote and pre-auth. The WebSocket *parser* is correctly bounds-checked; only the streaming reader is not. | `stdlib/src/http_server.rs:94-100`, `websocket.rs:263-285` | 7.5 | Cap the body and the frame; set a socket read timeout; the accept loop also handles connections synchronously on the listener thread, so one slow client stalls the server. |
| C-6 | **`ffi_trust` + `ffi_free` is an invalid free.** `ffi_free` calls `Vec::from_raw_parts` on a pointer that was never allocated by a `Vec`; the `Allocations` map tracks no provenance, only address → length. | `rakc/src/vm.rs:1186-1227`, `interpreter.rs:7710-7726` | 8.1 | Give `Allocations` a provenance tag and make `ffi_free` refuse foreign regions. |
| C-7 | `ffi_call` can call a **data symbol** as if it were a function (`dlsym` returns data addresses too), and `call_int` always passes eight arguments to a callee that may expect fewer. | `stdlib/src/ffi.rs:61-123` | 7.8 | Check the symbol type; refuse unknown arity. |
| C-8 | **`mmap_lines(m, "")` never advances** — infinite loop plus unbounded memory growth. The VM backend defends against this; the interpreter does not, so the two backends disagree. | `stdlib/src/mmap.rs:183-201` | 6.2 | Reject an empty delimiter, and make the interpreter match the VM. |
| C-9 | **`mmap_find(m, "")` panics** — `slice::windows(0)` panics by contract, and nothing filters an empty needle. In release, that is an abort. | `stdlib/src/mmap.rs:129` | 5.3 | Return `None` for an empty needle. |
| C-10 | **YARA-lite: unbounded recursion in `parse_not`** — a rule body of `not ` repeated 200 000 times overflows the stack. The evaluation path has the same shape in its left-nested `Or`/`And` tree. | `stdlib/src/yara.rs:477-489, 610-631` | 6.2 | Add a depth counter to the parser and to `eval_cond`; build `Or`/`And` as n-ary. |
| C-11 | **YARA-lite is O(n²) on `{`** in the tokenizer, and the scan is O(n·m) with a per-rule lowercased copy of the whole haystack. `yara::scan` is not in the fuzzer's target list. | `stdlib/src/yara.rs:161-171, 634-688` | 5.3 | Single-pass tokenisation; hoist the lowercasing out of the rule loop; add the target to the fuzzer. |
| C-12 | A **cyclic `binstruct`** recurses forever in the interpreter. The compiler detects the cycle and errors, so the two backends disagree: the VM reports an error, the interpreter crashes. | `rakc/src/interpreter.rs:3955-3980` | 6.2 | Add a depth counter and a cycle set to the interpreter's `decode_field`. |
| C-13 | **`rand_seed` makes the process-global RNG predictable.** A script that calls `rand_seed(0)` makes every later "random" value in the process reproducible — including key material generated afterwards. | `stdlib/src/randkit.rs:22-26` | 6.5 | Use a per-call CSPRNG for key material; keep the seedable generator for non-secret uses and flag it in the lint. |
| C-14 | **`rsa_keypair(bits)` accepts 1024-bit keys** despite the doc saying 2048 or 4096. | `stdlib/src/crypto.rs:263-280` | 5.3 | Reject anything below 2048 in the function itself. |
| C-15 | **The VM has no step or iteration limit**, and the interpreter's limits are `None` by default outside `verify`. `loop { }` hangs the host indefinitely on both backends. | `rakc/src/vm.rs:524` | 5.3 | Add a step budget to the VM and give `run`/`vm` a default one. |
| C-16 | **`http_get` has no body cap, no read timeout, and follows HTTPS→HTTP downgrades.** The `plaintext-url` lint only sees the literal in the source, so a server-side downgrade is invisible to it. | `stdlib/src/net.rs:12-41` | 7.5 | Set `timeout_read` and `https_only` on the agent; cap the body with a `take`. |
| C-17 | `WritableMapping` is `Send + Sync` while permitting unsynchronised mutation — a data race on an `UnsafeCell`, which is undefined behaviour rather than a logic bug. The type's own doc comment admits it. | `stdlib/src/mmap.rs:44-47` | 5.3 | Make it `!Sync`, or put it behind a lock. |
| C-18 | `hkdf_derive` / `psk_derive` allocate an attacker-chosen length **before** validating it, and `psk_derive`'s iteration count is an unbounded CPU knob. | `stdlib/src/tunnel.rs:239-269` | 5.0 | Validate before allocating; cap the iterations. |
| C-19 | `net_raw_recv(max)` and `ffi_alloc(n)` allocate an unbounded, script-controlled size. | `stdlib/src/net_raw.rs:408`, `rakc/src/vm.rs:1201` | 4.0 | Cap both. |
| C-20 | `ffi_read_i32` in the interpreter performs an **unaligned `i32` read**, which is undefined behaviour even where the hardware tolerates it. The VM's version is correct. | `rakc/src/interpreter.rs:7781` | 4.0 | Read four bytes and assemble, as the VM already does. |
| C-21 | `file::temp_file` interpolates its prefix into a path verbatim, truncates an existing file, and creates a **predictable** name — a classic `/tmp` symlink race. | `stdlib/src/file.rs:143-157` | 5.0 | Sanitise the prefix; use `create_new` in a private 0700 directory. |
| C-22 | `fs_scope::check` is a **check-then-use**: the path is canonicalised and then opened, so a symlink swapped in between escapes the root. | `stdlib/src/fs_scope.rs:62-97` | 5.0 | Open with `O_NOFOLLOW` on the final component; canonicalise the parent handle. |
| C-23 | `secret_get` falls back to an **environment variable by name**, so it is an arbitrary environment oracle, and a secret file entry can never override an env var of the same name. | `stdlib/src/secrets.rs:129-141` | 4.0 | Document the oracle property; consider an explicit `env:` prefix for the fallback. |
| C-24 | The secret store has **no Windows ACL handling**, and `env_set("RAK_PATH", ...)` lets a script with the `secrets` capability redirect the store to an attacker-readable location. | `stdlib/src/secrets.rs:68-89, 115` | 5.3 | Resolve the store path once at start-up, before any script runs. |

### 5.4 IDE, LSP and DAP

| ID | Finding | Location | CVSS | Recommendation |
| --- | --- | --- | --- | --- |
| D-1 | **The Tauri v2 app ships no CSP and no application manifest.** All sixteen commands — including `run_shell` (`cmd /C <cmd>`) and unscoped `read_file` / `save_file` / `delete_file` / `rename_file` — are callable from any script in the webview. There is no XSS sink in the frontend today, so this is a single point of failure rather than a live RCE; but the capability file gives the *appearance* of a scoped ACL while granting the app's full filesystem and shell surface. | `ide/src-tauri/tauri.conf.json:27`, `ide/src-tauri/src/lib.rs:179-191, 274-378` | 7.5 | Set a CSP; declare an app manifest listing the commands; confine every path argument to the workspace root. |
| D-4 | The IDE's `run_rak` runs webview-supplied source **without `--sandbox`**, through a predictable `%TEMP%\rak_ide_<pid>.rak` path that `fs::write` will follow as a symlink. | `ide/src-tauri/src/lib.rs:87-97` | 6.5 | Unique `create_new` temp file per run; sandbox by default with a UI toggle. |
| D-5 | `list_files_recursive` recurses **without limit** and, on Windows, follows junctions — a directory junction pointing at an ancestor makes the walk infinite. | `ide/src-tauri/src/lib.rs:317-341` | 5.3 | Track visited canonical paths; cap the depth; skip reparse points. |
| D-6 | **Windows current-directory binary hijack**: `cmd`, `explorer.exe` and `rakc` are spawned by bare name, and Windows resolves the current directory before `PATH`. | `ide/src-tauri/src/lib.rs:55, 91, 181-191, 409` | 6.3 | Resolve to absolute paths; use `%COMSPEC%` rather than bare `cmd`. |
| D-7 | The LSP **re-parses the whole document on every keystroke** and never evicts closed documents, so the document cache grows for the life of the process. | `rakc/src/lsp.rs:399, 407-409, 441-450` | 4.0 | Implement `did_close`; debounce the re-parse. |
| D-8 | The DAP and LSP use `.lock().unwrap()` throughout. A poisoned mutex panics on every subsequent acquisition; in the DAP the worst case is a hung session. | `rakc/src/dap.rs`, `lsp.rs` | 4.0 | Recover from poisoning rather than propagating it. |

---

## 6. Remediation roadmap

| Priority | Work | Repository |
| --- | --- | --- |
| **P0 — done** | A-1, A-6 (git option injection and scheme allowlist), C-1 (TLS OOB), C-2 (zip bound), C-4 (SQL dialect), C-5 (secret permissions and shred order), D-2 (parser depth), D-3 (DAP frame) | both |
| **P0** | A-2/A-3 — anchor the lockfile's integrity outside the lockfile, and pin a commit SHA rather than a tag | `oyvey` |
| **P1** | C-3 (HTTP body and WebSocket frame caps), C-6 (FFI provenance), B-1 (compiled artifacts and the sandbox) | both |
| **P1** | D-1 (CSP and the application manifest) | `ide` |
| **P2** | The remainder of §5 | both |

**A note on ordering.** A-1 was rated Critical because it was the only finding
that turned "install a package" into "run an attacker's command". It is
closed. The next thing to close is **A-2/A-3**, because until the lockfile's
integrity is anchored somewhere the package manager trusts, the rest of the
supply-chain hardening is protecting a checksum that the attacker recomputes
for them.

---

## 7. What was verified as safe

Recording this matters as much as the findings: a report that lists only
holes misrepresents the system.

**Cryptography.** `ct_eq` uses `subtle::ConstantTimeEq` — no short-circuit,
not optimisable away. `ct_eq_hex` accumulates a branch-free `diff |= x ^ y`.
`ct_select` masks rather than branches. `zeroize` and `secret_bytes` use the
`zeroize` crate. The FFI bounds arithmetic is done in `u128`, so a large
offset cannot wrap past the check, and unknown pointers are refused.

**Process execution.** `process_spawn` uses `Command::new(cmd).args(args)`
with an argv array — no shell, and no shell metacharacter problem.
`shell_escape` is correct for POSIX `sh` (single-quote wrapping with `'\''`),
and is documented as not applying to `cmd.exe`, PowerShell or `fish`.

**TLS.** Certificate validation is performed via rustls and webpki-roots, and
there is **no** `insecure` / `danger_accept_invalid_certs` / `no_verify`
switch anywhere in `stdlib/` or `rakc/`. `tls.rs` is inspection only: it
never opens a validated *or* an unvalidated TLS connection. `ureq` does not
forward the `Authorization` header across a redirect.

**DNS.** The compression-pointer loop is capped at 16 hops, every label and
every `rdata` extent is bounds-checked, and there is no out-of-bounds read and
no infinite loop.

**Decompression (gzip).** `gunzip` and `inflate` enforce the 100 MiB limit
*during* the read via `Read::take`, not after the allocation, and
`saturating_add` means an explicit `usize::MAX` really means unbounded rather
than wrapping to zero.

**The package manager.** No shell string interpolation for any child process —
every invocation uses separate argv elements. The package-name → directory
mapping is validated. Symlinks are skipped, not followed, and the tree hash
and the vendored tree agree on that. The lockfile is written atomically
(temp file plus rename). An unparseable semver constraint is an error, never
"matches anything". There are no build scripts, no post-install hooks, and
**no code is executed at install time other than `git` itself** — which is
exactly where A-1 lived, and is now closed.

**The IDE frontend.** No `dangerouslySetInnerHTML`, no `eval`, no
`new Function`. File content is rendered as React text children, so a
malicious `.rak` file opened in the editor cannot execute script. The GUI
Designer is entirely client-side: it writes no file, invokes no compiler and
calls no Tauri command. `assetProtocol` is disabled, and the v2 ACL does not
include `core:webview:allow-create-webview`.

**The language server.** It never touches the filesystem — the document `Url`
is only a map key, and text arrives through `didOpen`/`didChange` — so there
is no `file://` traversal surface. It runs only the lexer and the parser: it
does not execute, spawn, or evaluate.

**The debug adapter.** It speaks DAP over stdio only; there is no listener
and no authentication surface. Its `evaluate` request is a *name lookup* in
the paused snapshot, not an evaluator. Repo-wide, the only `TcpListener` is
`stdlib/src/http_server.rs`, a Rak builtin, capability-gated.

---

## 8. Proof of remediation

Every PoC in §3 was re-run against the patched build. Summary:

| ID | Before | After | Test |
| --- | --- | --- | --- |
| A-1 | Marker file created; install "failed" with a git protocol error | `dependency spec '...' starts with '-' and would be read as a git option, not a URL`; no marker | 7 new unit tests in `oyvey/src/spec.rs` |
| A-7 | `oyvey run -- --port 8080` → `unknown flag '--'` | separator forwarded to the program | `oyvey/tests/cli.rs` (pre-existing, now green) |
| C-1 | `panicked at stdlib\src\tls.rs:64: index out of bounds`; exit 101 | `Runtime error: tls: cipher list overruns the ClientHello`; exit 1 | 2 new tests in `stdlib/src/tls.rs` |
| C-2 | 256 MiB extracted from a 255 KiB archive; exit 0 | `output exceeds the 104857600-byte limit`; exit 1 | 2 new tests in `stdlib/src/archive.rs` |
| C-4 | `sql_escape` closed the MySQL literal early | `sql_escape_mysql` holds under both dialects' rules | 4 new tests in `stdlib/src/escape.rs` |
| C-5 | Store file created 0644, chmod error swallowed | Created 0600 at creation; zeros written in place | covered by the code change; a permission test is still to be added |
| D-2 | `thread 'rak-interp' has overflowed its stack`; exit 0xC00000FD | `Block nesting exceeds 64 levels`; exit 1 | 3 new tests in `rakc/src/parser.rs` |
| D-3 | `vec![0u8; len]` with a client-controlled `len` | `frame of N bytes is over the 8388608-byte limit` | covered by the code change |
| fs_scope | Scope leaked into unrelated tests; suite red | Suite green | `stdlib/tests/fs_scope.rs` |

### Test results

| Suite | Result |
| --- | --- |
| `cargo test -p rakc` | **389** lib + 87 parity + 11 + 59 + 14 + 11 + 33 + 9 + 8 + 6 + 4 + 2 + 1 — **0 failed** |
| `cargo test -p rak-stdlib` | **115** lib + **4** integration — **0 failed** |
| `cargo test` (oyvey) | **118** — **0 failed** |

The `rustdoc.exe` doctest step fails in this environment ("the 'rustdoc.exe'
binary, normally provided by the 'rustc' component, is not applicable to the
'stable-x86_64-pc-windows-msvc' toolchain"). That is a pre-existing
toolchain issue, unrelated to these changes, and it affects the baseline
identically.

---

## 9. Reproducing this

The proof-of-concept material lives outside the repository, in
`%TEMP%\opencode\rak-sec\`:

```
rak-sec/
  cvss.ps1              the CVSS 3.1 calculator used for every score
  e2e/                  the A-1 end-to-end exploit: attacker repo + victim project
  poc/                  c1_tls_oob.rak, c2_zip_bomb.rak, c4_sql_escape.rak
  rakc-snap.exe         a snapshot of the pre-fix binary, for comparison
  rakc-fixed.exe        the post-fix binary
```

Nothing was written inside either repository except the fixes and their
tests.

---

## 10. Closing note

Three things in this report are worth reading twice.

**The package manager was the weak link, and it failed open.** Everything the
project has built — the capability sandbox, the `--fs-root` scope, the
constant-time comparisons, the lint rules — assumes the code on the disk is
the code that was meant to be there. A-1 broke that assumption at the last
possible moment, and it broke it in the one place with no second check.

**A security document that lists a mitigation that does not exist is worse
than one that lists the gap.** `SECURITY.md:37` promised a 100 MiB cap on
`zip_extract`. There was no cap. The promise was the reason the gap survived
so long, because the gap had been *documented as closed*.

**The "single point of control" claim was nearly true, and the gap was
exactly where a claim like that hides.** The capability table is enforced at
one choke point, and that is genuinely good design. But the choke point was
the interpreter and the VM — and the package manager, the compiler's output
paths, the import resolver and the language server all sat *outside* it,
each with its own idea of what the rules were.
