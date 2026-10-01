#!/usr/bin/env python3
"""Verify that a built Rak binary actually carries its hardening flags.

Flags in a linker command line are a claim. This reads the produced PE or ELF
and checks what the linker really emitted, so a typo like passing both
/NXCOMPAT and /NXCOMPAT:NO cannot slip through unnoticed.

Usage:
    python dist/verify_hardening.py <path-to-binary> [...]

Exit code 0 if every expected bit is set, 1 otherwise.

What is checked, and what each bit is worth:

PE (Windows)
    IMAGE_DLLCHARACTERISTICS_GUARD_CF (0x4000)
        The image is CFG-compatible and the loader runs CFG validation. This
        protects the guard dispatch table. It does NOT mean every indirect call
        is instrumented: rustc does not emit CFG check calls for Rust code, so
        an attacker who gains execution can still redirect an uninstrumented
        indirect call. Partial by construction.
    IMAGE_DLLCHARACTERISTICS_DYNAMICBASE (0x0040)   ASLR
    IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA (0x0020)  64-bit ASLR
    IMAGE_DLLCHARACTERISTICS_NX_COMPAT (0x0100)     DEP, non-executable heap
    A non-empty LOAD_CONFIG directory. Without it the CFG bit is cosmetic.

ELF (Linux)
    e_type == ET_DYN                   PIE, so ASLR has something to relocate
    PT_GNU_RELRO present               GOT is read-only after startup
    DT_BIND_NOW or DF_BIND_NOW         eager binding, which RELRO needs
    PT_GNU_STACK without PF_X          non-executable stack
    __stack_chk_fail imported          stack canary
"""

import struct
import sys

# --- PE --------------------------------------------------------------------

IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA = 0x0020
IMAGE_DLLCHARACTERISTICS_DYNAMICBASE = 0x0040
IMAGE_DLLCHARACTERISTICS_NX_COMPAT = 0x0100
IMAGE_DLLCHARACTERISTICS_GUARD_CF = 0x4000
IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG = 10
# IMAGE_LOAD_CONFIG_DIRECTORY64 with the GuardCF* fields filled in.
LOAD_CONFIG64_WITH_GUARD = 0x140


class Result:
    """Collects checks, separating the ones that can fail from the advisory ones.

    A check the toolchain cannot satisfy is worse than no check at all: it is
    either a permanently red build or something everyone learns to ignore. The
    stack canary on Linux is the concrete case here. rustc's
    x86_64-unknown-linux-gnu target does not enable -fstack-protector and
    `-C target-feature=+stack-protector` is rejected outright, so on stable it
    is unreachable. Verified against a real 7.8 MB rakc build, which contains no
    __stack_chk_fail. It is reported, and it does not fail the build.
    """

    def __init__(self):
        self.checks = []  # (label, ok, detail, required)

    def add(self, label, ok, detail="", required=True):
        self.checks.append((label, bool(ok), detail, required))

    @property
    def failed(self):
        return [c for c in self.checks if not c[1] and c[3]]

    @property
    def advisory(self):
        return [c for c in self.checks if not c[1] and not c[3]]

    def report(self, path):
        print(f"=== {path} ===")
        width = max((len(c[0]) for c in self.checks), default=0)
        for label, ok, detail, required in self.checks:
            if ok:
                mark = "ok  "
            elif required:
                mark = "FAIL"
            else:
                mark = "note"
            line = f"  [{mark}] {label.ljust(width)}"
            if detail:
                line += f"  {detail}"
            print(line)
        if self.failed:
            print(f"  -> {len(self.failed)} required hardening check(s) FAILED")
        else:
            print("  -> all required hardening checks passed")
        if self.advisory:
            print(
                f"  -> {len(self.advisory)} advisory gap(s), see above; "
                "these are not achievable with the stable toolchain"
            )
        print()
        return not self.failed


