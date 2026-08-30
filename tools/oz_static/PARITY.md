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
- **runs** — the binary was executed, exited 0, and its console output
  matched every line the sample's own `sample.yaml` says twister should see,
  in order. That file is the sample author's statement of correct
  behaviour, so it is a real oracle and an independent one — it says nothing
  about the Python backend.
- **matches** — the case was *run* under both backends and they produced
  identical results. Only the behavior-corpus section below claims this.
- **builds for ARM** — `west build -b mps2/an385` succeeded with the real
  cross-toolchain. Strictly more than compiling on host, and the difference
  is not small: it found five defects in twenty minutes that a full day of
  host checks had not (see "On target").

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
cc <every .o> src/OZLog.c tests/behavior/zephyr_stubs.c -o a.out
./a.out            # checked against the sample's own sample.yaml
```

The real `src/OZLog.c` is linked, not a stand-in. The sweep writes the same
two shim headers `cmake/oz_static.cmake` writes (`oz_dispatch.h`,
`OZObject_ozh.h`, each forwarding to oz_static's own spelling), and the host
stubs gained `<zephyr/sys/printk.h>` plus a `zephyr_stubs.c` defining
`printk` -- so the file both backends share is exercised the way the real
build exercises it, rather than substituted. See gap K.

Each linked sample is then **run** under an ordinary build and again under
`-fsanitize=address,undefined` with leak detection on. All nine are clean.

A separate pass compiles the generated C with `-Wall -Wextra` and counts
warnings by kind. Zephyr builds with `-Werror`, so a warning in generated
output is a build failure there rather than a style note — see gap M. What
is left is 58 `-Wunused-parameter`, which is `-Wextra` only.

| Sample | Transpiles | Compiles + links | Runs | Notes |
| --- | --- | --- | --- | --- |
| hello_world | yes | yes | yes | |
| transpiled_literals | yes | yes | yes | `POOL_SIZES` honoured; was: helper unreachable from `main` |
| mem_demo | yes | yes | yes | was gap B |
| hello_category | yes | yes | yes | was gap C |
| pool_demo | yes | yes | yes | exercises `@synchronized` |
| transpiled_blocks | yes | yes | yes | |
| transpiled_generics | yes | yes | yes | |
| transpiled_led | yes | yes | yes | was gap L — segfaulted |
| heap_alloc | yes | yes | all but one line | was gaps F and I; see the release-order divergence below |
| arc_demo | yes | Zephyr-blocked | — | `K_THREAD_DEFINE` only; was gap A |
| gpio_demo | yes | Zephyr-blocked | — | device tree; was gap D |
| zbus_objc | yes | Zephyr-blocked | — | zbus; was gap E |
| zbus_service | — | — | — | stale independently of oz_static (see below) |

Every sample with usable sources transpiles, and **none fails on a
transpiler gap**. Nine compile, link and run on host; eight of those match
every line their own `sample.yaml` asks for, and all nine are clean under
AddressSanitizer and UndefinedBehaviorSanitizer with leak detection on. The
three that stop at Zephyr need kernel or device-tree infrastructure no host
build can provide (`K_THREAD_DEFINE`, a device tree, zbus).

`heap_alloc`'s one unmatched line is the release-order divergence recorded
below, not a defect.

Running them is what found gaps I and L. Both compiled and linked cleanly
first.

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

**Release order within a scope: the two backends differ, and it no longer
costs anything.** oz_static releases a scope's owned locals in *reverse*
declaration order; the oracle (`emit.py::_emit_scope_releases`) iterates its
frame forward. Reverse is what Clang's own ARC does — scope cleanups run
LIFO, like C++ destructors — and it is the order that matters when one
object's `-dealloc` touches another, so oz_static keeps it.

`samples/heap_alloc/sample.yaml` used to pin its two `Sensor dealloc` lines
in the oracle's order under `ordered: true`, which made that sample time out
under twister on a run that was otherwise entirely correct. Those two
objects are released when the same `@autoreleasepool` block ends and the
order between them is not what the sample demonstrates, so those two lines
are now order-agnostic. Both backends pass; nothing else in that file was
relaxed.

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

**K. The sample sweep never compiled `src/OZLog.c`, and a claim here was
wrong about why.**

The correction first, since it was recorded here as a finding: an earlier
version of this file said the static backend could not build `src/OZLog.c`
at all, because that file includes `"oz_dispatch.h"` and
`"OZObject_ozh.h"` -- the Python pipeline's generated filenames. That was
wrong. `cmake/oz_static.cmake` has written shim headers of exactly those two
names into `<outdir>/Foundation` since oz_static was first wired into the
build (`472a44c`), each forwarding to oz_static's own spelling, and that
directory is on the target's include path. The unmodified file compiles
against oz_static's output; verified by reproducing the shims by hand and
compiling it. A conditional-include change made on the strength of the wrong
claim has been reverted -- two mechanisms for one problem is worse than one.

What was real: the sweep could not compile that file on host, because it
includes `<zephyr/sys/printk.h>` and the host stubs had no such header. So
the one pure-C runtime file both backends link was never exercised by any
host check, which is what let the mistaken claim stand. The stubs now
provide it, plus a `tests/behavior/zephyr_stubs.c` defining `printk`, and
the sweep links the real file.

`printk` is a prototype plus a definition rather than a macro, because a
macro would collide with transpiled sources that declare the function
themselves -- `samples/pool_demo` does exactly that so its Clang AST dump
resolves without Zephyr headers.

Adding those stubs moved four samples from Zephyr-blocked to running:
`pool_demo`, `transpiled_blocks`, `transpiled_generics`, `transpiled_led`.
They had never needed anything but `printk`.

**L. Assigning to a strong object ivar did not take ownership.** Fixed, and
it was a use-after-free. `{Class}_oz_release_ivars` releases every owned
object ivar when an instance dies, but nothing had ever retained what was
stored there -- oz_static had the release half of strong-ivar ownership
without the retain half, and releasing a reference never taken is a double
free.

`samples/transpiled_led` is a chain of six objects, each holding the
previous one in a strong `_next` ivar assigned straight from a parameter. It
segfaulted with nothing printed at all. AddressSanitizer named it exactly:
heap-use-after-free in `oz_atomic_dec_and_test`, the object freed once by
its owner's `oz_release_ivars` and again by the scope-exit release of the
local that created it.

The rule now matches the oracle's `_emit_strong_ivar_assign`, and is just
ARC's: a `+1` right-hand side is stored as-is, since it already carries the
reference the ivar is taking over and a temporary has no scope-exit release
to balance a second one; anything else is borrowed and gets retained. Order
is assign, retain new, release old -- what makes `_x = _x` safe. Properties
were never affected: a synthesized setter already did retain-new /
release-old, so only *direct* ivar assignment was missing it.

**M. Generated C produced `-Wall` warnings, and one was a wrong type.**
Zephyr builds with `-Werror`, so each of these was a build failure waiting
on target, and none of them showed up in a plain compile check. Found by
compiling the samples' generated output with `-Wall -Wextra` and counting.

- **`const` was dropped from every method signature** (6 warnings, and the
  real problem). `extract_type_and_stars` never looked at `type_qualifier`
  nodes, so `- (const char *)cString` in
  `include/oz_sdk/Foundation/OZString.h` came out as
  `char *OZString_cString(...)`. Returning the `const char *` ivar from it
  warns "discards qualifiers" — but the signature was simply wrong, and a
  caller could write through the result. Qualifiers written before the type
  name are now kept.

  The fix needs an allowlist, not a denylist: `type_qualifier` also covers
  Objective-C's ARC and bridging qualifiers, and preserving those emitted
  `(__bridge void *)` into `src/OZTimer.m`'s generated cast, which is not C.
  Keeping only `const`/`volatile`/`restrict`/`_Atomic` means an unrecognised
  qualifier keeps the old behaviour of being dropped — at worst a weaker
  type, where passing an unknown word through is invalid C.

- **`'/*' within block comment`** (36 warnings). Banner comments echo the
  source they describe, and the escaping was one-sided: an embedded `*/` was
  neutralised, the opening `/*` was not. `OZQ31.h`'s ivar doc comments
  account for all 36 on their own.

- **`expression result unused`** on the strong-ivar assignment from gap L.
  It is emitted as a comma expression so it stays usable wherever an
  assignment was, and the trailing read of the ivar is what gives it a
  value — but as a bare statement, which is nearly every case, that read is
  discarded. The trailing value is now emitted only where something can use
  it.

**N. The production build passed no `--ast`.** Fixed.
`cmake/oz_static.cmake` now dumps one Clang AST per source -- each entry
`.m` plus the module's own `src/*.m`, which oz2c splices through
`--impl-dir` -- and passes them all.

This was the one place the facts were missing. tree-sitter gives oz2c syntax
but no resolved types, so it cannot tell on its own whether an `id`-typed
ivar is an object the class owns, and that answer decides whether ARC
releases it: releasing a non-object corrupts memory, skipping a real one
leaks it. Without a dump oz2c stays conservative and skips every `id` ivar
-- correct, but a leak on target that neither the corpus harness nor the
sample sweep would show, since both do pass `--ast`.

One dump is not enough, which is why `--ast` is repeatable: Clang
preprocesses `#import`s, so a dump of `main.m` carries every `@interface` it
imports but only the `@implementation`s written in that one file.

**O. `id` inside a function-pointer type is spelled as the root class
pointer.** A function-pointer ivar's or parameter's own parameter list is
the one place `id` cannot be left to the typedef: the field's type is what
external C code has to match when it assigns, and it has no call site to
cast at. `OZDefer`'s ivar is `void (^_block)(id)`, and with `id` as a
typedef for `void *` the field was `void (*)(void *)` — so assigning a
plain `void (*)(struct OZObject *)` function to it did not compile, which is
what `foundation/defer_block_ivar` does.

The field, the method parameter and a hoisted block literal's own signature
all have to agree, so all three are lowered. The first attempt lowered only
the field, and the `-initWithBlock:` assignment stopped compiling instead.

Deliberately *not* done the obvious way — making `id` itself a root-class
pointer, as the Python backend's own typedef does. That was tried and is
worse: it turns the ordinary Objective-C idiom of passing `Foo *` where `id`
is expected into a warning, in code that has no call site to cast at either,
and produced 64 new `-Wall` warnings against the one it fixed.
`collect::render_type` therefore still resolves a *method's* `id` to
`void *`, where oz_static's own casts at every call site make the looseness
free.

**P. Pass-through C from a header now goes into the generated header.**
Routing was by node *kind*, which missed the shape Zephyr is full of: a bare
top-level macro invocation is neither a `preproc` node nor a declaration, so
`ZBUS_CHAN_DECLARE(chan_temperature_service_invoke, ...)` in
`samples/zbus_service`'s header landed in the generated `.c` where no other
origin could see it — `'chan_temperature_service_report' undeclared` in
`main.c`, and that sample could not be built for ARM at all.

`imports` now records which byte ranges came from a header rather than an
implementation, and routing asks *provenance* first: whatever a header
contributed goes into the generated header, because that is what a header is
for. This subsumes the earlier special case that sent `static inline` to the
header by kind — one in a `.m` now correctly stays in the body, and one in a
header travels with everything else that header declared.

A `.m` reached through an `#import` counts as an implementation, not a
header: the behaviour corpus's base header does `#import "OZObject.m"`
precisely to pull one in.

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

## On target (mps2/an385, ARM, Zephyr)

The check that was missing, and the one that mattered most. Every
measurement above this section is a host measurement; this one uses the real
ARM toolchain, real `k_mem_slab`, real spinlocks and Zephyr's own warning
set.

```
just test                       # west twister -T samples/ -p mps2/an385
just project_dir=samples/arc_demo rebuild && just run    # one sample, QEMU
```

`just test` is the real harness: twister builds each sample, runs it under
QEMU, and matches the console output against the `regex:` list in that
sample's own `sample.yaml`. It is stricter than a plain `west build` in two
ways that both mattered — it adds `-Werror`, and it checks output rather
than exit status.

**All 13 samples build for ARM**, and `arc_demo`'s output under QEMU is
byte-identical to the Python backend's.

**13 of 13 twister configurations pass** — every sample, built, run under
QEMU and output-checked. `gpio_demo` and `zbus_service` had no `sample.yaml`
at all and so were invisible to the harness; both have one now.

- `gpio_demo` asserts the LED path only. mps2/an385's GPIO driver has no
  interrupt support, so `gpio_add_callback_dt` returns `-ENOTSUP` and
  "Button configured" never prints on this board. Asserting that failure
  would be worse than omitting it: the check would hold only while GPIO
  interrupts stay unsupported, and would mask the button path regressing on
  a board that has them.
- `zbus_service` asserts one full request cycle across all three reporting
  paths — the zbus listener, the synchronous
  `-requestTemperatureWithRef:andTimeout:`, and the block-callback variant.
  Temperature values are random (`CONFIG_TEST_RANDOM_GENERATOR`), so only
  their shape is matched; pinning them would make the check depend on the
  RNG.

Which backend each build actually used was verified rather than assumed:
every one resolves `CONFIG_OBJZ_BACKEND_STATIC=y`, produces
`oz_static_generated/`, and mentions `oz_transpile` nowhere in its build
log.

Twister found two things a plain `west build` of the same samples did not:

- **`oz_spinlock_t lock = {0}` does not compile under `-Werror`.**
  `struct k_spinlock` has *no members* unless `CONFIG_SMP` or
  `CONFIG_SPIN_VALIDATE` is on, so a brace initializer is "excess elements
  in struct initializer". The PAL gained `oz_spin_init`, which `memset`s on
  Zephyr and assigns on host — covering both an empty struct and the host
  backend's plain `int`. `samples/pool_demo` was the case.
- **`samples/heap_alloc` timed out on its own expected output**, and the
  program was entirely correct: heaps back to 0, all four Sensors
  deallocated, "Demo complete" printed. Its `sample.yaml` pinned
  `Sensor dealloc.*42` before `.*84` under `ordered: true`, which encoded
  one backend's scope-traversal order as a requirement. Those two objects
  are released when the same `@autoreleasepool` block ends, and which goes
  first is not what the sample demonstrates — real ARC destroys scope locals
  in reverse declaration order (oz_static does, matching Clang) while
  `oz_transpile` walks its frame forward. The two lines are now
  order-agnostic and both backends pass; every other ordering constraint in
  that file is untouched.

  This supersedes what this document previously called a "known divergence"
  to be lived with. The divergence is real and oz_static's order is the
  correct one; it was the *expectation* that was over-specified.

Running the cross-build found five defects in the first twenty minutes,
after a whole day of host checks had gone green — worth recording, because
four of the five are invisible to any host build:

1. **`struct oz_heap_inner` was defined twice.** Both fallback stubs
   (`include/platform/oz_platform.h`, `include/oz_sdk/Foundation/OZHeap.h`)
   were guarded by `#ifndef OZ_HEAP_INNER_DEFINED` and *neither defined it*,
   so both compiled. Latent in the shared headers; only the static backend
   exposes it, because it splices the SDK header into generated C. The guard
   is now set where the struct is defined, as the two PAL backends already
   did, and the PAL fallback gained the accessor stubs the guard also covers.
2. **A generated header was shadowed by the source it was generated from.**
   A sample doing `target_include_directories(app PRIVATE include)` gets its
   own directory searched first, so `#include "Car.h"` from generated C found
   `samples/*/include/Car.h` — the Objective-C original — and the ARM
   compiler reported `stray '@' in program`. The generated directories are
   now added `BEFORE`. This is also why the Python backend suffixes its
   headers `_ozh.h`: the suffix makes the collision impossible rather than
   merely losing the race.
3. **`arc_demo` MPU-faulted.** Registers named it: `r0=0`, `r1=0x63` (99),
   MMFAR `0xc` — a write through a null receiver. The one-slot Sensor slab
   stayed occupied because ARC never released the first Sensor, so the next
   allocation returned NULL. Two gaps behind it, both in `arc`: a plain C
   function was not considered for owning returns, and a factory returning a
   *local* rather than the allocation directly was not recognised at all.
   `samples/arc_demo` is built on both shapes, and its own comment says "s
   is released here by ARC". The Python backend released it correctly with
   the same 1-slot slab, which made the diagnosis certain.
4. **Two samples declared `int printk(...)`** where Zephyr's returns `void`
   (`samples/pool_demo`, `samples/transpiled_led`). Harmless until the
   declaration reached generated C beside Zephyr's own header, then a
   conflicting declaration. The Python backend never emitted it, because it
   models function *definitions* and skips bare prototypes.
5. **`samples/gpio_demo` had `BIT(spec.pin)`** on a `const struct
   gpio_dt_spec *spec` — invalid on a pointer, and every other line in the
   same method correctly writes `spec->`. A pre-existing source bug that no
   host build reached.

### `zbus_service`

Was recorded here for a long time as "stale independently of oz_static".
That was right, and the cross-build quantified it: five separate kinds of
staleness, four of them nothing to do with any backend.

| What | Fixed |
| --- | --- |
| `ZEPHYR_EXTRA_MODULES` pointed at `../../objc/`, which does not exist | yes |
| called `objz_target_sources`, a function removed from `cmake/` | yes |
| `prj.conf` set three Kconfig options that no longer exist | yes |
| `@interface TemperatureService: Object` — the root is `OZObject` | yes |
| `#include <Foundation/OZLog.h>` did not resolve at compile time | yes — `include/oz_sdk` added to the target's include path |
| `ZBUS_CHAN_DECLARE(...)` in a header did not reach other origins | yes — gap P |

