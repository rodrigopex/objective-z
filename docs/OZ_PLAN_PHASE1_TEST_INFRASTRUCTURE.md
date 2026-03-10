# Phase 1 — Test Infrastructure Foundation

## Goal

Set up the test framework, Python test runner, and initial golden-file tests
so that every future transpiler change can be validated automatically. By the
end of this phase, `just test` (or `pytest`) runs a full suite of golden-file
comparisons on the host with zero Zephyr dependency.

**Prerequisite:** Phase 0 (Platform Abstraction Layer) is complete. The
transpiler emits `oz_platform.h` includes and all PAL headers exist.

## Context

Golden-file testing is the first line of defense for a transpiler. Each test
is a `.m` input file paired with expected `.c` and `.h` output files. The test
runner transpiles the input, diffs against expected output, and fails on any
mismatch. This catches unintended output changes immediately and makes every
transpiler modification visible in code review (the golden files change too).

Unity (ThrowTheSwitch) is the C test framework for compiled behavior tests in
later phases, but we vendor it now so the infrastructure is ready.

## Deliverables

### File structure

```
test/
    conftest.py                     ← pytest configuration
    runner.py                       ← golden-file test orchestrator
    golden/
        README.md                   ← documents the golden test workflow
        empty_class/
            input.m                 ← Objective-C source
            expected.c              ← expected transpiler C output
            expected.h              ← expected transpiler H output
        class_with_method/
            input.m
            expected.c
            expected.h
        class_with_property/
            ...
        simple_inheritance/
            ...
        protocol_conformance/
            ...
        message_send_static/
            ...
        message_send_protocol/
            ...
        retain_release/
            ...
        nil_receiver/
            ...
        singleton_pattern/
            ...
        category_merge/
            ...
        multiple_args/
            ...
        struct_return/
            ...
        error_unsupported_feature/
            ...
        error_missing_method/
            ...
    lib/
        unity/                      ← vendored Unity (for Phase 2)
            unity.c
            unity.h
            unity_internals.h
    stubs/                          ← (empty placeholder — no longer needed for
                                       Zephyr stubs since PAL handles this)
```

### Step 1 — Vendor Unity test framework

Download Unity from ThrowTheSwitch/Unity on GitHub (MIT license). We only
need three files: `src/unity.c`, `src/unity.h`, `src/unity_internals.h`.

```bash
# From the repository root:
mkdir -p test/lib/unity
curl -L https://raw.githubusercontent.com/ThrowTheSwitch/Unity/master/src/unity.c \
     -o test/lib/unity/unity.c
curl -L https://raw.githubusercontent.com/ThrowTheSwitch/Unity/master/src/unity.h \
     -o test/lib/unity/unity.h
curl -L https://raw.githubusercontent.com/ThrowTheSwitch/Unity/master/src/unity_internals.h \
     -o test/lib/unity/unity_internals.h
```

Acceptance criteria:
- [ ] `test/lib/unity/unity.c` exists and compiles with `gcc -c -std=c11`
- [ ] `test/lib/unity/unity.h` exists
- [ ] License header preserved in all files

### Step 2 — Create the golden-file test runner

Create `test/runner.py` — a pytest-compatible module that discovers and runs
golden-file tests. Each subdirectory of `tests/golden/` is a test case.

The runner must:

1. Find all directories under `tests/golden/` that contain `input.m`
2. For each, invoke the transpiler: `python -m oz_transpiler input.m -o /tmp/out/`
3. Compare each file in `/tmp/out/` against the corresponding `expected.*` file
4. Report unified diff on mismatch
5. Support `--update-golden` flag to overwrite expected files with actual output

```python
"""
Golden-file test runner for the objective-z transpiler.

Usage:
    pytest test/runner.py                    # run all golden tests
    pytest test/runner.py -k empty_class     # run one test
    python test/runner.py --update-golden    # regenerate expected files

Each test directory under tests/golden/ must contain:
    input.m         — Objective-C source (required)
    expected.c      — expected C output (required)
    expected.h      — expected H output (optional)
    config.json     — transpiler flags override (optional)

config.json format:
{
    "flags": ["--no-arc"],           # extra transpiler flags
    "expect_error": true,            # if true, transpiler should fail
    "expected_stderr": "unsupported" # substring expected in stderr
}
"""
```

Key implementation details:

- Use `subprocess.run` to invoke the transpiler (not import — tests must
  validate the CLI interface, not internal APIs).
- Diff with `difflib.unified_diff` for readable output.
- `config.json` is optional — defaults are: no extra flags, expect success.
- For error tests (`expect_error: true`), the runner checks the exit code is
  nonzero and stderr contains `expected_stderr` substring.
- Normalize whitespace in comparisons: strip trailing whitespace per line,
  ensure single trailing newline. This avoids false failures from editors
  adding/removing final newlines.

Acceptance criteria:
- [ ] `pytest test/runner.py` discovers and runs all golden test directories
- [ ] Mismatch produces a clear unified diff showing expected vs actual
- [ ] `python test/runner.py --update-golden` overwrites expected files
- [ ] Error tests validate exit code and stderr content
- [ ] Runner works on Linux and macOS (no platform-specific paths)

### Step 3 — Create `conftest.py`

```python
"""
Pytest configuration for objective-z transpiler tests.

Sets up paths and shared fixtures.
"""
import pytest
import pathlib

@pytest.fixture
def project_root():
    """Return the project root directory."""
    return pathlib.Path(__file__).parent.parent

@pytest.fixture
def transpiler_cmd(project_root):
    """Return the command to invoke the transpiler."""
    return ["python", "-m", "oz_transpiler"]

@pytest.fixture
def pal_host_cflags(project_root):
    """Return compiler flags for host-mode compilation of transpiler output."""
    return [
        "gcc", "-std=c11", "-Wall", "-Werror",
        "-DOZ_PLATFORM_HOST",
        f"-I{project_root}/include",
    ]
```

