#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Smoke test: transpile → compile on host with PAL."""

import glob
import os
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
FIXTURE = REPO_ROOT / "tools" / "oz_transpile" / "tests" / "fixtures" / "simple_led.ast.json"
PAL_INC = REPO_ROOT / "include"


def main() -> int:
    with tempfile.TemporaryDirectory() as outdir:
        print("=== Transpiling ===")
        rc = subprocess.run(
            [sys.executable, "-m", "oz_transpile",
             "--input", str(FIXTURE),
             "--outdir", outdir,
             "--verbose"],
            env={**os.environ, "PYTHONPATH": str(REPO_ROOT / "tools")},
        ).returncode
        if rc != 0:
            print("FAILED: transpiler returned", rc)
            return 1

        print("\n=== Compiling (host, gcc) ===")
        c_files = sorted(glob.glob(os.path.join(outdir, "*.c")))
        for f in c_files:
            print(f"  cc {os.path.basename(f)}")
            result = subprocess.run(
                ["gcc", "-std=c11", "-Wall", "-Werror", "-Wno-unused-function",
                 "-DOZ_PLATFORM_HOST",
                 "-I", outdir, "-I", str(PAL_INC),
                 "-c", f, "-o", f + ".o"],
                capture_output=True, text=True,
            )
            if result.returncode != 0:
                print(f"FAILED: {os.path.basename(f)}\n{result.stderr}")
                return 1

    print("\n=== Smoke test PASSED ===")
    return 0


if __name__ == "__main__":
    sys.exit(main())
