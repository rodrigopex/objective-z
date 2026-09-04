#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Smoke test: transpile a real .m through oz2c, then compile it on host.

The cheapest end-to-end check there is, and the point is that it takes the
same path a build does: source in, oz2c, generated C out, host compiler with
the PAL. It is not a substitute for `cargo test` or the behaviour corpus --
both do far more -- but it fails fast and for an obvious reason when the
pipeline is wired up wrong.

It used to feed the Python pipeline a *committed AST fixture*
(`tools/oz_transpile/tests/fixtures/simple_led.ast.json`), which does not
port: oz_static parses source and takes a Clang AST only as an optional
oracle, so "AST in" is not a shape it has. Pointing it at the source instead
makes it a stricter test than it was -- the parse is now part of what is
being smoke-tested, where before it was pre-baked into the fixture.
"""

import glob
import os
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
OZ2C = REPO_ROOT / "tools" / "oz_static" / "target" / "debug" / "oz2c"
SOURCE = REPO_ROOT / "tests" / "behavior" / "cases" / "lifecycle" / "alloc_returns_valid.m"
PAL_INC = REPO_ROOT / "include"
SDK_INC = REPO_ROOT / "include" / "oz_sdk"
TEST_INC = REPO_ROOT / "tests" / "behavior" / "include"
OZ_SRC = REPO_ROOT / "src"


def main() -> int:
    if not OZ2C.is_file():
        print(f"FAILED: oz2c not built at {OZ2C}")
        print("  cargo build --manifest-path tools/oz_static/Cargo.toml")
        return 1

    with tempfile.TemporaryDirectory() as outdir:
        print("=== Transpiling (oz2c) ===")
        print(f"  {SOURCE.relative_to(REPO_ROOT)}")
        result = subprocess.run(
            [str(OZ2C),
             "-I", str(SDK_INC),
             "-I", str(TEST_INC),
             "--impl-dir", str(OZ_SRC),
             str(SOURCE), outdir],
            capture_output=True, text=True)
        if result.returncode != 0:
            print("FAILED: oz2c returned", result.returncode)
            print(result.stderr)
            return 1

        print("\n=== Compiling (host, gcc) ===")
        foundation_dir = os.path.join(outdir, "Foundation")
        c_files = sorted(glob.glob(os.path.join(outdir, "*.c")))
        if os.path.isdir(foundation_dir):
            c_files = sorted(glob.glob(os.path.join(foundation_dir, "*.c"))) + c_files
        if not c_files:
            print("FAILED: oz2c produced no .c files")
            return 1
        inc_dirs = [outdir, str(PAL_INC), str(TEST_INC / "zephyr_stubs")]
        if os.path.isdir(foundation_dir):
            inc_dirs.insert(0, foundation_dir)
        for f in c_files:
            print(f"  cc {os.path.basename(f)}")
            cmd = ["gcc", "-std=c11", "-Wall", "-Werror", "-Wno-unused-function",
                   "-DOZ_PLATFORM_HOST"]
            for d in inc_dirs:
                cmd += ["-I", d]
            cmd += ["-c", f, "-o", f + ".o"]
            result = subprocess.run(cmd, capture_output=True, text=True)
            if result.returncode != 0:
                print(f"FAILED: {os.path.basename(f)}\n{result.stderr}")
                return 1

    print("\n=== Smoke test PASSED ===")
    return 0


if __name__ == "__main__":
    sys.exit(main())
