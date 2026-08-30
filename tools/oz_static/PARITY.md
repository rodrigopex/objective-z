# oz_static parity status

Where the Rust backend (`oz2c`) stands against the Python pipeline
(`tools/oz_transpile`). A status record, not a claim of readiness.

These words are used precisely and are not interchangeable:

- **transpiles** — `oz2c` exits 0 and writes output. The input was understood.
- **compiles** — the generated `.c` files pass the host compiler. The output
  is real C.
- **links** — the generated objects link into a binary. Strictly more than
  compiling, and it took finding a bug to make the distinction earn its
  place: a call to a method that is declared but defined nowhere compiles
  perfectly well against the companion header's prototype and only fails at
  link, so a compile-only sweep reported "OK" for three samples that could
  not actually be built.
- **matches** — the case was *run* under both backends and they produced
  identical results. Only the behavior-corpus section below claims this;
  the sample table does not, and no sample was executed.

## Samples (13)

Measured by invoking `oz2c` directly with the same flags
`cmake/oz_static.cmake` passes (`-I <module>/include/oz_sdk`, one
`--impl-dir` per source directory, target include dirs, `--pool-sizes`
when the sample states it), plus one Clang AST dump per entry `.m` via
`--ast`. Compile and link check:

```
cc -DOZ_PLATFORM_HOST -DOZ_HEAP_SUPPORT \
   -I include -I tests/behavior/include/zephyr_stubs \
   -I <outdir> -I <outdir>/Foundation -c <file> -o <file>.o
cc <every .o> <host OZLog stand-in> -o a.out
```

`src/OZLog.c` itself cannot take part in a host link (it includes
`<zephyr/sys/printk.h>`), so a stand-in satisfies `OZLog` — the link step
is there to catch *generated* symbols that were referenced and never
defined, so substituting the one file neither backend generates keeps it
measuring oz_static's own output. See also the note on `src/OZLog.c` under
"Not verified" below.

| Sample | Transpiles | Compiles + links | Notes |
| --- | --- | --- | --- |
| hello_world | yes | yes | |
| transpiled_literals | yes | yes | `POOL_SIZES` honoured; was: helper unreachable from `main` |
| mem_demo | yes | yes | was gap B |
| hello_category | yes | yes | was gap C |
| pool_demo | yes | Zephyr-blocked (link) | needs `printk`; exercises `@synchronized` |
| transpiled_blocks | yes | Zephyr-blocked | `printk`/`k_*` only |
| transpiled_generics | yes | Zephyr-blocked | `printk`/`k_*` only |
| transpiled_led | yes | Zephyr-blocked | `printk`/`k_*` only |
| arc_demo | yes | Zephyr-blocked | `K_THREAD_DEFINE` only; was gap A |
| gpio_demo | yes | Zephyr-blocked | was gap D |
| zbus_objc | yes | Zephyr-blocked | was gap E |
| heap_alloc | yes | yes | was gaps F and I; runs correctly, see below |
| zbus_service | — | — | stale independently of oz_static (see below) |

Every sample with usable sources transpiles. Of those, 5 compile *and
link* cleanly on host and 8 stop only on Zephyr (expected — not a
transpiler problem). **No sample fails on a transpiler gap any more.**

`heap_alloc` was additionally *run*, and its output checked line by line
against the expectations in its own `sample.yaml`. That is the only sample
run so far, and it is what caught gap I below — it compiled, linked, and
leaked.

Each Zephyr-blocked sample was checked to be *only* that, rather than
assumed: `arc_demo`'s two remaining compile errors are both on its single
`K_THREAD_DEFINE(...)` line, which no host compiler can expand, and
`pool_demo` compiles completely and fails at link on `printk` alone — a
symbol Zephyr provides and the stub headers only declare.

Also fixed since: the always-visible includes (root macros, boxed-literal
helpers) now go into each `.c` rather than each `.h`, which is where the code
that needs them lives — an earlier attempt to keep them out of the shim
headers had excluded `main.h`, leaving `main.c` unable to see
`OZArray_oz_initWithItems`. A quoted `#include "X.h"` is now spliced when
that header declares Objective-C, which is how `zbus_objc`'s
`#include "Producer.h"` reaches its `@property`; a pure C header stays an
ordinary include. `@public`/`@private` visibility specifiers are dropped
rather than copied into the generated struct. And ivars declared in an
`@implementation` block rather than the `@interface` — valid modern
Objective-C, and what `hello_category`'s Car does — are collected and
emitted.

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

The three causes behind those, since fixed, and `hello_category` now
compiles:

