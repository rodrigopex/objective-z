# Phase 4 — Zephyr Integration Tests

## Goal

Validate that transpiled C code works within the real Zephyr RTOS kernel by
running behavior tests on the `native_sim` board using Zephyr's native `ztest`
framework and `twister` test runner. This is the final validation layer —
confirming that the PAL's Zephyr backend integrates correctly with actual
`k_mem_slab` allocation, real atomic operations, and real `printk` output.

By the end of this phase, a subset of behavior tests run on `native_sim`
in CI, proving end-to-end correctness from `.m` source to Zephyr binary.

**Prerequisite:** Phase 0 (PAL), Phase 1 (golden tests), Phase 2 (behavior
tests), and Phase 3 (CI pipeline) are complete.

## Context

The host-based behavior tests (Phase 2) use the PAL host backend — they
validate transpiler correctness against `malloc`/`printf`/C11 atomics. This
phase validates the *other side* of the PAL: the Zephyr backend. It answers
the question "does the transpiled code actually work on Zephyr?" without
requiring real hardware (native_sim runs the Zephyr kernel as a Linux process).

Zephyr's native_sim board (successor to native_posix since Zephyr 3.6) compiles
the entire Zephyr kernel into a native Linux executable. It provides real
`k_mem_slab`, real `atomic_*`, real `printk` → stdout, and real scheduling.
This is the standard Zephyr approach for kernel-level testing without hardware.

Zephyr's test framework `ztest` provides assertion macros (`zassert_equal`,
`zassert_not_null`, etc.) and test registration (`ZTEST`, `ZTEST_SUITE`).
The `twister` tool discovers and runs all test suites, outputting JUnit XML.

## Deliverables

### File structure

```
tests/
    zephyr/
        CMakeLists.txt              ← top-level CMake for all zephyr tests
        prj.conf                    ← Kconfig: enables ztest, mem_slab, etc.
        testcase.yaml               ← twister test case definition
        src/
            main.c                  ← ztest suite registration
            test_lifecycle.c        ← transpiled + adapted lifecycle tests
            test_dispatch.c         ← transpiled + adapted dispatch tests
            test_memory.c           ← transpiled + adapted memory tests
            test_protocol.c         ← transpiled + adapted protocol tests
        generated/
            README.md               ← explains these are transpiler outputs
            SimpleClass.c           ← transpiled from test inputs
            SimpleClass.h
            Widget.c
            Widget.h
            ...                     ← one pair per test class
        include/
            test_classes.h          ← forward declarations for all test classes
```

### Step 1 — Set up Zephyr test project structure

Create the CMake and Kconfig boilerplate for a Zephyr test application
targeting `native_sim`.

**`tests/zephyr/CMakeLists.txt`:**

```cmake
cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})

project(oz_transpiler_tests)

# PAL in Zephyr mode
target_compile_definitions(app PRIVATE OZ_PLATFORM_ZEPHYR)

# Include paths
target_include_directories(app PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../include    # oz_platform headers
    ${CMAKE_CURRENT_SOURCE_DIR}/include          # test-local headers
    ${CMAKE_CURRENT_SOURCE_DIR}/generated        # transpiled output
)

# Test sources
target_sources(app PRIVATE
    src/main.c
    src/test_lifecycle.c
    src/test_dispatch.c
    src/test_memory.c
    src/test_protocol.c
)

# Transpiled class sources
file(GLOB GENERATED_SOURCES "generated/*.c")
target_sources(app PRIVATE ${GENERATED_SOURCES})
```

**`tests/zephyr/prj.conf`:**

```ini
# Enable the Zephyr test framework
CONFIG_ZTEST=y

# Memory slab support (already enabled by default, explicit for clarity)
CONFIG_KERNEL_MEM_POOL=n

# Enough stack for tests
CONFIG_MAIN_STACK_SIZE=4096
CONFIG_ZTEST_STACK_SIZE=2048

# Log output (for OZLog / oz_platform_print)
CONFIG_PRINTK=y

# No networking, no filesystem — pure kernel tests
CONFIG_NETWORKING=n
```

**`tests/zephyr/testcase.yaml`:**

```yaml
tests:
  oz.transpiler.host:
    platform_allow: native_sim
    tags: oz transpiler
    timeout: 60
```

Acceptance criteria:
- [ ] `west build -b native_sim tests/zephyr/` succeeds
- [ ] `west build -b native_sim tests/zephyr/ -t run` executes tests
- [ ] `twister -T tests/zephyr/ -p native_sim` discovers and runs the suite

### Step 2 — Transpile test classes for Zephyr tests

Select a representative subset of the Phase 2 behavior test `.m` files and
transpile them into the `tests/zephyr/generated/` directory. These are
committed to the repository (not generated at build time) so the Zephyr build
doesn't depend on the Python transpiler.

