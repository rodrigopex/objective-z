# Phase 3 — CI Pipeline, Coverage, and Upstream Test Adaptation

## Goal

Automate all tests in GitHub Actions, add code coverage reporting for both
Python and C, and adapt the highest-value tests from LLVM/Clang and GNUstep
upstream suites. By the end of this phase, every PR triggers a full test
matrix and coverage report, and the test suite includes adapted upstream tests
that validate the transpiler against battle-tested behavioral specifications.

**Prerequisite:** Phase 0 (PAL), Phase 1 (golden tests), and Phase 2
(behavior tests) are complete.

## Context

The transpiler now has two layers of testing — golden files for output
stability and behavior tests for semantic correctness. This phase adds the
automation layer (CI) and the third source of truth (upstream tests adapted
from compiler projects that have tested these language semantics for decades).

The upstream tests worth adapting come from two primary sources:

1. **LLVM/Clang `clang/test/Rewriter/`** (~40–60 files, Apache 2.0 + LLVM
   exception) — tests Clang's ObjC-to-C++ rewriter, which is architecturally
   the closest analog to objective-z's transpiler. These validate that message
   sends become C function calls, interfaces become structs, and properties
   become getters/setters.

2. **GNUstep libobjc2 tests** (~79 files, MIT license) — standalone `.m` files
   with `main()` and `assert()`. The cleanest structure for adaptation, and
   the most permissive license.

Only ~15–20% of upstream tests are directly applicable to a static-dispatch
transpiler. The rest test dynamic runtime features (KVO, forwarding, tagged
pointers) that objective-z deliberately eliminates.

## Deliverables

### File structure

```
.github/
    workflows/
        ci.yml                      ← main CI pipeline
test/
    adapted/
        README.md                   ← documents provenance and license of each test
        llvm_rewriter/
            class_rewrite.m         ← adapted from Clang Rewriter tests
            property_rewrite.m
            protocol_rewrite.m
            method_rewrite.m
            arc_insertion.m
        gnustep/
            property_attribute.m    ← adapted from GNUstep libobjc2
            basic_messaging.m
            category_method.m
            inheritance_chain.m
            nil_messaging.m
        apple_spec/
            README.md               ← behavioral specs derived from objc4 (no copied code)
            struct_return.m         ← new test based on objc4 behavioral spec
            nil_return_types.m
```

### Step 1 — Create GitHub Actions CI pipeline

Create `.github/workflows/ci.yml`:

```yaml
name: Transpiler CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  # --------------------------------------------------
  # Job 1: Python tests (transpiler logic + golden files)
  # --------------------------------------------------
  python-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install dependencies
        run: pip install pytest pytest-cov

      - name: Run transpiler unit tests + golden tests
        run: pytest test/ -v --cov=oz_transpiler --cov-report=xml

      - name: Upload Python coverage
        uses: codecov/codecov-action@v4
        with:
          files: coverage.xml
          flags: python

  # --------------------------------------------------
  # Job 2: Behavior tests — compiler matrix
  # --------------------------------------------------
  behavior-tests:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        compiler: [gcc, clang]
        optimization: ['-O0', '-O2']
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install dependencies
        run: pip install pytest

      - name: Run behavior tests
        run: >
          pytest test/behavior/ -v
          --compiler=${{ matrix.compiler }}
          --opt=${{ matrix.optimization }}

  # --------------------------------------------------
  # Job 3: Sanitizer run (ASan + UBSan)
  # --------------------------------------------------
  sanitizers:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install dependencies
        run: pip install pytest

      - name: Run with AddressSanitizer + UBSan
        run: >
          pytest test/behavior/ -v
          --compiler=gcc
          --opt='-O0'
          --sanitize=address,undefined

  # --------------------------------------------------
  # Job 4: C coverage (gcov)
  # --------------------------------------------------
  c-coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install dependencies
        run: |
          pip install pytest
          sudo apt-get install -y gcovr

      - name: Run behavior tests with coverage
        run: >
          pytest test/behavior/ -v
          --compiler=gcc
          --opt='-O0'
          --cflags='--coverage'
          --ldflags='--coverage'

      - name: Generate C coverage report
        run: gcovr --xml -o c_coverage.xml --filter='.*\.c'

      - name: Upload C coverage
        uses: codecov/codecov-action@v4
        with:
          files: c_coverage.xml
          flags: c-transpiled
```

Key design decisions:

- **Compiler matrix** runs GCC and Clang at both O0 and O2. This catches UB
  that one compiler hides but the other exposes under optimization.
