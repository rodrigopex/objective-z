# Phase 6 — Adapt Bucket B Reference Tests and Deepen Upstream Coverage

## Current State

The `tests/ASSESSMENT.md` classifies existing runtime-era tests:

- **Bucket A (8 suites, directly usable):** refcount, protocols, literals,
  string, number, array, dictionary, mutable_string — these have already been
  leveraged for foundation behavior tests.
- **Bucket B (9 suites, adaptable):** message_dispatch, class_registry,
  categories, arc, arc_intensive, static_pools, memory, hash_table,
  flat_dispatch — valuable behavioral specs that need verification API
  replacement (strip runtime introspection, replace with observable-behavior
  assertions).
- **Bucket C (2 suites, not applicable):** blocks (capturing), gpio — out of
  scope.

Adapted upstream tests currently: 5 LLVM Rewriter, 5 GNUstep, 2 Apple spec =
**12 total**. The upstream suites have hundreds more applicable tests.

**Prerequisite:** Phase 5 (expanded behavior coverage) complete, so the test
infrastructure supports all current transpiler features.

## Goal

Mine the 9 Bucket B reference suites and the upstream LLVM/GNUstep suites for
additional behavioral specifications. Produce **15–25 new adapted tests** that
validate transpiler correctness against patterns proven over decades of
Objective-C runtime testing.

## Deliverables

### Step 1 — Audit Bucket B suites for extractable test patterns

For each of the 9 Bucket B suites, read the source `.m` files in
`tests/objc-reference/runtime/` and identify specific test patterns that
verify **observable behavior** once the runtime introspection is removed.

Produce `tests/adapted/BUCKET_B_AUDIT.md`:

```markdown
| Suite | Source file | Pattern | Adaptable? | Adaptation needed |
|-------|-------------|---------|------------|-------------------|
| arc | main.m | Scope-exit releases local | Yes | Remove objc_stats, use Unity assert on dealloc counter |
| arc | main.m | Property setter retains new, releases old | Yes | Direct retain count check via oz_atomic_get |
| arc_intensive | main.m | Nested retain/release with ivars | Yes | Strip objc_stats, add dealloc flag |
| categories | main.m | Category method callable | Yes | Replace class_respondsToSelector with direct call |
| message_dispatch | main.m | Method dispatches to correct impl | Yes | Replace objc_msg_lookup with OZ_SEND verification |
| flat_dispatch | main.m | Hierarchy + category dispatch | Yes | Replace objc_msg_lookup with behavior check |
| ...
```

Focus on patterns where the *behavioral assertion* (did the right method run?
did the object get freed? did the property retain?) is separable from the
*verification mechanism* (runtime API calls).

Acceptance criteria:
- [ ] `BUCKET_B_AUDIT.md` covers all 9 suites
- [ ] Each suite has at least 2 extractable patterns identified (or documented as "no extractable patterns")
- [ ] Estimated adaptation effort noted per pattern (trivial / moderate / complex)

### Step 2 — Adapt ARC reference tests (4 tests)

The `runtime/arc` and `runtime/arc_intensive` suites are the richest Bucket B
sources. They test the exact patterns the transpiler must handle:
`__attribute__((cleanup))` lowering, ivar release in dealloc, property
retain/release, scope-exit cleanup.

**`tests/adapted/bucket_b/arc_scope_cleanup.m`** — Derived from `runtime/arc`.
Object local variable released when scope exits. Verify via dealloc counter.

**`tests/adapted/bucket_b/arc_property_retain.m`** — Derived from `runtime/arc`.
Setting a `strong` property retains the new value and releases the old.
Verify via retain count queries.

**`tests/adapted/bucket_b/arc_ivar_dealloc.m`** — Derived from `runtime/arc`.
When an object with object-typed ivars is deallocated, those ivars are released.
Verify via dealloc counter on the ivar objects.

**`tests/adapted/bucket_b/arc_intensive_nested.m`** — Derived from
`runtime/arc_intensive`. Complex nested retain/release pattern with multiple
object locals and ivars. Verify final retain counts and dealloc order.

Each file includes provenance header:

```c
/*
 * Adapted from: tests/objc-reference/runtime/arc/main.m
 * Adaptation: Removed objc_stats/objc_retain/objc_release introspection.
 *             Replaced with dealloc counter flag and oz_atomic_get on _rc.
 *             Pattern: scope-exit cleanup of local object variables.
 */
```

