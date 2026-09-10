#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Locate the Clang that produces this project's AST dumps.

One implementation, because there are now four callers and they must agree:
`tests/tools/compile_and_run.py` (both host corpora), `tests/smoke/run.py`,
`scripts/regen_zephyr_tests.py`, and the Rust suite's own harness
(`tools/oz_static/tests/common/mod.rs`, which shells out to
`--print-clang` below). A dump is a transpiler *input* -- it decides ivar
ownership and method definedness -- so which clang made it is not an
implementation detail, and two harnesses picking different ones is the
#269 failure in miniature.

The search order mirrors `objz_find_clang()` in `cmake/ObjcClang.cmake`,
so a host build and a Zephyr build reason about the same translation unit:

  1. ``OZ_CLANG``                              (explicit override)
  2. ``$ZEPHYR_SDK_INSTALL_DIR/llvm/bin``      (the tested default)
  3. ``~/.local/zephyr-sdk-*/llvm/bin``        (the SDK's own default prefix)
  4. Homebrew LLVM                             (macOS)
  5. ``clang-23..clang-19``, then ``clang``, on PATH

The two SDK entries are what this file was written for. Both Python
harnesses used to start at Homebrew, so on a machine with the SDK's LLVM
installed they still picked a different clang from the one every CMake
build uses -- silently, and with no warning of the kind #269 showed nobody
reads anyway.

Nothing here falls back to "no AST": if clang is absent the caller must
fail, because the alternative is generated C that leaks every ``id``-typed
ivar. `find_clang` raises, and `main` exits non-zero with the install
command named.
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path

#: The clang major version the AST facts are validated against, and the one
#: the Zephyr SDK 1.0.1 ships. Kept in step with `_oz_tested_clang_ver` in
#: cmake/ObjcClang.cmake.
TESTED_CLANG_VERSION = "19"

#: Newest first, and stopping at 19: below that is older than the tested
#: version, and picking it silently is what this module exists to prevent.
VERSIONED_NAMES = [f"clang-{v}" for v in range(23, 18, -1)]

INSTALL_HINT = (
    "install the Zephyr SDK's LLVM component --\n"
    "  west sdk install --llvm --version 1.0.1 -b ~/.local\n"
    "or its setup.sh -l, which puts clang at\n"
    "  $ZEPHYR_SDK_INSTALL_DIR/llvm/bin/clang.\n"
    "Then export ZEPHYR_SDK_INSTALL_DIR, or name the binary outright with\n"
    "  OZ_CLANG=/path/to/clang.\n"
    "There is no fall-back: a Clang AST dump decides which ivars ARC\n"
    "releases, and transpiling without one leaks every `id`-typed ivar."
)


class ClangNotFound(RuntimeError):
    """No usable clang, and the caller must not proceed without one."""


def _sdk_llvm_dirs() -> list[Path]:
    """Every Zephyr SDK LLVM bin directory worth looking in, best first."""
    dirs: list[Path] = []
    env_sdk = os.environ.get("ZEPHYR_SDK_INSTALL_DIR")
    if env_sdk:
        dirs.append(Path(env_sdk) / "llvm" / "bin")
    # The SDK's own default prefix, and the one `west sdk install -b
    # ~/.local` produces. Newest version first, so a machine carrying both
    # 1.0.0 and 1.0.1 gets 1.0.1.
    for prefix in (Path.home() / ".local", Path.home(), Path("/opt")):
        if not prefix.is_dir():
            continue
        dirs.extend(sorted(prefix.glob("zephyr-sdk-*/llvm/bin"), reverse=True))
    return dirs


def _fallback_dirs() -> list[Path]:
    return [
        Path("/opt/homebrew/opt/llvm/bin"),
        Path("/usr/local/opt/llvm/bin"),
        Path("/usr/bin"),
    ]


def find_clang() -> str:
    """Return the clang to dump ASTs with. Raises `ClangNotFound`."""
    env_clang = os.environ.get("OZ_CLANG")
    if env_clang:
        if shutil.which(env_clang):
            return env_clang
        raise ClangNotFound(
            f"OZ_CLANG names '{env_clang}', which is not executable.\n"
            f"Unset it to search, or point it at a real clang."
        )

    for directory in _sdk_llvm_dirs() + _fallback_dirs():
        for name in VERSIONED_NAMES + ["clang"]:
            candidate = directory / name
            if candidate.is_file() and os.access(candidate, os.X_OK):
                return str(candidate)

    for name in VERSIONED_NAMES + ["clang"]:
        found = shutil.which(name)
        if found:
            return found

    raise ClangNotFound(f"cannot find clang for the AST dump.\n{INSTALL_HINT}")


def find_clang_or_exit() -> str:
    """`find_clang`, reporting to stderr and exiting 1 instead of raising."""
    try:
        return find_clang()
    except ClangNotFound as why:
        print(f"error: {why}", file=sys.stderr)
        sys.exit(1)


def main() -> int:
    """`objz_clang.py` prints the path it would use, for a shell or Rust caller."""
    print(find_clang_or_exit())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
