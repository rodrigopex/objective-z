# Plan: Compile-Time Dispatch via Token Concatenation (Vtable Elimination)

## Motivation

The current dispatch system uses **runtime vtable arrays** (`OZ_fn_sel OZ_vtable_sel[OZ_CLASS_COUNT]`) populated by a `__attribute__((constructor))` function. This costs:

- **RAM**: one function-pointer per class per protocol selector (e.g., 10 classes × 8 selectors = 80 pointers = 320–640 bytes on ARM/RISC-V)
- **Startup time**: constructor must run before `main()`
- **Indirection**: every protocol call is `vtable[class_id](obj, ...)` — an indirect function-pointer call that defeats branch prediction and is opaque to LTO

### Goal

Replace runtime vtable arrays with **compile-time token-concatenation macros** (Zephyr DTS style). All dispatch resolves to direct function calls or compile-time `#error` when a method is missing. **Zero RAM overhead, zero startup cost.**

---

## Current vs Proposed

### Current (runtime vtable)

```c
/* oz_dispatch.h */
typedef int (*OZ_fn_color)(struct OZObject *);
extern OZ_fn_color OZ_vtable_color[OZ_CLASS_COUNT];
#define OZ_SEND_color(obj) \
    OZ_vtable_color[((struct OZObject *)(obj))->oz_class_id]((struct OZObject *)(obj))

/* oz_dispatch.c */
OZ_fn_color OZ_vtable_color[OZ_CLASS_COUNT];
__attribute__((constructor))
static void oz_register_vtables(void) {
    OZ_vtable_color[OZ_CLASS_Circle] = (OZ_fn_color)Circle_color;
    OZ_vtable_color[OZ_CLASS_Square] = (OZ_fn_color)Square_color;
}
```

### Proposed (compile-time concatenation)

```c
/* oz_dispatch.h */

/* --- Method-existence map (one #define per class×selector pair) --- */
#define OZ_IMPL_Circle_color       Circle_color
#define OZ_IMPL_Square_color       Square_color
#define OZ_IMPL_OZObject_color     OZ_NOT_IMPLEMENTED(OZObject, color)

/* Fallback: compile-time error if called on a class that doesn't implement it */
#define OZ_NOT_IMPLEMENTED(cls, sel) \
    _Static_assert(0, #cls " does not implement " #sel)

/* --- Static dispatch (receiver type known at transpile time) --- */
#define OZ_SEND(cls, sel, self, ...) \
    OZ_IMPL_##cls##_##sel((struct cls *)(self), ##__VA_ARGS__)

/* --- Protocol dispatch (receiver type unknown, resolved at runtime) --- */
#define OZ_PROTOCOL_SEND_color(obj) ({                          \
    struct OZObject *_oz_r = (struct OZObject *)(obj);          \
    (OZ_PROTOCOL_RESOLVE_color[_oz_r->oz_class_id])(_oz_r);    \
})

/* Protocol tables are ONLY generated for true runtime-polymorphic calls */
extern const OZ_fn_color OZ_PROTOCOL_RESOLVE_color[OZ_CLASS_COUNT];
```

**Key insight**: the transpiler already classifies every call site — it knows whether the receiver type is statically inferrable or not. We exploit this:

| Call-site kind | Current | Proposed |
|---|---|---|
| **STATIC** (type known) | Direct call `Circle_color(self)` | Same — no change |
| **PROTOCOL, type inferrable** | `OZ_SEND_color(obj)` → vtable lookup | `OZ_SEND(Circle, color, obj)` → direct call via `##` |
| **PROTOCOL, truly polymorphic** | `OZ_SEND_color(obj)` → vtable lookup | `OZ_PROTOCOL_SEND_color(obj)` → `const` table lookup |

Most "PROTOCOL" calls in practice have an inferrable receiver type at the call site. Only truly polymorphic sites (e.g., iterating a heterogeneous collection typed as `id`) need runtime dispatch.

---

## Detailed Design

### Phase 1 — `OZ_SEND` for statically-typed protocol calls (biggest win)

**Transpiler changes (`emit.py`):**

1. At every PROTOCOL call site, attempt to infer the concrete receiver class (the infrastructure for this already exists in `_infer_receiver_class()`).
2. If the type **is** inferrable → emit `OZ_SEND(ClassName, selector, receiver, args...)`.
3. If the type is **not** inferrable → emit `OZ_PROTOCOL_SEND_selector(receiver, args...)`.

**Template changes (`oz_dispatch.h.j2`):**

1. Generate `OZ_IMPL_<Class>_<sel>` macros for every class×selector combination:
   - If class (or ancestor) implements it → `#define OZ_IMPL_Circle_color Circle_color`
   - If not implemented → `#define OZ_IMPL_Circle_color OZ_MISSING_Circle_color`
   - And: `#define OZ_MISSING_Circle_color(...) OZ_COMPILE_ERROR(Circle, color)`
2. Generate the `OZ_SEND` concatenation macro.
3. For unresolved calls, `OZ_COMPILE_ERROR` expands to `_Static_assert(0, ...)` or a `#error`-guarded function.

**Template changes (`oz_dispatch.c.j2`):**

