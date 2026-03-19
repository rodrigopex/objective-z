# Golden-File Tests

Snapshot tests for the oz_transpile transpiler. Each subdirectory is a test
case with an ObjC source (or handcrafted AST) and expected C output files.

## Structure

```
golden/
  test_name/
    source.m         # ObjC source — compiled via Clang at test time (primary)
    input.ast.json   # Handcrafted Clang JSON AST (fallback for edge cases)
    config.json      # Extra CLI flags or error expectations (optional)
    expected/        # Expected transpiler output files
      OZObject.h
      OZObject.c
      oz_dispatch.h
      ...
```

When `source.m` exists and `input.ast.json` does not, the test runner
compiles `source.m` through Clang (`-ast-dump=json`) at test time.
When only `input.ast.json` exists, it is used directly — this covers
edge cases that Clang cannot represent (circular inheritance, duplicate
methods).

## Adding a new golden test

1. Create a directory under `golden/` with a descriptive name
2. Write `source.m` with the ObjC code to transpile
3. Import only the SDK headers you need (`#import <Foundation/OZObject.h>`)
4. Run `just update-golden` to generate `expected/`
5. Review the generated files — they become the test's ground truth
6. Commit the entire directory

## Updating golden files

After an intentional transpiler change:
```bash
just update-golden      # regenerate all expected/ dirs
git diff                # review changes
just test-transpiler    # verify tests pass
```

## Error / negative tests

Add `config.json` to the test directory:
```json
{
  "expect_error": true,
  "expected_stderr": "error message substring"
}
```
The runner checks the transpiler returns a nonzero exit code.

## config.json reference

```json
{
  "flags": ["--pool-sizes", "OZLed=8"],
  "expect_error": false,
  "expected_stderr": "substring",
  "sources": ["source.m"],
  "needs_sources": true
}
```
- `flags`: extra CLI arguments passed to the transpiler
- `expect_error`: if true, transpiler must fail (nonzero exit code)
- `expected_stderr`: substring that must appear in stderr (error tests)
- `sources`: source files passed via `--sources` for generic extraction
- `needs_sources`: auto-pass `source.m` via `--sources` (generic tests)

## Relationship to behavior tests

Golden tests validate transpiler **output stability** (text diff).
Phase 2 behavior tests will **compile and run** the output to verify correctness.
Both are complementary — golden tests catch unintended changes, behavior tests
catch semantic bugs.
