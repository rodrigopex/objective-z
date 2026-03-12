# PLAN: `extract.py` + Slab Distribution Per-Class

## Context

Two changes that complement each other:

1. **`extract.py`** — Tree-sitter CST → Jinja2 template string. ObjC constructs become `{{ _n_L_C }}` placeholders with annotating comments about the original code; C code, comments, and whitespace are preserved verbatim. Context values are pre-rendered C from existing AST emitters.

2. **Slab distribution** — Move `ClassName_alloc()`/`ClassName_free()` and `OZ_SLAB_DEFINE` into per-class files. Eliminates `oz_mem_slabs.h/.c` as a monolithic central file. Each generated `.m` → `.c` becomes self-contained.

**Pipeline for user-class source files:**
```
.m source → tree-sitter → extract_template() → Jinja template string
                                                      ↓
Clang AST → collect() → resolve() → build_context() → context dict
                                                      ↓
                                      jinja2.render(template, context) → .c
```

Foundation classes, headers, dispatch tables continue using existing `.j2` file templates.

---

## Part A: `extract.py` — DONE

Committed as `2305c40`.

### What was implemented

- `extract.py` — tree-sitter CST → Jinja2 template string with annotating comments
- `context.py` — builds context dict mapping `_n_L_C` keys to rendered C from AST emitters
- `_emit_patched_source()` in `emit.py` rewired to use extract+context+render pipeline
- Handles: `@implementation`, `@interface`, `@protocol`, `@synthesize`, categories, instance_variables, class/instance method disambiguation, synthesized accessors, auto-dealloc, preamble (statics, string constants, block functions)

### Key design decisions

- `_impl_loc_key` — class-level preamble placeholder always emitted after `/* @implementation */` comment; holds statics, root retain/release, synthesized methods
- Method matching uses selector AND `is_class_method` flag (for `+greet` vs `-greet`)
- Auto-dealloc appended to last method's value, or preamble if no methods

---

## Part B: Slab Distribution Per-Class — DONE

Committed as `90b2670`.

### What was implemented

- Per-class `alloc/free` in `class_header.h.j2` + `OZ_SLAB_DEFINE` in `class_source.c.j2`
- `dispatch_free` + `oz_item_pool` moved to `oz_dispatch.h/.c`
- `oz_mem_slabs.h.j2` + `oz_mem_slabs.c.j2` deleted
- OZLock special case: `#include "platform/oz_lock.h"` instead of struct generation
- All golden files, tests, benchmarks updated

---

## Part C: Unit Tests + Cleanup — DONE

Committed as `bece655`.

### What was implemented

- C1: 44 unit tests for `extract.py` (helpers, templates, categories, fixtures)
- C2: 19 unit tests for `context.py` (includes, functions, declarations, methods, preamble, dealloc)
- C3: 3 fixture files (`extract_basic.m`, `extract_category.m`, `extract_multiclass.m`)
- C4: Removed ~87 lines dead code from `emit.py` (`_emit_class_methods`, `_extract_protocol_name`, 3 constants)
- C5: Documented preamble ordering with explicit comments + ordering test
- C6: Verified comment placement already correct (preamble before comments) + regression test

### Resolved limitations from prior analysis

- ~~L1: No unit tests for extract.py~~ → 44 tests
- ~~L2: No unit tests for context.py~~ → 19 tests
- ~~L3: Dead code in emit.py~~ → removed
- ~~L4: Preamble ordering fragile~~ → documented with comments + test
- ~~L5: Comments placement~~ → verified correct, regression test added
- ~~L6: No test fixtures~~ → 3 fixtures created

---

## Known Limitations (Post Part C)

### L7: Single-class-per-stem assumption unvalidated

`_emit_patched_source()` uses `classes[0]` for dependency includes (line ~2441). If a stem has multiple classes, sibling dependencies are missed. No assertion enforces one-class-per-stem.

### L8: Silent class-not-found in context.py

If `@implementation Foo` in source doesn't match any class in the module (collect.py bug), `_build_impl_context` sets `context[_impl_key] = ""` and returns. Methods vanish silently — no warning, no diagnostic.

### L9: Category double-processing risk

If `collect.py` fails to merge a category, context.py processes both `@implementation Foo` and `@implementation Foo (Category)` against the same `OZClass`. Category methods get lost because they have no matching `OZMethod` entries.

