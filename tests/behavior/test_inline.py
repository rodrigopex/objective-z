# SPDX-License-Identifier: Apache-2.0

import pytest
from conftest import discover_behavior_tests

INLINE_TESTS = list(discover_behavior_tests("inline"))


@pytest.mark.parametrize("m_file", INLINE_TESTS)
def test_inline(m_file, compile_and_run):
    result = compile_and_run(m_file)
    assert result.returncode == 0, (
        f"FAILED: {m_file.name}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )
