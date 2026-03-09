# Phase 2 — Compiled Behavior Tests

## Goal

Verify that transpiled C code **executes correctly** on the host, not just that
it matches expected text. Each test transpiles an `.m` file, compiles the C
output against the PAL host backend and Unity, runs the binary, and checks
assertions. By the end of this phase, 20–30 behavior tests cover object
lifecycle, dispatch, memory management, properties, and protocols.

**Prerequisite:** Phase 0 (PAL) and Phase 1 (golden tests + Unity vendored)
are complete.

## Context

Golden tests (Phase 1) catch *output regressions* — they tell you when the
transpiler's C output changed. Behavior tests catch *semantic bugs* — they
tell you when the transpiler's C output is wrong even if it hasn't changed.
The two are complementary: golden tests are fast and precise, behavior tests
are authoritative on correctness.

Each behavior test is a self-contained `.m` file that defines classes and
includes `test_*` functions with Unity assertions. A Python orchestrator
transpiles it, generates a Unity `main()` wrapper, compiles everything, and
runs the binary.

## Deliverables

### File structure

```
test/
    behavior/
        conftest.py                ← pytest fixtures for behavior tests
        test_lifecycle.py          ← pytest module (generated or hand-written)
        test_static_dispatch.py
        test_protocol_dispatch.py
        test_memory.py
        test_properties.py
        test_edge_cases.py
        cases/
            lifecycle/
                alloc_returns_valid.m
                init_sets_fields.m
                dealloc_frees_slab.m
                alloc_failure_enomem.m
                double_release_guard.m
            dispatch/
                send_routes_correct.m
                super_calls_parent.m
                method_override.m
                inherited_method.m
                class_method.m
            protocol/
                switch_routes_correct.m
                multiple_conformance.m
                protocol_inheritance.m
                typed_protocol_var.m
            memory/
                retain_increments.m
                release_decrements.m
                release_frees_at_zero.m
                nested_retain_release.m
                retain_count_query.m
            properties/
                getter_setter_gen.m
                dot_syntax.m
                readonly_property.m
                strong_vs_assign.m
                property_override.m
            edge/
                nil_returns_zero.m
                multiple_args_method.m
                empty_class_no_methods.m
                deep_inheritance.m
    tools/
        gen_test_main.py           ← generates Unity main() for a behavior test
        compile_and_run.py         ← transpile → compile → run pipeline
```

### Step 1 — Create the behavior test orchestrator

Create `test/tools/compile_and_run.py` — a reusable script that takes an
`.m` file and produces a pass/fail result.

Pipeline:

```
input.m
  │
  ├─ (1) python -m oz_transpiler input.m -o /tmp/oz_btest/
  │       → produces: ClassName.c, ClassName.h
  │
  ├─ (2) python test/tools/gen_test_main.py /tmp/oz_btest/ input.m
  │       → produces: test_main.c (Unity main calling all test_* functions)
  │
  ├─ (3) gcc -std=c11 -Wall -Werror -DOZ_PLATFORM_HOST \
  │       -I include -I test/lib/unity \
  │       /tmp/oz_btest/*.c test/lib/unity/unity.c \
  │       -o /tmp/oz_btest/test_bin
  │
  └─ (4) /tmp/oz_btest/test_bin
          → Unity output: "4 Tests 0 Failures 0 Ignored"
          → exit code 0 = pass, nonzero = fail
```

**`gen_test_main.py`** scans the `.m` file for functions matching `void test_*`
and generates a `test_main.c`:

```c
/* Auto-generated — do not edit */
#include "unity.h"

/* Forward declarations — extracted from input.m */
extern void test_alloc_returns_valid(void);
extern void test_init_sets_fields(void);

void setUp(void) {}
void tearDown(void) {}

int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_alloc_returns_valid);
    RUN_TEST(test_init_sets_fields);
    return UNITY_END();
}
```

The function discovery is a simple regex: `^\s*void\s+(test_\w+)\s*\(void\)`.
This scans the original `.m` file (not the transpiled C), so test function
names must survive transpilation unchanged.

Acceptance criteria:
- [ ] `compile_and_run.py` takes a single `.m` path and returns exit code
- [ ] `gen_test_main.py` discovers all `test_*` functions automatically
- [ ] Compilation uses `-Wall -Werror` — no warnings allowed
- [ ] Temporary files go in `/tmp/oz_btest_<hash>/` (no collisions)
- [ ] Both scripts work on Linux and macOS

### Step 2 — Create pytest integration

Create `test/behavior/conftest.py` with a fixture that wraps `compile_and_run.py`:

