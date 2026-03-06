# OZ Transpiler — Claude Code Implementation Plan

## Overview

Integrate a Python-based Objective-C → C transpiler into the `objective-z` project. The transpiler eliminates the Objective-C runtime entirely, generating plain C that any `arm-none-eabi-gcc` can compile. Dispatch uses macro-based token pasting (static) and `switch(class_id)` inline functions (polymorphic) — no vtables, no global dispatch table, no per-object overhead.

**Pipeline:**
```
foo.m → clang -Xclang -ast-dump=json → foo.ast.json → oz_transpile → foo.h + foo.c
```

**Repository:** `github.com/rodrigopex/objective-z`
**Branch:** Create `feature/transpiler` from `main`

---

## Phase 1 — Scaffold the transpiler package

**Goal:** Drop the `oz_transpile/` Python package into the repo with working tests.

### Task 1.1 — Create directory structure

```
tools/oz_transpile/
├── __init__.py
├── __main__.py          # CLI entry point
├── model.py             # Data model (OZClass, OZMethod, OZType, etc.)
├── collect.py           # Pass 1: Clang JSON AST → OZModule
├── resolve.py           # Pass 2: Hierarchy, dispatch decisions, validation
├── emit.py              # Pass 3: OZModule → C source files
├── tests/
│   ├── __init__.py
│   ├── test_collect.py
│   ├── test_resolve.py
│   ├── test_emit.py
│   └── test_e2e.py
└── README.md
```

- Copy `model.py`, `collect.py`, `resolve.py`, `emit.py` from the prototype (provided in this conversation).
- Move test files into `tests/` subdirectory and update imports.
- Ensure all tests pass: `python -m pytest tools/oz_transpile/tests/ -v`

### Task 1.2 — Create `__main__.py` CLI

```python
"""
CLI usage:
    # Single file
    python -m oz_transpile --input foo.ast.json --outdir generated/

    # Multiple files (whole-program analysis)
    python -m oz_transpile --input a.ast.json b.ast.json --outdir generated/

    # Generate AST first, then transpile
    clang -Xclang -ast-dump=json -fsyntax-only \
        -I${ZEPHYR_BASE}/include foo.m > foo.ast.json
    python -m oz_transpile --input foo.ast.json --outdir generated/
"""
```

Implement with `argparse`. Flags:
- `--input` (required): One or more `.ast.json` files
- `--outdir` (required): Output directory for generated `.c`/`.h` files
- `--verbose`: Print collect/resolve summary and diagnostics
- `--strict`: Treat warnings as errors (exit 1)
- `--root-class`: Name of root class (default: `OZObject`)

The CLI must:
1. Load and merge all input AST files into a single `OZModule`
2. Run `resolve()` and print diagnostics
3. Run `emit()` and write files to `--outdir`
4. Exit 0 on success, 1 on errors

### Task 1.3 — Add `pyproject.toml`

Create `tools/oz_transpile/pyproject.toml` with:
- No external dependencies (stdlib only — json, argparse, dataclasses, enum, pathlib)
- Python >= 3.11 (for `slots=True` on dataclasses)
- Entry point: `oz-transpile = "oz_transpile.__main__:main"`

---

## Phase 2 — Test against real objective-z source files

**Goal:** Run the transpiler on the actual `.m` files from the `objc/` and `samples/` directories.

### Task 2.1 — Create a `justfile` recipe for AST generation

Add to the existing `justfile`:

```just
# Generate Clang JSON AST dumps for all .m files
ast-dump:
    mkdir -p build/ast
    for f in $(find objc samples -name '*.m'); do \
        echo "AST: $f"; \
        clang -Xclang -ast-dump=json -fsyntax-only \
            -fobjc-arc \
            -I${ZEPHYR_BASE}/include \
            -include zephyr/kernel.h \
            "$f" > "build/ast/$(basename $f .m).ast.json" 2>/dev/null || true; \
    done

# Run the transpiler on all AST dumps
transpile: ast-dump
    python -m oz_transpile \
        --input build/ast/*.ast.json \
        --outdir build/generated \
        --verbose
```

Note: The `clang` invocation will likely fail on Zephyr headers that aren't available outside a west build. This is expected — the goal is to see which AST nodes the real code produces and identify gaps in the collector.

### Task 2.2 — Create a minimal standalone test `.m` file

Create `tools/oz_transpile/tests/fixtures/oz_led_sample.m`:

