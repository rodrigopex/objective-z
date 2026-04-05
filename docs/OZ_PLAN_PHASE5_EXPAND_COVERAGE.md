# Phase 5 — Expand Behavior Test Coverage

## Current State (as of v0.5.87)

The test infrastructure is mature:

- **501 transpiler Python tests** covering collect, resolve, emit, context, extract, model, e2e, roundtrip
- **46 behavior test `.m` files** across 9 categories: lifecycle (6), dispatch (5), edge (5), error (2), foundation (11), memory (6), properties (7), protocol (4), regression (0)
- **12 adapted upstream tests** (5 LLVM Rewriter, 5 GNUstep, 2 Apple spec)
- **17 golden test directories** including error cases and regression
- **21 Zephyr integration tests** (ztest on native_sim)
- **PAL tests** (slab, atomics, mem_blocks, lock)
- **CI pipeline**: 6 jobs (Python, behavior matrix GCC×Clang×O0×O2, ASan+UBSan, gcov, Zephyr, freshness)

## Gap Analysis

The transpiler now supports features that have **unit tests in `test_emit.py`
but NO compiled behavior tests** that prove the generated C actually works.
These are the coverage gaps, ordered by risk:

### High Priority — Features with complex codegen, no behavior test

| Feature | Transpiler tests | Behavior tests | Risk |
|---------|-----------------|----------------|------|
| `@synchronized` | 11 tests in test_emit | **0** | Spinlock acquire/release, early return, nested — easy to get wrong |
| `for-in` (IteratorProtocol) | 3 tests in test_emit | **0** | Iterator loop, typed var, struct prefix — scope/ARC interaction |
| Non-capturing blocks | 4 tests in test_emit | 1 (defer_block_ivar) | Function pointer emission, block naming |
| Multi-file transpilation | 6 tests in test_e2e | **0** | Cross-file class refs, inheritance, categories in separate files |
| Ivar access control | 5 tests in test_emit+e2e | **0** | protected/public/private rejection and acceptance |
| Enum collection | 14 tests in test_emit | **0** | Enum from header, transitive, no-duplicate, used in switch/ivar |
| Macro passthrough | 7 tests in test_emit | **0** | Object-like, function-like, stmt macros with ObjC args |

### Medium Priority — Features with some coverage, needs expansion

| Feature | Transpiler tests | Behavior tests | Gap |
|---------|-----------------|----------------|-----|
| Boxed expressions | 8 tests in test_emit | 1 (boxed_expression.m) | Missing: float boxing, enum boxing, call expr boxing |
| String dedup | 4 tests in test_emit | 0 direct | Dedup across methods and C functions |
| ARC scope exit | 20+ tests in test_emit | Covered via lifecycle/memory | break/continue/return in loops, nested scopes — could add explicit tests |
| Inline accessors | 6 tests in test_emit | 0 direct | OZArray count/objectAtIndex, OZString length/cString fast paths |
| Generics | 2 tests in test_e2e | **0** | Generic type mismatch rejection, correct acceptance |

### Low Priority — Already well covered or simple codegen

| Feature | Status |
|---------|--------|
| Static dispatch | 5 behavior tests ✓ |
| Protocol dispatch | 4 behavior tests ✓ |
| Properties (all variants) | 7 behavior tests ✓ |
| Retain/release/dealloc | 6 lifecycle + 6 memory tests ✓ |
| Nil messaging | 1 behavior test ✓ |
| Foundation classes | 11 behavior tests ✓ |

## Goal

Close the high-priority coverage gaps by adding **25–35 new behavior tests**
that compile and run the transpiler's C output for features that currently
only have Python-level unit tests. After this phase, every transpiler feature
has at least one compiled behavior test proving the generated code works.

## Deliverables

### Step 1 — `@synchronized` behavior tests (5 tests)

