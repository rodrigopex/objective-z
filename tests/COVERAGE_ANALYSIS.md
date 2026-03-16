# Test Coverage Analysis

Analysis of current test coverage gaps and proposals for improvement.

## Current Test Inventory

### Transpiler Unit Tests (`tools/oz_transpile/tests/`)

| File | Module Under Test | Coverage |
|------|-------------------|----------|
| `test_model.py` | `model.py` | OZType, OZParam, OZIvar, OZMethod, OZProtocol, OZClass, OZModule |
| `test_collect.py` | `collect.py` | Interface, Implementation, Protocol, Category, CategoryImpl, StaticVar, TypeDefs, VerbatimLines, Property |
| `test_resolve.py` | `resolve.py` | Hierarchy validation, class IDs, base depth, dispatch classification, property synthesis, protocol conformance |
| `test_emit.py` | `emit.py` | Helpers, file structure, dispatch header, class header/source, body emission, ARC (local release, auto-dealloc, break/continue cleanup, autorelease pool, parameter retain, strong ivar assign, local reassign), introspection, static vars, synthesized properties, @synchronized, patched emission, protocol dispatch, enums, switch/case, user includes, inherited method cast, parent ivar access |
| `test_extract.py` | `extract.py` | Template extraction: loc keys, class/protocol/selector extraction, interface/protocol/implementation/category/synthesize/ivars/comments/preprocessor/function/declaration handling |
| `test_context.py` | `context.py` | Include rewriting, function definitions, declarations, method rendering, synthesized accessors, static vars, preamble ordering, auto-dealloc, user dealloc, unknown class, category context, pool count |
| `test_e2e.py` | `__main__.py` | CLI pipeline, dispatch header content, class struct hierarchy, super calls, pool sizes, retain/release in root, GCC syntax check, @synchronized pipeline |
| `test_roundtrip.py` | Full pipeline | Patched source output, comment preservation, preamble structure, interface replacement, import removal, auto-dealloc |
| `test_golden.py` | Golden file comparison | Snapshot regression tests |

### Behavior Tests (`tests/behavior/`)