Minimum set to transpile:

- `SimpleClass` — basic alloc/init/release (from Phase 0 smoke test)
- `Widget` — property access (from lifecycle tests)
- `Animal` / `Dog` — inheritance chain (from dispatch tests)
- `Printable` protocol + `Item` — protocol dispatch (from protocol tests)
- A class with `num_blocks=2` slab — for exhaustion testing

Each transpiled file should be the exact output of `emit.py` with no manual
edits. If manual edits are needed, that's a transpiler bug to fix.

Create `tests/zephyr/generated/README.md`:

```
# Generated Files — Do Not Edit

These files are the output of the objective-z transpiler.
To regenerate: `python scripts/regen_zephyr_tests.py`

Any manual edits will be overwritten on regeneration.
If the transpiler output needs changes, fix the transpiler.
```

Create `scripts/regen_zephyr_tests.py` that transpiles the source `.m` files
into `tests/zephyr/generated/`.

Acceptance criteria:
- [ ] At least 5 transpiled class pairs (`.c` + `.h`) in `generated/`
- [ ] All generated files contain `#include "oz/platform/oz_platform.h"`
- [ ] All generated files compile with `-DOZ_PLATFORM_ZEPHYR` in the Zephyr build
- [ ] `regen_zephyr_tests.py` script exists and produces identical output

### Step 3 — Write ztest test suites

Translate a representative subset of Phase 2 Unity tests into ztest format.
The logical assertions are the same — only the API surface changes:

| Unity                              | ztest                               |
|------------------------------------|--------------------------------------|
| `TEST_ASSERT_NOT_NULL(p)`          | `zassert_not_null(p)`               |
| `TEST_ASSERT_EQUAL_INT(a, b)`     | `zassert_equal(a, b)`              |
| `TEST_ASSERT_TRUE(x)`             | `zassert_true(x)`                  |
| `TEST_ASSERT_NULL(p)`             | `zassert_is_null(p)`               |
| `void test_foo(void)` + `RUN_TEST`| `ZTEST(suite, test_foo)`           |

**`src/main.c`:**

```c
#include <zephyr/ztest.h>

/* All test suites register themselves via ZTEST_SUITE in their own files */
```

**`src/test_lifecycle.c`** (example structure):

```c
#include <zephyr/ztest.h>
#include "oz/platform/oz_platform.h"
#include "Widget.h"  /* transpiled */

ZTEST_SUITE(lifecycle, NULL, NULL, NULL, NULL, NULL);

ZTEST(lifecycle, test_alloc_returns_valid) {
    Widget *w = Widget_alloc();
    zassert_not_null(w, "Widget alloc returned NULL");
    Widget_release(w);
}

ZTEST(lifecycle, test_init_sets_default) {
    Widget *w = Widget_alloc();
    Widget_init(w);
    /* Assuming tag property defaults to 0 */
    zassert_equal(OZ_SEND(Widget, tag, w), 0, "Default tag should be 0");
    Widget_release(w);
}

ZTEST(lifecycle, test_slab_exhaustion) {
    /* Widget slab has num_blocks=N; allocate N+1 and verify failure */
    Widget *w1 = Widget_alloc();
    Widget *w2 = Widget_alloc();
    zassert_not_null(w1, "First alloc should succeed");
    /* Depending on slab size, w2 may or may not succeed */
    /* This test should be tailored to the actual slab config */
    if (w1) Widget_release(w1);
    if (w2) Widget_release(w2);
}
```

Target: translate at least **12 behavior tests** across the four suites
(lifecycle, dispatch, memory, protocol).

Acceptance criteria:
- [ ] At least 12 `ZTEST()` test cases across 4 test files
- [ ] All tests pass on `native_sim`
- [ ] Tests exercise all four PAL surfaces (slab, atomic int, atomic ptr, print)

### Step 4 — Add Zephyr CI job

Extend `.github/workflows/ci.yml` with a Zephyr integration job:

```yaml
  # --------------------------------------------------
  # Job 5: Zephyr integration tests (native_sim)
  # --------------------------------------------------
  zephyr-integration:
    runs-on: ubuntu-latest
    container:
      image: ghcr.io/zephyrproject-rtos/ci:v0.26.13
    env:
      ZEPHYR_SDK_INSTALL_DIR: /opt/toolchains/zephyr-sdk-0.16.8
    steps:
      - uses: actions/checkout@v4

      - name: Initialize Zephyr workspace
        run: |
          west init -l .
          west update --narrow --fetch-opt=--depth=1

      - name: Build and run tests on native_sim
        run: |
          west twister -T tests/zephyr/ -p native_sim --inline-logs

      - name: Upload twister results
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: twister-results
          path: twister-out/
```