These exercise the OZSpinLock-based lowering of `@synchronized`. Each test
must actually acquire/release the lock and verify mutual exclusion semantics
(or at minimum, verify that the generated code compiles and runs without
crashing, since host PAL spinlocks are no-ops on single-threaded tests).

**Upstream sources to study for behavioral specs:**
- Clang Rewriter: `clang/test/Rewriter/objc-synchronized-1.m` (Apache 2.0)
  — validates that `@synchronized` lowers to `objc_sync_enter`/`objc_sync_exit`
  C calls with proper control flow. Adapt by verifying OZ lowers to
  `oz_spin_lock`/`oz_spin_unlock` with RAII guard.
- GNUstep libobjc2: `Test/Synchronized.m` (MIT) — tests mutual exclusion
  and recursive locking on the same object. Adapt the recursive-lock test
  pattern (same object locked twice → must not deadlock).

**`cases/synchronized/basic_lock.m`** — `@synchronized(self)` block executes
body and releases lock on exit. Verify by setting a flag inside the block
and checking it after.

**`cases/synchronized/early_return.m`** — `return` inside `@synchronized`
must release the lock before returning. Verify by checking a counter that
the lock was properly released (if lock tracking is available in the host PAL)
or at minimum that the code runs without hanging. (Pattern from Clang
Rewriter `objc-synchronized-1.m` which tests early exits.)

**`cases/synchronized/nested.m`** — Two nested `@synchronized` blocks with
different objects. Verify both bodies execute. (Pattern from GNUstep
`Synchronized.m` recursive locking tests.)

**`cases/synchronized/with_locals.m`** — `@synchronized` block with local
object variables. ARC must release locals when leaving the synchronized
scope (including on early return).

**`cases/synchronized/counter.m`** — `@synchronized` protecting a shared
counter. Increment in two sequential calls, verify final count. Tests the
sequential lock/unlock pattern even without threads.

Acceptance criteria:
- [ ] 5 `.m` files in `tests/behavior/cases/synchronized/`
- [ ] All compile and pass on host
- [ ] test_synchronized.py added to `tests/behavior/`
- [ ] `just test-behavior` passes

### Step 2 — `for-in` behavior tests (4 tests)

These exercise the IteratorProtocol-based lowering of `for (id x in coll)`.

**Upstream sources to study for behavioral specs:**
- Clang Rewriter: `clang/test/Rewriter/objc-modern-fast-enumeration.mm`
  (Apache 2.0) — validates that `for-in` lowers to C enumeration calls.
  Adapt by verifying OZ lowers to IteratorProtocol `iter`/`next` loop.
- ObjFW: OFArrayTests.m (LGPL-3.0, spec only) — tests enumeration of
  OFArray via `for-in`, with typed iteration variables and mutations
  during iteration (should trap). Use as behavioral spec for OZArray
  iteration contract.

**`cases/forin/basic_array.m`** — Iterate over an OZArray, sum integer
values (via OZQ31 or boxed numbers), verify sum. (Pattern from ObjFW
OFArrayTests enumeration.)

**`cases/forin/typed_var.m`** — `for (OZString *s in array)` with a typed
iteration variable. Verify each element is accessible with the concrete type.
(Pattern from Clang Rewriter `objc-modern-fast-enumeration.mm` typed var.)

**`cases/forin/break_in_forin.m`** — `break` inside for-in loop. Verify
iteration stops early AND ARC releases the iterator and any loop-scoped
locals. (No upstream equivalent — this tests OZ-specific ARC + iterator
interaction.)

**`cases/forin/nested_forin.m`** — Nested for-in loops. Verify inner and
outer iterators don't interfere.

Acceptance criteria:
- [ ] 4 `.m` files in `tests/behavior/cases/forin/`
- [ ] All compile and pass on host
- [ ] test_forin.py added to `tests/behavior/`

### Step 3 — Block behavior tests (3 tests)

These exercise non-capturing block lowering (block → static C function with
function pointer).