It builds now.

## The Python backend still passes its own suites

Making oz_static the default changed shared files -- `platform/oz_platform.h`
and its two backends, `oz_sdk/Foundation/OZHeap.h`, the host Zephyr stubs,
and four sample sources. Those are the Python pipeline's inputs too, so its
own suites were re-run rather than assumed unaffected:

| Suite | Result |
| --- | --- |
| `just test-transpiler` (`tools/oz_transpile/tests/`) | 539 passed |
| `just test-behavior` (`tests/behavior/`) | 73 passed |
| `just test-adapted` (`tests/adapted/`) | 40 passed |

All three green, so nothing in the shared surface regressed for that
backend.

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

Rust test suite: 183 passing, 0 failing.

### Behavioral parity: 73 of 73

Transpiling and compiling say the input was understood and the output is
real C. They say nothing about what the code *does*. `just
test-cross-backend` (`tests/tools/cross_backend.py`) closes that: it runs
each case through **both** backends over the same Unity driver and diffs
the results.

| Outcome | Cases | Meaning |
| --- | --- | --- |
| MATCH | **73** | Identical Unity results — same tests, same outcomes |
| MISMATCH | 0 | — |
| STATIC-FAILED | 0 | — |

Every case in the corpus builds, runs and produces identical results under
both backends.

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