- **Sanitizer job** is separate (not in the matrix) because it adds ~2x
  runtime and only needs one compiler/opt combination.
- **C coverage** measures which lines of *transpiled* C code are exercised by
  behavior tests. This tells you which language features lack test coverage.
- **Python coverage** measures which lines of the transpiler itself are
  exercised. This tells you which transpiler code paths lack tests.

Acceptance criteria:
- [ ] CI triggers on push to `main` and on all PRs
- [ ] All four jobs pass on a clean checkout
- [ ] Coverage reports upload to Codecov (or equivalent)
- [ ] Failed behavior tests show Unity output in CI logs

### Step 2 — Add coverage support to `compile_and_run.py`

Extend `compile_and_run.py` to accept `--cflags` and `--ldflags` pass-through
arguments. When `--cflags='--coverage'` is passed, GCC instruments the binary
and produces `.gcda`/`.gcno` files on execution.

Also add `--sanitize` flag that appends `-fsanitize=<value>` to both compile
and link commands.

Acceptance criteria:
- [ ] `python test/tools/compile_and_run.py --cflags='--coverage' test.m` produces `.gcda` files
- [ ] `python test/tools/compile_and_run.py --sanitize=address test.m` runs under ASan
- [ ] ASan violations cause nonzero exit (test failure)

### Step 3 — Add error and negative tests (10–15 tests)

Create test cases that verify the transpiler fails gracefully on invalid or
unsupported input. These go in `test/golden/` (for output checking) or
`test/behavior/cases/error/` (for runtime behavior).

**Golden error tests** (transpiler should exit nonzero with clear message):

- `error_blocks.m` — input uses Objective-C blocks (`^{ ... }`). Transpiler
  should reject with message indicating blocks are unsupported.
- `error_try_catch.m` — input uses `@try/@catch/@finally`. Unsupported.
- `error_dynamic_typing.m` — input uses `id` type without protocol qualifier
  in a context requiring dispatch. Transpiler should warn or error.
- `error_kvo.m` — input uses `addObserver:forKeyPath:...`. Unsupported.
- `error_forward_invocation.m` — input implements `-forwardInvocation:`.
  Unsupported (no dynamic dispatch).
- `error_missing_protocol_method.m` — class declares protocol conformance
  but omits a required method.
- `error_circular_inheritance.m` — `A : B` and `B : A`. Must detect and reject.
- `error_duplicate_method.m` — same selector defined twice in one class.

**Runtime error behavior tests** (compiled code should handle gracefully):

- `error_slab_exhaustion.m` — allocate more objects than slab allows; verify
  alloc returns nil and code doesn't crash.
- `error_release_nil.m` — release a nil pointer; verify no crash.

Each golden error test needs a `config.json`:

```json
{
    "expect_error": true,
    "expected_stderr": "blocks are not supported"
}
```

Acceptance criteria:
- [ ] At least 8 error tests exist
- [ ] Each error test documents which Objective-C feature is unsupported and why
- [ ] Transpiler error messages are clear enough for a developer to understand the limitation

### Step 4 — Create regression test infrastructure

Establish a naming convention and workflow for regression tests:

- Directory: `test/golden/regression/` and `test/behavior/cases/regression/`
- Naming: `issue_NNN_short_description.m` (e.g., `issue_042_nil_struct_return.m`)
- Each regression test must have a comment at the top:

```objc
/*
 * Regression test for issue #42.
 * Previously, sending a struct-returning message to nil produced
 * uninitialized memory instead of a zeroed struct.
 * Fixed in commit abc1234.
 */
```

Create `test/regression_template.m` as a starting point for new regression tests.

