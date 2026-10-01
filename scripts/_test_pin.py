import io, os, subprocess, shutil, sys

BASE = os.path.abspath('.tmp-pin-test')

script = io.open(r"scripts/pin-ide-version.sh", encoding="utf-8").read()

FIXTURES = {
    "ide/src-tauri/Cargo.toml":
        '[package]\nname = "rak-ide"\nversion = "8.0.0"\nedition = "2021"\n\n[dependencies]\nserde = "1"\n',
    "ide/src-tauri/tauri.conf.json":
        '{\n  "productName": "Rak",\n  "version": "8.0.0",\n  "identifier": "dev.rak.ide"\n}\n',
    "ide/package.json":
        '{\n  "name": "rak-ide",\n  "version": "8.0.0",\n  "private": true\n}\n',
}


def tree(root):
    for f in sorted(FIXTURES):
        print("      " + f + ": " + io.open(os.path.join(root, f), encoding="utf-8").read().replace("\n", " | ").strip())


ok = True
for tag in ["v8.1.1", "v9.0.0-rc.1", "v10.2.3"]:
    shutil.rmtree(BASE, ignore_errors=True)
    d = BASE
    os.makedirs(d, exist_ok=True)
    for f, v in FIXTURES.items():
        p = os.path.join(d, f)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        # CRLF, because that is what the real ide/src-tauri/Cargo.toml uses, and an LF
        # fixture is what let a broken sed pattern pass this test.
        io.open(p, "w", encoding="utf-8", newline="\r\n").write(v)

    # A wrapper file, not `bash -c`: passing the script as a Windows command-line
    # argument mangles every double quote in it, which is what made the first
    # attempt of this look like a bug in the sed expressions.
    posix_d = d.replace(chr(92), "/")
    wrapper = os.path.join(d, "run.sh")
    io.open(wrapper, "w", encoding="utf-8", newline="\n").write(
        '#!/usr/bin/env bash\nTAG=%s\nexport TAG\ncd "%s"\n%s\n' % (tag, posix_d, script)
    )
    r = subprocess.run(["bash", "run.sh"], cwd=d, capture_output=True, text=True)
    print("  tag=%-12s exit=%d" % (tag, r.returncode))
    tree(d)
    if r.returncode != 0:
        ok = False
        print("      STDERR: " + r.stderr.strip()[-300:])
    else:
        expect = tag[1:]
        for f in FIXTURES:
            body = io.open(os.path.join(d, f), encoding="utf-8").read()
            if expect not in body:
                ok = False
                print("      !! %s never got %s" % (f, expect))
        # nothing else may be disturbed
        ct = io.open(os.path.join(d, "ide/src-tauri/Cargo.toml"), encoding="utf-8").read()
        for keep in ['name = "rak-ide"', "edition = \"2021\"", "serde = \"1\""]:
            if keep not in ct:
                ok = False
                print("      !! collateral damage, lost: " + keep)
        pj = io.open(os.path.join(d, "ide/package.json"), encoding="utf-8").read()
        if '"name": "rak-ide"' not in pj or '"private": true' not in pj:
            ok = False
            print("      !! collateral damage in package.json")
    shutil.rmtree(d, ignore_errors=True)

print("  PASS: every tag pinned, nothing else touched" if ok else "  FAIL")
sys.exit(0 if ok else 1)