5. *Visibility specifiers copied into C.* `@public`/`@private` have no C
   equivalent and were emitted into the generated struct → `Car.h:22 type
   name requires a specifier or qualifier`. They are dropped; nothing
   enforced visibility once the struct became plain C anyway.
6. *An AST shim emitted as a translation unit.* `include/oz_sdk/assert.h`
   exists so Clang keeps `oz_assert` calls in the AST — its own comment
   says the generated C gets the real macros from `platform/oz_assert.h`.
   Splicing it produced an `assert.c` defining `oz_assert_msg`, a name the
   PAL had already made a function-like macro → `expected identifier or
   '('`. A spliced file that reaches no Objective-C now gets no output
   pair at all: there is nothing in it to transpile, and the C compiler
   already has the real header.
7. *Top-level struct definitions dropped outright.* `emit_split` builds
   each file from what its per-kind arms push, and no arm handled a
   `struct Tag { ... };` with a body — so `struct color` came out as
   nothing but its trailing `;` → `variable has incomplete type 'struct
   color'`. (`emit()` never showed this: it patches the original text, so
   anything unpatched survives.) Struct *and* union definitions now hoist
   to the companion header, in source order, after the enums.

Two smaller gaps surfaced on the way and are also fixed: a stem that names
a class owned by another stem now includes that stem's header (without it,
`main` could not complete `struct Car`), and a `static inline` helper is
emitted into its origin's header rather than its body, so it is callable
from outside the file it was written in.

**F. Property dot syntax was not handled at all.** Fixed. A `.` on an
object passed straight through as C member access, so
`samples/heap_alloc`'s `[App sharedInstance].heap` became a member
reference on `struct App *` — "did you mean to use '->'?". It is now
lowered to the accessor call, in both read and write positions.

A survey of every `.m`/`.h` under `samples/`, `src/`, `include/oz_sdk/` and
the three test corpora found ten dot accesses, all in `samples/`, all
reads, in four shapes that differ in how the selector is found — and the
first three of them were shapes a naive implementation gets wrong:

- `super.spec` (`gpio_demo`) — dot syntax on `super`, which must stay a
  *direct* call. Routed through the receiver's own class_id switch the way
  an ordinary send is, a subclass override reading `super.thing` calls
  itself forever.
- `producer.ackCount` (`zbus_objc`) — the property is named `count` and
  carries `getter=ackCount`, so the field in source is not the property
  name. Accessor selectors are resolved through `getter=`/`setter=`.
- `str.cString` (`zbus_service`) — no `@property` at all, just a
  `- (const char *)cString` method. Objective-C accepts dot syntax against
  a bare getter, so a `@property` lookup alone is not enough.
- `[App sharedInstance].heap` (`heap_alloc`) — on a message-send result.

Chains need nothing special: `a.b.c` recurses, and the inner accessor's
return type resolves the outer field. Writes and compound writes occur
nowhere in the repository and are covered on their own account: a compound
assignment has to read and write back, which mentions the receiver twice,
so it is accepted only where the receiver is a plain identifier and stays a
hard error otherwise rather than sending twice.

Two bugs surfaced while testing this, both from `class_name_from_type`
being a pure spelling transform that says nothing about whether the name is
a *class* — `struct point` and `struct Widget` are spelled alike. Plain C
member access (`p.x`) was read as dot syntax and rejected, and the same
hole was latent in subscripting, where indexing a C array of structs would
have been reported as a class that "does not support subscripting". Both
now ask `Program::is_class`.

The oracle's own `tests/behavior/cases/properties/dot_syntax.m` is named
for this feature but never uses it — it declares a property and stops — so
there was no coverage on that side either.

**G. The protocol-dispatch table routed to methods that are never
defined.** Fixed, and it is why the sample sweep now links.
`include/oz_sdk/Foundation/OZArray.h` and `OZDictionary.h` both declare
`countByEnumeratingWithState:objects:count:`, which no `.m` in the
repository implements. oz_static collects a class's methods from its
*declarations*, so both classes appeared to have it, and the generated
dispatch function called
`OZArray_countByEnumeratingWithState_objects_count_` — an undefined
symbol that broke the link of every sample pulling in Foundation. The
Python pipeline never mentions that selector at all, because it collects
from implementations.

`Program::method_is_defined` existed for exactly this but could only answer
with a Clang AST supplied, and abstained otherwise. It now rests on the
parse instead, which is both simpler and strictly better founded:
oz_static emits a definition exactly when it parsed an `@implementation`
defining the method or synthesizes the accessor for a `@property`, so it
already knows what its own output will contain. The AST is kept as an
additional *positive* source only, so supplying one can never suppress
more than not supplying one.

