#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Gate: a configure-time oz2c failure reports what oz2c said, and its status.

`oz_static.cmake` runs oz2c at configure time to learn the generated file
list. That call used to pass `RESULT_VARIABLE` alone, so a failure produced
one line -- "oz2c failed at configure time" -- and threw away both the
transpiler's own diagnostic and the status that says whether it even ran.

That is not a cosmetic loss. It cost #308 four days in the wrong subsystem:
under a parallel twister sweep a configure step failed with *no* oz2c output,
which reads as a transpiler bug. It was not. When a child cannot be started at
all, CMake puts an error *string* in RESULT_VARIABLE and leaves both output
streams empty, and the old `if(NOT rc EQUAL 0)` fired with nothing to print.
`result:` in the message is what tells those two cases apart:

    result: 1                          -> oz2c ran and rejected the input
    result: no such file or directory  -> oz2c never started

So this configures a sample whose `.m` oz2c is guaranteed to reject and asks
whether the failure explains itself. A missing `#import` target is the
provocation, deliberately: it is an `imports.rs` resolution error rather than
a static-bar rejection, so this gate stays valid as the supported subset moves.

Usage: objz_check_oz2c_diagnostics.py <sample-dir> --board <board> [--zephyr-base <dir>]
"""

import argparse
import os
import pathlib
import shutil
import subprocess
import sys


# oz2c's own message for the provocation below, and the status line the
# capture adds. Both have to be there: the message alone still leaves a
# failed *start* indistinguishable from a rejected input.
REQUIRED = (
    "oz_static: error: cannot resolve #import",
    "result:",
)

BAD_IMPORT = '#import "OZDefinitelyNotAHeader.h"\n'


def fail(msg):
    print(f"FAIL: {msg}")
    return 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sample", help="sample directory to copy and break")
    ap.add_argument("--board", required=True)
    ap.add_argument("--zephyr-base", default=os.environ.get("ZEPHYR_BASE"))
    args = ap.parse_args()

    sample = pathlib.Path(args.sample).resolve()
    if not (sample / "CMakeLists.txt").is_file():
        return fail(f"{sample} is not a sample directory")

    # The working copy has to keep the sample's own depth below the
    # repository root: every sample's CMakeLists.txt points
    # ZEPHYR_EXTRA_MODULES at `${CMAKE_CURRENT_SOURCE_DIR}/../../` with a
    # plain `set()`, which no `-D` on the command line can override. So it
    # goes in `build/<sample>` -- two levels down, like `samples/<sample>`,
    # and gitignored -- and not in /tmp, where `../../` would name something
    # that is not this module at all.
    root = sample.parent.parent
    work = root / "build" / sample.name
    build_dir = root / "build" / f"{sample.name}-oz2c-diag"
    try:
        if work.exists():
            shutil.rmtree(work)
        work.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(sample, work)

        # Prepend, so the unresolvable import is the first thing oz2c meets
        # and the failure cannot depend on anything else in the file.
        mains = sorted(work.glob("src/*.m"))
        if not mains:
            return fail(f"no src/*.m in {sample}")
        main_m = mains[0]
        main_m.write_text(BAD_IMPORT + main_m.read_text())

        env = dict(os.environ)
        if args.zephyr_base:
            env["ZEPHYR_BASE"] = args.zephyr_base
        proc = subprocess.run(
            ["west", "build", "-b", args.board, "-p", "always",
             "-d", str(build_dir), str(work)],
            capture_output=True, text=True, env=env)

        log = proc.stdout + proc.stderr
        if proc.returncode == 0:
            return fail("the build succeeded; oz2c accepted an unresolvable "
                        "#import, so this gate provoked nothing")

        missing = [needle for needle in REQUIRED if needle not in log]
        if missing:
            print("FAIL: the configure-time failure does not explain itself.")
            for needle in missing:
                print(f"  missing from the output: {needle!r}")
            print("--- build output (tail) ---")
            print("\n".join(log.splitlines()[-40:]))
            return 1
    finally:
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(build_dir, ignore_errors=True)

    print("OK: a configure-time oz2c failure reports its status and oz2c's "
          "own diagnostic")
    return 0


if __name__ == "__main__":
    sys.exit(main())
