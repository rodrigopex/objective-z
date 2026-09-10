#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Gate: a collection without CONFIG_SYS_MEM_BLOCKS names the option (#366).

Zephyr's contiguous block allocator lives behind `CONFIG_SYS_MEM_BLOCKS`,
which defaults to `n`. `sys/mem_blocks.h` declares
`sys_mem_blocks_alloc_contiguous` and its `_free_` twin either way, so a
program that writes `@[ ... ]` used to compile clean and then fail at the
link with four undefined references pointing into
`include/platform/oz_platform_zephyr.h` -- a PAL header the author never
wrote, naming neither the option nor the Objective-C that needed it:

    oz_platform_zephyr.h:51:(.text.OZArray_oz_initWithItems+0x14):
        undefined reference to `sys_mem_blocks_alloc_contiguous'

`OZ_MEM_BLOCKS_DEFINE` now opens with a `BUILD_ASSERT` on the option, so
the failure is a located compile error at the item pool's one definition
site instead. This gate asserts both halves of that, because each is a way
for the assert to be wrong:

1. A program that *does* use a collection, built with the option off,
   fails with a message naming `CONFIG_SYS_MEM_BLOCKS=y`, quoting
   `OZ_MEM_BLOCKS_DEFINE`, and reaching no link at all -- so the four
   undefined references must not appear.
2. A program that uses *no* collection still builds with the option off.
   That is the half an over-broad fix breaks: put the assert in
   `oz_mem_blocks_alloc_contiguous` instead and it fires on every Zephyr
   build that includes the PAL header, because a `static inline`'s body is
   compiled whether or not anything calls it. `select SYS_MEM_BLOCKS` from
   `CONFIG_OBJZ` breaks it the other way, by making the option
   unassignable.

`-DCONFIG_SYS_MEM_BLOCKS=n` on the command line outranks the `=y` the
collection samples carry in their own `prj.conf`, so neither sample needs
copying or editing to provoke this.

Usage: objz_check_mem_blocks_diagnostic.py <collection-sample>
           --collection-free <sample> --board <board> [--zephyr-base <dir>]
"""

import argparse
import os
import pathlib
import shutil
import subprocess
import sys


# The message has to name the option and quote the construct that needs
# it. `OZ_MEM_BLOCKS_DEFINE` is what makes it *located*: GCC's expansion
# notes carry it down to the `oz_static_dispatch.c` line holding the pool.
REQUIRED = (
    "CONFIG_SYS_MEM_BLOCKS=y",
    "OZ_MEM_BLOCKS_DEFINE",
)

# The bare link failure this replaced. Its absence is the point: the
# assert has to fire while the dispatch source is being compiled, which is
# before anything is linked.
FORBIDDEN = (
    "undefined reference to `sys_mem_blocks_alloc_contiguous'",
    "undefined reference to `sys_mem_blocks_free_contiguous'",
)


def fail(msg):
    print(f"FAIL: {msg}")
    return 1


def build(sample, board, build_dir, env, target=None):
    """Configure and build `sample` with CONFIG_SYS_MEM_BLOCKS forced off."""
    cmd = ["west", "build", "-b", board, "-p", "always",
           "-d", str(build_dir), str(sample)]
    if target:
        cmd += ["-t", target]
    cmd += ["--", "-DCONFIG_SYS_MEM_BLOCKS=n"]
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env)
    return proc, proc.stdout + proc.stderr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sample", help="a sample that uses a collection")
    ap.add_argument("--collection-free", required=True,
                    help="a sample that uses none")
    ap.add_argument("--board", required=True)
    ap.add_argument("--zephyr-base", default=os.environ.get("ZEPHYR_BASE"))
    args = ap.parse_args()

    sample = pathlib.Path(args.sample).resolve()
    clean = pathlib.Path(args.collection_free).resolve()
    for path in (sample, clean):
        if not (path / "CMakeLists.txt").is_file():
            return fail(f"{path} is not a sample directory")

    env = dict(os.environ)
    if args.zephyr_base:
        env["ZEPHYR_BASE"] = args.zephyr_base

    root = sample.parent.parent
    provoked_dir = root / "build" / f"{sample.name}-memblocks-off"
    clean_dir = root / "build" / f"{clean.name}-memblocks-off"

    try:
        # 1. The collection sample must be rejected, by name.
        #
        # A full build rather than `-t app`, so "no link error" is a claim
        # about a build that would have reached the link. It does not:
        # ninja stops at the first failure, which is the dispatch source.
        proc, log = build(sample, args.board, provoked_dir, env)
        if proc.returncode == 0:
            return fail(f"{sample.name} built with CONFIG_SYS_MEM_BLOCKS=n; "
                        "the item pool's BUILD_ASSERT did not fire, so a "
                        "program using collections is still heading for a "
                        "bare link failure")

        missing = [needle for needle in REQUIRED if needle not in log]
        present = [needle for needle in FORBIDDEN if needle in log]
        if missing or present:
            print("FAIL: the failure does not name CONFIG_SYS_MEM_BLOCKS at "
                  "the item pool.")
            for needle in missing:
                print(f"  missing from the output: {needle!r}")
            for needle in present:
                print(f"  reached the link anyway: {needle!r}")
            print("--- build output (tail) ---")
            print("\n".join(log.splitlines()[-40:]))
            return 1

        # 2. The collection-free sample must be left alone.
        #
        # `-t app` is enough here and much cheaper: the assert, if it were
        # over-broad, would fire while compiling this target's generated
        # sources -- the same target that failed above.
        proc, log = build(clean, args.board, clean_dir, env, target="app")
        if proc.returncode != 0:
            print(f"FAIL: {clean.name} uses no collection, yet it no longer "
                  "builds with CONFIG_SYS_MEM_BLOCKS=n. The dependency has "
                  "been made unconditional; it belongs to programs that "
                  "build an item pool, not to every CONFIG_OBJZ build.")
            print("--- build output (tail) ---")
            print("\n".join(log.splitlines()[-40:]))
            return 1
    finally:
        shutil.rmtree(provoked_dir, ignore_errors=True)
        shutil.rmtree(clean_dir, ignore_errors=True)

    print("OK: a collection without CONFIG_SYS_MEM_BLOCKS is a located "
          "compile error naming the option, and a program without one still "
          "builds with it off")
    return 0


if __name__ == "__main__":
    sys.exit(main())