```python
import subprocess
import pathlib
import pytest

CASES_DIR = pathlib.Path(__file__).parent / "cases"

def discover_behavior_tests():
    """Find all .m files under cases/ and yield (category, name, path)."""
    for m_file in sorted(CASES_DIR.rglob("*.m")):
        category = m_file.parent.name
        name = m_file.stem
        yield pytest.param(m_file, id=f"{category}/{name}")

@pytest.fixture
def compile_and_run(project_root):
    """Return a callable that transpiles, compiles, and runs a .m test file."""
    def _run(m_path: pathlib.Path) -> subprocess.CompletedProcess:
        result = subprocess.run(
            ["python", "test/tools/compile_and_run.py", str(m_path)],
            capture_output=True, text=True, cwd=str(project_root)
        )
        return result
    return _run
```

Each `test_*.py` file parametrizes over its category's `.m` files:

```python
# test/behavior/test_lifecycle.py
import pytest
from conftest import CASES_DIR, discover_behavior_tests

LIFECYCLE_TESTS = [
    p for p in discover_behavior_tests()
    if "lifecycle" in str(p.values[0])
]

@pytest.mark.parametrize("m_file", LIFECYCLE_TESTS)
def test_lifecycle(m_file, compile_and_run):
    result = compile_and_run(m_file)
    assert result.returncode == 0, (
        f"FAILED: {m_file.name}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )
```

Acceptance criteria:
- [ ] `pytest test/behavior/ -v` discovers and runs all behavior tests
- [ ] Each test shows `category/name` in pytest output (e.g., `lifecycle/alloc_returns_valid`)
- [ ] Failed tests print both stdout (Unity output) and stderr (compiler errors)

### Step 3 — Write lifecycle behavior tests (5 tests)

**Leverage the Phase 0 assessment first.** Before writing these `.m` files from
scratch, check `test/ASSESSMENT.md` for Bucket A tests that already cover
lifecycle behavior (alloc, init, dealloc, retain, release). Copy those `.m`
files into the appropriate `cases/` subdirectory, add Unity `#include` and
`test_*` function wrappers if needed, and verify they work through the
transpile → compile → run pipeline. Only write new tests for gaps not covered
by existing Bucket A files.

For Bucket B tests (those using runtime introspection for verification), adapt
them by replacing runtime API assertions with observable-behavior assertions.
For example, replace `assert(class_getName([obj class])` with a direct check
on method return values or side effects.

**Test: `alloc_returns_valid.m`**

```objc
@interface Widget : OZObject
@property (nonatomic) int tag;
@end

@implementation Widget
@end

/* ---------- test functions ---------- */
#include "unity.h"

void test_alloc_returns_valid(void) {
    Widget *w = [Widget alloc];
    TEST_ASSERT_NOT_NULL(w);
    [w release];
}
```

**Test: `init_sets_fields.m`** — Verify init method sets property defaults.

**Test: `dealloc_frees_slab.m`** — Allocate max slab blocks, release one,
allocate again — should succeed (proves slab was returned).

**Test: `alloc_failure_enomem.m`** — Configure a slab with `num_blocks=1`,
allocate twice — second alloc must return nil (or appropriate error).

**Test: `double_release_guard.m`** — If the transpiler generates a guard
against double-free (retain count already 0), test that it doesn't crash.
If no guard exists, mark this test as `XFAIL` and document the gap.

Acceptance criteria:
- [ ] All 5 `.m` files exist under `test/behavior/cases/lifecycle/`
- [ ] Each contains at least one `void test_*(void)` function
- [ ] Each file compiles and runs on host with PAL host backend
- [ ] `pytest test/behavior/test_lifecycle.py -v` passes (or XFAILs where noted)

### Step 4 — Write static dispatch behavior tests (5 tests)

**Test: `send_routes_correct.m`** — `OZ_SEND(T, sel, obj)` calls the right
C function. Use a method that sets a global flag, assert flag is set.

**Test: `super_calls_parent.m`** — Child calls `[super method]`, verify
parent's implementation runs.

**Test: `method_override.m`** — Child overrides parent's method. Sending to
child must call child's version, not parent's.

**Test: `inherited_method.m`** — Child does NOT override parent's method.
Sending to child must call parent's version.

**Test: `class_method.m`** — `+classMethod` dispatch works. Class methods
should be emitted as regular C functions with a distinct naming convention.

Acceptance criteria:
- [ ] All 5 `.m` files exist under `test/behavior/cases/dispatch/`
- [ ] Tests verify actual behavior (method was called), not just compilation

### Step 5 — Write protocol dispatch behavior tests (4 tests)

**Test: `switch_routes_correct.m`** — Two classes conform to same protocol.
`OZ_PROTOCOL_SEND` dispatches to the correct implementation based on
`oz_class_id`. Verify by calling through protocol on each class and checking
different side effects.

