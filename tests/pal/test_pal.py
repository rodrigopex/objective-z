# SPDX-License-Identifier: Apache-2.0

import pytest

from .conftest import discover_pal_tests

PAL_TESTS = list(discover_pal_tests())


@pytest.mark.parametrize("c_file", PAL_TESTS)
def test_pal(c_file, compile_and_run_c):
    result = compile_and_run_c(c_file)
    assert result.returncode == 0, (
        f"FAILED: {c_file.name}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )
