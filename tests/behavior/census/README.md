# `tests/behavior/census/` — fixtures that prove the census can fail

These are **not** corpus cases, and they deliberately sit outside
`tests/behavior/cases/` so `conftest.discover_behavior_tests` (which globs
`cases/**/*.m`) never picks them up. One of them leaks on purpose and must
exit non-zero; putting it under `cases/` would turn every corpus run red.

They exist because of the failure mode #451 is about: **a census that
reports zero leaks because nothing ever calls it is the same defect as no
census at all**, and both look identical from a green test run. So each
property is pinned from both sides:

| fixture | what it proves |
|---|---|
| `leaks_one_object.m` | the census reports a leak, names the class, and fails the run — while Unity itself passes, so the failure is provably the census and not an assertion |
| `balanced_release.m` | the same program with the release restored exits zero and prints no `LEAK:` — so the leak report is not unconditional |
| `immortal_singleton.m` | a class conforming to `OZSingletonProtocol` holds its slab slot forever and is *not* reported — the one honest complication #451 names |

`tests/behavior/test_census.py` drives all three.
