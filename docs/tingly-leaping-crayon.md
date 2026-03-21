# OZ-063: Heap allocation via CONTAINER_OF + oz_metadata

## Context

All OZ objects use slab allocation. Need heap alternative for dynamic scenarios. Current partial implementation uses `_oz_heap` pointer in every object — causes template/struct layout issues. Redesign: use `CONTAINER_OF` pattern (heap header before object in memory) + rename `oz_class_id` → `struct oz_metadata` bitfield with `heap_allocated` flag. Also wire `deallocating` bit for re-entrant dealloc guard. Open separate issue for `singleton` bit.

## API (unchanged)

```objc
static char heapBuffer[2048];
static OZHeap *myHeap;

+ (void)initialize {
    myHeap = [OZHeap alloc];
    [OZHeap initHeap:myHeap buffer:heapBuffer size:sizeof(heapBuffer)];
}
- (void)run {
    Widget *w = [[Widget allocWithHeap: myHeap] init];   /* user heap */
    Widget *w2 = [[Widget allocWithHeap: nil] init];     /* system heap */
    Widget *w3 = [[Widget alloc] init];                  /* slab (default) */
}
```

## Design

### oz_metadata bitfield (replaces `enum oz_class_id oz_class_id`)

```c
struct oz_metadata {
    uint32_t class_id        : 10;  /* 0-9: max 1024 classes */
    uint32_t heap_allocated  :  1;  /* 10: free via CONTAINER_OF */
    uint32_t deallocating    :  1;  /* 11: re-entrant dealloc guard */
    uint32_t singleton       :  1;  /* 12: skip dealloc (separate issue) */
    uint32_t reserved        : 19;  /* 13-31 */
};
```

Root struct becomes:
```c
struct OZObject {
    struct oz_metadata _meta;
    oz_atomic_t _refcount;
};
```

### CONTAINER_OF heap header

```c
struct oz_heap_hdr {
    struct OZHeap *heap;   /* NULL=sys heap, non-NULL=user heap */
    char obj[];            /* flexible array — actual object follows */
};
```

Memory layout:
```
Slab:  [oz_metadata | _refcount | ...ivars...]
Heap:  [heap_ptr | oz_metadata | _refcount | ...ivars...]
                   ^-- returned pointer, CONTAINER_OF recovers heap_ptr
```

Alloc: `oz_heap_alloc(sizeof(hdr) + sizeof(obj))`, set `hdr->heap`, return `&hdr->obj`
Free: check `_meta.heap_allocated` → `CONTAINER_OF` → read `hdr->heap` → free via heap or sys_heap

### deallocating bit

In `dispatch_free` / dealloc chain: set `_meta.deallocating = 1` before calling dealloc. If already set, return immediately (prevents ARC cycles from double-freeing).

### OZHeap PAL struct (simplified — no `_oz_heap` field)

```c
struct OZHeap {
    struct oz_metadata _meta;
    oz_atomic_t _refcount;
    oz_heap_inner_t _inner;
};
```

## Steps