Alternatively, if the project uses `rodrigopex/zephyr-sdk-ng` (the custom SDK
fork), adjust the container image and SDK paths accordingly.

Note: The Zephyr CI container image version and SDK version should match the
Zephyr version the project targets. Check `west.yml` or the project's
`zephyr/module.yml` for the correct base version.

Acceptance criteria:
- [ ] CI job runs on every PR (same trigger as other jobs)
- [ ] `twister` discovers and runs all tests
- [ ] Test results appear as a downloadable artifact
- [ ] Job passes with all tests green on `native_sim`

### Step 5 — Add a hardware build-verification job (optional)

This doesn't run tests on real hardware (no hardware in CI), but verifies
that the transpiled code compiles for a real Cortex-M target. This catches
issues like missing `__attribute__((always_inline))`, incorrect type sizes,
or PAL functions that weren't truly inlined.

```yaml
  # --------------------------------------------------
  # Job 6: Hardware build verification (compile only)
  # --------------------------------------------------
  hw-build-check:
    runs-on: ubuntu-latest
    container:
      image: ghcr.io/zephyrproject-rtos/ci:v0.26.13
    steps:
      - uses: actions/checkout@v4

      - name: Initialize Zephyr workspace
        run: |
          west init -l .
          west update --narrow --fetch-opt=--depth=1

      - name: Build for nrf52840dk (compile only)
        run: west build -b nrf52840dk/nrf52840 tests/zephyr/ --build-dir build-hw

      - name: Verify PAL inlining (no oz_ symbols in call sites)
        run: |
          arm-none-eabi-objdump -d build-hw/zephyr/zephyr.elf | \
            grep -c 'bl.*oz_slab_alloc\|bl.*oz_atomic' | \
            (read count; [ "$count" -eq 0 ] || \
              (echo "FAIL: PAL functions not inlined ($count call sites)"; exit 1))
```

This job also serves as the zero-cost proof — if any `oz_` PAL function
appears as a branch-and-link target in the disassembly, the inlining failed.

Acceptance criteria:
- [ ] Build succeeds for `nrf52840dk` (or another Cortex-M board in the project)
- [ ] `objdump` check confirms zero `oz_` call sites (all inlined)
- [ ] Job is optional (allowed to fail without blocking PR merge) until stable

### Step 6 — Create a regeneration and synchronization workflow

The Zephyr tests depend on transpiled `.c` files in `generated/`. These must
stay in sync with the transpiler. Create a CI check that verifies freshness:

```yaml
  # --------------------------------------------------
  # Job 7: Generated file freshness check
  # --------------------------------------------------
  generated-freshness:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Regenerate Zephyr test sources
        run: python scripts/regen_zephyr_tests.py

      - name: Check for uncommitted changes
        run: |
          if ! git diff --quiet tests/zephyr/generated/; then
            echo "FAIL: Transpiled test sources are stale. Run:"
            echo "  python scripts/regen_zephyr_tests.py"
            echo "and commit the updated files."
            git diff tests/zephyr/generated/
            exit 1
          fi
```

Acceptance criteria:
- [ ] `regen_zephyr_tests.py` is idempotent (running twice produces identical output)
- [ ] CI fails if generated files are stale after a transpiler change
- [ ] Error message tells the developer exactly what command to run

## Summary of the complete test pyramid after all four phases

```
                    ┌─────────────────┐
                    │  Phase 4        │
                    │  Zephyr/native  │  ~12 tests on real kernel
                    │  sim + twister  │  (k_mem_slab, real atomics)
                    └────────┬────────┘
                             │
                    ┌────────┴────────┐
                    │  Phase 2 + 3    │
                    │  Behavior tests │  ~30 tests compiled + run on host
                    │  Unity + ASan   │  (malloc, C11 atomics)
                    │  + upstream     │  + adapted LLVM/GNUstep tests
                    └────────┬────────┘
                             │
              ┌──────────────┴──────────────┐
              │  Phase 1                     │
              │  Golden-file tests           │  ~15 tests: output stability
              │  (transpiler output diffing) │  (no compilation, just diff)
              └──────────────┬──────────────┘
                             │
              ┌──────────────┴──────────────┐
              │  Phase 0                     │
              │  Platform Abstraction Layer  │  Foundation: zero-cost PAL
              │  (oz_platform.h)             │  enables everything above
              └─────────────────────────────┘
```

Total test count across all phases: **~55–65 tests** covering output stability,
semantic correctness, memory safety, and Zephyr integration. The full suite
runs in CI on every PR, with the host-based tests completing in seconds and
the Zephyr job completing in 2–3 minutes.