def check_pe(path, data):
    r = Result()
    if data[:2] != b"MZ":
        return None
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\0\0":
        print(f"{path}: not a PE image (bad PE signature)", file=sys.stderr)
        return None
    coff = pe_off + 4
    opt = coff + 20
    magic = struct.unpack_from("<H", data, opt)[0]
    if magic == 0x20B:  # PE32+
        dchars_at, dirs_at, dirs_count_at = 70, 112, 108
    elif magic == 0x10B:  # PE32
        dchars_at, dirs_at, dirs_count_at = 70, 96, 92
    else:
        print(f"{path}: unknown optional header magic 0x{magic:X}", file=sys.stderr)
        return None

    dchars = struct.unpack_from("<H", data, opt + dchars_at)[0]
    r.add(
        "ASLR (DYNAMICBASE)",
        dchars & IMAGE_DLLCHARACTERISTICS_DYNAMICBASE,
        f"DllCharacteristics=0x{dchars:04X}",
    )
    r.add(
        "ASLR 64-bit (HIGH_ENTROPY_VA)",
        dchars & IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA,
    )
    r.add(
        "DEP (NX_COMPAT)",
        dchars & IMAGE_DLLCHARACTERISTICS_NX_COMPAT,
    )
    r.add(
        "Control Flow Guard (GUARD_CF)",
        dchars & IMAGE_DLLCHARACTERISTICS_GUARD_CF,
        "image is CFG-compatible; call sites are NOT instrumented (rustc limitation)",
    )

    n_dirs = struct.unpack_from("<I", data, opt + dirs_count_at)[0]
    lc_size = 0
    if n_dirs > IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG:
        lc_rva, lc_size = struct.unpack_from(
            "<II", data, opt + dirs_at + IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG * 8
        )
        r.add(
            "LOAD_CONFIG directory present",
            lc_size >= LOAD_CONFIG64_WITH_GUARD,
            f"size=0x{lc_size:X} (want >= 0x{LOAD_CONFIG64_WITH_GUARD:X})",
        )
    else:
        r.add("LOAD_CONFIG directory present", False, f"only {n_dirs} data directories")
    return r


# --- ELF --------------------------------------------------------------------

ET_DYN = 3
PT_DYNAMIC = 2
PT_GNU_STACK = 0x6474E551
PT_GNU_RELRO = 0x6474E552
PF_X = 0x1
DT_BIND_NOW = 24
DT_FLAGS = 30
DF_BIND_NOW = 0x8
DT_NULL = 0