Acceptance criteria:
- [ ] `pytest --collect-only` from project root discovers tests
- [ ] Fixtures provide correct paths regardless of working directory

### Step 4 — Create initial golden tests (10–15 tests)

**Start with the Phase 0 assessment.** Before writing new `.m` inputs from
scratch, check `tests/ASSESSMENT.md` for Bucket A tests — those are existing
`.m` files that exercise supported features with observable-behavior assertions.
Each Bucket A file can serve double duty: as a golden test input AND (in
Phase 2) as a behavior test. Use them first, then fill gaps with new tests
for any language features not already covered.

Create one test directory per language feature. Each must have `input.m` and
at least `expected.c`. Write the `input.m` first (or copy from Bucket A),
run the transpiler manually to generate initial output, review and correct
the output, then save as `expected.c` / `expected.h`.

**Test 1: `empty_class`** — Minimal class with no methods or properties.
Tests that the transpiler generates correct struct definition and slab.

```objc
// input.m
@interface EmptyClass : OZObject
@end

@implementation EmptyClass
@end
```

Expected output should contain: struct definition with base fields,
`OZ_SLAB_DEFINE`, alloc/dealloc functions, and `#include "oz/platform/oz_platform.h"`.

**Test 2: `class_with_method`** — One instance method, verifies `OZ_SEND`
token-pasting expansion target is generated.

```objc
@interface Greeter : OZObject
- (void)greet;
@end

@implementation Greeter
- (void)greet {
    oz_platform_print("Hello\n");
}
@end
```

**Test 3: `class_with_property`** — Verifies getter/setter generation.

```objc
@interface Counter : OZObject
@property (nonatomic) int count;
@end

@implementation Counter
@end
```

**Test 4: `simple_inheritance`** — Parent and child class. Verifies child
struct includes parent fields and method resolution follows hierarchy.

```objc
@interface Animal : OZObject
- (void)speak;
@end

@interface Dog : Animal
- (void)fetch;
@end
```

**Test 5: `protocol_conformance`** — Class conforms to a protocol. Verifies
switch-based dispatch function generation via `OZ_PROTOCOL_SEND`.

```objc
@protocol Printable
- (void)printDescription;
@end

@interface Item : OZObject <Printable>
- (void)printDescription;
@end
```

**Test 6: `message_send_static`** — Verifies direct static dispatch. The
generated code should use `OZ_SEND(T, sel, obj)` which expands to
`T##_##sel(obj)` — a direct function call.

**Test 7: `message_send_protocol`** — Verifies protocol dispatch. The
generated code should use `OZ_PROTOCOL_SEND(sel, obj)` which dispatches
through a static inline switch on `oz_class_id`.

**Test 8: `retain_release`** — Object with explicit retain/release. Verifies
`oz_atomic_inc` / `oz_atomic_dec_and_test` in generated code.

**Test 9: `nil_receiver`** — Message send to nil. Verifies the generated
code handles nil receivers safely (returns zero/nil).

**Test 10: `singleton_pattern`** — Class with a shared-instance method.
Verifies `oz_atomic_ptr_cas` usage in generated code.

**Test 11: `category_merge`** — Category adds method to existing class.
Verifies `resolve.py` merges category methods and `emit.py` generates them
in the same compilation unit.

**Test 12: `multiple_args`** — Method with 2+ arguments. Verifies correct
C function signature generation from ObjC selector components.

**Test 13: `error_unsupported_feature`** — Input uses `@try/@catch` or blocks.
Config sets `expect_error: true`. Verifies transpiler rejects gracefully.

**Test 14: `error_missing_method`** — Protocol conformance without implementing
required method. Verifies transpiler emits a clear error.

Acceptance criteria:
- [ ] At least 12 golden test directories exist under `tests/golden/`
- [ ] Each has `input.m` and `expected.c` (and `expected.h` where applicable)
- [ ] `pytest test/runner.py` passes with all green
- [ ] At least 2 error/negative tests included
- [ ] Each `expected.c` file contains `#include "oz/platform/oz_platform.h"`
      (NOT Zephyr headers — validates Phase 0 integration)

### Step 5 — Add `justfile` target (or Makefile target)

```just
# Run all transpiler tests
test:
    pytest test/ -v

# Update golden files after intentional transpiler changes
update-golden:
    python test/runner.py --update-golden

# Run only golden tests
test-golden:
    pytest test/runner.py -v

# Quick compile-check: transpile + compile on host (no run)
smoke:
    python -m oz_transpiler tests/golden/empty_class/input.m -o /tmp/oz_smoke/
    gcc -std=c11 -Wall -Werror -DOZ_PLATFORM_HOST -I include -fsyntax-only /tmp/oz_smoke/*.c
```

Acceptance criteria:
- [ ] `just test` runs the full suite
- [ ] `just update-golden` regenerates expected files
- [ ] `just smoke` does a quick compile-check

### Step 6 — Document the golden-test workflow

Create `tests/golden/README.md` explaining:

1. How to add a new golden test (create directory, write `input.m`, run
   `--update-golden`, review and commit expected files)
2. How to update golden files after an intentional transpiler change
3. How to write error/negative tests with `config.json`
4. The relationship between golden tests and behavior tests (Phase 2)

Acceptance criteria:
- [ ] README exists and covers all four topics
- [ ] A new contributor can add a golden test by following the README alone

## What this phase does NOT include

- Compiled behavior tests that *run* the transpiler output (Phase 2)
- CI pipeline (Phase 3)
- Coverage reporting (Phase 3)
- Zephyr integration tests (Phase 4)