| Category | Tests |
|----------|-------|
| **dispatch/** | class_method, inherited_method, method_override, send_routes_correct, super_calls_parent |
| **edge/** | deep_inheritance, empty_class_no_methods, multiple_args_method, nil_returns_zero |
| **error/** | release_nil_safe, slab_reuse_after_free |
| **lifecycle/** | alloc_failure_enomem, alloc_returns_valid, dealloc_frees_slab, double_release_guard, init_sets_fields |
| **memory/** | nested_retain_release, release_decrements, release_frees_at_zero, retain_count_query, retain_increments |
| **properties/** | atomic_property, custom_accessors, dot_syntax, getter_setter_gen, property_override, readonly_property, strong_vs_assign |
| **protocol/** | multiple_conformance, protocol_inheritance, switch_routes_correct, typed_protocol_var |

### Adapted Tests (`tests/adapted/`)

| Suite | Tests |
|-------|-------|
| **apple_spec/** | nil_return_types, struct_return |
| **gnustep/** | basic_messaging, category_method, inheritance_chain, nil_messaging, property_attribute |
| **llvm_rewriter/** | arc_insertion, class_rewrite, method_rewrite, property_rewrite, protocol_rewrite |

### Zephyr Integration Tests (`tests/zephyr/`)

| Test File | Coverage |
|-----------|----------|
| `test_dispatch.c` | Protocol dispatch on Zephyr target |
| `test_lifecycle.c` | Alloc/init/release on Zephyr target |
| `test_memory.c` | Retain/release on Zephyr target |
| `test_protocol.c` | Protocol conformance on Zephyr target |

---

## Coverage Gaps and Proposals

### 1. `collect.py` — Missing Coverage for `merge_modules()` and `is_stub_source()`

**Gap**: `merge_modules()` is a critical function for multi-file transpilation (used when multiple AST files are provided) but has **zero unit tests**. Similarly, `is_stub_source()` has no dedicated tests.

**Proposal**: Add `TestMergeModules` covering:
- Merging two modules with disjoint classes
- Merging modules with overlapping classes (superclass fill-in, ivar fill-in, method body_ast override)
- Protocol deduplication during merge
- Property merge behavior
- Diagnostics/errors accumulation across modules

Add `TestIsStubSource` covering:
- Foundation source detection
- User source detection

**Priority**: High — `merge_modules` is used in every multi-file build.

---

### 2. `__main__.py` — CLI Argument Parsing and `_associate_module_items_with_class()`

**Gap**: `_associate_module_items_with_class()` is untested. It handles associating free functions, statics, and verbatim lines with the correct class, or creating `OrphanSource` entries when no class exists. `parse_pool_sizes()` and `_source_stem()` also lack unit tests.

**Proposal**: Add tests covering:
- `parse_pool_sizes`: empty string, single pair, multiple pairs, whitespace handling
- `_source_stem`: basic path, nested path, .m extension
- `_associate_module_items_with_class`: module with items but no classes (orphan), module with one class that has implementations, module with no items (no-op), module with classes but no implementations (orphan path)
- CLI error paths: `--strict` with diagnostics, resolve errors returning exit code 1, `--manifest` file generation

**Priority**: High — these are core pipeline orchestration functions.

---

### 3. `emit.py` — Missing Edge Cases in Body Emission

**Gap**: While `test_emit.py` is the most comprehensive test file (3200+ lines), several AST node kinds that `emit.py` handles are not covered or lightly covered:
- **ObjCBoxedExpr** (boxed expressions like `@(value)`)
- **ObjCArrayLiteral** and **ObjCDictionaryLiteral** (collection literals)
- **ForIn loops** (fast enumeration transpilation)
- **ObjCBoolLiteralExpr** (`YES`/`NO`)
- **Block expressions** (error path — should emit diagnostic)
- **NullStmt** handling
- **StringLiteral** with special characters (escaping)

**Proposal**: Add targeted tests for each missing AST node type in body emission. These can be small, focused tests that construct AST fragments and verify the emitted C code.

**Priority**: Medium — most of these are covered transitively by e2e/golden tests, but unit-level coverage would catch regressions earlier.

---

### 4. Platform Abstraction Layer (PAL) — No Direct Tests

**Gap**: The PAL headers (`include/platform/oz_platform_host.h`, `oz_lock.h`, `oz_platform_types.h`) define the memory slab, atomic operations, spinlock, and assertion primitives used by all generated code. There are **no dedicated PAL unit tests**. The `just smoke` command exists but there's no corresponding test file in the repository.

**Proposal**: Create a host-side C test suite (`tests/pal/`) using Unity that directly exercises:
- `OZ_SLAB_DEFINE` / `oz_slab_alloc` / `oz_slab_free` — alloc, free, reuse, exhaustion
- `oz_atomic_inc` / `oz_atomic_dec_and_test` — basic atomics correctness
- `OZLock` — init, lock/unlock, RAII-style usage
- `OZ_ASSERT` — trigger and non-trigger paths
- Slab alignment and sizing

**Priority**: High — PAL is the foundation of all transpiled code. Bugs here affect everything.

---

### 5. Behavior Tests — Missing Foundation Class Coverage

**Gap**: The behavior tests cover dispatch, lifecycle, memory management, properties, and protocols well, but **Foundation classes are not tested at the behavior level**:
- **OZString** — no behavior tests for creation, `cStr`, `length`, `isEqual:`, immutable semantics
- **OZNumber** — no behavior tests for numeric boxing, `intValue`, `floatValue`, `isEqual:`, `hash`
- **OZArray** — no behavior tests for `@[...]` literals, `count`, `objectAtIndex:`, bounds
- **OZDictionary** — no behavior tests for `@{...}` literals, `count`, `objectForKey:`, missing keys
- **OZLog** — no behavior tests for `%@` format specifier

Per `tests/ASSESSMENT.md`, these are classified as Bucket A (directly usable patterns from the legacy runtime tests) but have not been adapted yet.

**Proposal**: Port the Bucket A test suites from the legacy runtime to compiled behavior tests:
- `test_string.py` + `cases/foundation/string_*.{m,c}`
- `test_number.py` + `cases/foundation/number_*.{m,c}`
- `test_array.py` + `cases/foundation/array_*.{m,c}`
- `test_dictionary.py` + `cases/foundation/dictionary_*.{m,c}`
- `test_log.py` + `cases/foundation/log_format.{m,c}`

**Priority**: High — Foundation classes are used by all user code and the ASSESSMENT.md already identifies these as ready-to-port.

---

### 6. Error Handling and Diagnostic Paths

**Gap**: The transpiler has extensive error and diagnostic reporting (unsupported init expressions, weak properties, missing protocol methods, duplicate methods, inheritance cycles, unsupported AST node kinds). While some are tested, many diagnostic code paths are not:
- `_UNSUPPORTED_METHOD_SELECTORS` filtering in `collect.py` (KVO methods, `forwardInvocation:`)
- `_UNSUPPORTED_AST_KINDS` filtering (`ObjCAtTryStmt`)
- `--strict` mode returning exit code 1 on diagnostics
- Error accumulation during `emit` phase (boxed expr errors, capturing block errors)
- Multiple errors in a single transpilation run

**Proposal**: Add a `TestDiagnostics` class in `test_collect.py` and a `TestCLIErrors` class in `test_e2e.py` covering:
- Each unsupported selector being silently skipped
- `@try` statements producing diagnostics
- `--strict` mode with warnings failing the build
- Error exit codes for various failure modes

**Priority**: Medium — these are defensive paths, but important for user-facing error messages.

---

### 7. Multi-File / Multi-Class Transpilation

**Gap**: E2E tests only test single-file transpilation (`simple_led.ast.json`, `synchronized_sample.ast.json`). Real projects use multiple `.m` files that get merged. No tests exercise:
- Multiple AST inputs being merged correctly
- Cross-file class references (class in file A, subclass in file B)
- Category in a separate file from the base class
- `OrphanSource` emission (standalone C functions without ObjC classes)
- `--sources` flag mapping source paths to AST files

**Proposal**: Create multi-file fixture pairs and an E2E test class `TestMultiFileTranspilation` that exercises the full `main()` pipeline with 2-3 AST files and corresponding sources.

**Priority**: High — this is the real-world usage pattern and is currently untested end-to-end.

---

### 8. `context.py` — Missing Multi-Class Source Context

**Gap**: `test_context.py` tests single-class scenarios. It does not cover:
- Source file with two `@implementation` blocks (two classes in one .m file)
- Source with category `@implementation Car (Maintenance)` alongside regular `@implementation`
- Interaction between user-defined dealloc and auto-dealloc in multi-class contexts

**Proposal**: Add `TestMultiClassContext` exercising source files with multiple `@implementation` blocks and verifying each gets its own correct context entries.

**Priority**: Medium — single-class-per-file is the common pattern, but multi-class is supported.

---

### 9. Zephyr Integration Tests — Limited Scope

**Gap**: The Zephyr integration tests (`tests/zephyr/`) only cover basic dispatch, lifecycle, memory, and protocol. Missing:
- Foundation classes on Zephyr (OZString, OZNumber, OZArray, OZDictionary)
- Properties on Zephyr
- `@synchronized` on Zephyr (uses OZLock → spinlock)
- Deep inheritance chains on Zephyr
- Slab pool exhaustion behavior on Zephyr

**Proposal**: Extend the Zephyr test suite with additional test files:
- `test_foundation.c` — basic OZString/OZNumber on Zephyr
- `test_properties.c` — property accessors on Zephyr
- `test_synchronized.c` — lock-based synchronization

**Priority**: Low — these are integration tests and slower to run. Host-side behavior tests should be prioritized first.

---

### 10. `extract.py` — Tree-Sitter Edge Cases

**Gap**: While `test_extract.py` is thorough, it doesn't cover:
- Nested categories (category on a category-extended class)
- `@dynamic` declarations (currently untested whether they're stripped)
- Error recovery: malformed ObjC syntax
- Multiple `@synthesize` in same `@implementation`
- `extern` declarations at file scope

**Proposal**: Add edge case tests for uncommon but valid ObjC patterns.

**Priority**: Low — these are rare patterns but could cause silent bugs.

---

## Summary Table

| # | Area | Priority | Estimated Effort |
|---|------|----------|------------------|
| 1 | `merge_modules()` unit tests | High | Small |
| 2 | `__main__.py` helpers + CLI error paths | High | Small |
| 3 | `emit.py` missing AST node types | Medium | Medium |
| 4 | PAL host-side unit tests | High | Medium |
| 5 | Foundation class behavior tests | High | Large |
| 6 | Error/diagnostic path tests | Medium | Small |
| 7 | Multi-file E2E tests | High | Medium |
| 8 | Multi-class context tests | Medium | Small |
| 9 | Extended Zephyr integration tests | Low | Large |
| 10 | `extract.py` edge cases | Low | Small |
