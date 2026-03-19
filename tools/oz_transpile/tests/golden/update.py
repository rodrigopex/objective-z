# SPDX-License-Identifier: Apache-2.0
#
# update.py - Regenerate golden expected files.
#
# When a test directory contains source.m (without input.ast.json),
# Clang compiles it to a temp AST, then the transpiler regenerates
# expected/.
#
# Usage: PYTHONPATH=tools python3 tools/oz_transpile/tests/golden/update.py

import json
import os
import shutil
import sys
import tempfile

from oz_transpile.__main__ import main

GOLDEN_DIR = os.path.dirname(os.path.abspath(__file__))

# Import clang_ast_dump from conftest (same directory).
sys.path.insert(0, GOLDEN_DIR)
from conftest import clang_ast_dump  # noqa: E402


def update_all():
    for entry in sorted(os.listdir(GOLDEN_DIR)):
        test_dir = os.path.join(GOLDEN_DIR, entry)
        source_m = os.path.join(test_dir, "source.m")
        ast_file = os.path.join(test_dir, "input.ast.json")

        if not os.path.isdir(test_dir):
            continue
        if not os.path.isfile(source_m) and not os.path.isfile(ast_file):
            continue

        cfg_path = os.path.join(test_dir, "config.json")
        extra_flags = []
        if os.path.isfile(cfg_path):
            with open(cfg_path) as f:
                cfg = json.load(f)
            extra_flags = cfg.get("flags", [])
            if cfg.get("expect_error"):
                print(f"SKIP: {entry} (expect_error)")
                continue

        expected_dir = os.path.join(test_dir, "expected")
        if os.path.exists(expected_dir):
            shutil.rmtree(expected_dir)
        os.makedirs(expected_dir)

        # Compile source.m to temp AST when no handcrafted AST exists
        if os.path.isfile(source_m) and not os.path.isfile(ast_file):
            with tempfile.NamedTemporaryFile(
                suffix=".ast.json", delete=False
            ) as tmp:
                tmp_ast = tmp.name
            try:
                clang_ast_dump(source_m, tmp_ast)
                argv = [
                    "--input", tmp_ast, "--outdir", expected_dir
                ] + extra_flags
                rc = main(argv)
            except RuntimeError as exc:
                print(f"FAIL: {entry} ({exc})")
                continue
            finally:
                os.unlink(tmp_ast)
        else:
            argv = [
                "--input", ast_file, "--outdir", expected_dir
            ] + extra_flags
            rc = main(argv)

        print(f"{'OK' if rc == 0 else 'FAIL'}: {entry}")


if __name__ == "__main__":
    update_all()
