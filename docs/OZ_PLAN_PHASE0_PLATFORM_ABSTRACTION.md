# Phase 0 — Platform Abstraction Layer (PAL)

## Goal

Decouple all transpiler-generated C code from Zephyr RTOS so that the output
compiles and runs on **any POSIX host** (Linux, macOS) with zero modification.
The abstraction MUST be zero-cost: every PAL function is `static inline` and
vanishes completely at `-O1` or higher. Verify with `objdump`.

This phase is a prerequisite for ALL testing work. Nothing else starts until
this is done and validated.

## Context

The objective-z transpiler (`collect.py` → `resolve.py` → `emit.py`) converts
Objective-C to plain C. Currently `emit.py` generates direct Zephyr API calls
(`k_mem_slab_alloc`, `atomic_inc`, `printk`). This phase replaces those with
thin `oz_platform_*` wrappers that resolve to Zephyr or POSIX at compile time.

The transpiler output depends on exactly **three platform concerns**:

1. **Slab allocator** — `k_mem_slab` on Zephyr, `malloc` on host
2. **Atomic integers** — `atomic_t` for retain counts
3. **Atomic pointers** — `atomic_ptr_t` for singleton pattern
4. **Formatted output** — `printk` on Zephyr, `printf` on host

There is **no dependency** on `STRUCT_SECTION_ITERABLE`. Each class slab is
referenced by direct symbol name in the transpiled code — no iteration needed.

## Deliverables

### Step 0 — Assess existing runtime test suites

Before building any new infrastructure, catalogue the existing 18 runtime test
suites and classify each test file into one of three buckets. This assessment
drives how much new test code Phases 1–2 actually need to write.

**Procedure:**

1. List every `.m` test file in the existing test directories.
2. For each file, read the test and classify it:

   **Bucket A — Directly usable:** Tests that exercise language features the
   transpiler supports (object lifecycle, property access, static dispatch,
   retain/release, nil messaging, inheritance, categories, protocols) AND
   whose assertions check observable behavior (return values, side effects,
   output) rather than runtime internals. These `.m` files can be fed through
   the transpiler as-is and become behavior tests in Phase 2.

   **Bucket B — Adaptable:** Tests that exercise supported features BUT rely
   on runtime-specific APIs for verification (e.g., `class_getName`,
   `class_copyMethodList`, `object_getClass`, `protocol_conformsToProtocol`,
   or direct `objc_msgSend` calls). The test *logic* is valuable but the
   verification mechanism needs rewriting. Note what specific changes each
   test needs.

   **Bucket C — Not applicable:** Tests that exercise features the transpiler
   deliberately does not support: dynamic method resolution
   (`+resolveInstanceMethod:`), message forwarding (`-forwardInvocation:`),
   KVO, blocks, `@try/@catch`, runtime introspection, tagged pointers, or
   non-fragile ABI layout checks. These are out of scope for the transpiler.

3. Produce a summary file: `tests/ASSESSMENT.md` with a table:

```markdown
| Test file               | Bucket | Notes                                    |
|-------------------------|--------|------------------------------------------|
| test_alloc_dealloc.m    | A      | Direct lifecycle test, usable as-is      |
| test_property_access.m  | A      | Property getter/setter, usable as-is     |
| test_method_dispatch.m  | B      | Uses class_getName for verification      |
| test_kvo_observer.m     | C      | KVO — not supported by transpiler        |
| ...                     | ...    | ...                                      |
```

4. Rename (do NOT delete) the existing test directory:
   `tests/` → `tests/objc-reference/`
   This preserves every file in place while signaling these are the original
   runtime-era behavioral specifications.

5. Create `tests/objc-reference/README.md` explaining:
   - These tests were written for the objective-z runtime track
   - They serve as behavioral specifications for the transpiler
   - See `tests/ASSESSMENT.md` for which tests apply to the transpiler
   - The runtime code is available at tag `<insert tag name>`

**Why this matters:** The existing tests represent months of work encoding
Objective-C semantics on Zephyr. Bucket A tests give Phase 2 a head start —
instead of writing 28 behavior tests from scratch, you may already have 8–12
ready to go. Bucket B tests tell you exactly what adaptations are needed.
Bucket C tests become your compatibility matrix (documenting what the
transpiler intentionally does not support).

