# Test Architecture

## Test Pyramid

```
┌───────────────────────┐
│  Zephyr Integration   │  24 cases, 7 suites — real kernel on native_sim
│  (tests/zephyr/)      │  just test-zephyr
├───────────────────────┤
│  Behavior Tests       │  81 tests — transpiled C compiled & run
│  (tests/behavior/)    │  just test-behavior
├───────────────────────┤
│  Adapted Upstream     │  37 tests — LLVM/GNUstep/Apple/ObjFW/mulle/Bucket B
│  (tests/adapted/)     │  just test-adapted
├───────────────────────┤
│  PAL Tests            │  4 test files — platform abstraction layer
│  (tests/pal/)         │  just test-pal
├───────────────────────┤
│  Transpiler Unit      │  Rust tests for oz2c — the primary gate
│  (tools/oz2c/    │  cargo test --manifest-path
│   tests/)             │    tools/oz2c/Cargo.toml
└───────────────────────┘
```

## Running Tests

| Command | What it runs |
|---------|-------------|
| `cargo test --manifest-path tools/oz2c/Cargo.toml` | The transpiler's own suite. The primary gate; it has no `just` recipe. Its size is not recorded here — see below |
| `just test-behavior` | 81-case behavior corpus through `oz2c` (host). Takes `--compiler`, `--opt`, `--sanitize`, `--check-leaks` |
| `just test-adapted` | 37 adapted upstream tests |
| `just test-pal` | PAL function tests (pure C, no transpiler) |
| `just test-regression` | Regression tests only |
| `just test-all` | The host suites above + smoke |
| `just test-ci-local` | Full CI matrix locally |
| `just test-zephyr` | Zephyr integration over the committed C in `tests/zephyr/generated/` |
| `just test-hardware` | Every single-core sample flashed and run on an nRF52833DK |
| `just smoke` | Transpile-and-compile smoke test |

## Why the Rust suite's size is not written down

Every other number here is derived from a glob and is gated
(`corpus_parity.rs`): 81 behaviour cases, 37 adapted, 24 ztest cases in 7
suites. Those earn their place, because a documented count is what lets a
reader tell a **narrowed** sweep from a complete one — a wrong glob does not
fail, it returns a smaller set, every case in it passes, and the run reports a
clean number for a corpus it never opened (#400).

The Rust suite's total earns nothing by the same test. Nobody sweeps it with a
glob, no gate reads the figure, and it moves on nearly every PR that adds a
test — so recording it means a three-site edit per PR and a conflict point with
every sibling branch, for a number that is stale within hours.

That is the pathology #499 removed from `tools/oz2c/Cargo.toml`'s version, for
the same reasons in the same words: *write-only, colliding with every sibling
branch, verifying nothing.* #601 wrote **953** and #603 measured **954** an
hour later, having added one test — the second of two moves in one afternoon.

Run `cargo test --manifest-path tools/oz2c/Cargo.toml` when the number is
wanted. It is authoritative and takes a couple of minutes.

## Adding a New Test

- **Transpiler logic bug:** Add a test under `tools/oz2c/tests/`
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
`tests/tools/oz2c_build.py` writes a shim bridging those names to
oz2c's. That backend is readable at the `python-backend-final` tag.

## Adapted Test Sources

| Source | License | Tests |
|--------|---------|-------|
| LLVM Clang Rewriter | Apache 2.0 + LLVM | 10 |
| GNUstep libobjc2 | MIT | 8 |
| Apple objc4 | APSL (spec only) | 2 |
| Bucket B reference | Internal | 9 |
| ObjFW | LGPL-3.0 (spec only) | 5 |
| mulle-objc | BSD-3-Clause | 3 |
