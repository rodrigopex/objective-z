# SPDX-License-Identifier: Apache-2.0
#
# The exit-time live-object census, proved from both sides (#451).
#
# The census is wired into every generated main() by
# `tests/tools/gen_test_main.py`, so every behaviour and adapted case
# carries it. That is exactly the situation in which a broken census is
# invisible: 121 green cases are equally consistent with a census that
# works, a census that always answers zero, and a census nothing ever
# calls. `oz_slab_check_leaks` sat in the host PAL with zero call sites for
# the life of the repo and no test noticed, which is the same failure one
# step earlier.
#
# So the fixtures under `census/` pin it from both directions, and they
# live outside `cases/` because one of them must fail:
#
#   - `leaks_one_object` leaks one object. The run must exit non-zero, the
#     class must be named on stderr, and -- the part that makes it a proof
#     rather than a coincidence -- Unity itself must report zero failures,
#     so the non-zero status can only have come from the census.
#   - `balanced_release` is the same program with the release restored. It
#     must exit zero with no `LEAK:` anywhere, so the report above is not
#     unconditional.
#   - `immortal_singleton` holds a slab slot forever by design. It must
#     exit zero, and its own driver asserts the slot really is outstanding
#     at that moment -- an absence check paired with a presence check.

from __future__ import annotations

import pathlib

import pytest

CENSUS_DIR = pathlib.Path(__file__).parent / "census"


def _run(compile_and_run, stem: str):
    m_file = CENSUS_DIR / f"{stem}.m"
    assert m_file.is_file(), f"missing census fixture {m_file}"
    return compile_and_run(m_file)


def test_a_leak_is_reported_and_fails_the_run(compile_and_run):
    """The census can fail, and says which class leaked."""
    result = _run(compile_and_run, "leaks_one_object")

    # Unity passed: every assertion in the driver held. Checked first and
    # explicitly, because "exited non-zero" on its own does not say the
    # census was what objected -- a compile error, a crash or a failed
    # assertion all look the same from the status.
    assert "0 Failures" in result.stdout, (
        "expected Unity itself to pass, so that a non-zero exit can only be "
        f"the census\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert "LEAK: Leaky has 1 outstanding allocation(s)" in result.stderr, (
        "the census must name the leaking class and its count\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert result.returncode != 0, (
        "a reported leak must fail the run, or the census is a log line\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )


def test_a_balanced_program_reports_nothing(compile_and_run):
    """The control: the same shape, released, must come back clean."""
    result = _run(compile_and_run, "balanced_release")

    assert "LEAK:" not in result.stderr, (
        "a balanced program must produce no leak report\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert result.returncode == 0, (
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )


def test_an_immortal_singleton_is_not_a_leak(compile_and_run):
    """A held-forever slot is by design, and excluded by construction.

    The driver asserts the slot is genuinely outstanding at the moment the
    census answers zero, so this is not a vacuous pass.
    """
    result = _run(compile_and_run, "immortal_singleton")

    assert "LEAK:" not in result.stderr, (
        "an OZSingletonProtocol class holds its slot by design and must not "
        f"be reported\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert result.returncode == 0, (
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )


@pytest.mark.parametrize("stem", ["leaks_one_object", "balanced_release",
                                  "immortal_singleton"])
def test_the_fixtures_are_outside_the_corpus(stem):
    """The leak fixture must never be collected as a corpus case.

    Paired with the runtime checks above for the reason this whole file
    exists: if `census/` were ever moved under `cases/`, the corpus would
    go red for a reason unrelated to any change under test, and the first
    fix attempted would be to delete the assertion.
    """
    cases = pathlib.Path(__file__).parent / "cases"
    assert not list(cases.rglob(f"{stem}.m")), (
        f"{stem}.m must stay outside tests/behavior/cases/"
    )