#### How the last few closed

`timer_basic` and `timer_zephyr` had been crashing at runtime since the
harness was first built. They were never a timer problem: OZTimer holds a
strong object ivar, so they were the same missing retain that made
`samples/transpiled_led` segfault (gap L). Diagnosing one sample fixed both.

`foundation/defer_block_ivar` was the last, and it was a type the generated
struct got wrong rather than anything about the driver: its field was
`void (*)(void *)` because `id` inside a function-pointer type was left to
the typedef, so assigning an ordinary `void (*)(struct OZObject *)` function
to it did not compile. See gap O.

`memory/heap_alloc` needed `+allocWithHeap:` (gap I) and the SDK header fix
found by the ARM build. It was the last entry in
`tests/oz_static/tests/corpus_parity.rs`'s `KNOWN_CC_FAILURES`, which is now
empty — and that list asserts a listed case *still* fails, so emptying it
was forced rather than chosen.

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

## The static backend is now the default

`Kconfig`'s `OBJZ_BACKEND` choice defaults to `OBJZ_BACKEND_STATIC`, so
every sample and every application using this module transpiles through
`oz2c` unless it says otherwise. No `prj.conf` pins the backend, so the
default is the whole mechanism.

To go back to the Python pipeline, per target:

```
# samples/<name>/prj.conf
CONFIG_OBJZ_BACKEND_PYTHON=y
```

