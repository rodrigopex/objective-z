# SPDX-License-Identifier: Apache-2.0
#
# conftest.py - Golden-file test fixtures for oz_transpile.

import json
import os

import pytest

GOLDEN_DIR = os.path.dirname(__file__)


def _discover_golden_dirs():
    """Find all subdirs of golden/ that contain input.ast.json."""
    dirs = []
    for entry in sorted(os.listdir(GOLDEN_DIR)):
        full = os.path.join(GOLDEN_DIR, entry)
        if os.path.isdir(full) and os.path.isfile(os.path.join(full, "input.ast.json")):
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
