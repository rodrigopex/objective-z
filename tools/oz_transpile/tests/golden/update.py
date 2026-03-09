# SPDX-License-Identifier: Apache-2.0
#
# update.py - Regenerate golden expected files.
#
# Usage: PYTHONPATH=tools python3 tools/oz_transpile/tests/golden/update.py

import json
import os
import shutil
import sys

from oz_transpile.__main__ import main

GOLDEN_DIR = os.path.dirname(os.path.abspath(__file__))


def update_all():
    for entry in sorted(os.listdir(GOLDEN_DIR)):
        test_dir = os.path.join(GOLDEN_DIR, entry)
        ast_file = os.path.join(test_dir, "input.ast.json")
        if not os.path.isfile(ast_file):
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

        argv = ["--input", ast_file, "--outdir", expected_dir] + extra_flags
        rc = main(argv)
        print(f"{'OK' if rc == 0 else 'FAIL'}: {entry}")


if __name__ == "__main__":
    update_all()