### L10: Include deduplication string-based

`_emit_patched_source` deduplicates includes by comparing stripped context values against `emitted_includes` set. No tolerance for whitespace or formatting differences.

### L11: `pool_count_fn` not validated

If `pool_count_fn` returns `None` or raises, `_emit_patched_source` crashes without useful context.

### L12: Unsupported AST expression kinds

`ObjCBoxedExpr`, `BlockExpr` without `BlockDecl`, and unknown expression kinds emit `/* TODO: ... */` comments in generated C. Code using these features won't compile.

### L13: Module diagnostics not surfaced

`ctx.module.diagnostics` is appended to (e.g., capturing blocks) but never checked after context building. Warnings are collected but not shown to user.

---

## Part D: Robustness + Validation

Unit tests mandatory for every step.

### Step D1: Assert single-class-per-stem

Add assertion in `_emit_patched_source()`:
```python
assert len(classes) >= 1, f"No classes for stem {stem}"
```

Aggregate dependency includes from ALL classes in the stem, not just `classes[0]`:
```python
dep_stems = set()
for cls in classes:
    dep_stems |= set(_dep_includes(cls, module, stem))
```

Tests:
- Verify assertion fires when `classes` is empty
- Verify multiple classes aggregate dependencies correctly

### Step D2: Warn on class-not-found in context.py

Add diagnostic when `@implementation` class name not found in module:
```python
if not cls:
    module.diagnostics.append(
        f"warning: @implementation {class_name} not found in module"
    )
```

Tests:
- Verify diagnostic is appended when class is unknown
- Verify preamble is empty string (existing behavior preserved)

### Step D3: Surface module diagnostics in `__main__.py`

After emit, check `module.diagnostics` and print warnings to stderr:
```python
for diag in module.diagnostics:
    print(f"oz_transpile: {diag}", file=sys.stderr)
```

If `--strict` mode, exit non-zero when diagnostics present.

Tests:
- Verify warnings printed to stderr
- Verify `--strict` exits non-zero on diagnostics

### Step D4: Validate `pool_count_fn` return

Wrap the call:
```python
pc = pool_count_fn(cls.name) if pool_count_fn else 1
if not isinstance(pc, int) or pc < 1:
    pc = 1
```

Tests:
- `pool_count_fn` returns `None` → defaults to 1
- `pool_count_fn` returns 0 → defaults to 1
- `pool_count_fn` returns valid int → used as-is

### Step D5: Category context integration test

Add test in `test_context.py` that verifies a category `@implementation Car (Maintenance)` correctly matches methods from the merged `OZClass`.

Tests:
- Category methods rendered with correct C signatures
- Category preamble key present

### Step D6: Include deduplication robustness

Normalize include comparison: strip whitespace, normalize quotes. Add test with whitespace variants.

Tests:
- `#include "Foo_ozh.h"` and `#include  "Foo_ozh.h"` deduplicated
- Self-include (`stem_ozh.h`) always deduplicated

---

## Part E: Round-Trip Integration Test — DONE

Committed as `TBD`.

### Step E1: Golden test for extract→context→render pipeline

No existing test takes a `.m` fixture + AST through the full `extract_template()` → `build_source_context()` → Jinja render pipeline and compares output character-by-character. Golden tests only cover the `.j2` template path (Foundation classes).

Add `test_roundtrip.py`:
- Hand-craft AST JSON for OZObject + OZBlinky matching `extract_basic.m`
- Go through `collect()` → `resolve()` → `emit()` with `source_paths` set
- Compare `OZBlinky_ozm.c` output against golden expected `.c` file
- Verify comments preserved, includes deduplicated, slab define present, auto-dealloc emitted

Tests:
- Output matches golden `.c` character-by-character (normalized)
- Key structural assertions: preamble header, slab define, comment preservation

---

## Execution Order

1. ~~**Part B**~~ DONE
2. ~~**Part A**~~ DONE
3. ~~**Part C**~~ DONE
4. ~~**Part D**~~ DONE
5. ~~**Part E**~~ DONE

## Verification

```
just test-transpiler   # all existing + new tests
just test              # full ARM twister suite
just test-behavior     # behavior tests
just test-adapted      # adapted tests
```
