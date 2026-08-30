# oz_static parity status

Where the Rust backend (`oz2c`) stands against the Python pipeline
(`tools/oz_transpile`). A status record, not a claim of readiness.

Two words are used precisely and are not interchangeable:

- **transpiles** — `oz2c` exits 0 and writes output. The input was understood.
- **compiles** — the generated `.c` files pass the host compiler. The output
  is real C.
- **matches** — the case was *run* under both backends and they produced
  identical results. Only the behavior-corpus section below claims this;
  the sample table does not, and no sample was executed.

## Samples (13)

Measured by invoking `oz2c` directly with the same flags
`cmake/oz_static.cmake` passes (`-I <module>/include/oz_sdk`, one
`--impl-dir` per source directory, target include dirs, `--pool-sizes`
when the sample states it). Compile check:

```
cc -DOZ_PLATFORM_HOST -DOZ_HEAP_SUPPORT \
   -I include -I tests/behavior/include/zephyr_stubs \
   -I <outdir> -I <outdir>/Foundation -c <file> -o /dev/null
```

| Sample | Transpiles | Generated C compiles | Notes |
| --- | --- | --- | --- |
| hello_world | yes | yes | |
| pool_demo | yes | yes | exercises `@synchronized` |
| transpiled_literals | yes | yes | `POOL_SIZES` honoured |
| transpiled_blocks | yes | Zephyr-blocked | `printk`/`k_*` only |
| transpiled_generics | yes | Zephyr-blocked | `printk`/`k_*` only |
| transpiled_led | yes | Zephyr-blocked | `printk`/`k_*` only |
| arc_demo | yes | **no — transpiler** | gap A below |
| mem_demo | yes | **no — transpiler** | gap B below |
| hello_category | yes | **no — transpiler** | gap C below; 3 of 20 generated files, down from 20 |
| gpio_demo | **no** | — | gap D below |
| heap_alloc | **no** | — | gap D, plus no `allocWithHeap:` |
| zbus_objc | **no** | — | gap E below |
| zbus_service | **no** | — | gap E, plus stale (see below) |

9 of 13 transpile. Of those, 3 compile cleanly, 3 fail only on Zephyr
headers (expected on host — not a transpiler problem), and 3 fail on
transpiler gaps.

"Zephyr-blocked" means the only compile errors reference `zephyr/*`,
`printk`, `k_msleep`, `gpio_*`, `zbus`, `DT_*` or similar. Those samples
target hardware; a host compiler cannot resolve those regardless of
backend.

### zbus_service is stale independently of oz_static

Its `CMakeLists.txt` calls `objz_target_sources`, which exists nowhere in
`cmake/`, and points `ZEPHYR_EXTRA_MODULES` at `../../objc/`, which does
not exist in this layout. It also subclasses `Object` rather than
`OZObject`. It cannot build under **either** backend. Its `oz2c` failure
(gap E) is real but is not the only thing wrong with it.

## Open gaps found

Each was reduced to a specific cause, not left as "sample fails".

**A. A bare class name in a free function's signature keeps the untagged
spelling.** `arc_demo`'s `static Sensor *createSensor(int v)` emits
verbatim, giving `error: must use 'struct' tag to refer to type 'Sensor'`.
The function *body* is converted correctly (`struct Sensor *s = ...`), so
this is specific to top-level function signatures — local declarations,
ivars and method signatures all route through `collect::render_type`
already.

**B. `__objc_refcount_get` is not emitted.** `mem_demo` calls it (the
oracle emits it as refcount introspection alongside
`retain`/`release`/`retainCount`), producing `call to undeclared function
'__objc_refcount_get'`.

**C. Generated-header ordering — four causes found, four fixed, three new
ones behind them.** `hello_category` originally failed to compile in all
20 of its generated files. Fixed since:

1. *Include cycle.* `always_visible` in `emit::emit_split` made every stem
   include OZString/OZArray/OZDictionary — including the root class's own
   header, which all three depend on. With `#pragma once`, whichever
   header was entered first left the other with an incomplete `struct
   OZObject`, which `struct OZString` embeds by value. Now an
   always-visible edge is never added into a stem owning an *ancestor* of
   that class.
