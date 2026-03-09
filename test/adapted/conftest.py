# SPDX-License-Identifier: Apache-2.0
#
# conftest.py - Pytest fixtures for adapted upstream tests.
# Reuses the same compile_and_run pipeline as behavior tests.

from __future__ import annotations

import os
import pathlib
import subprocess
import sys

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
ADAPTED_DIR = pathlib.Path(__file__).parent
COMPILE_AND_RUN = REPO_ROOT / "test" / "tools" / "compile_and_run.py"


def discover_adapted_tests(category: str | None = None):
    """Find all .m files under adapted test dirs."""
    for m_file in sorted(ADAPTED_DIR.rglob("*.m")):
        cat = m_file.parent.name
        if category and cat != category:
            continue
        yield pytest.param(m_file, id=f"{cat}/{m_file.stem}")


@pytest.fixture
def compile_and_run(request):
    """Return a callable that transpiles, compiles, and runs a .m test file."""
    opt = request.config.getoption("--opt")
    compiler = request.config.getoption("--compiler")
    sanitize = request.config.getoption("--sanitize")
    cflags = request.config.getoption("--cflags")
    ldflags = request.config.getoption("--ldflags")

    def _run(m_path: pathlib.Path) -> subprocess.CompletedProcess:
        cmd = [sys.executable, str(COMPILE_AND_RUN), str(m_path),
               "--opt", opt, "--compiler", compiler]
        if sanitize:
            cmd.extend(["--sanitize", sanitize])
        if cflags:
            cmd.extend(["--cflags", cflags])
        if ldflags:
            cmd.extend(["--ldflags", ldflags])
        result = subprocess.run(
            cmd,
            capture_output=True, text=True,
            cwd=str(REPO_ROOT),
            env={**os.environ, "PYTHONPATH": str(REPO_ROOT / "tools")},
            timeout=60,
        )
        return result

    return _run
