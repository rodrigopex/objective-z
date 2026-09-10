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
port: oz_static parses the source itself. Pointing it at the source instead
makes it a stricter test than it was -- the parse is now part of what is
being smoke-tested, where before it was pre-baked into the fixture.

The AST is back, produced rather than committed. `--ast` is no longer
optional for a source that declares a class, so this dumps one first -- and
that is the right shape for a smoke test anyway: dumping and transpiling is
what every real build path does, and this now fails fast if either half is
wired up wrong.
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
LIBC_STUBS = REPO_ROOT / "tests" / "behavior" / "include" / "stubs"
ZEPHYR_STUBS = REPO_ROOT / "tests" / "behavior" / "include" / "zephyr_stubs"

sys.path.insert(0, str(REPO_ROOT / "scripts"))
import objz_clang  # noqa: E402  (path set up above)


def main() -> int:
    if not OZ2C.is_file():
        print(f"FAILED: oz2c not built at {OZ2C}")
        print("  cargo build --manifest-path tools/oz_static/Cargo.toml")
        return 1

    clang = objz_clang.find_clang_or_exit()

    with tempfile.TemporaryDirectory() as outdir:
        print("=== Clang AST dump ===")
        print(f"  {clang}")
        ast_json = os.path.join(outdir, "smoke.ast.json")
        # The same flags `tests/tools/compile_and_run.py` dumps the whole
        # behaviour corpus with, and this source is one of its cases -- so
        # the two harnesses cannot disagree about the translation unit.
        dump = subprocess.run(
            [clang, "-Xclang", "-ast-dump=json", "-fsyntax-only",
             "-fobjc-runtime=macosx", "-fobjc-arc", "-fblocks",
             "--target=x86_64-unknown-linux-gnu",
             "-isystem", str(LIBC_STUBS),
             "-isystem", str(ZEPHYR_STUBS),
             "-I", str(TEST_INC),
             "-I", str(SDK_INC),
             "-I", str(OZ_SRC),
             str(SOURCE)],
            capture_output=True, text=True)
        if dump.returncode != 0:
            print("FAILED: clang AST dump returned", dump.returncode)
            print(dump.stderr)
            return 1
        with open(ast_json, "w") as handle:
            handle.write(dump.stdout)

        print("=== Transpiling (oz2c) ===")
        print(f"  {SOURCE.relative_to(REPO_ROOT)}")
        result = subprocess.run(
            [str(OZ2C),
             "-I", str(SDK_INC),
             "-I", str(TEST_INC),
             "--impl-dir", str(OZ_SRC),
             "--ast", ast_json,
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