2. *Typedefs after includes.* The companion header declared `id`/`Class`/
   `BOOL` below its `#include`s, but the PAL re-enters generated headers
   (see 3), so prototypes naming those types were reached while the
   companion was four lines in → `unknown type name 'Class'`. The
   typedefs are hoisted above every include; they need only `bool`.
3. *A content-free header shadowing a system one.* `include/oz_sdk/assert.h`
   is a shim that exists so Clang keeps `oz_assert` calls in the AST. Its
   generated header lands on the include path as `assert.h` and shadows
   the real one, so the PAL's own `#include <assert.h>` reached it — and
   it had been given the always-visible includes, pulling the whole class
   graph in from inside the companion header. A stem that declares nothing
   no longer receives those includes.
4. *Prototype-scoped struct tags.* The companion declares every class's
   prototypes, and a signature can name a struct defined only in a
   per-class header it does not include (`struct color *` from the
   sample's `Car.h`) → `conflicting types for
   'Car_initWithColor_andModel_'`. Every struct tag the companion mentions
   but never declares is now forward-declared.

Three distinct causes remain, each in one file: `Car.h:22 type name
requires a specifier or qualifier`; `assert.c:20 expected identifier or
'('` (the shim's `static inline` stubs); and `main.c:24 variable has
incomplete type 'struct color'` (a by-value struct needing the definition
hoisted, not just a tag).

**D. File-scope `static` object variables are not type-tracked.** Reduced
to a 20-line reproducer:

```objc
static Widget *g_widget;
int main(void) { g_widget = [Widget alloc]; [g_widget poke]; return 0; }
```

→ `cannot statically resolve the receiver type for selector 'poke'
(receiver type is 'id')`. `gpio_demo` (`static GPIOOutput *led;`) and
`heap_alloc` (`static OZHeap *sHeap;`) both hit this. The oracle collects
file-scope statics (`collect.py`), so this is a parity gap rather than a
deliberate restriction.

**E. A quoted `#include "X.h"` is not resolved — only `#import`.**
`imports.rs` deliberately treats `#include` as never a resolution
candidate. `zbus_objc`'s `Producer.m` opens with `#include "Producer.h"`,
so the `@interface` carrying `@property count` is never spliced in and
`@synthesize count` fails. Verified by changing that single word in a
scratch copy: the sample then transpiles (10 files, exit 0). Angled
system includes must keep passing through untouched; only a quoted
include resolvable in the search path should be spliced.

`hello_category` survives the same pattern by luck — its `Car.m` also uses
`#include "Car.h"`, but `main.m` reaches `Car.h` through
`Car+Maintenance.h` via `#import`.

## Behavior corpus (73 cases)

`tests/behavior/cases/*/*.m` is the Python pipeline's own behavior suite,
driven through oz_static by `tools/oz_static/tests/corpus_parity.rs`
rather than being re-implemented as separate fixtures.

- **73 of 73 transpile.** Enforced with no allowlist.
- **72 of 73 produce compiling C.** The one exception is listed in that
  file's `KNOWN_CC_FAILURES` with its cause: `memory/heap_alloc.m`, where
  `struct oz_heap_inner` is defined by both `OZHeap.h` and
  `platform/oz_platform.h` — each guarded on `OZ_HEAP_INNER_DEFINED`,
  which neither defines outside `OZ_HEAP_SUPPORT` — and which also needs
  the `allocWithHeap:` path oz_static does not emit.

That allowlist asserts the listed case *still* fails, so fixing it without
updating the list also fails the test; it cannot decay into silently
skipped cases.

Rust test suite: 158 passing, 0 failing.

### Behavioral parity: 44 of 73 cases agree

Transpiling and compiling say the input was understood and the output is
real C. They say nothing about what the code *does*. `just
test-cross-backend` (`tests/tools/cross_backend.py`) closes that: it runs
each case through **both** backends over the same Unity driver and diffs
the results.

| Outcome | Cases | Meaning |
| --- | --- | --- |
| MATCH | 44 | Identical Unity results — same tests, same outcomes |
| MISMATCH | 4 | Both ran; they disagree. Real differences, listed below |
| STATIC-FAILED | 25 | oz_static's side could not be built or run |

Unity *results* are compared, not generated C: the two backends emit
deliberately different C, so a textual diff would be noise.

The drivers are written against the Python backend's ABI, which differs
from oz_static's in naming only (`<Class>_ozh.h` headers, `Class_alloc` vs
`Class_oz_alloc`, `OZObject_release` vs `oz_static_release`,
`Class_cls_sel` vs `Class_sel_cls`, `OZ_CLASS_X` vs `OZ_STATIC_CLASS_X`).
A generated shim header bridges exactly those, so one unmodified driver
exercises both backends. **This means the harness proves behavioral
agreement, not ABI compatibility** — the two backends' generated C is not
link-compatible, and that is not currently a goal.

