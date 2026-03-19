# SPDX-License-Identifier: Apache-2.0
#
# test_golden.py - Golden-file snapshot tests for oz_transpile.
#
# Each subdirectory of golden/ with a source.m (or input.ast.json for
# handcrafted-only cases) is a test case.  When source.m is present the
# runner compiles it through Clang to get a real AST; otherwise it falls
# back to the pre-built input.ast.json.

import difflib
import io
import os
import sys
import tempfile

import pytest

from oz_transpile.__main__ import main

from .conftest import clang_ast_dump


def _normalize(text):
    """Strip trailing whitespace per line, ensure single trailing newline."""
    lines = [line.rstrip() for line in text.splitlines()]
    return "\n".join(lines) + "\n"


def _collect_files(root):
    """Recursively collect files under root, returning {relative_path: abs_path}."""
    result = {}
    for dirpath, _, filenames in os.walk(root):
        for fname in filenames:
            abs_path = os.path.join(dirpath, fname)
            rel_path = os.path.relpath(abs_path, root)
            result[rel_path] = abs_path
    return result


def test_golden(golden_case):
    name, test_dir, cfg = golden_case
    source_m = os.path.join(test_dir, "source.m")
    fallback_ast = os.path.join(test_dir, "input.ast.json")
    expected_dir = os.path.join(test_dir, "expected")

    extra_flags = cfg.get("flags", [])
    expect_error = cfg.get("expect_error", False)
    expected_stderr = cfg.get("expected_stderr", None)

    with tempfile.TemporaryDirectory() as tmpdir:
        # Determine AST source: compile source.m when no handcrafted AST exists
        if os.path.isfile(source_m) and not os.path.isfile(fallback_ast):
            ast_file = os.path.join(tmpdir, "input.ast.json")
            clang_ast_dump(source_m, ast_file)
        else:
            ast_file = fallback_ast

        argv = ["--input", ast_file, "--outdir", tmpdir] + extra_flags

        # Pass --sources when config requests source-level extraction
        sources = cfg.get("sources", [])
        if sources:
            src_paths = [os.path.join(test_dir, s) for s in sources]
            argv.extend(["--sources"] + src_paths)
        elif cfg.get("needs_sources") and os.path.isfile(source_m):
            argv.extend(["--sources", source_m])

        captured_stderr = io.StringIO()
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

        expected_files = _collect_files(expected_dir)
        actual_files = _collect_files(tmpdir)

        missing = set(expected_files) - set(actual_files)
        assert not missing, f"{name}: missing output files: {missing}"

        for rel_path in sorted(expected_files):
            exp_path = expected_files[rel_path]
            act_path = actual_files[rel_path]

            with open(exp_path) as f:
                exp_text = _normalize(f.read())
            with open(act_path) as f:
                act_text = _normalize(f.read())

            if exp_text != act_text:
                diff = list(difflib.unified_diff(
                    exp_text.splitlines(keepends=True),
                    act_text.splitlines(keepends=True),
                    fromfile=f"expected/{rel_path}",
                    tofile=f"actual/{rel_path}",
                ))
                pytest.fail(f"{name}/{rel_path} mismatch:\n" + "".join(diff))
