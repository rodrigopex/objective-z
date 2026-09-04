# SPDX-License-Identifier: Apache-2.0
#
# tests/conftest.py - Shared pytest options for behavior and adapted tests.


def pytest_addoption(parser):
    # The transpiler under test. Defaults to the Rust backend, which is the
    # one Zephyr builds select (CONFIG_OBJZ_BACKEND_STATIC) and the only one
    # these corpora will run on once the Python pipeline is retired; `python`
    # remains selectable while that backend still exists, so the two can be
    # compared over the same drivers.
    parser.addoption("--backend", default="static", choices=["static", "python"],
                     help="transpiler backend (default: static / oz2c)")
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
