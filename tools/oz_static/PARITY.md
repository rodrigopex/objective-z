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

### Behavioral parity: 60 of 73 cases agree

Transpiling and compiling say the input was understood and the output is
real C. They say nothing about what the code *does*. `just
test-cross-backend` (`tests/tools/cross_backend.py`) closes that: it runs
each case through **both** backends over the same Unity driver and diffs
the results.

| Outcome | Cases | Meaning |
| --- | --- | --- |
| MATCH | 60 | Identical Unity results — same tests, same outcomes |
| MISMATCH | 2 | Both ran; they disagree. Real differences, listed below |
| STATIC-FAILED | 11 | oz_static's side could not be built or run |

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

#### The 2 mismatches

Both are the known missing ARC (#189), now measurable rather than inferred:

    arc/break_releases_loop_local      python PASS / static FAIL: Expected 1 Was 0
    arc/continue_releases_loop_local   python PASS / static FAIL: Expected 3 Was -1

Note `arc/reassign_releases_old` *matches*, so the gap is narrower than
"no ARC at all".

#### Fixed: strong object ivars were never released

The harness's first run also found two mismatches in
`properties/atomic_property` and `properties/strong_vs_assign` — a held
object's refcount never came back down (`Expected 1 Was 2`) — because
nothing released a class's strong object ivars when an instance was
deallocated. `companion::render_release_ivars` now emits a per-class
`{Class}_oz_release_ivars`, called from the release path once the class's
own `-dealloc` body has run. Both cases now MATCH.

This is where oz_static **deliberately parts company with the oracle**
rather than porting it. `emit.py::_emit_user_dealloc` appends the same
automatic releases *after* a user-written `-dealloc` body, so a `-dealloc`
doing ordinary manual-retain/release teardown — releasing what it owns —
has every one of those ivars released twice, silently. That is neither MRR
nor ARC: real ARC makes `[_ivar release]` in `-dealloc` a *compile error*,
so its safety comes from forbidding the manual release, not from adding a
second one.

oz_static follows ARC's rule: the release is automatic, and an explicit
release of an owned ivar inside `-dealloc` is a hard, located error naming
the ivar. Releasing a local, a parameter, or an `__unsafe_unretained` ivar
is untouched — nothing releases those automatically. The oracle's shape is
latent rather than observed there (its only corpus case with a user
`dealloc` has an empty body and no object ivars), but it is a double free
waiting for the first person to write conventional teardown.

`id`-typed ivars are deliberately *not* auto-released: `id` lowers to `void
*`, indistinguishable from a non-object pointer, and releasing a
non-object crashes whereas failing to release an object only leaks. The
oracle can release them because Clang tells it which are objects.

#### The 11 static-side failures

Six are runtime crashes (`exit -11`) that were previously masked -- those
cases could not be linked at all, so their generated code had never run.
Triaged, and they are **not six separate bugs**: all six are the missing ARC
(#189) again, reached by a different route.

Each dies at the same instruction, `OZQ31_fixedWithInt32__cls` writing
through a NULL. The slab ran out, `alloc` returned nil as it is supposed to,
and the factory in the real `OZQ31.m` writes through the result without
checking. The pool ran out because oz_static has no ARC: every temporary
Q31 stays live for the whole run, where the oracle releases each at the end
of the method that made it. `inline/array_fast_access.m` declares
`OZQ31=3`, which is ample with ARC and hopeless without it.

Raising the sizes for the static side only was considered and rejected:
`lifecycle/alloc_failure_enomem.m` asserts the pool bound *exactly* -- a
one-block pool whose second `alloc` must be NULL -- so pool sizes are part
of what is under test, not a knob the harness may turn. These six stay
unmeasurable until ARC lands, and that is the honest state.

What did change is the diagnosis. Building with
`-DOZ_STATIC_TRAP_POOL_EXHAUSTION` turns

    EXC_BAD_ACCESS (code=1, address=0x18) in OZQ31_fixedWithInt32__cls

into

    Assertion failed: OZQ31 pool exhausted -- raise it with --pool-sizes OZQ31=N

naming both the pool and the fix, at the point of exhaustion rather than
wherever the nil happens to be dereferenced. Off by default, because
returning nil is the contract.

The other five are individual and understood: two drivers reach for
`_meta`, the oracle's name for the root tracking struct that oz_static
spells as flat `oz_*` fields; a by-value `struct sensor_msg` needing its
definition hoisted (header preservation); the `oz_heap_inner` collision
described above; and one `void (*)(id)` vs `void (*)(struct OZObject *)`
function-pointer divergence in a driver, which no shim can bridge because
it is in the driver's own code.

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