```objc
#import <stdint.h>

@protocol OZToggleable
- (void)toggle;
@end

@interface OZObject {
    uint32_t oz_refcount;
    uint16_t oz_class_id;
}
- (instancetype)retain;
- (void)release;
- (void)dealloc;
@end

@interface OZLed : OZObject <OZToggleable> {
    int _pin;
}
@property (nonatomic) int pin;
- (instancetype)initWithPin:(int)pin;
- (void)toggle;
- (int)brightness;
@end

@implementation OZLed
- (instancetype)initWithPin:(int)pin {
    _pin = pin;
    return self;
}
- (void)toggle {
    // gpio_pin_toggle_dt(&led_spec);
}
- (int)brightness {
    return _pin * 10;
}
@end

@interface OZRgbLed : OZLed
- (void)toggle;
- (void)setColorR:(int)r g:(int)g b:(int)b;
@end

@implementation OZRgbLed
- (void)toggle {
    [self setColorR:0 g:0 b:0];
}
- (void)setColorR:(int)r g:(int)g b:(int)b {
    // pwm_set_rgb(r, g, b);
}
@end
```

Add a test that:
1. Runs `clang -Xclang -ast-dump=json -fsyntax-only` on this file
2. Feeds the real AST into the pipeline
3. Verifies the generated C compiles with `gcc -fsyntax-only -Wall`

### Task 2.3 — Fix collector gaps from real AST

The real Clang AST will contain nodes the prototype doesn't handle yet. Likely gaps:

- `ObjCIvarRefExpr` (direct ivar access like `_pin`) — emit as `self->_pin`
- `PseudoObjectExpr` wrapping property accesses
- `OpaqueValueExpr` in compound assignments
- `ImplicitCastExpr` chains (LValueToRValue, etc.)
- `ObjCStringLiteral` (`@"hello"`) — emit as `"hello"` (C string)
- `CompoundAssignOperator` (`+=`, `-=`)
- `NullStmt` (empty statement)
- `BreakStmt`, `ContinueStmt`

For each gap:
1. Add a test case in `test_collect.py` with the AST node
2. Update `collect.py` or `emit.py` to handle it
3. Verify the generated C compiles

---

## Phase 3 — Emit Zephyr-specific constructs

**Goal:** The generated C files integrate with Zephyr's build system and memory model.

### Task 3.1 — Emit `K_MEM_SLAB_DEFINE` with sizing from AST analysis

Currently the `.h` files have a commented-out `K_MEM_SLAB_DEFINE`. Make this real:

- In `emit.py`, generate a top-level `oz_mem_slabs.h`:

```c
/* oz_mem_slabs.h — Auto-generated pool definitions */
#pragma once
#include <zephyr/kernel.h>
#include "OZLed.h"
#include "OZRgbLed.h"

#ifndef OZ_OZLED_MAX
#define OZ_OZLED_MAX 4
#endif

K_MEM_SLAB_DEFINE(oz_slab_OZLed, sizeof(struct OZLed), OZ_OZLED_MAX, 4);
K_MEM_SLAB_DEFINE(oz_slab_OZRgbLed, sizeof(struct OZRgbLed), OZ_OZRGBLED_MAX, 4);
```

- The `OZ_<CLASS>_MAX` defaults come from a new CLI flag `--pool-sizes OZLed=4,OZRgbLed=2` or from a `oz_config.json` file.
- Also generate `alloc`/`free` functions per class:

```c
static inline struct OZLed *OZLed_alloc(void) {
    struct OZLed *obj;
    if (k_mem_slab_alloc(&oz_slab_OZLed, (void **)&obj, K_NO_WAIT) != 0) {
        return NULL;
    }
    memset(obj, 0, sizeof(*obj));
    obj->base.oz_class_id = OZ_CLASS_ID_OZLed;
    obj->base.oz_refcount = 1;
    return obj;
}

static inline void OZLed_free(struct OZLed *obj) {
    k_mem_slab_free(&oz_slab_OZLed, (void *)obj);
}
```

### Task 3.2 — Emit retain/release with `k_mem_slab` free

Generate `OZObject_retain` and `OZObject_release` in `OZObject.c`:

```c
struct OZObject *OZObject_retain(struct OZObject *self) {
    __ASSERT_NO_MSG(self != NULL);
    atomic_inc((atomic_t *)&self->oz_refcount);
    return self;
}

void OZObject_release(struct OZObject *self) {
    __ASSERT_NO_MSG(self != NULL);
    if (atomic_dec((atomic_t *)&self->oz_refcount) == 1) {
        /* Dispatch dealloc — may be overridden */
        OZ_PROTOCOL_SEND(dealloc, self);
    }
}
```

Note: `dealloc` must be in the dynamic dispatch set since subclasses override it.

### Task 3.3 — Emit `[super ...]` as direct parent call

The emit pass currently handles `receiverKind: "super"`. Verify it works for:

```objc
- (instancetype)initWithPin:(int)pin {
    self = [super init];
    if (self) { _pin = pin; }
    return self;
}
```