Acceptance criteria:
- [ ] Regression directories exist
- [ ] Template file exists with comment format documented
- [ ] Existing tests are not in the regression directory (it's empty until needed)

### Step 5 — Adapt LLVM/Clang Rewriter tests (5 tests)

These come from `clang/test/Rewriter/` in the LLVM project (Apache 2.0 with
LLVM exception — compatible with any project license).

The original tests use `FileCheck` assertions to verify Clang's ObjC-to-C++
rewriter output. We adapt them by replacing `FileCheck` patterns with either
golden-file expectations (for output shape) or Unity assertions (for behavior).

**Adapted test 1: `class_rewrite.m`** — derived from `objc-modern-metadata-visibility.mm`.
Verifies that `@interface` becomes a C struct and `@implementation` methods
become C functions with the correct naming convention.

**Adapted test 2: `property_rewrite.m`** — derived from `objc-modern-property-attributes.mm`.
Verifies that `@property` generates correct getter/setter with the naming
convention expected by `OZ_SEND` token pasting.

**Adapted test 3: `protocol_rewrite.m`** — verifies protocol declaration
produces the `OZ_PROTOCOL_SEND` switch dispatch function.

**Adapted test 4: `method_rewrite.m`** — verifies multi-argument selectors
are correctly mangled into C function names (e.g.,
`-[Foo doSomething:with:]` → `Foo_doSomething_with_`).

**Adapted test 5: `arc_insertion.m`** — derived from `clang/test/CodeGenObjC/arc-precise-lifetime.m`.
Verifies that retain/release calls are inserted at the correct scope boundaries
in the transpiled C output.

Each adapted test must include a header comment documenting:

```c
/*
 * Adapted from: clang/test/Rewriter/objc-modern-metadata-visibility.mm
 * LLVM commit: <hash or "latest as of YYYY-MM-DD">
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Replaced FileCheck assertions with Unity assertions
 *             verifying OZ_SEND expansion and struct layout.
 */
```

Acceptance criteria:
- [ ] 5 adapted tests exist in `test/adapted/llvm_rewriter/`
- [ ] Each has provenance comment with original file path and license
- [ ] Tests compile and run on host via the behavior test pipeline
- [ ] `test/adapted/README.md` documents the adaptation methodology

### Step 6 — Adapt GNUstep libobjc2 tests (5 tests)

These come from GNUstep's libobjc2 test suite (MIT license — no attribution
concerns, but we document provenance anyway for traceability).

The original tests are standalone `.m` files with `main()` and `assert()`.
We adapt them by replacing `assert()` with Unity assertions and replacing
runtime-dependent code with PAL equivalents.

**Adapted test 1: `property_attribute.m`** — verifies property getter/setter
behavior matches expected ObjC semantics.

**Adapted test 2: `basic_messaging.m`** — verifies simple message send and
return value.

**Adapted test 3: `category_method.m`** — verifies category methods are
merged into the class and callable.

**Adapted test 4: `inheritance_chain.m`** — verifies method resolution
through a multi-level inheritance hierarchy.

**Adapted test 5: `nil_messaging.m`** — verifies nil receiver returns zero
for all return types (int, float, pointer, struct).

Each adapted test header:

```c
/*
 * Adapted from: GNUstep libobjc2 — Test/PropertyAttributeTest.m
 * License: MIT
 * Adaptation: Replaced objc_msgSend with OZ_SEND, assert with TEST_ASSERT_*.
 *             Removed runtime introspection calls (class_getName, etc).
 */
```

Acceptance criteria:
- [ ] 5 adapted tests exist in `test/adapted/gnustep/`
- [ ] Each has provenance comment
- [ ] No runtime introspection calls remain (all removed during adaptation)

### Step 7 — Create Apple objc4-derived behavioral specifications (2–3 tests)

Apple's objc4 is APSL-licensed (restrictive), so we do NOT copy code. Instead,
we study the *behavioral specifications* documented in Apple's tests and write
**entirely new tests** that verify the same behavior.

**Spec-derived test 1: `struct_return.m`** — Apple's `msgSend.m` tests that
struct-returning messages work correctly for various struct sizes (1 byte,
2 bytes, up to 32+ bytes). Write a new test that sends messages returning
structs of different sizes and verifies all fields are correct.

**Spec-derived test 2: `nil_return_types.m`** — Apple's tests verify that
nil messaging returns zero for integer, float, pointer, and struct return
types. Write a new test covering all four cases.

The `test/adapted/apple_spec/README.md` must clearly state:

```
Tests in this directory are NOT copies or adaptations of Apple code.
They are original tests written to verify behavioral specifications
observed in Apple's objc4 test documentation.

Apple's objc4 is licensed under APSL 2.0. No APSL-licensed code
has been copied, modified, or adapted in these files.

These tests verify that objective-z's transpiled output matches the
behavioral expectations established by the Objective-C language,
as documented in:
  - https://opensource.apple.com/source/objc4/
  - Apple's Objective-C runtime documentation
```

Acceptance criteria:
- [ ] 2–3 tests exist in `test/adapted/apple_spec/`
- [ ] README clearly disclaims any APSL code derivation
- [ ] Tests are entirely original code (no structural similarity to Apple's tests)

## What this phase does NOT include

- Zephyr integration tests on native_sim or real hardware (Phase 4)
- twister configuration (Phase 4)
- Hardware target builds (Phase 4)
