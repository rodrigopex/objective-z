# SPDX-License-Identifier: Apache-2.0

import pytest
from conftest import discover_adapted_tests

GNUSTEP_TESTS = list(discover_adapted_tests("gnustep"))


@pytest.mark.parametrize("m_file", GNUSTEP_TESTS)
def test_gnustep(m_file, compile_and_run):
    result = compile_and_run(m_file)
    assert result.returncode == 0, (
        f"FAILED: {m_file.name}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )
