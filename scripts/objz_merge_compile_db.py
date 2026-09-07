#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Merge ObjC compile commands into CMake's compile_commands.json, and
synthesise an entry for every Objective-C header those commands can see.

The merge half is the original job: CMake writes C entries, the AST-dump loop
collects `.m` entries into compile_commands_objc.json, and the two have to end
up in one database.

The header half exists because a `.h` with no entry of its own is not left
alone -- clangd interpolates one, and the nearest match is the *generated* C
twin under `oz_static_generated/`. That command is wrong for a header twice
over: it is `-x c-header -std=c17`, so every `@interface` is a syntax error;
and its `-I .../oz_static_generated` precedes the SDK's, so the generated
plain-C `Foundation/Foundation.h` shadows the Objective-C one and `OZObject` is
never declared. An exact filename match always beats interpolation, so an entry
per header removes both mechanisms rather than compensating for them (#320).

This is not the globbing fallback #304 retired. That one globbed the *module's*
directories, so an out-of-tree app was never covered, and it reused the first
entry's argument list verbatim -- `-c <source> -o <object>` included -- so every
synthetic entry named the wrong input. Here the directories come from the `-I`
flags of the real commands and are clipped to `--root`, and each entry gets its
own input with the object output stripped.

Usage:
  objz_merge_compile_db.py <compile_commands.json> <objc_commands.json>
                           [--root DIR ...] [--build-dir DIR]
"""

import argparse
import json
import os
import pathlib
import sys


def command_of(entry):
    """An entry carries either `arguments` or `command`; normalise to a list."""
    if "arguments" in entry:
        return list(entry["arguments"])
    return (entry.get("command") or "").split()


def include_dirs(args):
    """The -I directories in an argument list, joined or separated."""
    dirs = []
    for i, arg in enumerate(args):
        if not arg.startswith("-I"):
            continue
        if len(arg) > 2:
            dirs.append(arg[2:])
        elif i + 1 < len(args):
            dirs.append(args[i + 1])
    return dirs


def header_dirs(objc, roots, build_dir):
    """Directories to scan for headers: the -I paths of the real Objective-C
    commands, kept only when they live under a root and outside the build tree.

    Deriving them from the commands is what keeps an out-of-tree app covered
    without naming it, and clipping to the roots is what keeps Zephyr's own
    include tree -- thousands of headers that are not ours and already have
    proper C entries -- out of the database.
    """
    keep = set()
    for entry in objc:
        for raw in include_dirs(command_of(entry)):
            path = os.path.realpath(raw)
            if not os.path.isdir(path):
                continue
            if build_dir and (path == build_dir
                              or path.startswith(build_dir + os.sep)):
                continue
            if any(path == r or path.startswith(r + os.sep) for r in roots):
                keep.add(path)
    return sorted(keep)


def header_entry(header, source):
    """Clone `source`'s command for `header`, as a header rather than a TU.

    The object output has to go: an entry that still says `-o <foo.m.o>` names
    a file this command does not produce, and a consumer that believes the
    database would overwrite the real object with a header's.
    """
    args = command_of(source)
    for flag in ("-c", "-o"):
        while flag in args:
            i = args.index(flag)
            del args[i:i + 2]
    args += ["-x", "objective-c-header", "-c", header]
    return {"directory": source["directory"], "file": header, "arguments": args}


def synthesise_headers(objc, roots, build_dir):
    """One entry per header, cloned from the `.m` that shares its stem.

    Falling back to any collected command matters for the headers that own no
    implementation at all -- a protocol or a category contract. They still need
    the Objective-C dialect and the same include order, and any real entry
    carries both.
    """
    by_stem = {}
    for entry in objc:
        stem = pathlib.Path(entry["file"]).stem
        by_stem.setdefault(stem, entry)

    entries = []
    for directory in header_dirs(objc, roots, build_dir):
        for path in sorted(pathlib.Path(directory).rglob("*.h")):
            source = by_stem.get(path.stem) or objc[0]
            entries.append(header_entry(str(path), source))
    return entries


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("db", type=pathlib.Path)
    ap.add_argument("objc", type=pathlib.Path)
    ap.add_argument("--root", action="append", default=[],
                    help="only scan include dirs under this prefix; repeatable")
    ap.add_argument("--build-dir", default=None,
                    help="never scan include dirs under here (the generated C)")
    args = ap.parse_args()

    db = json.loads(args.db.read_text()) if args.db.exists() else []
    objc = json.loads(args.objc.read_text()) if args.objc.exists() else []

    if not objc:
        return 0

    roots = [os.path.realpath(r) for r in args.root]
    build_dir = os.path.realpath(args.build_dir) if args.build_dir else None

    headers = synthesise_headers(objc, roots, build_dir) if roots else []

    # Replacing by filename is what makes this idempotent: the target runs on
    # every build, and CMake only rewrites compile_commands.json on configure,
    # so a second run must not append a second copy of everything.
    generated = objc + headers
    seen = {e["file"] for e in generated}
    merged = [e for e in db if e["file"] not in seen] + generated

    args.db.write_text(json.dumps(merged, indent=2) + "\n")
    print(f"ObjZ: {len(objc)} Objective-C and {len(headers)} header entries "
          f"in {args.db.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
