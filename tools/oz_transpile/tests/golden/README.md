# Golden-File Tests

Snapshot tests for the oz_transpile transpiler. Each subdirectory is a test
case with an input AST and expected C output files.

## Structure

```
golden/
  test_name/
    input.ast.json   # Hand-crafted Clang JSON AST (required)
    source.m         # Human-readable ObjC equivalent (optional, documentation only)
    config.json      # Extra CLI flags or error expectations (optional)
    expected/        # Expected transpiler output files
      OZObject.h
      OZObject.c
      oz_dispatch.h
      ...
```

## Adding a new golden test

1. Create a directory under `golden/` with a descriptive name
2. Write `input.ast.json` (hand-crafted simplified Clang AST)
3. Optionally add `source.m` for documentation
4. Run `just update-golden` to generate `expected/`
5. Review the generated files — they become the test's ground truth
6. Commit the entire directory

To generate an AST from a `.m` file for reference:
```bash
just ast-dump path/to/file.m
```
Then strip noise fields (`loc`, `range`, `id`) to create a minimal `input.ast.json`.

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
  "flags": ["--strict"]
}
```
The runner checks the transpiler returns a nonzero exit code.

## config.json reference

```json
{
  "flags": ["--pool-sizes", "OZLed=8"],
  "expect_error": false
}
```
- `flags`: extra CLI arguments passed to the transpiler
- `expect_error`: if true, transpiler must fail (nonzero exit code)

## Relationship to behavior tests

Golden tests validate transpiler **output stability** (text diff).
Phase 2 behavior tests will **compile and run** the output to verify correctness.
Both are complementary — golden tests catch unintended changes, behavior tests
catch semantic bugs.
