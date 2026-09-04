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
│  Transpiler Unit      │  288 tests — Rust tests for oz2c
│  (tools/oz_static/    │  cargo test --manifest-path
│   tests/)             │    tools/oz_static/Cargo.toml
└───────────────────────┘
```

## Running Tests

| Command | What it runs |
|---------|-------------|
| `cargo test --manifest-path tools/oz_static/Cargo.toml` | The transpiler's own suite, 288 tests. The primary gate; it has no `just` recipe |
| `just test-behavior` | 71-case behavior corpus through `oz2c` (host). Takes `--compiler`, `--opt`, `--sanitize`, `--check-leaks` |
| `just test-adapted` | 40 adapted upstream tests |
| `just test-pal` | PAL function tests (pure C, no transpiler) |
| `just test-regression` | Regression tests only |
| `just test-all` | The host suites above + smoke |
| `just test-ci-local` | Full CI matrix locally |
| `just test-zephyr` | Zephyr integration over the committed C in `tests/zephyr/generated/` |
| `just test-hardware` | Every single-core sample flashed and run on an nRF52833DK |
| `just smoke` | Transpile-and-compile smoke test |

## Adding a New Test

- **Transpiler logic bug:** Add a test under `tools/oz_static/tests/`
- **Generated C doesn't compile:** Add a `.m` to the corpus — `corpus_parity.rs`
  compiles every case as `-std=c17 -pedantic-errors` and gates on it
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

Pipeline: `.m` → tree-sitter CST → `oz2c` → `.c` + `.h` → GCC/Clang → run

The drivers were written against the retired Python pipeline's generated ABI
(`<Class>_ozh.h`, `Class_alloc`, `OZObject_release`) and are kept unmodified;
`tests/tools/oz_static_build.py` writes a shim bridging those names to
oz_static's. That backend is readable at the `python-backend-final` tag.

## Adapted Test Sources

| Source | License | Tests |
|--------|---------|-------|
| LLVM Clang Rewriter | Apache 2.0 + LLVM | 10 |
| GNUstep libobjc2 | MIT | 8 |
| Apple objc4 | APSL (spec only) | 5 |
| Bucket B reference | Internal | 9 |
| ObjFW | LGPL-3.0 (spec only) | 5 |
| mulle-objc | BSD-3-Clause | 3 |