Acceptance criteria:
- [ ] 4 `.m` + `_test.c` pairs in `tests/adapted/bucket_b/`
- [ ] Provenance headers present
- [ ] All compile and pass on host via adapted test pipeline

### Step 3 — Adapt dispatch reference tests (3 tests)

The `runtime/message_dispatch`, `runtime/flat_dispatch`, and
`runtime/hash_table` suites test method dispatch patterns.

**`tests/adapted/bucket_b/dispatch_hierarchy.m`** — Derived from
`runtime/message_dispatch`. Parent and child classes, method override,
`[super method]`. Verify correct method runs by checking side effects.

**`tests/adapted/bucket_b/dispatch_category_merge.m`** — Derived from
`runtime/categories`. Category adds method to class. Verify the category
method is callable and returns the expected value.

**`tests/adapted/bucket_b/dispatch_flat_chain.m`** — Derived from
`runtime/flat_dispatch`. Multi-level hierarchy with category methods at
different levels. Verify dispatch resolution follows the correct precedence.

Acceptance criteria:
- [ ] 3 `.m` + `_test.c` pairs in `tests/adapted/bucket_b/`
- [ ] No runtime introspection calls remain (no objc_msg_lookup, no class_respondsToSelector)

### Step 4 — Adapt pool/slab reference tests (2 tests)

The `runtime/static_pools` and `runtime/memory` suites test allocation
patterns.

**`tests/adapted/bucket_b/slab_pool_reuse.m`** — Derived from
`runtime/static_pools`. Allocate, free, re-allocate from the same slab.
Verify the re-allocation succeeds (proves slab block was returned).

**`tests/adapted/bucket_b/slab_exhaustion_recovery.m`** — Derived from
`runtime/memory`. Exhaust slab, free one, allocate again. Verify the
sequence works without crash.

Acceptance criteria:
- [ ] 2 `.m` + `_test.c` pairs in `tests/adapted/bucket_b/`

### Step 5 — Expand LLVM Rewriter adapted tests (5 tests)

The current 5 LLVM Rewriter tests cover class rewrite, property rewrite,
protocol rewrite, method rewrite, and ARC insertion. Expand to cover
newer transpiler features using specific Clang Rewriter test files:

**`tests/adapted/llvm_rewriter/synchronized_rewrite.m`** — Adapted from
`clang/test/Rewriter/objc-synchronized-1.m`. Verify `@synchronized`
lowering produces correct lock/unlock C structure with RAII guard.

**`tests/adapted/llvm_rewriter/forin_rewrite.m`** — Adapted from
`clang/test/Rewriter/objc-modern-fast-enumeration.mm`. Verify `for-in`
lowering produces correct IteratorProtocol loop.

**`tests/adapted/llvm_rewriter/block_rewrite.m`** — Adapted from
`clang/test/Rewriter/blockcast3.mm` and `blockstruct.m`. Verify
non-capturing block lowering produces correct function pointer C code.

**`tests/adapted/llvm_rewriter/boxing_rewrite.m`** — Adapted from
`clang/test/Rewriter/objc-modern-boxing.mm` and
`objc-modern-numeric-literal.mm`. Verify `@42`, `@(expr)` lowering produces
correct OZQ31 factory calls.

**`tests/adapted/llvm_rewriter/func_in_impl_rewrite.m`** — Adapted from
`clang/test/Rewriter/func-in-impl.m`. Verify C functions defined inside
`@implementation` are preserved verbatim in output.

Acceptance criteria:
- [ ] 5 new `.m` + `_test.c` pairs in `tests/adapted/llvm_rewriter/`
- [ ] Provenance headers referencing specific Clang Rewriter files (Apache 2.0)

### Step 6 — Expand GNUstep adapted tests (3 tests)

**`tests/adapted/gnustep/synchronized_recursive.m`** — Adapted from
`Test/Synchronized.m` (MIT). Tests that `@synchronized` on the same object
from the same thread doesn't deadlock (recursive lock). Verify sequential
re-entry works.

**`tests/adapted/gnustep/category_properties.m`** — Adapted from
`Test/category_properties.m` (MIT). Properties declared in categories.
Verify getter/setter are generated and functional.