Acceptance criteria:
- [ ] `tests/ASSESSMENT.md` exists with every test file classified
- [ ] Existing test directory renamed to `tests/objc-reference/`
- [ ] `tests/objc-reference/README.md` explains provenance and links to tag
- [ ] Bucket counts are known (how many A, B, C)
- [ ] No test files deleted

### File structure (PAL headers)

```
include/oz/platform/
    oz_platform.h               ← single include point (ifdef router)
    oz_platform_zephyr.h        ← Zephyr backend (pass-through to Zephyr APIs)
    oz_platform_host.h          ← Host backend (POSIX / C11 standard library)
    oz_platform_types.h         ← Shared type aliases used by both backends
```

### Step 1 — Create `oz_platform_types.h`

Define return codes and forward declarations shared by both backends so that
transpiler output never includes Zephyr-specific error codes directly.

```c
#ifndef OZ_PLATFORM_TYPES_H
#define OZ_PLATFORM_TYPES_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

/* Standard return codes — match Zephyr's conventions */
#ifndef OZ_OK
#define OZ_OK       0
#endif
#ifndef OZ_ENOMEM
#define OZ_ENOMEM  (-12)  /* Same numeric value as POSIX ENOMEM */
#endif

#endif /* OZ_PLATFORM_TYPES_H */
```

Acceptance criteria:
- [ ] File exists at `include/oz/platform/oz_platform_types.h`
- [ ] No Zephyr includes, no POSIX includes — pure C types only

### Step 2 — Create `oz_platform_host.h`

This is the host/POSIX backend. It must be fully self-contained — compilable
with just a standard C11 compiler (gcc or clang) and no external dependencies.

**Allocator section** — malloc-backed slab with block-count tracking:

```c
#include <stdlib.h>
#include "oz_platform_types.h"

typedef struct {
    size_t    block_size;
    uint32_t  num_blocks;    /* configured maximum */
    uint32_t  num_used;      /* current count — NOT atomic; host tests are single-threaded */
} oz_slab_t;

/* Static initializer macro — mirrors K_MEM_SLAB_DEFINE */
#define OZ_SLAB_DEFINE(name, blk_size, n_blocks, alignment) \
    static oz_slab_t name = {                               \
        .block_size  = (blk_size),                          \
        .num_blocks  = (n_blocks),                          \
        .num_used    = 0                                    \
    }

static inline int oz_slab_alloc(oz_slab_t *slab, void **mem) {
    if (slab->num_used >= slab->num_blocks) {
        *mem = NULL;
        return OZ_ENOMEM;
    }
    *mem = malloc(slab->block_size);
    if (!*mem) return OZ_ENOMEM;
    slab->num_used++;
    return OZ_OK;
}

static inline void oz_slab_free(oz_slab_t *slab, void *mem) {
    free(mem);
    if (slab->num_used > 0) slab->num_used--;
}
```

**Atomic integer section** — C11 `<stdatomic.h>` for retain counts:

```c
#include <stdatomic.h>

typedef _Atomic(int) oz_atomic_t;

static inline void oz_atomic_init(oz_atomic_t *target, int val) {
    atomic_store(target, val);
}

static inline int oz_atomic_inc(oz_atomic_t *target) {
    return atomic_fetch_add(target, 1) + 1;  /* returns NEW value */
}

/* Returns true when count reached zero — caller must dealloc */
static inline bool oz_atomic_dec_and_test(oz_atomic_t *target) {
    return atomic_fetch_sub(target, 1) == 1;  /* old was 1 → now 0 */
}

static inline int oz_atomic_get(oz_atomic_t *target) {
    return atomic_load(target);
}
```

**Atomic pointer section** — for singleton pattern:

```c
typedef _Atomic(void *) oz_atomic_ptr_t;

static inline void *oz_atomic_ptr_get(oz_atomic_ptr_t *target) {
    return atomic_load(target);
}

static inline void oz_atomic_ptr_set(oz_atomic_ptr_t *target, void *value) {
    atomic_store(target, value);
}

/* Returns true if swap succeeded (old value matched expected) */
static inline bool oz_atomic_ptr_cas(oz_atomic_ptr_t *target,
                                     void *expected, void *desired) {
    void *exp = expected;  /* C11 takes pointer — local copy avoids side effects */
    return atomic_compare_exchange_strong(target, &exp, desired);
}
```

