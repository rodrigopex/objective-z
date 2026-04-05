# SPDX-License-Identifier: Apache-2.0
#
# tests/conftest.py - Shared pytest options for behavior and adapted tests.


def pytest_addoption(parser):
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
