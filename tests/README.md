# Test Architecture

## Test Pyramid

```
┌───────────────────────┐
│  Zephyr Integration   │  21 tests — real kernel on native_sim
│  (tests/zephyr/)      │  just test-zephyr
├───────────────────────┤
│  Behavior Tests       │  72 tests — transpiled C compiled & run
│  (tests/behavior/)    │  just test-behavior
├───────────────────────┤
│  Adapted Upstream     │  40 tests — LLVM/GNUstep/Apple/ObjFW/mulle/Bucket B
│  (tests/adapted/)     │  just test-adapted
├───────────────────────┤
│  PAL Tests            │  4 test files — platform abstraction layer
│  (tests/pal/)         │  just test-pal
├───────────────────────┤
│  Transpiler Unit      │  517+ tests — Python tests for each pass
│  + Golden Files       │  just test-transpiler
│  (tools/oz_transpile/ │
│   tests/)             │
└───────────────────────┘
```

## Running Tests

| Command | What it runs |
|---------|-------------|
| `just test-transpiler` | Python unit + golden tests |
| `just test-behavior` | Compiled behavior tests (host) |
| `just test-adapted` | Adapted upstream tests |
| `just test-pal` | PAL function tests |
| `just test-regression` | Regression tests only |
| `just test-all` | Everything above + smoke |
| `just test-ci-local` | Full CI matrix locally |
| `just test-zephyr` | Zephyr integration (native_sim) |
| `just smoke` | Host-side PAL smoke test |

## Adding a New Test

- **Transpiler logic bug:** Add test in `tools/oz_transpile/tests/test_*.py`
- **Generated C doesn't compile:** Add golden test in `tools/oz_transpile/tests/golden/`
- **Generated C compiles but wrong behavior:** Add `.m` + `_test.c` in `tests/behavior/cases/<category>/`
- **Bug regression:** Use `scripts/new_regression_test.sh <issue> "description"`
- **Upstream behavioral spec:** Add `.m` + `_test.c` in `tests/adapted/<source>/`
- **Zephyr-specific failure:** Add ZTEST in `tests/zephyr/src/`
- **PAL function incorrect:** Add test in `tests/pal/`

## Behavior Test Structure

Each behavior test is a `.m` + `_test.c` pair:

- `.m` — Objective-C class definitions (transpiled to C)
- `_test.c` — Unity test functions calling the generated C API
- Optional `/* oz-pool: Class=N */` comment for slab size
- Optional `/* oz-heap */` marker for heap support

Pipeline: `.m` → Clang AST → `oz_transpile` → `.c` + `.h` → GCC/Clang → run

## Adapted Test Sources

| Source | License | Tests |
|--------|---------|-------|
| LLVM Clang Rewriter | Apache 2.0 + LLVM | 10 |
| GNUstep libobjc2 | MIT | 8 |
| Apple objc4 | APSL (spec only) | 5 |
| Bucket B reference | Internal | 9 |
| ObjFW | LGPL-3.0 (spec only) | 5 |
| mulle-objc | BSD-3-Clause | 3 |