**Output section** — just printf:

```c
#include <stdio.h>

#define oz_platform_print(fmt, ...) printf(fmt, ##__VA_ARGS__)
```

Acceptance criteria:
- [ ] Compiles with `gcc -std=c11 -Wall -Werror` and `clang -std=c11 -Wall -Werror`
- [ ] No Zephyr includes anywhere in the file
- [ ] Header guard `OZ_PLATFORM_HOST_H` present

### Step 3 — Create `oz_platform_zephyr.h`

This is the Zephyr backend. Every function is a direct pass-through — the
compiler inlines them away completely.

**Allocator section** — direct k_mem_slab pass-through:

```c
#include <zephyr/kernel.h>
#include "oz_platform_types.h"

typedef struct k_mem_slab oz_slab_t;

#define OZ_SLAB_DEFINE(name, blk_size, n_blocks, alignment) \
    K_MEM_SLAB_DEFINE(name, blk_size, n_blocks, alignment)

static inline int oz_slab_alloc(oz_slab_t *slab, void **mem) {
    return k_mem_slab_alloc(slab, mem, K_NO_WAIT);
}

static inline void oz_slab_free(oz_slab_t *slab, void *mem) {
    k_mem_slab_free(slab, mem);
}
```

**Atomic integer section** — Zephyr atomic wrappers:

```c
#include <zephyr/sys/atomic.h>

typedef atomic_t oz_atomic_t;

static inline void oz_atomic_init(oz_atomic_t *target, atomic_val_t val) {
    atomic_set(target, val);
}

static inline atomic_val_t oz_atomic_inc(oz_atomic_t *target) {
    return atomic_inc(target) + 1;  /* Zephyr returns old value; we return new */
}

static inline bool oz_atomic_dec_and_test(oz_atomic_t *target) {
    return atomic_dec(target) == 1;  /* old was 1 → now 0 */
}

static inline atomic_val_t oz_atomic_get(oz_atomic_t *target) {
    return atomic_get(target);
}
```

**Atomic pointer section**:

```c
typedef atomic_ptr_t oz_atomic_ptr_t;

static inline void *oz_atomic_ptr_get(oz_atomic_ptr_t *target) {
    return atomic_ptr_get(target);
}

static inline void oz_atomic_ptr_set(oz_atomic_ptr_t *target, void *value) {
    atomic_ptr_set(target, value);
}

static inline bool oz_atomic_ptr_cas(oz_atomic_ptr_t *target,
                                     void *expected, void *desired) {
    return atomic_ptr_cas(target, expected, desired);
}
```

**Output section**:

```c
#include <zephyr/sys/printk.h>

#define oz_platform_print(fmt, ...) printk(fmt, ##__VA_ARGS__)
```

Acceptance criteria:
- [ ] Every function body is a single expression (guarantees inlining)
- [ ] Return-value conventions match host backend exactly (inc returns new, dec_and_test returns bool)
- [ ] Header guard `OZ_PLATFORM_ZEPHYR_H` present

### Step 4 — Create `oz_platform.h` (the router)

```c
#ifndef OZ_PLATFORM_H
#define OZ_PLATFORM_H

#include "oz_platform_types.h"

#ifdef OZ_PLATFORM_ZEPHYR
  #include "oz_platform_zephyr.h"
#elif defined(OZ_PLATFORM_HOST)
  #include "oz_platform_host.h"
#else
  #error "Define OZ_PLATFORM_ZEPHYR or OZ_PLATFORM_HOST"
#endif

#endif /* OZ_PLATFORM_H */
```

Acceptance criteria:
- [ ] `gcc -DOZ_PLATFORM_HOST -I include -c test.c` compiles
- [ ] `gcc -DOZ_PLATFORM_ZEPHYR -I include ...` compiles in a Zephyr build
- [ ] Forgetting to define either produces a clear `#error` message

### Step 5 — Update `emit.py` to use PAL

This is the critical integration step. Modify the transpiler's emit pass so
that generated C code uses `oz_platform.h` instead of Zephyr headers directly.

