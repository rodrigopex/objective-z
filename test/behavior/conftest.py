# SPDX-License-Identifier: Apache-2.0
#
# conftest.py - Pytest fixtures for behavior tests.

from __future__ import annotations

import os
import pathlib
import subprocess
import sys

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
CASES_DIR = pathlib.Path(__file__).parent / "cases"
COMPILE_AND_RUN = REPO_ROOT / "test" / "tools" / "compile_and_run.py"


def pytest_addoption(parser):
    parser.addoption("--opt", default="O0", choices=["O0", "O2"],
                     help="Optimization level for behavior tests")


def discover_behavior_tests(category: str | None = None):
    """Find all .m files under cases/, optionally filtered by category."""
    for m_file in sorted(CASES_DIR.rglob("*.m")):
        cat = m_file.parent.name
        if category and cat != category:
            continue
        yield pytest.param(m_file, id=f"{cat}/{m_file.stem}")


@pytest.fixture
def compile_and_run(request):
    """Return a callable that transpiles, compiles, and runs a .m test file."""
    opt = request.config.getoption("--opt")

    def _run(m_path: pathlib.Path) -> subprocess.CompletedProcess:
        result = subprocess.run(
            [sys.executable, str(COMPILE_AND_RUN), str(m_path), "--opt", opt],
            capture_output=True, text=True,
            cwd=str(REPO_ROOT),
            env={**os.environ, "PYTHONPATH": str(REPO_ROOT / "tools")},
            timeout=60,
        )
        return result

    return _run