1. Remove mutable vtable arrays and constructor.
2. If there are any protocol selectors that still need runtime dispatch (Phase 2), generate `const` arrays instead.

### Phase 2 — `OZ_PROTOCOL_SEND` for truly polymorphic calls (fallback)

For call sites where the receiver type cannot be inferred:

1. Keep a **`const` function-pointer array** (not mutable + constructor):
   ```c
   const OZ_fn_color OZ_PROTOCOL_RESOLVE_color[OZ_CLASS_COUNT] = {
       [OZ_CLASS_OZObject] = (OZ_fn_color)OZ_MISSING_OZObject_color,
       [OZ_CLASS_Circle]   = (OZ_fn_color)Circle_color,
       [OZ_CLASS_Square]   = (OZ_fn_color)Square_color,
   };
   ```
2. These go in `.rodata` (flash on embedded) → **zero RAM cost**.
3. The constructor function is eliminated entirely.
4. `OZ_PROTOCOL_SEND_color(obj)` uses a statement-expression to evaluate `obj` once, then indexes the const array.

### Phase 3 — Resolve pass enhancement

Introduce a third dispatch kind to distinguish the two protocol sub-cases:

```python
class DispatchKind(Enum):
    STATIC = "static"             # Only one class implements; direct call
    PROTOCOL_STATIC = "proto_s"   # Protocol method, but type known at call site
    PROTOCOL_DYNAMIC = "proto_d"  # Protocol method, type unknown → const table
```

In `resolve.py`, keep method-level classification as `PROTOCOL`. In `emit.py`, refine **per call site**:
- If `_infer_receiver_class()` succeeds → emit `OZ_SEND`
- Otherwise → emit `OZ_PROTOCOL_SEND`

### Phase 4 — `dispatch_free` refactor

Current `dispatch_free` uses a `switch` on `oz_class_id`. Refactor to use the same `OZ_SEND` pattern:
- `dispatch_free(obj)` → `OZ_PROTOCOL_SEND_dealloc(obj)` (dealloc is always protocol)
- Or if the type is known at the call site: `OZ_SEND(Circle, dealloc, obj)`

### Phase 5 — `oz_class_id` field reduction (optional future)

If **all** protocol calls become statically resolved (no `OZ_PROTOCOL_SEND` remaining), the `oz_class_id` field in `struct OZObject` can be removed entirely — saving 1–4 bytes per object instance. This is an optional future optimization gated on whether truly polymorphic calls exist in a given module.

---

## Memory Savings Estimate

For a module with C classes and S protocol selectors:

| Component | Current | Proposed |
|---|---|---|
| Vtable arrays | C × S × sizeof(ptr) in **RAM** | 0 (or C × S × sizeof(ptr) in **flash/rodata** if protocol_dynamic calls exist) |
| Constructor code | ~(C × S × 8) bytes `.text` | 0 |
| `oz_class_id` field | per-object | retained (needed only if PROTOCOL_DYNAMIC calls exist) |

Example: 10 classes, 8 protocol selectors, ARM (4-byte ptrs):
- **Current**: 320 bytes RAM + ~640 bytes `.text` for constructor
- **Proposed (all static)**: 0 bytes RAM, 0 constructor
- **Proposed (some dynamic)**: 320 bytes `.rodata` (flash), 0 RAM, 0 constructor

---

## Files to Modify

| File | Change |
|---|---|
| `tools/oz_transpile/model.py` | (Optional) Add `PROTOCOL_STATIC` / `PROTOCOL_DYNAMIC` to DispatchKind |
| `tools/oz_transpile/resolve.py` | No change (method-level classification stays the same) |
| `tools/oz_transpile/emit.py` | Refine per-call-site dispatch; update template context builders |
| `templates/oz_dispatch.h.j2` | Generate `OZ_IMPL_*` macros, `OZ_SEND`, `OZ_PROTOCOL_SEND_*`, remove mutable vtable decls |
| `templates/oz_dispatch.c.j2` | Generate `const` arrays (if needed), remove constructor |
| Golden test expectations | Update all `expected/` dirs for new macro style |
| `tests/behavior/`, `tests/adapted/` | Should pass unchanged (transpiler output changes, not user-facing API) |
| `tests/zephyr/` | Re-generate and verify |

---

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| `_infer_receiver_class()` misidentifies type | Conservative: fall back to `OZ_PROTOCOL_SEND` (correct, just slower) |
| `_Static_assert` not supported on all compilers | Use `#error` inside a never-called function, or GCC `__attribute__((error))` |
| Designated initializers for const array | C99+ — already required by Zephyr |
| Statement-expressions (`({...})`) for `OZ_PROTOCOL_SEND` | GCC/Clang extension — already used in Zephyr extensively |
| Increased header size from `OZ_IMPL_*` macros | Macros cost zero at runtime; preprocessor handles them efficiently |

---

## Implementation Order

1. **Phase 1+2 together** — modify templates and emit.py to generate new macro style
2. **Phase 3** — add per-call-site inference (leveraging existing `_infer_receiver_class`)
3. **Phase 4** — refactor `dispatch_free`
4. **Update golden tests** — throughout each phase
5. **Run `just test`** — validate at each phase
6. **Phase 5** — optional `oz_class_id` elimination (separate PR)
