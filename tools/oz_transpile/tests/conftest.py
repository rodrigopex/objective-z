# SPDX-License-Identifier: Apache-2.0
#
# conftest.py - Shared test helpers for oz_transpile tests.
#
# Provides Clang-based helpers that compile real .m sources through the
# full pipeline: .m -> Clang JSON AST -> collect() -> resolve().
#
# Uses -fobjc-runtime=macosx to avoid SIGSEGV in Clang's ObjC method
# name mangler on Linux (affects both Clang 18 and 20).

import json
import os
import subprocess
import tempfile

from oz_transpile.collect import collect
from oz_transpile.resolve import resolve

OZ_SDK_DIR = os.path.join(
    os.path.dirname(__file__), "..", "..", "..", "include", "oz_sdk"
)


def clang_collect(source, *, extra_files=None):
    """Compile ObjC source via Clang and run the collect pass.

    Args:
        source: ObjC source code string.
        extra_files: dict of {filename: content} for companion headers.

    Returns:
        OZModule from the collect pass.
    """
    with tempfile.TemporaryDirectory() as tmpdir:
        if extra_files:
            for name, content in extra_files.items():
                path = os.path.join(tmpdir, name)
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "w") as f:
                    f.write(content)

        src_path = os.path.join(tmpdir, "source.m")
        with open(src_path, "w") as f:
            f.write(source)

        result = subprocess.run(
            [
                "clang", "-Xclang", "-ast-dump=json", "-fsyntax-only",
                "-fobjc-runtime=macosx",
                "-I", OZ_SDK_DIR, "-I", tmpdir, src_path,
            ],
            capture_output=True,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"clang failed:\n{result.stderr.decode()}"
            )
        ast = json.loads(result.stdout)
        return collect(ast)


def clang_collect_resolve(source, **kwargs):
    """Compile ObjC source via Clang, collect, and resolve.

    Returns:
        Resolved OZModule.
    """
    mod = clang_collect(source, **kwargs)
    resolve(mod)
    return mod
