# SPDX-License-Identifier: Apache-2.0
#
# test_golden.py - Golden-file snapshot tests for oz_transpile.
#
# Each subdirectory of golden/ with an input.ast.json is a test case.
# The runner transpiles the input, then diffs every file in expected/
# against actual output. Mismatch produces a unified diff.

import difflib
import io
import os
import tempfile

import pytest

from oz_transpile.__main__ import main


def _normalize(text):
    """Strip trailing whitespace per line, ensure single trailing newline."""
    lines = [line.rstrip() for line in text.splitlines()]
    return "\n".join(lines) + "\n"


def test_golden(golden_case):
    name, test_dir, cfg = golden_case
    ast_file = os.path.join(test_dir, "input.ast.json")
    expected_dir = os.path.join(test_dir, "expected")

    extra_flags = cfg.get("flags", [])
    expect_error = cfg.get("expect_error", False)
    expected_stderr = cfg.get("expected_stderr", None)

    with tempfile.TemporaryDirectory() as tmpdir:
        argv = ["--input", ast_file, "--outdir", tmpdir] + extra_flags

        captured_stderr = io.StringIO()
        import sys
        old_stderr = sys.stderr
        sys.stderr = captured_stderr
        try:
            rc = main(argv)
        finally:
            sys.stderr = old_stderr
        stderr_text = captured_stderr.getvalue()

        if expect_error:
            assert rc != 0, f"{name}: expected transpiler to fail but got rc=0"
            if expected_stderr:
                assert expected_stderr in stderr_text, (
                    f"{name}: expected stderr to contain '{expected_stderr}', "
                    f"got: {stderr_text!r}")
            return

        assert rc == 0, f"{name}: transpiler failed with rc={rc}"

        expected_files = set(os.listdir(expected_dir))
        actual_files = set(os.listdir(tmpdir))

        missing = expected_files - actual_files
        assert not missing, f"{name}: missing output files: {missing}"

        for fname in sorted(expected_files):
            exp_path = os.path.join(expected_dir, fname)
            act_path = os.path.join(tmpdir, fname)

            with open(exp_path) as f:
                exp_text = _normalize(f.read())
            with open(act_path) as f:
                act_text = _normalize(f.read())

            if exp_text != act_text:
                diff = list(difflib.unified_diff(
                    exp_text.splitlines(keepends=True),
                    act_text.splitlines(keepends=True),
                    fromfile=f"expected/{fname}",
                    tofile=f"actual/{fname}",
                ))
                pytest.fail(f"{name}/{fname} mismatch:\n" + "".join(diff))