#### The 4 mismatches

Two are the known missing ARC (#189), now measurable rather than inferred:

    arc/break_releases_loop_local      python PASS / static FAIL: Expected 1 Was 0
    arc/continue_releases_loop_local   python PASS / static FAIL: Expected 3 Was -1

Note `arc/reassign_releases_old` *matches*, so the gap is narrower than
"no ARC at all".

Two are a distinct bug the harness found, not an ARC-scope issue —
**strong object ivars are never released when their owner is
deallocated**:

    properties/atomic_property     strong_retains FAIL: Expected 1 Was 2
                                   strong_releases_old FAIL: Expected 1 Was 0
    properties/strong_vs_assign    strong_retains_on_set FAIL: Expected 1 Was 2
                                   strong_releases_old_on_overwrite FAIL: Expected 2 Was 0

The oracle emits an auto-dealloc (`emit.py::_emit_auto_dealloc`) for any
class with object ivars or a non-root superclass: it releases the owned
ivars, then chains to the parent. oz_static's dealloc dispatch falls back
to the root's no-op for a class with no user `-dealloc`, so a held object's
refcount never comes back down. Unlike full ARC this needs no scope
tracking and has well-defined semantics.

#### The 25 static-side failures

19 of them are one cause: oz_static emits prototypes and dispatch-switch
references for methods that are *declared but never defined* — the real
`OZArray.m`/`OZDictionary.m` have no body for
`countByEnumeratingWithState:objects:count:` (vestigial in the oracle
too), and the companion header declares `OZLog`/`_oz_get_log_precision`
unconditionally while only sometimes emitting them. Both surface as
link errors rather than located transpile errors, which is the wrong end
of the pipeline to learn about them.

The rest are individual: a by-value `struct sensor_msg` needing its
definition hoisted (header preservation), the `oz_heap_inner` collision
above, one `void (*)(id)` vs `void (*)(struct OZObject *)` function-pointer
divergence, and one crash.

### The compile check needs `-DOZ_HEAP_SUPPORT`

Without it, five otherwise-fine samples hit the same
`redefinition of 'oz_heap_inner'` described above, because
`Foundation.h` pulls in `OZHeap.h`. The generated header contains exactly
one definition — the collision is between SDK header content and the PAL,
not something oz_static emits. Worth knowing before reading a bare
`cc` failure as a codegen bug.

## Trying a sample on the static backend

No sample selects it; every `samples/*/prj.conf` uses the default Python
backend, and this document changes none of them. To try one:

```
# samples/<name>/prj.conf
CONFIG_OBJZ_BACKEND_STATIC=y
```

`cmake/oz_static.cmake` still hard-errors on `CONFIG_OBJZ_HEAP`, since
`allocWithHeap:` is not emitted.

## Not verified

**No Zephyr cross-build was run.** `west` v1.4.0 and `cmake` 4.4.2 are
present and `deps/zephyr` exists, but `ZEPHYR_BASE`,
`ZEPHYR_SDK_INSTALL_DIR` and `ZEPHYR_TOOLCHAIN_VARIANT` are all unset and
no SDK is installed, so no cross-toolchain is configured in this
environment. Nothing here claims any sample builds or runs on target.

**Nothing was executed.** The corpus cases each ship a Unity `_test.c`
driver; wiring those up is the cross-backend behavioural comparison still
outstanding. Compiling is the strongest check available without it.