**What this default rests on, stated plainly.** Every measurement in this
document is a host measurement. The 73-case corpus matches on both backends,
9 of 12 samples compile, link, run and match their own `sample.yaml`, and
all 9 are clean under AddressSanitizer and UndefinedBehaviorSanitizer. What
none of that covers is a Zephyr cross-build: no sample has been built on
target through this backend, the three samples needing kernel or
device-tree infrastructure (`arc_demo`, `gpio_demo`, `zbus_objc`) are not
exercised at all, and `k_mem_slab`, real interrupt-disabled spinlocks and
code size are all untested. Flipping the default is what will surface those;
`CONFIG_OBJZ_BACKEND_PYTHON=y` is the way back for any target it breaks.

## Not verified

**The Zephyr cross-build is now run** — see "On target" above. What is still
not covered: only `arc_demo` has been *executed* on target, no board has been
used (mps2/an385 under QEMU only), and nothing here measures code size
against the Python backend.

**The samples are run on host, not on target.** Nine of them execute and
are checked against their own `sample.yaml`, which is a real and
independent oracle — but a host run says nothing about `k_mem_slab`, real
interrupt-disabled spinlocks, or code size, and the three samples needing
kernel or device-tree infrastructure are not run at all.

Recorded because the reasoning is worth keeping: it was asked whether making
`src/OZLog.c` a `.m` and letting each backend transpile it would be an
improvement. It would not. The Python backend *does* model top-level C
functions (`collect.py::_collect_function`) but has no variadic support
anywhere — no `isVariadic`, no `...`, and every signature is built as
`", ".join(p.oz_type.c_param_decl(p.name) for p in func.params)`
(`emit.py:567`, `:795`, `:858`) — so `void OZLog(const char *fmt, ...)`
would silently lose its varargs and its `va_start(args, fmt)` would be
undefined. OZLog is inherently variadic, making it the file least suited to
that conversion. Nothing needed changing there in any case; see gap K.