**Upstream sources to study for behavioral specs:**
- Clang Rewriter: `clang/test/Rewriter/blockcast3.mm`,
  `clang/test/Rewriter/blockstruct.m` (Apache 2.0) — validates that block
  expressions rewrite to C struct + function pointer patterns.
- GNUstep libobjc2: `Test/BlockTest_arc.m` (MIT) — tests block lifecycle
  under ARC including `__block` variable semantics.
- ObjFW uses blocks extensively as callbacks (e.g., `enumerateObjectsUsingBlock:`
  in OFArrayTests.m) — provides behavioral spec for block-as-method-parameter.

**`cases/blocks/non_capturing_basic.m`** — Define a block that takes an int
and returns an int. Pass it as a callback. Verify the callback executes
correctly. (Pattern from Clang Rewriter `blockcast3.mm`.)

**`cases/blocks/block_as_method_param.m`** — Method takes a block parameter.
Call the method, verify the block runs inside the method body. (Pattern from
ObjFW's `enumerateObjectsUsingBlock:` usage.)

**`cases/blocks/block_with_static_var.m`** — Block references a file-scope
`static` variable (the supported capture pattern). Modify the static var
from outside the block, verify the block sees the updated value. (OZ-specific
— `__block` promotes to file-scope `static`.)

Acceptance criteria:
- [ ] 3 `.m` files in `tests/behavior/cases/blocks/`
- [ ] All compile and pass on host

### Step 4 — Multi-file transpilation behavior tests (3 tests)

These exercise the transpiler's ability to handle classes spread across
multiple `.m` files. The `compile_and_run.py` script must support
multi-file input (pass multiple `.m` paths or a directory).

**`cases/multifile/cross_file_ref.m` + `cross_file_ref_helper.m`** — Class A
in one file references Class B in another. Verify both classes instantiate
and interact correctly.

**`cases/multifile/cross_file_inherit.m` + `cross_file_inherit_base.m`** —
Child class in one file inherits from parent in another. Verify method
dispatch resolves correctly across files.

**`cases/multifile/category_separate_file.m` + `category_separate_file_cat.m`** —
Category defined in a separate file from its class. Verify category methods
are callable.

Note: These tests may require extending `compile_and_run.py` to accept
multiple input files. Check if it already supports this; if not, add
`--extra-sources` flag.

Acceptance criteria:
- [ ] 3 test sets (6 `.m` files total) in `tests/behavior/cases/multifile/`
- [ ] `compile_and_run.py` supports multi-file input
- [ ] All compile and pass on host

### Step 5 — Enum and switch behavior tests (3 tests)

**`cases/enum/enum_in_switch.m`** — User-defined enum used in switch/case.
Verify all cases dispatch correctly.

**`cases/enum/enum_as_ivar.m`** — Enum used as an ivar type. Set and read
the enum value through property or direct access.

**`cases/enum/enum_from_header.m`** — Enum defined in a `.h` file, used in
the `.m` file's method body. Verify the transpiler collects and emits it.

Acceptance criteria:
- [ ] 3 `.m` files in `tests/behavior/cases/enum/`
- [ ] All compile and pass on host

### Step 6 — Macro passthrough behavior tests (2 tests)

**`cases/macro/object_macro.m`** — `#define MAX_COUNT 10` used in a method
body. Verify the macro value is correct at runtime.

**`cases/macro/function_macro_with_objc.m`** — Function-like macro that
takes an ObjC expression (e.g., `LOG_VALUE([obj value])`). Verify the
macro expands correctly and the ObjC expression evaluates.

Acceptance criteria:
- [ ] 2 `.m` files in `tests/behavior/cases/macro/`
- [ ] All compile and pass on host

### Step 7 — Inline accessor behavior tests (2 tests)

**`cases/inline/array_fast_access.m`** — Call `[array objectAtIndex:i]` and
`[array count]`. Verify the fast inline paths produce correct results.

**`cases/inline/string_fast_access.m`** — Call `[str cString]` and
`[str length]`. Verify the fast inline paths produce correct results.

Acceptance criteria:
- [ ] 2 `.m` files in `tests/behavior/cases/inline/`
- [ ] All compile and pass on host

### Step 8 — Boxed expression expansion (3 tests)

Existing `boxed_expression.m` covers basic int boxing. Add:

**Upstream sources:**
- Clang Rewriter: `clang/test/Rewriter/objc-modern-boxing.mm` and
  `clang/test/Rewriter/objc-modern-numeric-literal.mm` (Apache 2.0) —
  validate lowering of `@42`, `@3.14f`, `@(expr)` to factory method calls.
- Clang Rewriter: `clang/test/Rewriter/objc-bool-literal-modern.mm`,
  `objc-bool-literal-modern-1.mm`, `objc-bool-literal-check.mm` (Apache 2.0)
  — validate `@YES`/`@NO` lowering.

**`cases/edge/boxed_float.m`** — `@(3.14f)` → OZQ31 float path. Verify
float-to-fixed conversion is correct. (Pattern from Clang Rewriter
`objc-modern-numeric-literal.mm`.)

**`cases/edge/boxed_enum.m`** — `@(MyEnumValue)` → OZQ31 int32 path. Verify
enum constant is boxed correctly. (Pattern from Clang Rewriter
`objc-modern-boxing.mm`.)

**`cases/edge/boxed_call_expr.m`** — `@(getValue())` where `getValue`
returns int. Verify the call is evaluated and result boxed. (Pattern from
Clang Rewriter `objc-modern-boxing.mm` expression boxing.)

Acceptance criteria:
- [ ] 3 `.m` files in `tests/behavior/cases/edge/`
- [ ] All compile and pass on host

### Step 9 — ARC scope cleanup stress tests (4 tests)

These test the hardest ARC patterns — areas poorly covered by ALL upstream
suites. Apple's objc4 tests exercise these via runtime counters, but the
behavioral contract is clear: every `__strong` local must be released when
its scope exits, regardless of how the exit happens.

**Upstream behavioral specs (Apple objc4, APSL — derive only, no code copy):**
- `objc4/test/arc-dealloc.m` — verifies dealloc order with object ivars
- `objc4/test/arc-perf-*.m` — exercises retain/release in loops
- GNUstep libobjc2 `Test/WeakReferences_arc.m` (MIT) — weak zeroing during
  dealloc; load-and-retain semantics

**`cases/arc/break_releases_loop_local.m`** — Object allocated inside a
`for` loop body. `break` must release it before exiting. Verify via dealloc
counter on a tracking class.

**`cases/arc/continue_releases_loop_local.m`** — Same pattern but with
`continue`. Local must be released before the next iteration begins.

**`cases/arc/return_in_nested_scope.m`** — `return` inside an `if` inside
a `for` inside a method. All locals at every nesting level must be released
in reverse order. Verify dealloc order.

**`cases/arc/reassign_releases_old.m`** — `obj = [[Foo alloc] init]` then
`obj = [[Bar alloc] init]`. The first object must be released when the
second is assigned. Verify old object is deallocated.

Acceptance criteria:
- [ ] 4 `.m` files in `tests/behavior/cases/arc/`
- [ ] All pass under ASan (no leaks, no use-after-free)
- [ ] Dealloc counters verify correct cleanup order

### Step 10 — Leak detection infrastructure (mulle-testallocator pattern)

**Upstream source:** mulle-objc `mulle-testallocator` (BSD-3-Clause) —
hooks `mulle_malloc`/`mulle_free` and verifies zero outstanding allocations
at test exit. Activated via `MULLE_TESTALLOCATOR=YES` environment variable.

Adopt this pattern for the host PAL slab allocator. Add tracking to
`oz_platform_host.h`:

- Add `oz_slab_outstanding_count()` that returns `slab->num_used`
- Add `oz_slab_check_leaks(slab, name)` that prints a diagnostic and
  returns nonzero if `num_used > 0` at program exit
- Add `OZ_TEST_CHECK_LEAKS` environment variable: when set, the smoke
  test and behavior test harness call `oz_slab_check_leaks` after each test

This is NOT a test file — it's infrastructure that makes every existing
behavior test also a leak test. Extend `compile_and_run.py` to set the
env var and check for nonzero exit indicating a leak.

Acceptance criteria:
- [ ] `oz_slab_check_leaks` function exists in `oz_platform_host.h`
- [ ] `compile_and_run.py` supports `--check-leaks` flag
- [ ] All existing behavior tests pass with leak checking enabled
- [ ] Any test that leaks is either fixed or explicitly marked with a comment

### Step 11 — Add new tests to Zephyr integration suite

Select the most representative 5–8 new behavior tests from Steps 1–8
and add Zephyr integration equivalents in `tests/zephyr/`:

- At least 1 `@synchronized` test (proves real spinlock works)
- At least 1 `for-in` test (proves IteratorProtocol on real slab)
- At least 1 multi-file test (proves cross-file dispatch on target)
- At least 1 enum test (proves enum collection in Zephyr build)

Transpile the source `.m` files into `tests/zephyr/generated/`, add
corresponding `ZTEST()` test cases, and update `scripts/regen_zephyr_tests.py`.

Acceptance criteria:
- [ ] 5–8 new `ZTEST()` cases added
- [ ] `west twister -T tests/zephyr/ -p native_sim` passes
- [ ] `regen_zephyr_tests.py` updated and idempotent

### Step 12 — Update test counts and documentation

Update `CLAUDE.md` test infrastructure section with new counts.
Update `tests/ASSESSMENT.md` if any Bucket B tests were consumed.
Verify `just test-all-transpiler` passes end-to-end.

Acceptance criteria:
- [ ] `CLAUDE.md` reflects actual test counts
- [ ] All `just` test targets pass

## Upstream Source Reference

All upstream test patterns referenced in this phase:

| Source | License | Files Referenced | Used For |
|--------|---------|-----------------|----------|
| Clang Rewriter | Apache 2.0 + LLVM | `objc-synchronized-1.m`, `objc-modern-fast-enumeration.mm`, `objc-modern-boxing.mm`, `objc-modern-numeric-literal.mm`, `objc-bool-literal-*.mm`, `blockcast3.mm`, `blockstruct.m`, `func-in-impl.m` | @synchronized, for-in, boxing, blocks patterns |
| GNUstep libobjc2 | MIT | `Synchronized.m`, `BlockTest_arc.m`, `WeakReferences_arc.m` | Recursive locking, block lifecycle, weak refs |
| ObjFW | LGPL-3.0 (spec only) | `OFArrayTests.m`, `OFStringTests.m` | Enumeration patterns, block-as-callback |
| mulle-objc | BSD-3-Clause | `mulle-testallocator` | Leak detection infrastructure pattern |
| Apple objc4 | APSL (behavioral specs only) | `arc-dealloc.m`, `msgSend.m`, `arr-weak.m` | ARC scope cleanup, nil messaging, dealloc order |

## Expected outcome

After this phase:
- Behavior tests increase from **46 → ~79–89** `.m` files
- New categories added: `synchronized/` (5), `forin/` (4), `blocks/` (3),
  `multifile/` (3), `enum/` (3), `macro/` (2), `inline/` (2), `arc/` (4),
  plus 3 additional `edge/` tests
- Zephyr integration tests increase from **21 → ~26–29** ZTEST cases
- Leak detection infrastructure covers all tests automatically
- Every transpiler feature with ≥3 unit tests in `test_emit.py` has at least
  one compiled behavior test proving the generated C works on host
- Zero transpiler features remain "tested only at the Python level"