Search `emit.py` for these patterns and replace:

| Old (Zephyr-direct)                         | New (PAL)                              |
|----------------------------------------------|----------------------------------------|
| `#include <zephyr/kernel.h>`                 | `#include "oz/platform/oz_platform.h"` |
| `#include <zephyr/sys/atomic.h>`             | (covered by oz_platform.h)             |
| `#include <zephyr/sys/printk.h>`             | (covered by oz_platform.h)             |
| `K_MEM_SLAB_DEFINE(name, ...)`               | `OZ_SLAB_DEFINE(name, ...)`           |
| `k_mem_slab_alloc(&slab, &mem, K_NO_WAIT)`  | `oz_slab_alloc(&slab, &mem)`          |
| `k_mem_slab_free(&slab, mem)`               | `oz_slab_free(&slab, mem)`            |
| `atomic_inc(&obj->_rc)`                     | `oz_atomic_inc(&obj->_rc)`            |
| `atomic_dec(&obj->_rc)`                     | `oz_atomic_dec_and_test(&obj->_rc)`   |
| `atomic_get(&obj->_rc)`                     | `oz_atomic_get(&obj->_rc)`            |
| `atomic_ptr_cas(...)`                        | `oz_atomic_ptr_cas(...)`              |
| `printk(...)`                                | `oz_platform_print(...)`              |

Also update the type declarations emitted for instance variables:
- `atomic_t _rc;` → `oz_atomic_t _rc;`
- `atomic_ptr_t _shared;` → `oz_atomic_ptr_t _shared;`

Acceptance criteria:
- [ ] `emit.py` no longer generates any `#include <zephyr/...>` lines
- [ ] `emit.py` no longer generates any direct `k_mem_slab_*`, `atomic_*`, or `printk` calls
- [ ] Generated C code compiles with `-DOZ_PLATFORM_HOST` on a plain Linux/macOS machine
- [ ] Generated C code still compiles with `-DOZ_PLATFORM_ZEPHYR` in a Zephyr west build

### Step 6 — Write a smoke test

Create a minimal `.m` input file, transpile it, and compile the output on host:

```bash
# 1. Transpile
python -m oz_transpiler tests/smoke/SimpleClass.m -o /tmp/smoke_out/

# 2. Compile on host (must succeed with zero warnings)
gcc -std=c11 -Wall -Werror -DOZ_PLATFORM_HOST \
    -I include \
    /tmp/smoke_out/SimpleClass.c \
    -o /tmp/smoke_test

# 3. Run
/tmp/smoke_test
# Expected: allocates an object, calls a method, prints, deallocates, exits 0
```

The `SimpleClass.m` should exercise all four PAL surfaces:

```objc
@interface SimpleClass : OZObject
@property (nonatomic) int value;
- (void)printValue;
@end

@implementation SimpleClass
- (void)printValue {
    OZLog(@"Value is %d", self.value);
}
@end

int main(void) {
    SimpleClass *obj = [[SimpleClass alloc] init];
    obj.value = 42;
    [obj printValue];   // exercises oz_platform_print
    [obj release];      // exercises oz_atomic_dec_and_test + oz_slab_free
    return 0;
}
```

Acceptance criteria:
- [ ] Transpiler produces compilable C from this input
- [ ] Compiled binary runs and prints "Value is 42" on host
- [ ] Compiled binary exits with code 0
- [ ] Valgrind reports zero leaks (if available)

## Verification — zero-cost proof

After the Zephyr build still works, disassemble and confirm PAL vanished:

```bash
arm-none-eabi-objdump -d build/zephyr/zephyr.elf | grep oz_slab_alloc
# Expected: no results — function was inlined away

arm-none-eabi-objdump -d build/zephyr/zephyr.elf | grep oz_atomic
# Expected: no results — all inlined
```

If any `oz_` symbol appears as a call target, add `__attribute__((always_inline))`
to that specific function as a belt-and-suspenders fix.

## What this phase does NOT include

- Test framework setup (Phase 1)
- Golden file tests (Phase 1)
- Behavior tests (Phase 2)
- CI pipeline (Phase 3)
- Zephyr integration tests (Phase 4)

Those all depend on this phase being complete first.