**`tests/adapted/gnustep/nil_msgSend_types.m`** — Adapted from
`Test/objc_msgSend.m` (MIT). Nil messaging returns `0` for int, `0.0` for
float, `nil` for object, zeroed struct for struct return. The behavioral
pattern from GNUstep's nil-check assertions ports directly to OZ_SEND nil
guard verification.

Acceptance criteria:
- [ ] 3 new `.m` + `_test.c` pairs in `tests/adapted/gnustep/`
- [ ] MIT license provenance headers

### Step 7 — ObjFW Foundation-class behavioral specs (5 tests) ← NEW

ObjFW (LGPL-3.0) provides the richest Foundation-class test suites, with
direct class-to-class mappings to objective-z's Foundation layer. These
tests are written as **original code inspired by ObjFW behavioral specs**,
NOT code copied from ObjFW (to avoid LGPL obligations).

**Key ObjFW test files to study for behavioral specs:**
- `OFStringTests.m` (1,862 lines) — equality, hashing, length, encoding,
  substring, trim, replace, comparison. Maps to OZString.
- `OFArrayTests.m` (~400-600 lines) — indexing, count, containsObject,
  enumeration, sorting. Maps to OZArray.
- `OFDictionaryTests.m` (~400-600 lines) — key-value store/retrieve, key
  enumeration, containsKey. Maps to OZDictionary.
- `OFObjectTests.m` (~200-300 lines) — alloc/init/dealloc, equality/hash
  contract, description. Maps to OZObject.
- `OFNumberTests.m` (~200-300 lines) — numeric boxing, type conversion,
  arithmetic identity. Maps to OZQ31.

**`tests/adapted/objfw_spec/string_equality_hash.m`** — Behavioral spec
from OFStringTests.m. Two strings with same content must be `isEqual:` and
have the same `hash`. OZString constant and dynamically created OZString
must compare equal. Verify the equality/hash contract.

**`tests/adapted/objfw_spec/string_length_cstring.m`** — Behavioral spec
from OFStringTests.m. UTF-8 string `length` returns character count (or
byte count per OZ convention). `cString` returns null-terminated C string.
Verify round-trip: create from cString, read back cString, compare.

**`tests/adapted/objfw_spec/array_index_count.m`** — Behavioral spec from
OFArrayTests.m. Create array literal `@[a, b, c]`. Verify `count` returns 3.
Verify `objectAtIndex:0` returns `a`, `objectAtIndex:2` returns `c`.
Verify out-of-bounds returns nil (OZ convention) without crash.

**`tests/adapted/objfw_spec/dictionary_store_retrieve.m`** — Behavioral spec
from OFDictionaryTests.m. Create dictionary `@{key: val}`. Verify
`objectForKey:` returns the value. Verify `objectForKey:` with missing key
returns nil. Verify `count` is correct.

**`tests/adapted/objfw_spec/object_equality_contract.m`** — Behavioral spec
from OFObjectTests.m. Every object `isEqual:` to itself. Two distinct objects
are not equal unless their class defines otherwise. `hash` is consistent with
`isEqual:` (equal objects must have equal hashes).

Each file includes:

```c
/*
 * Behavioral spec derived from: ObjFW OFStringTests.m
 * ObjFW license: LGPL-3.0-only
 * This test is ORIGINAL CODE — no ObjFW code was copied.
 * The behavioral contract tested here (equality/hash, indexing, etc.)
 * is part of the Objective-C language specification, not ObjFW IP.
 */
```

Acceptance criteria:
- [ ] 5 `.m` + `_test.c` pairs in `tests/adapted/objfw_spec/`
- [ ] README clearly states no ObjFW code was copied
- [ ] Tests validate OZString, OZArray, OZDictionary, OZObject behavioral contracts
- [ ] ObjFW's CustomString wrapper pattern noted for possible future adoption

### Step 8 — mulle-objc MRR lifecycle patterns (3 tests) ← NEW

mulle-objc (BSD-3-Clause) is the only major ObjC runtime that **exclusively
uses manual retain/release** (no ARC ever). Its lifecycle patterns are
directly relevant because objective-z's ARC lowering must produce code that
follows the same retain/release balance rules.

**Key mulle-objc patterns to study:**
- Two-phase teardown: `-finalize` releases properties while object alive,
  `-dealloc` handles non-property ivars
- `mulle-testallocator` leak detection at test exit
- Explicit `+initialize` / `+deinitialize` lifecycle

