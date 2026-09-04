# SPDX-License-Identifier: Apache-2.0
#
# Introspection and reflection (#226): class identity, -isKindOfClass:,
# -conformsToProtocol:, @selector, SEL, -respondsToSelector: and the
# -performSelector: family.
#
# Every case here goes through the real Clang AST path, which is what
# distinguishes it from tools/oz_static/tests/{class_objects,introspection,
# reflection}.rs: those transpile a single string with oz_static alone, so a
# construct Clang itself refuses -- `Nil`, whose int-to-Class cast ARC
# disallows -- would pass there and fail a real build.

import pytest
from conftest import discover_behavior_tests

REFLECTION_TESTS = list(discover_behavior_tests("reflection"))


@pytest.mark.parametrize("m_file", REFLECTION_TESTS)
def test_reflection(m_file, compile_and_run):
    result = compile_and_run(m_file)
    assert result.returncode == 0, (
        f"FAILED: {m_file.name}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )
