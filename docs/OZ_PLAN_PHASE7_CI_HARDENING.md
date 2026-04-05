# Phase 7 — CI Hardening, Hardware Verification, and Regression Workflow

## Current State

CI pipeline has 6 jobs:
1. `python-tests` — transpiler unit + golden tests with coverage
2. `behavior-tests` — GCC×Clang × O0×O2 matrix (4 combinations)
3. `sanitizers` — ASan + UBSan with GCC O0
4. `c-coverage` — gcov on transpiled C
5. `zephyr-integration` — native_sim + twister
6. `generated-freshness` — staleness check on transpiled Zephyr test sources

Missing from CI:
- Hardware build verification (compile-only for Cortex-M, verify PAL inlining)
- RISC-V build verification
- Regression test workflow (auto-create regression test from bug fix)
- Adapted test job (currently not in CI, only runnable locally via `just test-adapted`)
- PAL test job (currently not in CI, only runnable locally via `just test-pal`)
- Mutation testing or fuzz testing

## Goal

Harden the CI pipeline with hardware build verification, add missing test
jobs, establish a regression test workflow, and add the PAL inlining proof
as an automated check. After this phase, the CI provides complete confidence
that every PR produces correct, zero-cost code for both host and embedded
targets.

## Deliverables

### Step 1 — Add adapted and PAL tests to CI

Currently `just test-adapted` and `just test-pal` are local-only. Add them
to the CI pipeline.

Add to `.github/workflows/ci.yml`:

```yaml
  adapted-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'
      - name: Install system dependencies
        run: |
          wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key | sudo tee /etc/apt/trusted.gpg.d/llvm.asc
          echo "deb http://apt.llvm.org/$(lsb_release -cs)/ llvm-toolchain-$(lsb_release -cs)-20 main" | sudo tee /etc/apt/sources.list.d/llvm-20.list
          sudo apt-get update && sudo apt-get install -y clang-20
      - name: Install Python dependencies
        run: pip install pytest jinja2 tree-sitter tree-sitter-objc
      - name: Run adapted upstream tests
        run: PYTHONPATH=tools python3 -m pytest tests/adapted/ -v

  pal-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'
      - name: Install Python dependencies
        run: pip install pytest
      - name: Run PAL tests
        run: python3 -m pytest tests/pal/ -v
```

Acceptance criteria:
- [ ] `adapted-tests` job runs on every PR
- [ ] `pal-tests` job runs on every PR
- [ ] Both pass on a clean checkout

### Step 2 — Add hardware build verification job

This does NOT run tests on real hardware — it compiles for Cortex-M and
RISC-V targets and verifies the PAL inlining proof via `objdump`.

```yaml
  hw-build-check:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        board: [mps2/an385, qemu_riscv32]
    env:
      ZEPHYR_SDK_INSTALL_DIR: /home/runner/zephyr-sdk-1.0.0
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - name: Install system deps
        run: |
          sudo apt-get update
          sudo apt-get install -y ninja-build gperf device-tree-compiler gcc-multilib g++-multilib
      - name: Install west
        run: pip install west
      - name: Cache Zephyr SDK
        uses: actions/cache@v4
        with:
          path: /home/runner/zephyr-sdk-1.0.0
          key: zephyr-sdk-1.0.0-${{ matrix.board }}
      - name: Install Zephyr SDK
        run: |
          if [ ! -d "/home/runner/zephyr-sdk-1.0.0" ]; then
            wget -q https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v1.0.0/zephyr-sdk-1.0.0_linux-x86_64_minimal.tar.xz
            tar xf zephyr-sdk-1.0.0_linux-x86_64_minimal.tar.xz -C /home/runner
            /home/runner/zephyr-sdk-1.0.0/setup.sh -t arm-zephyr-eabi -t riscv64-zephyr-elf -h -c
          fi
      - name: Initialize workspace
        run: |
          west init -l .
          west update --narrow --fetch-opt=--depth=1
      - name: Install Zephyr deps
        run: pip install -r ../zephyr/scripts/requirements-base.txt natsort tabulate
      - name: Build sample (compile only)
        run: west build -b ${{ matrix.board }} samples/hello_world
      - name: Verify PAL inlining (ARM only)
        if: contains(matrix.board, 'mps2')
        run: |
          OBJDUMP=$(find $ZEPHYR_SDK_INSTALL_DIR -name "arm-zephyr-eabi-objdump" | head -1)
          PAL_CALLS=$($OBJDUMP -d build/zephyr/zephyr.elf | grep -c 'bl.*oz_slab_alloc\|bl.*oz_atomic\|bl.*oz_spin' || true)
          if [ "$PAL_CALLS" -ne 0 ]; then
            echo "FAIL: PAL functions not inlined ($PAL_CALLS call sites found)"
            $OBJDUMP -d build/zephyr/zephyr.elf | grep 'bl.*oz_'
            exit 1
          fi
          echo "PASS: All PAL functions inlined (zero call sites)"
```