Expected C:
```c
struct OZLed *OZLed_initWithPin(struct OZLed *self, int pin) {
    self = (struct OZLed *)OZObject_init((struct OZObject *)self);
    if (self) { self->_pin = pin; }
    return self;
}
```

Add a test case with a `[super init]` pattern in the AST fixture.

### Task 3.4 — Emit `oz_class_id` initialization in alloc

Every alloc function must set `oz_class_id` so protocol dispatch works:

```c
obj->base.oz_class_id = OZ_CLASS_ID_OZLed;
```

For subclasses with deeper inheritance, chain through base:
```c
obj->base.base.oz_class_id = OZ_CLASS_ID_OZRgbLed;
```

The emit pass should compute the depth by walking `cls.superclass` and emit the correct number of `.base` dereferences.

---

## Phase 4 — CMake integration

**Goal:** The transpiler runs as a CMake custom command in the Zephyr build.

### Task 4.1 — Create `cmake/oz_transpile.cmake` module

```cmake
# oz_transpile.cmake — integrate transpiler into Zephyr build
#
# Usage in app CMakeLists.txt:
#   include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/oz_transpile.cmake)
#   oz_transpile(
#     SOURCES src/OZLed.m src/OZRgbLed.m
#     OUTPUT_DIR ${CMAKE_CURRENT_BINARY_DIR}/generated
#     POOL_SIZES OZLed=4 OZRgbLed=2
#   )

find_program(CLANG_EXE clang REQUIRED)
find_program(PYTHON_EXE python3 REQUIRED)

function(oz_transpile)
    cmake_parse_arguments(OZ "" "OUTPUT_DIR" "SOURCES;POOL_SIZES" ${ARGN})

    set(AST_DIR "${CMAKE_CURRENT_BINARY_DIR}/oz_ast")
    file(MAKE_DIRECTORY ${AST_DIR})
    file(MAKE_DIRECTORY ${OZ_OUTPUT_DIR})

    set(AST_FILES "")
    foreach(src ${OZ_SOURCES})
        get_filename_component(basename ${src} NAME_WE)
        set(ast_file "${AST_DIR}/${basename}.ast.json")
        list(APPEND AST_FILES ${ast_file})

        add_custom_command(
            OUTPUT ${ast_file}
            COMMAND ${CLANG_EXE}
                -Xclang -ast-dump=json -fsyntax-only
                -fobjc-arc
                "$<TARGET_PROPERTY:app,INCLUDE_DIRECTORIES>"
                ${src}
                > ${ast_file}
            DEPENDS ${src}
            COMMENT "AST dump: ${src}"
        )
    endforeach()

    # Join pool sizes
    set(POOL_ARG "")
    if(OZ_POOL_SIZES)
        list(JOIN OZ_POOL_SIZES "," POOL_JOINED)
        set(POOL_ARG "--pool-sizes" "${POOL_JOINED}")
    endif()

    add_custom_command(
        OUTPUT ${OZ_OUTPUT_DIR}/oz_dispatch.h
        COMMAND ${PYTHON_EXE} -m oz_transpile
            --input ${AST_FILES}
            --outdir ${OZ_OUTPUT_DIR}
            --strict
            ${POOL_ARG}
        DEPENDS ${AST_FILES}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/tools
        COMMENT "oz_transpile: generating C from ObjC"
    )

    # Add generated sources to the Zephyr app
    file(GLOB GEN_SOURCES "${OZ_OUTPUT_DIR}/*.c")
    target_sources(app PRIVATE ${GEN_SOURCES})
    target_include_directories(app PRIVATE ${OZ_OUTPUT_DIR})
endfunction()
```

### Task 4.2 — Create a sample app using the transpiler

Create `samples/transpiled_led/`:

```
samples/transpiled_led/
├── CMakeLists.txt
├── prj.conf
├── src/
│   ├── OZLed.m          # Objective-C source
│   └── main.c           # Pure C, calls generated functions
└── README.md
```

`CMakeLists.txt`:
```cmake
cmake_minimum_required(VERSION 3.20)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(transpiled_led)

include(${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/oz_transpile.cmake)

oz_transpile(
    SOURCES src/OZLed.m
    OUTPUT_DIR ${CMAKE_CURRENT_BINARY_DIR}/generated
    POOL_SIZES OZLed=1
)

target_sources(app PRIVATE src/main.c)
```

`src/main.c`:
```c
#include "OZLed.h"
#include "oz_mem_slabs.h"

void main(void) {
    struct OZLed *led = OZLed_alloc();
    if (led) {
        OZLed_initWithPin(led, 13);
        OZ_SEND(OZLed, toggle, led);
        printk("Brightness: %d\n", OZ_SEND(OZLed, brightness, led));
        OZLed_free(led);
    }
}
```

Build target: `west build -b qemu_cortex_m3 samples/transpiled_led`

---

