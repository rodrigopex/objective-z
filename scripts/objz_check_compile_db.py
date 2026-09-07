#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Gate: every .m file the build transpiles has an Objective-C entry in
compile_commands.json, every .h it can see has one too, and those entries are
ones Clang can actually parse.

Every other check in this repo reads the *generated C*. That is why the ObjC
entries could disappear for four days without a single gate turning red: the
only call site of `_objz_collect_compile_db()` lived in the Python backend's
`oz_transpile.cmake` and went with it, so `compile_commands_objc.json`, the
merge target and `.clangd` all stopped being produced (#304). Nothing compiled
any differently -- only clangd did, and clangd is not on the CI runner.

So this asks the questions no build can: is the real Objective-C in the
database, and does its command carry the flags that make it Objective-C?

Headers are checked separately and more strictly. A `.h` without an entry is
not left alone -- clangd interpolates the *generated* C twin's command, which
parses `@interface` as C and whose `-I .../oz_static_generated` shadows the
SDK's Objective-C headers with their generated plain-C namesakes, so `OZObject`
goes undeclared (#320). Both failures are silent: the database looks populated,
and only an editor shows the difference. Hence FORBIDDEN_HEADER_SUBSTRINGS
below -- an entry that carries a generated include path has regressed to
exactly the command this gate exists to keep out.

Usage: objz_check_compile_db.py <build-dir> [--expect <relative/path.m> ...]
                                            [--expect-header <path.h> ...]
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

# A header entry has to say it is a header, or Clang infers the language from
# the `.h` suffix and lands on C.
REQUIRED_HEADER_FLAGS = ("-x", "objective-c-header")

# The two ways a header entry silently reverts to the generated C command it
# was introduced to replace. `-std=c17` is that command's dialect; an include
# path into the generated tree is what shadows the SDK's ObjC headers.
FORBIDDEN_HEADER_FLAGS = ("-std=c17", "-x c-header")
FORBIDDEN_HEADER_SUBSTRINGS = ("oz_static_generated",)


def fail(msg):
    print(f"FAIL: {msg}")
    return 1


def check_headers(db, expected):
    """Header entries exist, say they are Objective-C headers, and carry no
    trace of the generated C command."""
    entries = [e for e in db if e["file"].endswith(".h")]
    if not entries:
        return fail("no .h entries in the database; clangd will interpolate a "
                    "C command from the generated output for every header")

    rc = 0
    for want in expected:
        if not any(want in e["file"] for e in entries):
            rc |= fail(f"no header entry for {want}")

    for e in entries:
        args_list = e.get("arguments") or (e.get("command") or "").split()
        joined = " ".join(args_list)
        name = e["file"]
        if not all(flag in args_list for flag in REQUIRED_HEADER_FLAGS):
            rc |= fail(f"{name}: header entry is missing "
                       f"{' '.join(REQUIRED_HEADER_FLAGS)}, so Clang will "
                       "parse it as C")
        for flag in REQUIRED_FLAGS:
            if flag not in args_list:
                rc |= fail(f"{name}: header entry is missing {flag}")
        for prefix in REQUIRED_PREFIXES:
            if not any(a.startswith(prefix) for a in args_list):
                rc |= fail(f"{name}: header entry is missing {prefix}*")
        for flag in FORBIDDEN_HEADER_FLAGS:
            if flag in joined:
                rc |= fail(f"{name}: header entry carries {flag}, which is the "
                           "generated C command it should have replaced")
        for frag in FORBIDDEN_HEADER_SUBSTRINGS:
            if frag in joined:
                rc |= fail(f"{name}: header entry references {frag}, whose "
                           "plain-C namesakes shadow the SDK's ObjC headers")

    return rc


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("build_dir", type=pathlib.Path)
    ap.add_argument("--expect", action="append", default=[],
                    help="path fragment that must appear as an .m entry")
    ap.add_argument("--expect-header", action="append", default=[],
                    help="path fragment that must appear as a .h entry")
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

    rc |= check_headers(db, args.expect_header)

    if rc == 0:
        headers = [e for e in db if e["file"].endswith(".h")]
        print(f"PASS: {len(entries)} Objective-C entries and {len(headers)} "
              "header entries, all with ObjC flags")
    return rc


if __name__ == "__main__":
    sys.exit(main())