**H. The Clang AST could not be supplied for a multi-file program.** Fixed.
`--ast` takes one dump, but a dump of `main.m` carries every `@interface`
it imports and only the `@implementation`s written in that one file — so a
sample's dumps cover none of the SDK's implementations in `src/*.m`.
`--ast` is now repeatable and the facts are unioned.

That exposed a sharper problem: treating "the dump described this class" as
"I would have seen its method bodies" made oz_static *drop* the
declarations of everything the SDK implements elsewhere, including
`OZ_PROTOCOL_SEND_cDescription_maxLength_`, while still emitting the calls
— so supplying an AST made the output stop compiling. `AstFacts` now
tracks which classes it saw an `@implementation` *for*, separately from
which it merely saw, and the guard abstains without that stronger evidence.

**I. `+allocWithHeap:` and the heap-aware free path.** Implemented, so
`CONFIG_OBJZ_HEAP` is no longer a `FATAL_ERROR` in
`cmake/oz_static.cmake`. `--heap-support` generates, per class, a
`{Class}_oz_alloc_with_heap` taking its storage from an `OZHeap` (or the
system heap for a nil argument); the root gains an `oz_heap_allocated`
flag, so free returns the object where it came from; and the companion
defines `oz_heap_obj_alloc`/`oz_heap_obj_free`, which the PAL declares and
deliberately leaves to generated code because both need `struct OZHeap`
complete. All of it behind `OZ_HEAP_SUPPORT` as well as the flag, matching
the oracle.

`+allocWithHeap:` resolves to the *receiver's* allocator, not the declaring
class's, exactly as `+alloc` does — dispatched as an ordinary class method
it became `OZObject_allocWithHeap__cls`, which would allocate an
OZObject-sized block for a Sensor, and which is generated nowhere at all.

Two things only running the sample could show:

- **Every heap-allocated object leaked.** `@autoreleasepool` has its own arm
  in `emit::render_expr`'s match, ahead of the ARC one, so a pool block that
  declared an owned local got the pool renderer and never the releases. Not
  heap-specific at all — *any* `@autoreleasepool { Foo *f = [Foo alloc]; }`
  leaked — but `samples/heap_alloc` is built entirely from that shape and
  states the consequence in its own expected output ("Sensor dealloc",
  "app heap after free: 0 bytes used"). The three `arc_*` helpers now do
  that bookkeeping in one place so the two block renderers cannot drift
  again.
- **`+allocWithHeap:` was not an owning selector.** It is `+alloc` with
  different storage, so it returns +1; `arc::is_owning_selector` did not
  list it.

Both compiled and linked cleanly throughout. This is the clearest case so
far for the sample table's link column not being the last word either.

**Known divergence: release order within a scope.** oz_static releases a
scope's owned locals in *reverse* declaration order; the oracle
(`emit.py::_emit_scope_releases`) iterates its frame in declaration order.
Reverse is what Clang's own ARC does — its scope cleanups run LIFO, like C++
destructors — and it is the order that matters when one object's `-dealloc`
touches another, so oz_static keeps it. The visible cost is that
`samples/heap_alloc/sample.yaml` lists its two `Sensor dealloc` lines in the
oracle's order under `ordered: true`, so that sample's twister check would
not pass under the static backend without editing a file the Python backend
also uses. Nothing fails today, since no sample selects the static backend.

**J. The root object's tracking fields are now the PAL's own
`struct oz_metadata`.** oz_static had rolled its own: three `uint8_t`
siblings named `oz_class_id`, `oz_deallocating`, `oz_heap_allocated`. The
PAL already defines the type both backends want
(`platform/oz_platform_types.h`) -- a packed bitfield carrying `class_id`,
`heap_allocated`, `deallocating` and `immortal` -- and the Python backend's
root struct embeds it as `_meta`. oz_static now does the same, with
`oz_refcount` left a sibling exactly as the oracle leaves `_refcount`
(it is an `oz_atomic_t`, not a bitfield).

Three of the six remaining corpus failures were nothing but that spelling.
Their drivers assert `obj->base._meta.class_id`, and no `#define` can
rewrite `a._meta.b` into a flat `a.oz_b` -- the names are separate tokens
joined by `.`, so the shim had no way to bridge it. They were unbuildable
for no better reason than two structures having answered the same question
differently.

Adopting the shared type is a small win on its own account too: four flags
in the four bytes one of them used to take, no invented layout to keep in
step, and an `immortal` bit that names what oz_static currently expresses
by setting `deallocating = 1` on a boxed literal from birth -- which says
"currently being deallocated" to mean "never deallocate". That one is left
as is for now; it works, and changing the release path is a separate step.

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

