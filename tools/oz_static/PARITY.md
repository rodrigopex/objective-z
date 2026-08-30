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
| mem_demo | yes | yes | was gap B |
| arc_demo | yes | Zephyr-blocked | `K_THREAD_DEFINE` only; was gap A |
| gpio_demo | yes | Zephyr-blocked | was gap D |
| heap_alloc | yes | **no — transpiler** | property dot syntax; was gap D |
| hello_category | yes | **no — transpiler** | gap C below |
| transpiled_literals | yes | **no — transpiler** | boxed-literal helper not visible in `main` |
| zbus_objc | **no** | — | gap E below |
| zbus_service | **no** | — | gap E, plus stale (see below) |

12 of 13 transpile. Of those, 4 compile cleanly, 5 fail only on Zephyr
headers (expected on host — not a transpiler problem), and 3 fail on
transpiler gaps.

Fixed since the first measurement: **file-scope object variables** are now
type-tracked, so a send to a `static GPIOOutput *led;` resolves instead of
reporting the receiver as `id` (`emit::file_scope_vars`, threaded into method
*and* plain-function scopes -- `gpio_demo`'s `[led toggle]` sits in `main`).
**Bare class names now get their `struct` tag** in the two positions that
were copied through verbatim, a top-level declaration and a free function's
signature (`emit::class_tag_edits`). And `__objc_refcount_get` is emitted --
as a function rather than the oracle's macro, because the real
`src/OZObject.m` already declares it as one and a macro of that name would
be expanded inside that declaration and break it.

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

### Behavioral parity: 66 of 73, and zero disagreements

Transpiling and compiling say the input was understood and the output is
real C. They say nothing about what the code *does*. `just
test-cross-backend` (`tests/tools/cross_backend.py`) closes that: it runs
each case through **both** backends over the same Unity driver and diffs
the results.

| Outcome | Cases | Meaning |
| --- | --- | --- |
| MATCH | 66 | Identical Unity results — same tests, same outcomes |
| MISMATCH | **0** | No case that runs on both backends behaves differently |
| STATIC-FAILED | 7 | oz_static's side could not be built or run |

Every case that builds under both backends now produces identical results.
What remains is seven cases oz_static cannot build, not seven it gets wrong.

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

Both backends are also given the same pool sizes (the case's `oz-pool`
directive, else 4 per class, matching what `compile_and_run.py` does). A
slab that is too small makes a test fail on a null receiver rather than on
behavior, which would be measuring configuration, not parity.

The harness passes each case's Clang AST to `oz2c --ast` as well, using the
same dump it produces for the oracle — see "Clang as the authority" below.
That alone moved 14 cases from unbuildable to matching.

#### Fixed: scope-based ARC

Both remaining mismatches were the missing ARC (#189), and so were all six
runtime crashes: `arc/break_releases_loop_local` and
`arc/continue_releases_loop_local` failed directly, while the crashes were
pool exhaustion caused by temporaries that were never released. All eight
now match.

`emit::render_scoped_block` releases the object locals a block owns when the
block ends, and `render_loop_jump` / `render_return_statement` release what a
`break`, `continue` or `return` unwinds past. A `break` releases out to the
nearest loop body and no further, since a local declared *after* the loop is
still live once it exits.

Two rules keep it from doing damage:

**Only provably-owned locals are released.** `arc::is_owning_expr` accepts
`alloc`/`init`/`copy`/`new`/`retain`, boxed and collection literals, and
methods whose *every* return path is itself owning — computed to a fixed
point, which catches a factory that returns another factory's result (the
oracle's single pass does not). Anything unrecognised is treated as
borrowed, so an unknown shape leaks rather than double-frees. That asymmetry
is deliberate: a leak is a bug, a double free is memory corruption.

**ARC defers to manual retain/release.** oz_static supports manual memory
management as a feature of its own, and a variable cannot be managed both
ways — adding an automatic release to code that already releases is a double
free. So a local the body releases by hand is left entirely to the body. The
oracle never faces this choice: its sources are compiled `-fobjc-arc`, under
which an explicit `release` is a compile error, and indeed no `.m` under
`tests/behavior/cases/` contains one.

#### The 7 remaining static-side failures

Two are `timer_basic`/`timer_zephyr` crashing at runtime. Two drivers reach
for `_meta`, the oracle's name for the root tracking struct that oz_static
spells as flat `oz_*` fields. One needs a by-value `struct sensor_msg`
definition hoisted (header preservation). One is the `oz_heap_inner`
redefinition plus missing `allocWithHeap:`. One is a `void (*)(id)` vs
`void (*)(struct OZObject *)` divergence inside a driver, which no shim can
bridge because it is the driver's own code.

### Clang as the authority on what oz_static cannot see

oz_static parses with tree-sitter, which yields syntax but no resolved
types. Two questions it therefore cannot answer alone, both of which
decide whether generated code is *correct* rather than merely plausible:

1. **Is this ivar an object the class owns?** `id _thing` looks identical
   to any other pointer. Releasing a non-object corrupts memory; skipping
   every `id` ivar silently leaks it.
2. **Does this method actually exist?** A selector declared in an
   `@interface` and never defined is not a callable function, and emitting
   a call to it fails at *link* time with an undefined symbol rather than
   at transpile time with a located message.

`oz2c --ast <dump.json>` answers both from the same Clang dump the oracle
already produces (`tools/oz_static/src/astinfo.rs`). Under `-fobjc-arc`
Clang writes ARC ownership straight into each `qualType`, and a real
definition carries a `CompoundStmt` body that a bare declaration does not.

Without `--ast` the previous, narrower rules still apply, so nothing
regresses for a caller that does not pass one; a malformed dump is a hard
error rather than a silent fall-back to guessing.

**What is deliberately *not* taken from the AST:** lightweight generics.
Clang erases them from `qualType` — the oracle needed a secondary
tree-sitter pass (`collect.py::extract_source_generics`) to recover
`OZArray<OZQ31 *>`, which oz_static has natively. The split is therefore
principled: Clang for resolved semantics, tree-sitter for surface syntax
Clang discards. The AST also cannot become oz_static's parse tree at all,
being post-preprocessor, while in-place textual substitution needs the
original text; it stays an oracle for facts.

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