Acceptance criteria:
- [ ] Builds succeed for ARM (mps2/an385) and RISC-V (qemu_riscv32)
- [ ] PAL inlining check passes on ARM (zero `oz_*` call sites in objdump)
- [ ] Job is in CI but `continue-on-error: true` initially until stable

### Step 3 — Establish regression test workflow

Create a documented workflow for turning bug fixes into regression tests.

**`tests/behavior/cases/regression/README.md`:**

```markdown
# Regression Tests

Each file in this directory prevents a specific bug from reoccurring.

## Naming Convention

    issue_NNN_short_description.m

where NNN is the GitHub issue number (or a sequential local number
if no issue exists).

## Required Header

Every regression test MUST start with:

    /*
     * Regression test for issue #NNN.
     *
     * Bug: <one-line description of the incorrect behavior>
     * Fix: <one-line description of what was fixed>
     * Commit: <hash of the fix commit>
     */

## Workflow

1. Reproduce the bug in a minimal .m file
2. Verify it fails (transpiler error, wrong output, or crash)
3. Fix the transpiler
4. Name the file `issue_NNN_description.m`, add the header
5. Verify it passes: `just test-behavior`
6. Commit the test alongside the fix
```

Create a script `scripts/new_regression_test.sh`:

```bash
#!/bin/bash
# Usage: ./scripts/new_regression_test.sh 42 "nil struct return"
# Creates: tests/behavior/cases/regression/issue_042_nil_struct_return.m
NUM=$(printf "%03d" "$1")
DESC=$(echo "$2" | tr ' ' '_')
FILE="tests/behavior/cases/regression/issue_${NUM}_${DESC}.m"
cat > "$FILE" << EOF
/*
 * Regression test for issue #$1.
 *
 * Bug: TODO — describe the incorrect behavior
 * Fix: TODO — describe what was fixed
 * Commit: TODO — hash of the fix commit
 */
#include "unity.h"

void test_issue_${NUM}(void) {
    /* TODO: Add test body */
    TEST_PASS();
}
EOF
echo "Created: $FILE"
```

Acceptance criteria:
- [ ] `tests/behavior/cases/regression/README.md` exists
- [ ] `scripts/new_regression_test.sh` creates correctly named files
- [ ] Script is executable (`chmod +x`)

### Step 4 — Add `just test-all` target

Currently there's `just test-all-transpiler` but it doesn't include PAL
tests. Create a comprehensive target:

```just
test-all:
    just test-transpiler
    just test-behavior
    just test-adapted
    just test-pal
    just smoke
```

Also add a `test-ci-local` that mimics the full CI matrix locally (useful
before pushing):

```just
test-ci-local:
    just test-all
    just test-behavior -- --compiler=clang
    just test-behavior -- --opt=O2
    just test-behavior -- --sanitize=address,undefined
```

Acceptance criteria:
- [ ] `just test-all` runs everything
- [ ] `just test-ci-local` mimics CI matrix
- [ ] Both pass on a clean checkout

### Step 5 — Add LeakSanitizer to CI

