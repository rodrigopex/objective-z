#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Gate: every .m file the build transpiles has an Objective-C entry in
compile_commands.json, and that entry is one Clang can actually parse.

Every other check in this repo reads the *generated C*. That is why the ObjC
entries could disappear for four days without a single gate turning red: the
only call site of `_objz_collect_compile_db()` lived in the Python backend's
`oz_transpile.cmake` and went with it, so `compile_commands_objc.json`, the
merge target and `.clangd` all stopped being produced (#304). Nothing compiled
any differently -- only clangd did, and clangd is not on the CI runner.

So this asks the questions no build can: is the real Objective-C in the
database, and does its command carry the flags that make it Objective-C?

Usage: objz_check_compile_db.py <build-dir> [--expect <relative/path.m> ...]
"""

import argparse
import json
import pathlib
import sys

# Without these an .m entry is worse than useless: it looks authoritative and
# then reports `blocks support disabled` on every OZFN, leaves each ivar
# unparsed, and resolves `#import "Foo.h"` against the wrong include order.
REQUIRED_FLAGS = ("-fobjc-arc", "-fblocks")
REQUIRED_PREFIXES = ("-fobjc-runtime=", "-fconstant-string-class=", "--target=")

# `-w` belongs to the AST dump, whose diagnostics are noise to the transpiler.
# An IDE entry carrying it silences the diagnostics that are the whole point.
FORBIDDEN_FLAGS = ("-w",)


def fail(msg):
    print(f"FAIL: {msg}")
    return 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("build_dir", type=pathlib.Path)
    ap.add_argument("--expect", action="append", default=[],
                    help="path fragment that must appear as an .m entry")
    args = ap.parse_args()

    db_path = args.build_dir / "compile_commands.json"
    if not db_path.exists():
        return fail(f"{db_path} does not exist")

    objc_path = args.build_dir / "compile_commands_objc.json"
    if not objc_path.exists():
        return fail(f"{objc_path} does not exist -- _objz_write_compile_db() "
                    "never ran, so nothing collected an entry")

    db = json.loads(db_path.read_text())
    entries = [e for e in db if e["file"].endswith(".m")]
    if not entries:
        exts = sorted({e["file"].rsplit(".", 1)[-1] for e in db})
        return fail(f"no .m entries among {len(db)} commands (extensions: "
                    f"{', '.join(exts)}); clangd will interpolate a C command "
                    "from the generated output instead")

    rc = 0
    for want in args.expect:
        if not any(want in e["file"] for e in entries):
            rc |= fail(f"no entry for {want}")

    for e in entries:
        args_list = e.get("arguments") or (e.get("command") or "").split()
        name = e["file"]
        for flag in REQUIRED_FLAGS:
            if flag not in args_list:
                rc |= fail(f"{name}: entry is missing {flag}")
        for prefix in REQUIRED_PREFIXES:
            if not any(a.startswith(prefix) for a in args_list):
                rc |= fail(f"{name}: entry is missing {prefix}*")
        for flag in FORBIDDEN_FLAGS:
            if flag in args_list:
                rc |= fail(f"{name}: entry carries {flag}, which suppresses "
                           "the diagnostics the IDE exists to show")

    if rc == 0:
        print(f"PASS: {len(entries)} Objective-C entries, all with ObjC flags")
    return rc


if __name__ == "__main__":
    sys.exit(main())
