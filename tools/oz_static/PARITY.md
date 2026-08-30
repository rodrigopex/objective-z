# oz_static parity status

Where the Rust backend (`oz2c`) stands against the Python pipeline
(`tools/oz_transpile`). A status record, not a claim of readiness.

Two words are used precisely and are not interchangeable:

- **transpiles** — `oz2c` exits 0 and writes output. The input was understood.
- **compiles** — the generated `.c` files pass the host compiler. The output
  is real C.

Neither means *runs*. Nothing below was executed.

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
| hello_category | yes | **no — transpiler** | gap C below |
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

**C. Generated headers can form an include cycle.** `hello_category` fails
with `field has incomplete type 'struct OZObject'` inside the generated
`Foundation/OZString.h`. `OZObject.h` includes `OZString.h` and
`OZString.h` includes `OZObject.h`; with `#pragma once`, whichever is
entered first leaves the other looking at an incomplete `struct OZObject`,
and `struct OZString` embeds it by value. This is a consequence of the
`always_visible` set in `emit::emit_split` — every stem is made to include
OZString/OZArray/OZDictionary, including the root class's own header,
which those three in turn depend on. Guarding against a class including a
header that (transitively) includes it back would fix it.

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