def check_elf(path, data):
    r = Result()
    if data[:4] != b"\x7fELF":
        return None
    is64 = data[4] == 2
    little = data[5] == 1
    end = "<" if little else ">"
    if is64:
        e_type = struct.unpack_from(end + "H", data, 16)[0]
        e_phoff = struct.unpack_from(end + "Q", data, 32)[0]
        e_phentsize = struct.unpack_from(end + "H", data, 54)[0]
        e_phnum = struct.unpack_from(end + "H", data, 56)[0]
        p_off_at, p_filesz_at, p_flags_at = 8, 32, 4
    else:
        e_type = struct.unpack_from(end + "H", data, 16)[0]
        e_phoff = struct.unpack_from(end + "I", data, 28)[0]
        e_phentsize = struct.unpack_from(end + "H", data, 42)[0]
        e_phnum = struct.unpack_from(end + "H", data, 44)[0]
        p_off_at, p_filesz_at, p_flags_at = 4, 16, 24

    r.add("PIE / ASLR (ET_DYN)", e_type == ET_DYN, f"e_type={e_type}")

    has_relro = False
    stack_exec = None
    dyn_off = dyn_size = 0
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type = struct.unpack_from(end + "I", data, off)[0]
        if p_type == PT_GNU_STACK:
            p_flags = struct.unpack_from(end + "I", data, off + p_flags_at)[0]
            stack_exec = bool(p_flags & PF_X)
        elif p_type == PT_GNU_RELRO:
            has_relro = True
        elif p_type == PT_DYNAMIC:
            # The dynamic segment is a program header; its p_offset/p_filesz
            # give the Elf*_Dyn array. Reading fixed file offsets 120/128 here
            # would land inside the program headers themselves.
            width = "Q" if is64 else "I"
            dyn_off = struct.unpack_from(end + width, data, off + p_off_at)[0]
            dyn_size = struct.unpack_from(end + width, data, off + p_filesz_at)[0]

    r.add("RELRO (PT_GNU_RELRO)", has_relro)
    r.add(
        "non-executable stack (PT_GNU_STACK)",
        stack_exec is False,
        "absent means the linker default, which is non-executable"
        if stack_exec is None
        else f"PF_X={stack_exec}",
    )

    # Walk .dynamic for BIND_NOW. Full RELRO needs it, otherwise the GOT stays
    # writable until the first lazy resolution.
    bind_now = False
    if dyn_off and dyn_size and dyn_off + dyn_size <= len(data):
        width = "Q" if is64 else "I"
        ent = 16 if is64 else 8
        for o in range(dyn_off, dyn_off + dyn_size, ent):
            tag = struct.unpack_from(end + width, data, o)[0]
            if tag == DT_NULL:
                break
            val = struct.unpack_from(end + width, data, o + ent // 2)[0]
            if tag == DT_BIND_NOW:
                bind_now = True
            elif tag == DT_FLAGS and (val & DF_BIND_NOW):
                bind_now = True
    r.add("eager binding (BIND_NOW, needed for full RELRO)", bind_now)

    r.add(
        "stack canary (__stack_chk_fail)",
        b"__stack_chk_fail" in data,
        "unreachable on stable: rustc's linux-gnu target does not enable "
        "-fstack-protector and rejects target-feature=+stack-protector; needs "
        "nightly -Z stack-protector or a C object built with GCC",
        required=False,
    )
    return r


def _selftest():
    """Exercise both parsers on synthetic headers.

    A verifier that always passes is worse than no verifier, and the ELF path
    only ever runs on the Linux CI runner. So build minimal headers by hand and
    assert the parsers reach the right conclusion, including the negative cases.
    """
    failures = []

    def check(label, got, want):
        if got != want:
            failures.append(f"{label}: got {got!r}, want {want!r}")

    # --- synthetic PE32+ with CFG + ASLR + DEP ---
    pe = bytearray(0x200)
    pe[0:2] = b"MZ"
    struct.pack_into("<I", pe, 0x3C, 0x80)
    pe[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", pe, 0x80 + 24, 0x20B)  # magic PE32+
    struct.pack_into(
        "<H", pe, 0x80 + 24 + 70, 0x4000 | 0x0040 | 0x0020 | 0x0100
    )  # dchars
    struct.pack_into("<I", pe, 0x80 + 24 + 108, 16)  # NumberOfRvaAndSizes
    struct.pack_into("<I", pe, 0x80 + 24 + 112 + 10 * 8, 0x1000)  # LOAD_CONFIG rva
    struct.pack_into("<I", pe, 0x80 + 24 + 112 + 10 * 8 + 4, 0x140)  # size
    r = check_pe("synthetic", bytes(pe))
    if r is None:
        failures.append("PE parser returned None on a valid synthetic header")
    else:
        check("PE all-pass", r.failed, [])
        labels = {c[0]: c[1] for c in r.checks}
        check("PE guard_cf set", labels["Control Flow Guard (GUARD_CF)"], True)

    # --- same PE but with GUARD_CF cleared: must be reported as a failure ---
    # Keep every other bit set so exactly one required check flips.
    struct.pack_into("<H", pe, 0x80 + 24 + 70, 0x0040 | 0x0020 | 0x0100)
    r2 = check_pe("synthetic", bytes(pe))
    check("PE missing-guard_cf detected", len(r2.failed), 1)

    # --- synthetic ELF64: PIE, RELRO, BIND_NOW, non-exec stack, canary ---
    # Layout: ehdr(64) | phdrs(3*56) | dynamic(2*16) | strtab text
    DYN = 64 + 3 * 56
    STR = DYN + 2 * 16
    elf = bytearray(STR + 32)
    elf[0:4] = b"\x7fELF"
    elf[4] = 2  # 64-bit
    elf[5] = 1  # little endian
    struct.pack_into("<H", elf, 16, ET_DYN)
    struct.pack_into("<H", elf, 54, 56)  # e_phentsize
    struct.pack_into("<H", elf, 56, 3)  # e_phnum
    struct.pack_into("<Q", elf, 32, 64)  # e_phoff
    # phdr 0: PT_GNU_STACK, flags = RW (no X)
    struct.pack_into("<I", elf, 64 + 0 * 56 + 0, PT_GNU_STACK)
    struct.pack_into("<I", elf, 64 + 0 * 56 + 4, 0x6)
    # phdr 1: PT_GNU_RELRO
    struct.pack_into("<I", elf, 64 + 1 * 56 + 0, PT_GNU_RELRO)
    # phdr 2: PT_DYNAMIC, pointing at the Elf64_Dyn array
    struct.pack_into("<I", elf, 64 + 2 * 56 + 0, PT_DYNAMIC)
    struct.pack_into("<Q", elf, 64 + 2 * 56 + 8, DYN)  # p_offset
    struct.pack_into("<Q", elf, 64 + 2 * 56 + 32, 32)  # p_filesz
    # dynamic: DT_BIND_NOW, DT_NULL
    struct.pack_into("<Q", elf, DYN + 0, DT_BIND_NOW)
    struct.pack_into("<Q", elf, DYN + 8, 0)
    struct.pack_into("<Q", elf, DYN + 16, DT_NULL)
    struct.pack_into("<Q", elf, DYN + 24, 0)
    elf[STR : STR + 15] = b"__stack_chk_fail"
    # Keep a clean copy: the cases below mutate `elf` in place to break bits.
    good_elf = bytes(elf)
    r3 = check_elf("synthetic", bytes(elf))
    if r3 is None:
        failures.append("ELF parser returned None on a valid synthetic header")
    else:
        # The synthetic ELF embeds the symbol, so the canary check passes. That
        # proves the advisory branch is not taken when a canary really exists.
        check("ELF no required failures", r3.failed, [])
        labels = {c[0]: c[1] for c in r3.checks}
        check("ELF canary present", labels["stack canary (__stack_chk_fail)"], True)
        check("ELF nothing advisory", len(r3.advisory), 0)
        check("ELF PIE", labels["PIE / ASLR (ET_DYN)"], True)
        check("ELF RELRO", labels["RELRO (PT_GNU_RELRO)"], True)
        check("ELF BIND_NOW", labels["eager binding (BIND_NOW, needed for full RELRO)"], True)

    # --- ELF with an executable stack, no RELRO and no BIND_NOW: three
    #     required bits break at once ---
    struct.pack_into("<I", elf, 64 + 0 * 56 + 4, 0x7)  # RWX
    struct.pack_into("<I", elf, 64 + 1 * 56 + 0, 0)  # clobber RELRO phdr type
    struct.pack_into("<Q", elf, DYN + 16, DT_NULL)
    struct.pack_into("<Q", elf, DYN + 0, DT_NULL)  # remove BIND_NOW
    struct.pack_into("<Q", elf, DYN + 8, 0)
    struct.pack_into("<Q", elf, DYN + 24, 0)
    r4 = check_elf("synthetic", bytes(elf))
    labels = {c[0]: c[1] for c in r4.checks}
    check("ELF exec-stack detected", labels["non-executable stack (PT_GNU_STACK)"], False)
    check("ELF missing-RELRO detected", labels["RELRO (PT_GNU_RELRO)"], False)
    check("ELF missing-BIND_NOW detected", labels["eager binding (BIND_NOW, needed for full RELRO)"], False)
    check("ELF three required failures", len(r4.failed), 3)

    # --- an ELF with no canary at all: advisory, never a required failure ---
    # Start from the clean copy so the earlier mutations do not leak in.
    r5 = check_elf("synthetic", good_elf[0:STR])
    check("ELF missing canary is not required", len(r5.failed), 0)
    check(
        "ELF missing canary is advisory",
        any(c[0] == "stack canary (__stack_chk_fail)" for c in r5.advisory),
        True,
    )

    # --- non-binary input must be rejected, not silently accepted ---
    check("garbage rejected", check_pe("x", b"not a binary at all"), None)
    check("garbage rejected (elf)", check_elf("x", b"nope"), None)

    if failures:
        print("verify_hardening.py SELF-TEST FAILED:")
        for f in failures:
            print(f"  {f}")
        return False
    print("verify_hardening.py self-test passed (PE + ELF, positive and negative)")
    return True


def main(argv):
    if len(argv) > 1 and argv[1] == "--self-test":
        return 0 if _selftest() else 1
    if len(argv) < 2:
        print(__doc__)
        return 2
    all_ok = True
    for path in argv[1:]:
        with open(path, "rb") as fh:
            data = fh.read()
        result = check_pe(path, data) or check_elf(path, data)
        if result is None:
            print(f"{path}: unrecognized binary format (not PE or ELF)", file=sys.stderr)
            all_ok = False
            continue
        if not result.report(path):
            all_ok = False
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