LeakSanitizer (LSan) catches memory leaks in transpiled code — particularly
important for ARC correctness. It's included with ASan on Linux but can
also run standalone.

Add a dedicated leak check job or extend the sanitizer job:

```yaml
      - name: Run with LeakSanitizer
        run: >
          PYTHONPATH=tools
          python3 -m pytest tests/behavior/ -v
          --compiler=gcc
          --opt=O0
          --sanitize=leak
```

If any behavior test leaks memory, that's a transpiler ARC bug — the
transpiler failed to insert a release somewhere.

Acceptance criteria:
- [ ] LSan job runs in CI
- [ ] All behavior tests pass without leaks
- [ ] Any leak is flagged as a transpiler ARC bug (not a test issue)

### Step 6 — Add transpiler test coverage badge

Add a Codecov badge to README.md showing Python test coverage of the
transpiler code and C test coverage of transpiled output.

```markdown
[![Transpiler Coverage](https://codecov.io/gh/rodrigopex/objective-z/branch/main/graph/badge.svg?flag=python)](https://codecov.io/gh/rodrigopex/objective-z)
[![C Coverage](https://codecov.io/gh/rodrigopex/objective-z/branch/main/graph/badge.svg?flag=c-transpiled)](https://codecov.io/gh/rodrigopex/objective-z)
```

Requires Codecov token to be set as a repository secret. If not using
Codecov, generate local HTML reports via:

```bash
# Python coverage
PYTHONPATH=tools python3 -m pytest tools/oz_transpile/tests/ --cov=oz_transpile --cov-report=html:htmlcov-python

# C coverage
python3 -m pytest tests/behavior/ --compiler=gcc --opt=O0 --cflags=--coverage --ldflags=--coverage
gcovr --html-details -o htmlcov-c/index.html
```

Acceptance criteria:
- [ ] Coverage reports generated (local or Codecov)
- [ ] Transpiler Python coverage ≥ 80%
- [ ] C transpiled coverage ≥ 60%

### Step 7 — Document the complete test architecture

Create `tests/README.md` that explains the full test pyramid, how to run
each layer, and where to add new tests:

```markdown
# Test Architecture

## Test Pyramid

    ┌───────────────────────┐
    │  Zephyr Integration   │  21+ tests — real kernel on native_sim
    │  (tests/zephyr/)      │  `just test-zephyr`
    ├───────────────────────┤
    │  Behavior Tests       │  46+ tests — transpiled C compiled & run
    │  (tests/behavior/)    │  `just test-behavior`
    ├───────────────────────┤
    │  Adapted Upstream     │  12+ tests — LLVM/GNUstep/Apple specs
    │  (tests/adapted/)     │  `just test-adapted`
    ├───────────────────────┤
    │  PAL Tests            │  4 test files — platform abstraction layer
    │  (tests/pal/)         │  `just test-pal`
    ├───────────────────────┤
    │  Transpiler Unit      │  501+ tests — Python tests for each pass
    │  + Golden Files       │  `just test-transpiler`
    │  (tools/oz_transpile/ │
    │   tests/)             │
    └───────────────────────┘

## Adding a new test

- **Transpiler logic bug:** Add test in `tools/oz_transpile/tests/test_*.py`
- **Generated C doesn't compile:** Add golden test in `tools/oz_transpile/tests/golden/`
- **Generated C compiles but wrong behavior:** Add `.m` in `tests/behavior/cases/<category>/`
- **Bug regression:** Use `scripts/new_regression_test.sh`
- **Zephyr-specific failure:** Add ZTEST in `tests/zephyr/src/`
- **PAL function incorrect:** Add test in `tests/pal/`
```

Acceptance criteria:
- [ ] `tests/README.md` exists and covers all test layers
- [ ] A new contributor can find the right place to add a test

## Expected outcome

After this phase:
- CI has **8–9 jobs** covering all test layers + hardware verification
- PAL inlining is proven automatically on every PR
- Regression workflow is documented and scripted
- `just test-all` runs the complete test suite locally
- LeakSanitizer catches ARC omissions
- Coverage reporting shows gaps
