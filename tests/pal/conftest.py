# SPDX-License-Identifier: Apache-2.0
#
# conftest.py - Pytest fixtures for PAL host-side unit tests.

from __future__ import annotations

import os
import pathlib
import shlex
import subprocess
import sys
import tempfile

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
PAL_INC = REPO_ROOT / "include"
UNITY_DIR = REPO_ROOT / "tests" / "lib" / "unity"
GEN_MAIN = REPO_ROOT / "tests" / "tools" / "gen_test_main.py"
PAL_DIR = pathlib.Path(__file__).parent


def discover_pal_tests():
    """Find all test_*.c files in tests/pal/."""
    for c_file in sorted(PAL_DIR.glob("test_*.c")):
        yield pytest.param(c_file, id=c_file.stem)


@pytest.fixture
def compile_and_run_c(request):
    """Return a callable that compiles and runs a pure C test file with Unity."""
    opt = request.config.getoption("--opt")
    compiler = request.config.getoption("--compiler")
    sanitize = request.config.getoption("--sanitize")
    cflags = request.config.getoption("--cflags")
    ldflags = request.config.getoption("--ldflags")

    def _run(c_path: pathlib.Path) -> subprocess.CompletedProcess:
        with tempfile.TemporaryDirectory(prefix="oz_pal_") as tmpdir:
            tmpdir = pathlib.Path(tmpdir)

            # Generate test_main.c
            test_main = tmpdir / "test_main.c"
            result = subprocess.run(
                [sys.executable, str(GEN_MAIN),
                 "--scan", str(c_path),
                 "--output", str(test_main)],
                capture_output=True, text=True)
            if result.returncode != 0:
                return subprocess.CompletedProcess(
                    args=result.args, returncode=1,
                    stdout=result.stdout,
                    stderr=f"gen_test_main failed:\n{result.stderr}")

            # Compile
            test_bin = tmpdir / "test_bin"
            cc_flags = [compiler, "-std=c11", f"-{opt}",
                        "-Wall", "-Werror", "-Wno-unused-function",
                        "-DOZ_PLATFORM_HOST",
                        "-I", str(PAL_INC),
                        "-I", str(UNITY_DIR),
                        str(c_path), str(test_main),
                        str(UNITY_DIR / "unity.c")]
            if sanitize:
                cc_flags.extend([f"-fsanitize={sanitize}",
                                 "-fno-omit-frame-pointer"])
            if cflags:
                cc_flags.extend(shlex.split(cflags))
            if ldflags:
                cc_flags.extend(shlex.split(ldflags))
            cc_flags.extend(["-o", str(test_bin)])

            result = subprocess.run(cc_flags, capture_output=True, text=True)
            if result.returncode != 0:
                return subprocess.CompletedProcess(
                    args=result.args, returncode=1,
                    stdout=result.stdout,
                    stderr=f"Compilation failed:\n{result.stderr}")

            # Run
            env = dict(os.environ)
            if sanitize:
                env["ASAN_OPTIONS"] = "detect_leaks=0"

            return subprocess.run(
                [str(test_bin)],
                capture_output=True, text=True, timeout=30, env=env)

    return _run
