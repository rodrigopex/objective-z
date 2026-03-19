# SPDX-License-Identifier: Apache-2.0
#
# conftest.py - Golden-file test fixtures for oz_transpile.

import json
import os
import subprocess

import pytest

GOLDEN_DIR = os.path.dirname(__file__)
OZ_SDK_DIR = os.path.join(
    os.path.dirname(__file__), "..", "..", "..", "..", "include", "oz_sdk"
)


def clang_ast_dump(src_file, ast_file):
    """Compile an ObjC source to Clang JSON AST."""
    result = subprocess.run(
        [
            "clang",
            "-Xclang",
            "-ast-dump=json",
            "-fsyntax-only",
            "-fobjc-runtime=macosx",
            "-I",
            OZ_SDK_DIR,
            src_file,
        ],
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"clang failed on {src_file}:\n{result.stderr.decode()}"
        )
    json.loads(result.stdout)  # validate JSON
    with open(ast_file, "wb") as f:
        f.write(result.stdout)


def _discover_golden_dirs():
    """Find all subdirs of golden/ that contain source.m or input.ast.json."""
    dirs = []
    for entry in sorted(os.listdir(GOLDEN_DIR)):
        full = os.path.join(GOLDEN_DIR, entry)
        if not os.path.isdir(full):
            continue
        has_src = os.path.isfile(os.path.join(full, "source.m"))
        has_ast = os.path.isfile(os.path.join(full, "input.ast.json"))
        if has_src or has_ast:
            dirs.append(entry)
    return dirs


def _load_config(test_dir):
    """Load config.json if present, else return defaults."""
    cfg_path = os.path.join(test_dir, "config.json")
    if os.path.isfile(cfg_path):
        with open(cfg_path) as f:
            return json.load(f)
    return {}


@pytest.fixture(params=_discover_golden_dirs(), ids=_discover_golden_dirs())
def golden_case(request):
    """Yield (test_name, test_dir_path, config) for each golden test."""
    name = request.param
    test_dir = os.path.join(GOLDEN_DIR, name)
    cfg = _load_config(test_dir)
    return name, test_dir, cfg
