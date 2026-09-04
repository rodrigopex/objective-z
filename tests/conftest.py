# SPDX-License-Identifier: Apache-2.0
#
# tests/conftest.py - Shared pytest options for behavior and adapted tests.


def pytest_addoption(parser):
    # Retained as a single-valued option rather than deleted: the CI jobs and
    # the `just` recipes pass it explicitly, and a reader of either should be
    # able to see which transpiler produced the C under test without knowing
    # the history. There was a `python` choice until that backend was retired
    # (see the `python-backend-final` tag).
    parser.addoption("--backend", default="static", choices=["static"],
                     help="transpiler backend (only 'static' / oz2c)")
    parser.addoption("--opt", default="O0", choices=["O0", "O2"],
                     help="Optimization level for tests")
    parser.addoption("--compiler", default="gcc", choices=["gcc", "clang"],
                     help="C compiler for tests")
    parser.addoption("--sanitize", default=None,
                     help="Sanitizers to enable (e.g. address,undefined)")
    parser.addoption("--cflags", default="",
                     help="Extra compiler flags")
    parser.addoption("--ldflags", default="",
                     help="Extra linker flags")
    parser.addoption("--check-leaks", action="store_true", default=False,
                     help="Enable leak detection via LSan")