**E. A quoted `#include "X.h"` was not resolved — only `#import`.** Fixed.
`zbus_objc`'s `Producer.m` opens with `#include "Producer.h"`, so the
`@interface` carrying `@property count` was never spliced in and
`@synthesize count` failed. Objective-C draws no semantic line between the
two directives here; only `#import`'s once-only behaviour differs, and the
seen-set gives that to both. A quoted include is now a resolution
candidate, and is declined — left exactly as written — when the header it
names reaches no Objective-C, or cannot be resolved at all (it may
legitimately name something only the target's own toolchain provides).

`hello_category` had survived the same pattern by luck: its `Car.m` also
uses `#include "Car.h"`, but `main.m` reached `Car.h` through
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

Rust test suite: 180 passing, 0 failing.

### Behavioral parity: 70 of 73, and zero disagreements

Transpiling and compiling say the input was understood and the output is
real C. They say nothing about what the code *does*. `just
test-cross-backend` (`tests/tools/cross_backend.py`) closes that: it runs
each case through **both** backends over the same Unity driver and diffs
the results.

| Outcome | Cases | Meaning |
| --- | --- | --- |
| MATCH | 70 | Identical Unity results — same tests, same outcomes |
| MISMATCH | **0** | No case that runs on both backends behaves differently |
| STATIC-FAILED | 3 | oz_static's side could not be built or run |

Every case that builds under both backends now produces identical results.
What remains is three cases oz_static cannot build, not three it gets wrong.

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

#### The 3 remaining static-side failures

Two are `timer_basic`/`timer_zephyr` crashing at runtime. One is a
`void (*)(id)` vs `void (*)(struct OZObject *)` divergence inside
`defer_block_ivar`'s driver, which no shim can bridge because it is the
driver's own code.

The three that cleared this round were all the `_meta` spelling —
`edge/empty_class_no_methods`, `lifecycle/alloc_returns_valid` and
`memory/heap_alloc`. See gap J; none of them was a behavioural difference,
and none needed a shim entry once the two backends named the same field the
same way.

`regression/issue_090_header_preservation` was the seventh and now matches.
It is the oracle's own regression test for this exact bug — "transpiler
drops struct/union/enum/macro definitions from companion headers when they
are not referenced by ObjC interface members" — and its driver uses all six
kinds it names. oz_static was already carrying the enum and the macros; the
struct, the union and the `static inline` are the fixes described under
gap C above.

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

**No Zephyr cross-build was run.** Everything above is a host measurement.
Nothing here claims any sample builds or runs on target, and two findings
say the Zephyr path has not been exercised at all:

- **`cmake/oz_static.cmake` links `src/OZLog.c`, which cannot compile
  against oz_static's output.** That file is written against the *Python*
  backend's generated header names — it opens with `#include
  "oz_dispatch.h"` and `#include "OZObject_ozh.h"`, which oz_static spells
  `oz_static_dispatch.h` and `OZObject.h`. Its one real dependency beyond
  those, `OZ_PROTOCOL_SEND_cDescription_maxLength_`, both backends do
  provide under the same name and signature (a macro on one side, a
  function on the other), so the incompatibility is the two `#include`
  lines and nothing more. No sample selects `CONFIG_OBJZ_BACKEND_STATIC`,
  which is why this has gone unnoticed; it is not fixed here.

  Making the file a `.m` and letting each backend transpile it would give
  oz_static exactly what it needs -- a transpiled file already includes its
  own dispatch header, so both stale includes could go -- but it does not
  work for the Python backend today. That backend *does* model top-level C
  functions (`collect.py::_collect_function`), yet it takes their parameters
  from `ParmVarDecl` nodes alone and has no variadic support anywhere: no
  `isVariadic`, no `...`, and every signature is built as
  `", ".join(p.oz_type.c_param_decl(p.name) for p in func.params)`
  (`emit.py:567`, `:795`, `:858`). `void OZLog(const char *fmt, ...)` would
  silently lose its varargs, leaving the body's `va_start(args, fmt)`
  undefined. OZLog is the one file least suited to the conversion, being
  inherently variadic.

  The cheap fix, if this is worth closing: make the two includes conditional
  on a macro `cmake/oz_static.cmake` defines. Five lines in one shared file,
  no emitter change on either side, one implementation.
- **`cmake/oz_static.cmake` passes no `--ast`.** So the production build
  path gets none of the Clang ownership facts, and `--ast` support is
  exercised only by the corpus harness and the sample sweep. Since
  definedness now rests on the parse (gap G), the AST's remaining job there
  is ivar ownership — which is what decides whether ARC releases an
  `id`-typed ivar.

**The samples are compiled and linked, not run.** Only the behavior corpus
is executed, and only under the ABI shim described above.
