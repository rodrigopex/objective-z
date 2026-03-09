# Adapted Upstream Tests

Tests adapted from established ObjC test suites to validate the OZ transpiler
against battle-tested behavioral specifications.

## Methodology

Each adapted test:
1. Studies the *behavioral specification* from the upstream test
2. Rewrites as a `.m` + `_test.c` pair using OZTestBase and Unity assertions
3. Removes all runtime introspection (class_getName, objc_msgSend, etc.)
4. Uses static dispatch via OZ_SEND / direct C calls

## Directories

- `llvm_rewriter/` — Adapted from LLVM Clang Rewriter tests (Apache 2.0 + LLVM Exception)
- `gnustep/` — Adapted from GNUstep libobjc2 tests (MIT)
- `apple_spec/` — Original tests based on Apple objc4 behavioral specs (NO APSL code)

## Running

```
python3 -m pytest test/adapted/ -v
just test-adapted
```
