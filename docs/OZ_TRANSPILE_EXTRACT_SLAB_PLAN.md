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

## Known Limitations

### L1: No unit tests for `extract.py`

`extract_template()` has no dedicated tests. Covered only indirectly by golden/e2e tests. Edge cases (empty `@implementation`, nested preprocessor, multiple classes in one file) are untested in isolation.

### L2: No unit tests for `context.py`

`build_source_context()` and `_build_impl_context()` have no dedicated tests. Method matching, synthesized accessor emission, preamble assembly — all untested in isolation.

### L3: Dead code in `emit.py`

Functions superseded by extract.py pipeline but not removed:
- `_emit_class_methods()` (~80 lines)
- `_extract_protocol_name()` (~8 lines)
- `_OBJC_INTERFACE_TYPES`, `_OBJC_IMPL_TYPES`, `_OBJC_SKIP_TYPES` constants

These are duplicated in `extract.py` and no longer called from `emit.py`.

### L4: Preamble ordering fragile

`_build_impl_context` appends to `_impl_` preamble in this order: statics → root introspection → string constants → block functions → synthesized methods → (maybe) auto-dealloc. Order is implicit, not enforced. If a block function references a string constant defined after it, compilation may fail.

### L5: Comments inside `@implementation` between methods

Comments between methods are preserved by `extract.py` but may appear BEFORE the preamble placeholder. If preamble emits statics or synthesized methods, comments can end up interleaved with generated code.

### L6: No test fixtures for extract+context pipeline

Original plan called for `OZBlinky.m` + `OZBlinky.ast.json` fixtures. Not created.

---

## Part C: Improvements

Unit tests are mandatory for every step.

### Step C1: Unit tests for `extract.py`

File: `tools/oz_transpile/tests/test_extract.py`

Tests:
- Basic `@implementation` → annotating comments + placeholders
- `@interface` → `/* @interface Foo — see Foo_ozh.h */`
- `@protocol` → `/* @protocol Bar — see oz_dispatch.h */`
- Category: `@implementation Car (Maintenance)` — no `(`, `)` leaked
- `@synthesize` inside `@implementation` — skipped (no output)
- Instance variables block — skipped
- Comments between methods — preserved verbatim
- `preproc_*` inside `@implementation` — preserved verbatim
- Top-level `#include`/`#import` → placeholder
- Top-level `function_definition` → placeholder
- Top-level `declaration` → placeholder
- Top-level C code — preserved verbatim
- Class method `+` vs instance method `-` — correct sign in annotating comment
- Empty `@implementation` (no methods) — preamble placeholder still present
- Multiple `@implementation` blocks in one file

### Step C2: Unit tests for `context.py`

File: `tools/oz_transpile/tests/test_context.py`

Tests:
- Include rewriting (`#import <Foundation/Foundation.h>` → empty, `#include "foo.h"` → preserved)
- Function definition → transpiled C
- Declaration: prototype → empty, collected static → empty, other → preserved
- Method body rendering — correct C signature
- Class method vs instance method — matched by selector + flag
- Synthesized accessor emission into preamble
- Auto-dealloc appended to last method
- Auto-dealloc in preamble when no methods
- Root class special methods skipped (`retain`, `release`, etc.)
- User dealloc handling
- String constants + block functions in preamble
- Static variables in preamble

### Step C3: Test fixtures

Files:
- `tools/oz_transpile/tests/fixtures/extract_basic.m` — minimal class with two methods, comment between them
- `tools/oz_transpile/tests/fixtures/extract_category.m` — category implementation
- `tools/oz_transpile/tests/fixtures/extract_multiclass.m` — interface + implementation + free function

These are for `test_extract.py` only (pure tree-sitter, no AST needed).

### Step C4: Dead code removal from `emit.py`

Remove from `emit.py`:
- `_emit_class_methods()` function
- `_extract_protocol_name()` function
- `_OBJC_INTERFACE_TYPES`, `_OBJC_IMPL_TYPES`, `_OBJC_SKIP_TYPES` constants

Verify: `just test-transpiler` + `just test`

### Step C5: Preamble ordering

Refactor `_build_impl_context` preamble assembly:
- Explicit ordered sections: (1) statics, (2) string constants, (3) block functions, (4) root retain/release + introspection, (5) synthesized methods, (6) auto-dealloc
- String constants always before block functions (dependency order)
- Unit test verifying order

### Step C6: Comment placement relative to preamble

Fix `_emit_impl_block` so comments between `@implementation` and first method appear AFTER the preamble placeholder, not before. This prevents generated preamble code from splitting user comments.

---

## Execution Order

1. ~~**Part B first**~~ DONE
2. ~~**Part A**~~ DONE
3. **Part C** — C1 → C2 → C3 → C4 → C5 → C6, commit after each green step

## Verification

```
just test-transpiler   # all existing + new tests
just test              # full ARM twister suite
just test-behavior     # behavior tests
just test-adapted      # adapted tests
```
