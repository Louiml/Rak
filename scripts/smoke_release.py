#!/usr/bin/env python3
"""Release smoke test: the artifact must behave like the documented CLI.

Run against a freshly built binary, before it is uploaded. This exists because the
release was once built with `--features gui`, which routes `rakc run` through
`eval_in_cli_with_gui`: a worker thread runs the program while the main thread
becomes the tao event loop and opens a window.

Every other subcommand was unaffected -- `check`, `lex`, `parse`, `vm`, `lint`, `fmt`
never take that path -- so the release looked fine from every angle except the one that
matters most. `rakc run` blocked in an event loop until the window was dismissed, and a
window appeared instead of output. From a terminal it looks like a hang; from a script or
CI it *is* a hang.

Nothing caught it because the test workflow never builds with `--features gui`, so the
suite only ever exercised the console configuration. This checks the built artifact
rather than the source, with a timeout, so a front end that blocks is a failure instead of
a stuck build.

Usage:  python scripts/smoke_release.py target/release/rakc [expected_version]
"""

import os
import subprocess
import sys
import tempfile

TIMEOUT = 60

# Each case is (subcommand, program, expected substring in the combined output).
CASES = [
    ("--version", None, ""),
    ("run", 'dump "smoke"\n', "smoke"),
    ("run", 'let mut s = 0\nfor i in [1, 2, 3] { s = s + i }\ndump s\n', "[DUMP] 6"),
    # `main` is called with the script's arguments, so it declares them.
    ("run", 'fn main(argv) { dump "main-ran" }\n', "main-ran"),
    # An error inside `main` has to reach the user rather than be swallowed into an
    # exit code -- that was the CLI bug fixed in 4f4bf71.
    ("run", 'fn main(argv) { raise "boom" }\n', "boom"),
    # The checks from this release, so a regression in any of them fails the release.
    ("run", 'struct P { x: int }\ndump P { x: 1 } == P { x: 2 }\n', "false"),
    ("run", "dump 9223372036854775807 + 1", "integer overflow"),
    ("run", 'let c: int = "x"\n', "type mismatch"),
    ("run", "dump 1.0 / 0.0", "inf"),
    ("vm", 'let c: int = "x"\n', "type mismatch"),
    ("vm", 'dump sort([10, 9, 2])\n', "[2, 9, 10]"),
    ("check", 'struct P { v: int }\nlet p = P { nope: 1 }\n', "no field"),
]


def run(exe, argv, src, timeout=TIMEOUT):
    """Run with a hard timeout and stdin closed, so a blocking front end fails."""
    try:
        p = subprocess.run(
            [exe] + argv,
            input=src,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return None, "TIMED OUT after %ss (a blocking front end looks like this)" % timeout
    return p.returncode, " ".join(((p.stdout or "") + (p.stderr or "")).split())


def main():
    if len(sys.argv) < 2:
        print("usage: smoke_release.py <rakc> [expected_version]", file=sys.stderr)
        return 2
    exe = sys.argv[1]
    expected = sys.argv[2] if len(sys.argv) > 2 else None

    if not os.path.isfile(exe):
        print("FAIL: no such binary: %s" % exe, file=sys.stderr)
        return 1

    failures = 0

    if expected:
        _, out = run(exe, ["--version"], "")
        if expected not in out:
            print("FAIL: --version is %r, expected to contain %r" % (out, expected))
            failures += 1
        else:
            print("  ok  version reports %s" % expected)

    tmp = tempfile.mkdtemp(prefix="rak_smoke_")
    path = os.path.join(tmp, "smoke.rak")

    for i, (cmd, src, want) in enumerate(CASES):
        argv = [cmd] if src is None else [cmd, path]
        if src is not None:
            with open(path, "w", encoding="utf-8") as f:
                f.write(src + "\n")
        code, out = run(exe, argv, "" if src is None else src)
        label = "%s %s" % (cmd, (src or "").strip().split("\n")[0][:38])
        if out.startswith("TIMED OUT"):
            print("FAIL %-52s %s" % (label, out))
            failures += 1
            continue
        if want and want not in out:
            print("FAIL %-52s expected %r in %r" % (label, want, out[:90]))
            failures += 1
            continue
        print("  ok  %s" % label)

    print()
    if failures:
        print("smoke test FAILED: %d of %d" % (failures, len(CASES)))
        return 1
    print("smoke test passed: %d cases" % len(CASES))
    return 0


if __name__ == "__main__":
    sys.exit(main())