## Phase 5 — ARC-style ownership in generated C

**Goal:** Emit retain/release calls at scope boundaries, matching ARC semantics without the Clang ARC runtime hooks.

### Task 5.1 — Scope analysis in emit pass

Add a new sub-pass in `emit.py` that walks `stmt_ast` and inserts:
- `OZObject_retain()` after every assignment to an object-typed local variable
- `OZObject_release()` at every scope exit (end of `CompoundStmt`, early `return`)
- Skip for `__unsafe_unretained` typed variables (check `OZType.raw_qual_type`)

This is conceptually simple because the Clang AST already marks which variables have object types and which casts are ARC-relevant (`ImplicitCastExpr` with `ARCConsumeObject`, etc.).

### Task 5.2 — Emit `__attribute__((cleanup))` scope guards

As an alternative/complement to explicit release calls, emit cleanup attributes:

```c
static inline void oz_release_cleanup(struct OZObject **p) {
    if (*p) OZObject_release(*p);
}

#define OZ_STRONG __attribute__((cleanup(oz_release_cleanup)))

void some_function(void) {
    OZ_STRONG struct OZLed *led = OZLed_alloc();
    // ... use led ...
    // led is automatically released when it goes out of scope
}
```

This matches the existing `OZ_SCOPED_PUSH` pattern already established in objective-z.

### Task 5.3 — Leak detection integration

Emit test helpers that use the `k_mem_slab` free count:

```c
#define OZ_LEAK_CHECK_BEGIN(cls) \
    uint32_t _oz_free_##cls = oz_slab_##cls.info.num_free

#define OZ_LEAK_CHECK_END(cls) \
    __ASSERT(oz_slab_##cls.info.num_free == _oz_free_##cls, \
             #cls " leaked %d objects", \
             _oz_free_##cls - oz_slab_##cls.info.num_free)
```

---

## Phase 6 — Documentation and repo push

### Task 6.1 — Write `tools/oz_transpile/README.md`

Cover:
- What the transpiler does (one-paragraph overview)
- Pipeline diagram (ASCII art of the flow)
- Installation (just Python 3.11+, no deps)
- CLI usage with examples
- Generated file descriptions
- Dispatch strategy explanation with ARM instruction counts
- Known limitations

### Task 6.2 — Update top-level `README.rst` → `README.md`

The current README is a minimal Zephyr template. Replace with:
- Project description: what objective-z is and the transpiler approach
- Quick start (build a sample)
- Architecture overview
- Benchmarks placeholder
- Link to `tools/oz_transpile/README.md`

### Task 6.3 — Push to `feature/transpiler` branch

```bash
git checkout -b feature/transpiler
git add tools/oz_transpile/ cmake/ samples/transpiled_led/
git commit -m "feat: add ObjC→C transpiler with macro dispatch

- Three-pass pipeline: collect (Clang AST) → resolve (hierarchy) → emit (C)
- Static dispatch via OZ_SEND() token pasting (zero overhead)
- Protocol dispatch via switch(class_id) inline functions
- No runtime, no vtables, no global dispatch table
- K_MEM_SLAB integration for zero-heap allocation
- CMake integration for Zephyr builds"
git push origin feature/transpiler
```

---

## Execution order for Claude Code

When implementing, follow this sequence. Each step should be a separate Claude Code invocation with the instruction to read this plan first.

1. **Phase 1.1–1.3**: Scaffold package, copy prototype files, get tests green
2. **Phase 2.2**: Create the standalone `.m` fixture and real-AST test
3. **Phase 2.3**: Fix collector gaps discovered by the real AST (iterative)
4. **Phase 3.1–3.2**: Emit `K_MEM_SLAB`, alloc/free, retain/release
5. **Phase 3.3–3.4**: Handle `[super ...]` and `oz_class_id` init depth
6. **Phase 4.1–4.2**: CMake module and sample app
7. **Phase 5.1–5.3**: ARC scope analysis (can be deferred)
8. **Phase 6.1–6.3**: Docs and push

Each phase should end with all existing tests still passing plus new tests for the added functionality.

---

## Key design constraints (reference for all phases)

- **Zero heap allocation** — `k_mem_slab` only, pool sizes known at compile time
- **Deterministic dispatch** — static calls = direct `BL`; protocol calls = bounded `switch`
- **No Clang runtime dependency** — generated C compiles with any C11 compiler
- **ISR safety** — generated code must not call blocking functions; alloc functions use `K_NO_WAIT`
- **No `__weak` support** — use `__unsafe_unretained` for back-pointers
- **Mixed compilation** — the transpiler output must coexist with hand-written C in the same Zephyr app
- **OZ prefix** on all generated symbols
- **Python 3.11+, no external dependencies** — the transpiler runs in any west/Zephyr environment