**Test: `multiple_conformance.m`** — One class conforms to two protocols.
Both protocol dispatches route correctly.

**Test: `protocol_inheritance.m`** — Protocol B inherits from Protocol A.
A class conforming to B must also respond to A's methods via protocol dispatch.

**Test: `typed_protocol_var.m`** — Variable typed as `id<Protocol>` dispatches
through the protocol switch, not static dispatch.

Acceptance criteria:
- [ ] All 4 `.m` files exist under `test/behavior/cases/protocol/`
- [ ] At least one test uses two different classes with the same protocol

### Step 6 — Write memory management behavior tests (5 tests)

**Test: `retain_increments.m`** — After retain, `oz_atomic_get(&obj->_rc)`
returns one more than before.

**Test: `release_decrements.m`** — After release (from rc > 1), retain count
decreases by one and object is still alive.

**Test: `release_frees_at_zero.m`** — Release from rc=1 should dealloc.
Verify by checking that a subsequent alloc reuses the slab block (for a
slab with num_blocks=1, the second alloc succeeds only if the first was freed).

**Test: `nested_retain_release.m`** — Retain twice, release twice — object
freed only on second release.

**Test: `retain_count_query.m`** — If the transpiler exposes a retainCount-like
function, verify it returns the correct value at each step. If not, verify
the atomic count directly.

Acceptance criteria:
- [ ] All 5 `.m` files exist under `test/behavior/cases/memory/`
- [ ] Tests verify retain counts via either public API or direct atomic reads

### Step 7 — Write property behavior tests (5 tests)

**Test: `getter_setter_gen.m`** — Set a property, read it back, assert equal.

**Test: `dot_syntax.m`** — `obj.prop = 42; assert(obj.prop == 42);` — verifies
dot-syntax maps to generated getter/setter.

**Test: `readonly_property.m`** — Verify getter exists. Verify there is no
setter (this may need to be a golden test if the test is about compilation
failure rather than runtime behavior).

**Test: `strong_vs_assign.m`** — `assign` property stores the raw pointer;
`strong` property retains the assigned object. Verify retain count difference.

**Test: `property_override.m`** — Subclass redefines a readonly property as
readwrite. Verify setter exists on subclass but not parent.

Acceptance criteria:
- [ ] All 5 `.m` files exist under `test/behavior/cases/properties/`

### Step 8 — Write edge case behavior tests (4 tests)

**Test: `nil_returns_zero.m`** — Send a message to nil. Verify return value
is 0 (for int-returning method) and nil (for object-returning method).

**Test: `multiple_args_method.m`** — Method with 3+ arguments. Verify all
arguments arrive at the implementation correctly.

**Test: `empty_class_no_methods.m`** — Class with no methods or properties.
Alloc, init, release — verify it doesn't crash.

**Test: `deep_inheritance.m`** — 4-level inheritance chain. Verify method
resolution at each level.

Acceptance criteria:
- [ ] All 4 `.m` files exist under `test/behavior/cases/edge/`
- [ ] `nil_returns_zero` is especially important — embedded code must not crash on nil

## Compiler matrix

All behavior tests must compile and pass under both GCC and Clang, and at
both `-O0` and `-O2`. This catches undefined behavior that optimization
may expose (or hide).

```bash
# GCC debug
gcc -std=c11 -O0 -g -Wall -Werror -DOZ_PLATFORM_HOST ...

# GCC optimized
gcc -std=c11 -O2 -Wall -Werror -DOZ_PLATFORM_HOST ...

# Clang debug
clang -std=c11 -O0 -g -Wall -Werror -DOZ_PLATFORM_HOST ...

# Clang optimized
clang -std=c11 -O2 -Wall -Werror -DOZ_PLATFORM_HOST ...
```

The `compile_and_run.py` script should accept a `--compiler` and `--opt`
flag, with `just test-matrix` running all four combinations.

Acceptance criteria:
- [ ] All behavior tests pass under GCC `-O0`, GCC `-O2`, Clang `-O0`, Clang `-O2`
- [ ] `just test-matrix` or equivalent runs all four

## Sanitizer support

Add AddressSanitizer and UndefinedBehaviorSanitizer runs as optional flags:

```bash
gcc -std=c11 -O0 -g -fsanitize=address,undefined -DOZ_PLATFORM_HOST ...
```

These catch buffer overflows, use-after-free, signed integer overflow, and
other UB that the transpiler might accidentally introduce. Not required for
every test run, but should be in CI (Phase 3).

## What this phase does NOT include

- CI pipeline automation (Phase 3)
- Coverage reporting (Phase 3)
- Upstream test adaptation from Apple/LLVM/GNUstep (Phase 3)
- Zephyr on-device tests (Phase 4)