**`tests/adapted/mulle_spec/retain_release_balance.m`** — Behavioral spec.
Every `retain` must have a matching `release`. Create object, retain 3x,
release 3x, verify dealloc fires on 4th release (original retain from alloc).
Verify retain count at each step via `oz_atomic_get`.

**`tests/adapted/mulle_spec/ivar_release_in_dealloc.m`** — Behavioral spec
derived from mulle's two-phase teardown. Object with strong ivar pointing to
another object. When outer object is released, inner object's retain count
must decrease. Verify via dealloc counter.

**`tests/adapted/mulle_spec/init_deinit_lifecycle.m`** — Behavioral spec.
`+initialize` runs before first use. Object lifecycle from alloc through
init through dealloc follows deterministic order. Verify via ordered flag
sequence.

Each file includes:

```c
/*
 * Behavioral spec derived from: mulle-objc runtime lifecycle patterns
 * mulle-objc license: BSD-3-Clause
 * This test is ORIGINAL CODE inspired by mulle-objc's MRR conventions.
 */
```

Acceptance criteria:
- [ ] 3 `.m` + `_test.c` pairs in `tests/adapted/mulle_spec/`
- [ ] BSD-3-Clause provenance headers
- [ ] Tests focus on retain/release balance and deterministic lifecycle

### Step 9 — Expand Apple spec behavioral tests (3 tests)

Current Apple spec tests: struct_return, nil_return_types. Add:

**`tests/adapted/apple_spec/retain_cycle_detection.m`** — Behavioral spec
derived from Apple's ARC documentation. Create an intentional retain cycle
(A holds strong ref to B, B holds strong ref to A). Verify that using
`__unsafe_unretained` on one reference breaks the cycle and both objects
dealloc correctly.

**`tests/adapted/apple_spec/property_attributes.m`** — Behavioral spec
derived from Apple's property documentation and `objc4/test/propertyattr.m`
and `objc4/test/accessors.m` behavioral contracts. Verify `nonatomic` vs
`atomic` getter/setter behavior, `readonly` enforcement, `assign` vs
`strong` retain semantics.

**`tests/adapted/apple_spec/weak_zeroing_dealloc.m`** — Behavioral spec
derived from `objc4/test/arr-weak.m` (329 lines). When an object is
deallocated, any weak references to it must read as nil. OZ may not support
`__weak` yet — if not, mark this test as a specification for future
implementation and add to backlog.

Acceptance criteria:
- [ ] 3 new `.m` + `_test.c` pairs in `tests/adapted/apple_spec/`
- [ ] README disclaiming APSL code derivation remains accurate
- [ ] Tests are entirely original code

### Step 10 — Update documentation and counts

- Update `tests/adapted/README.md` with new test inventory including
  ObjFW and mulle-objc sections
- Update `tests/ASSESSMENT.md` to mark consumed Bucket B patterns
- Update `CLAUDE.md` test section with new counts
- Verify `just test-adapted` passes end-to-end

Acceptance criteria:
- [ ] All documentation reflects actual test counts
- [ ] `just test-adapted` passes with all new tests

## Upstream Source Summary

| Source | License | Adaptations in This Phase | New Test Count |
|--------|---------|--------------------------|----------------|
| LLVM Clang Rewriter | Apache 2.0 + LLVM | @synchronized, for-in, blocks, boxing, func-in-impl | 5 |
| GNUstep libobjc2 | MIT | Recursive @synchronized, category properties, nil messaging | 3 |
| ObjFW | LGPL-3.0 (spec only) | String, Array, Dictionary, Object behavioral contracts | 5 |
| mulle-objc | BSD-3-Clause | Retain/release balance, ivar release, init lifecycle | 3 |
| Apple objc4 | APSL (spec only) | Retain cycle, property attributes, weak zeroing | 3 |
| Bucket B reference | (internal) | ARC scope, property retain, ivar dealloc, dispatch patterns | 9 |

## Expected outcome

After this phase:
- Adapted tests increase from **12 → ~40–45**
- Bucket B coverage moves from **0 adapted → 9 adapted** patterns
- The test suite draws from **six distinct sources**: original behavior tests,
  Bucket B reference tests, LLVM/GNUstep upstream, ObjFW Foundation specs,
  mulle-objc MRR patterns, and Apple behavioral specifications
- OZString, OZArray, OZDictionary have behavioral contract tests derived from
  ObjFW's extensive Foundation test suite
- Each adapted test has clear provenance documentation and license compliance