### 0. Revert current heap changes
Revert all uncommitted + committed heap changes (commits `d846595`, `28bc5d5`) to start fresh:
- `git reset HEAD~2` to unstage the two heap commits
- `git checkout -- .` to discard all modified files
- Keep `samples/heap_alloc/` directory (untracked, won't be affected by checkout)
- Verify clean state with `just test-transpiler`

### 1. PAL: `struct oz_metadata` + `struct oz_heap_hdr`
**Files**: `include/platform/oz_platform_types.h`, `oz_platform_zephyr.h`, `oz_platform_host.h`

- Add `struct oz_metadata` to `oz_platform_types.h` (shared between backends)
- Add `struct oz_heap_hdr` + `OZ_HEAP_HDR(obj)` macro under `#ifdef OZ_HEAP_SUPPORT`
  - Zephyr: use `CONTAINER_OF` from `<zephyr/sys/util.h>`
  - Host: define fallback `CONTAINER_OF` using `offsetof`
- Update `struct OZHeap` in both backends: remove `_oz_heap`, use `struct oz_metadata _meta` instead of `oz_atomic_t _oz_class_id`
- Remove `OZ_SYS_HEAP_SENTINEL` — no longer needed
- Keep existing `oz_heap_init`, `oz_heap_alloc_obj`, `oz_heap_free_obj`, `oz_sys_heap_alloc`, `oz_sys_heap_free`

### 2. Templates: `oz_class_id` → `_meta.class_id` rename

**File**: `tools/oz_transpile/templates/class_header.h.j2`
- Root struct: `enum oz_class_id oz_class_id;` → `struct oz_metadata _meta;`
- Remove `#ifdef OZ_HEAP_SUPPORT` / `struct OZHeap *_oz_heap` from root struct
- `_alloc()`: `{{ base_chain }}oz_class_id = OZ_CLASS_X` → `{{ base_chain }}_meta.class_id = OZ_CLASS_X`
- `_allocWithHeap_()`: rewrite using `oz_heap_hdr` + CONTAINER_OF pattern, set `_meta.heap_allocated = 1`
- `_free()`: check `_meta.heap_allocated` → `OZ_HEAP_HDR` → 2-way branch (heap vs sys_heap). Else slab.
- Remove ALL `{% if not (name == "OZHeap" and heap_support) %}` guards on alloc/free — no longer needed
- OZHeap block: keep struct skip + `initHeap` inline, remove custom alloc/free stubs

**File**: `tools/oz_transpile/templates/oz_dispatch.h.j2`
- Line 79: `((struct OZObject *)(obj))->oz_class_id` → `((struct OZObject *)(obj))->_meta.class_id`

**File**: `tools/oz_transpile/templates/oz_dispatch.c.j2`
- Line 33: `obj->oz_class_id` → `obj->_meta.class_id`
- Wire `deallocating` bit: check/set before dealloc dispatch

### 3. Transpiler emit: rename references
**File**: `tools/oz_transpile/emit.py`
- Line 391: `_root_builtins` set: `"oz_class_id"` → `"_meta"` (skip as ivar)
- Line 796: `self->oz_class_id` → `self->_meta.class_id` (in `_emit_root_introspection`)
- Remove `initHeap:buffer:size:` skip from `_class_header_ctx` — no longer needed
- Remove skip for `initHeap:buffer:size:` prototype (line ~413) — no longer needed if OZHeap alloc/free aren't special-cased

### 4. CMake: add OZHeap.m (already partially done)
**File**: `cmake/oz_transpile.cmake`
- Verify `_oz_heap_src` + conditional prepend is present

### 5. Behavior tests: update assertions
**Files**: `tests/behavior/cases/*/`
- `heap_alloc_test.c`: `((struct OZObject *)w)->oz_class_id` → `((struct OZObject *)w)->_meta.class_id`
- `alloc_returns_valid_test.c`: `w->base.oz_class_id` → `w->base._meta.class_id`
- `empty_class_no_methods_test.c`: same rename
- Rewrite heap test assertions for CONTAINER_OF pattern (no sentinel, check `heap_allocated` bit)
- Add `deallocating` test: trigger release cycle, verify no double-free

### 6. Zephyr tests: regenerate
**Dir**: `tests/zephyr/generated/`
- Delete and regenerate via transpiler (all `oz_class_id` refs auto-updated)

### 7. Sample: `samples/heap_alloc/`
- Already exists, verify it builds with new approach
- `prj.conf`: `CONFIG_OBJZ_HEAP=y`, `CONFIG_HEAP_MEM_POOL_SIZE=4096`

### 8. Open singleton issue
- Create GitHub issue for `singleton` bit implementation (skip dealloc for `+initialize` singletons)

## Key simplifications vs old approach

| Old (\_oz\_heap pointer) | New (CONTAINER\_OF) |
|---|---|
| `_oz_heap` field in every object | Zero overhead for slab objects |
| `OZ_SYS_HEAP_SENTINEL` 3-way branch | 1 bit check + 2-way branch |
| `base_chain._oz_heap` template hell | `_meta.heap_allocated` — works with any `base_chain` |
| OZHeap custom alloc/free stubs | OZHeap uses normal slab alloc/free |
| 6+ template guards for OZHeap | 1 guard (struct skip only) |

## Verification

```sh
just test-transpiler     # Python transpiler tests
just test-behavior       # Host behavior tests
just test                # Full twister suite (Zephyr)
just project_dir=samples/heap_alloc rebuild && just project_dir=samples/heap_alloc run
```

## Files

| File | Action |
|------|--------|
| `include/platform/oz_platform_types.h` | add `struct oz_metadata`, `struct oz_heap_hdr` |
| `include/platform/oz_platform_zephyr.h` | update OZHeap struct, remove sentinel |
| `include/platform/oz_platform_host.h` | update OZHeap struct, add CONTAINER_OF fallback |
| `tools/oz_transpile/templates/class_header.h.j2` | rename + rewrite alloc/free |
| `tools/oz_transpile/templates/oz_dispatch.h.j2` | rename |
| `tools/oz_transpile/templates/oz_dispatch.c.j2` | rename + deallocating guard |
| `tools/oz_transpile/emit.py` | rename refs |
| `cmake/oz_transpile.cmake` | verify OZHeap.m inclusion |
| `tests/behavior/cases/memory/heap_alloc_test.c` | update for new pattern |
| `tests/behavior/cases/lifecycle/alloc_returns_valid_test.c` | rename |
| `tests/behavior/cases/edge/empty_class_no_methods_test.c` | rename |
| `tests/zephyr/generated/` | regenerate all |
| `samples/heap_alloc/` | verify builds |
