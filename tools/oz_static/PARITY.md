# oz_static parity status

Where the Rust backend (`oz2c`) stands against the Python pipeline
(`tools/oz_transpile`). A status record, not a claim of readiness.

These words are used precisely and are not interchangeable:

- **transpiles** — `oz2c` exits 0 and writes output. The input was understood.
- **compiles** — the generated `.c` files pass the host compiler as
  `-std=c17 -pedantic-errors`: ISO C17, with no constraint violation. The
  output is real C. The flags are part of the definition and were added late
  (gap Y). Without them this word meant "compiles as GNU C with whatever the
  compiler defaults to", which is how a constraint violation lived in every
  generated program for the life of the backend while satisfying every use of
  the word on this page.
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

## Samples (14)

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
output is a build failure there rather than a style note — see gap M.
**Nothing is left: the generated files are `-Wall -Wextra` clean across all 13
samples** (gap S). The 58 `-Wunused-parameter` recorded here previously were
both stale and measured with the wrong instrument — the real figure was 89.

Warning-free is not the same as *valid*, and the two were read as one until
gap X. Validity has its own instrument now: `-std=c17 -pedantic-errors` over
the whole corpus, which is a gate at **0**, and `just test-pedantic` over the
samples on ARM, which is a report at **26** sites. See gap Y — including why
the obvious way to measure that on target reports zero regardless.

| Sample | Transpiles | Compiles + links | Runs | Notes |
| --- | --- | --- | --- | --- |
| hello_world | yes | yes | yes | |
| transpiled_literals | yes | yes | yes | `POOL_SIZES` honoured; was: helper unreachable from `main` |
| mem_demo | yes | yes | yes | was gap B |
| hello_category | yes | yes | yes | was gap C |
| pool_demo | yes | yes | yes | uses `@synchronized`, but single-threaded -- it exercises the lowering, not the lock (see gap W) |
| transpiled_blocks | yes | yes | yes | carries the two top-level block shapes (gap Z) and the `OZM(K_TIMER_DEFINE, ...)` timer that replaced OZTimer (gap AB) |
| transpiled_generics | yes | yes | yes | |
| transpiled_led | yes | yes | yes | was gap L — segfaulted |
| heap_alloc | yes | yes | all but one line | was gaps F and I; see the release-order divergence below |
| arc_demo | yes | Zephyr-blocked | — | `K_THREAD_DEFINE` only; was gap A |
| gpio_demo | yes | Zephyr-blocked | — | device tree; was gap D |
| zbus_objc | yes | Zephyr-blocked | — | zbus; was gap E |
| zbus_service | yes | Zephyr-blocked | — | zbus; writes its listener as an inline block via `OZM` since gap Z |
| smp_shared | yes | — | SMP only | two cores contending on one object; needs `CONFIG_SMP`, so no host or single-core run — see gap W |

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

Fixed twice, which is the interesting part: once in `emit_split`, and then
again in the single-file `emit()`, where it had stayed open for both
positions. See gap U.

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
   color'`. (`emit()` never showed this: it patched the original text, so
   anything unpatched survived — it shares the one walk since #254, and so
   shares this arm.) Struct *and* union definitions now hoist
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
step, and an `immortal` bit that names what oz_static used to express by
setting `deallocating = 1` on a boxed literal from birth -- which says
"currently being deallocated" to mean "never deallocate". That bit is now
used for what it names; see gap T.

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
release-old, so among *ivars* only direct assignment was missing it.

A plain strong **local** was missing it too, which this entry originally
implied it was not. That is a different storage class and was fixed
separately -- see gap Q.

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
  (That file is gone since #267; `__bridge` remains ordinary Objective-C
  that any source may write, and the allowlist is pinned by
  `behavior_ivar_and_cast_lowering.rs`'s own fixtures.)
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

That list of three was read as the complete set of block positions and was
not: three more were lowered nowhere at all, and #272 (gap Z) found them.
The three named here are the ones routed through `collect::render_type`;
what the sentence above should have said is that those are the positions
which *have* a rendered type, and that a position assembled by patching the
original text has none.

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

**Q. Assigning to a strong object *local* did not release the old value, and
the static bar never saw a plain C function at all.** Both fixed (#234), and
the two are the same story: the bar was rejecting a shape only because ARC
leaked in it.

`render_strong_ivar_assign` did retain-new/release-old for a strong *ivar*
(gap L) and a synthesized setter did it for a property, but a plain local did
neither -- that function bails out on `ctx.locals` explicitly. So each
iteration of

```objc
Counter *c;
for (int i = 0; i < 100; i++) {
        c = [Counter alloc];
}
```

abandoned a live object, and `staticbar` rejected the loop rather than emit
the leak. Real ARC releases the previous value at every store to a strong
variable, so this is ordinary Objective-C, and the oracle has had the same
transform all along (`emit.py::_emit_strong_local_assign`). Diffed on the same
source, the two now put the releases in exactly the same place: release old,
then assign, plus one scope-exit release.

`emit::render_strong_local_assign` releases before evaluating an owning
right-hand side, which is what lets **one** slab slot serve the whole loop --
the slot goes back to the slab and the next allocation takes it again.
Measured on one slot: 100 allocations, 100 deallocs. Without the fix the same
test reports `ok=1` and `deallocs=0`.

Three things this needed that are worth recording:

- **A bare declaration gets ARC's implicit `= nil`.** Not tidiness: the first
  overwrite releases what the variable held, and `oz_static_release`
  dereferences its argument.
- **No temporary.** The first attempt captured the previous value into a
  `ctx.pre_stmts` local, which is drained by the enclosing *top-level*
  statement -- so inside a loop it hoisted above the `for`, read the variable
  once while it was still nil, and every iteration released nil. The loop
  leaked exactly as before and the output looked entirely plausible. Naming
  the variable inside the comma expression is both simpler and correct, since
  the comma operator sequences left to right.
- **Ownership stays symmetric.** A local is managed only when every value it
  can hold is owned; a store that would need a temporary to be retained
  exactly once leaves the local unmanaged. Releasing a reference never taken
  is a double free -- gap L in reverse.

The bar itself was entered from one place, `emit.rs`'s method-body renderer,
so a plain top-level C function was never scanned: not for `@try`, reflection
selectors, `@selector`/`@protocol`, `@synchronized` jumps, block captures, or
allocation. `staticbar::check_function_body` now runs the same walk there. No
scope mode was needed -- `class_ivars` is read in exactly one place and a free
function has none, so an empty set is the truth rather than a stand-in.

One position was still left unscanned and this entry did not say so, because
nothing could reach it yet: a block literal at *file scope*, which was not
hoisted at all until #272 and so never had a body to walk. It is scanned now
(gap Z). Two entered positions out of three is the same shape as the rest of
this family -- the count in a sentence like the one above is a count of what
was looked at.

Two consequences:

- **The allocation rule now distinguishes reassignment from accumulation.**
  `c = [Counter alloc]` in a loop is bounded at one live instance; storing
  into an array element or an ivar is not, and stays a hard error. This
  narrows OZ-098's collection-literal rule, which had rejected the
  reassignment shape too -- verified bounded rather than assumed, with
  `OZArray=1` and a 2-slot element pool running 100 iterations, since freeing
  an array returns both its slot and its buffer.
- **A bare `Counter *c;` is now type-tracked.** `collect_local_decls` matched
  `init_declarator` and `identifier` but not the `pointer_declarator` a
  declaration without an initializer produces, so such a local never reached
  `ctx.scope` and a later `[c poke]` was rejected as an `id` receiver -- while
  the identical code written `Counter *c = ...;` resolved fine. The local
  twin of gap D.

Validated on ARM, not only on host. No sample used the reassignment shape, so
the twister run proved *no regression* and nothing about the new path --
`samples/arc_demo` now exercises it, in `main()`, which is a plain C function
and so covers the free-function scan at the same time. Its generated C carries
the new form

```c
(oz_static_release((struct OZObject *)(r)), r = createSensor(100 + i));
```

and under QEMU it prints each value's dealloc before the next value exists,
which is the release-then-allocate ordering the single-slot claim rests on.
That sample's output stays byte-identical between the two backends with the
loop added, so the new path is confirmed against the oracle on target and not
only by the host tests. Its local is written `Sensor *r = nil;` rather than
left bare for exactly that reason: the Python backend has no emission rule for
the implicit nil (`ImplicitValueInitExpr`), and oz_static treats the two
spellings identically.
`Sensor` is given one slot of headroom there deliberately: the loop needs only
one, but the moment between its release and its next allocation leaves the slot
free, and pinning the pool to exactly 1 would make the sample depend on never
interleaving with the Sensor that `arc_demo_extra_thread_entry`'s Driver holds.

Objective-C inside a `#define` *body* is the other half of #234 and is not
addressed: a macro body is one opaque `preproc_arg` token, so it is emitted
verbatim and the generated C then fails to compile. Filed as #238 with a
prototyped detector (0 false positives over the repo's 40 macro bodies).

**R. Three checks skipped a declaration written without an initializer.** Fixed
(#240), and found by auditing #234 for claims it had made stale rather than by
anything failing.

One grammar detail behind all of it: a declaration with no initializer has no
`init_declarator` anywhere. A pointer gives `pointer_declarator`
(`Counter *c;`), a non-pointer gives a bare `identifier` (`int n;`). Several
places matched declarators by kind and listed only `init_declarator`, so each
silently skipped those forms. #234 fixed one of them (`collect_local_decls`,
which left such a local out of `ctx.scope`, so a send to it was rejected as an
`id` receiver). These were the rest:

| Where | Before | Consequence |
| --- | --- | --- |
| `staticbar` block-capture check | a bare local never entered `scope.locals` | the capture was **accepted**; the hoisted block then gave `use of undeclared identifier` against generated code the user never wrote |
| `generics::check_declaration` | a bare declaration was skipped entirely | **silent** — the element-type constraint was bypassed and the program compiled and ran unchecked |
| `emit::hoist_block_var` | a bare *pointer* `__block` local was not hoisted | the block referenced a name that was not there |

Each was confirmed by pairing the bare spelling against the initialized one,
because in every case the check itself was correct and simply never ran —
asserting only the bare form would pass equally against a build with the check
deleted.

The generics one is the worst of the three, being the only silent one: a
constraint that can be sidestepped by how a declaration is spelled is not a
constraint.

Two things worth recording, both caught by measuring rather than reasoning:

- `hoist_block_var` was first ruled *out* as a false lead, because
  `__block int q;` hoists correctly. It does — a bare *non-pointer* declarator
  is itself an `identifier`, which is exactly why the gap went unnoticed. Only
  `__block Foo *p;` fell through, and then nothing was hoisted at all.
- The first `staticbar` fix matched the right node kinds and still did nothing.
  `find_first_identifier_before_eq` searches only a node's *children*, and a
  bare `identifier` declarator has none, so it returned `None` — the caller was
  correct and got no name back.

The bar's declarator set is now deliberately the same one
`emit::collect_local_decls` uses. The two disagreeing about what counts as a
local is what produced the asymmetry to begin with.

**S. Generated C is now `-Wall -Wextra` clean.** Fixed (#229). Measured across
all 13 samples with the real ARM toolchain: **89 `-Wunused-parameter` before, 0
after**, and nothing of any other kind either before or after.

`-Wunused-parameter` is `-Wextra` only, so none of these was a build failure --
Zephyr's default warning set does not include it. They mattered as noise: three
of the four defects gap M found were visible only because someone compiled the
samples with `-Wall -Wextra` and counted warnings by kind, and 89 lines of it
made that harder than it needed to be.

`emit::unused_param_acks` emits `(void)param;` at the top of a translated
method body for each parameter the body never mentions, `self` included -- an
empty `-dealloc` is idiomatic Objective-C, so the warning fired on entirely
correct code. The same acknowledgement the SDK's own C already uses
(`(void)inner;` in `oz_platform.h`'s heap stubs).

Three things this took that are worth recording, because two of them were
mistakes:

- **The count in the issue was wrong, and so was the first instrument.** The
  issue said 58; the real figure is 89. The first attempt at re-measuring
  compiled the samples' *ARM-generated* C on host with `-Wall -Wextra` and
  produced 39 errors about `fprintf`, `stderr`, `memset` and
  `K_THREAD_DEFINE` -- host/ARM header mismatches, not codegen warnings.
  Building each sample for ARM with `-Wextra` added and counting only warnings
  whose file lives under `oz_static_generated/` is the measurement that means
  something.
- **Usage must be decided from the *rendered* C, not the Objective-C source.**
  An ivar reference like `_n` lowers to `self->_n`, so
  `- (int)useAll:(int)a other:(int)b { return a + b + _n; }` uses `self`
  despite never writing the word. Deciding from source emitted a redundant
  `(void)self;` there.
- **Hoisted block literals needed it too, and free functions must not have
  it.** `render_block` synthesizes a function and its signature outright, so
  its unused parameters are oz_static's to acknowledge -- that was 4 of the
  last 7. The other 3 were in `samples/zbus_objc`'s own
  `thread_entry_producer`, a plain C function whose body is the author's text
  patched in place; adding acknowledgements to code someone wrote is not the
  transpiler's business, and `samples/arc_demo`'s equivalent thread entry
  already writes `(void)p1;` itself. Fixed in the sample, not the transpiler.

The issue asked whoever took it to re-run the sweep afterwards rather than
trust its number. That was the right instruction and it is the reason the
figure here is 89 rather than 58.

One caution this entry now carries, added by gap X: it says nothing about
whether the output is *valid* C. Every generated program was carrying a bare
`;` at file scope throughout, which is a constraint violation and needs
`-Wpedantic` to be diagnosed — so it passed this sweep and Zephyr's `-Werror`
alike. Read "clean" here as clean under the flags actually passed.

Validity is measured on its own account since gap Y, so that caution is now a
pointer rather than an open hole: the corpus is gated at `-std=c17
-pedantic-errors` and the samples are swept on ARM. Gap Y also found that
gap X's fix had missed a producer which was *only* wrong on target, so the
distinction this caution draws turned out to be load-bearing twice.

**T. Immortality was expressed by a field that meant something else, and for
singletons it was not expressed at all.** Fixed (#228). Two halves, filed as
one cosmetic issue; only the first half was cosmetic.

A boxed literal lives in static storage, so it must never be freed -- and
something does try, because a collection that absorbed it releases its
elements when it dies. oz_static marked literals `_meta.deallocating = 1`
from birth and relied on the release path's re-entrancy guard to turn the
free into a no-op. That worked, but `deallocating` means "teardown is running
right now", not "never tear down", so the generated C said something false
and any second reader of the flag would have had to know the trick. The PAL's
`struct oz_metadata` has carried an `immortal` bit for exactly this since
OZ-064 (#97), and **the Python backend already used it** -- `emit.py` writes
`.immortal = 1` on its literals and checks it in release -- so this was a
plain parity gap with the intended shape already visible in the oracle.

Placement is the substance of the fix: the check goes *before* the refcount
decrement, not after it. The old guard sat after, so a literal's refcount
really did sink through zero -- releasing one three times reported
`retainCount` of **-2**. An immortal object's refcount now never moves.

`retain` is deliberately left unconditional, matching the oracle: it
increments whatever it is given. Pinning an immortal object's count would
have been a second divergence dressed up as tidiness, and `retainCount`
reporting the true number is more honest than a fabricated constant.

**The singleton half was a use-after-free, not a naming problem.**
`Singleton+Protocol.h` states the contract outright -- "Singleton objects are
immortal, they are never deallocated" -- and nothing enforced it. It held only
because no code in the repository happens to release a singleton. With the
marker removed, releasing one runs `-dealloc` and hands its slab slot back
while `+sharedInstance` keeps returning the same pointer; the test prints
`config dealloc` and then reads `rate_after=0` out of a freed slot.

Conformance to `SingletonProtocol` is the signal, via the existing
`Program::class_conforms_to`, rather than a heuristic on the
`+sharedInstance` shape: all three singletons in the repository declare it
(`samples/arc_demo`'s AppConfig, `samples/heap_alloc`'s App,
`samples/zbus_service`'s TemperatureService), and a wrong guess would mark an
ordinary object immortal, whose slab slot then never comes back. The marker
is emitted in the *allocator*, which is the one place every instance passes
through -- so `heap_alloc`'s App gets it in both `App_oz_alloc` and
`App_oz_alloc_with_heap`, verified in the ARM build's output.

Here the oracle is *not* followed: it does not mark singletons at all. That is
a deliberate departure on the grounds the CLAUDE.md rule states -- the Python
pipeline is a reference, not an authority -- and it is invisible to
`just test-cross-backend` precisely because nothing in the corpus releases a
singleton. So this half rests on its own tests rather than on cross-backend
agreement, which is worth knowing when reading the 73/73.

Two things this needed that are worth recording:

- **The four pre-existing tests could not have caught any of it.** Both
  mechanisms prevent the crash, so `behavior_immortal_literals.rs`'s original
  cases pass either way. The discriminating observation is the *refcount*, not
  whether the program survives -- which is why the new cases assert
  `retainCount` and were each checked to fail with the fix disabled.
- **The real singleton spelling could not be used in a test.** All three
  samples hold their instance in a file-scope `static Config *_shared;`, and
  the single-file emitter this suite drives did not tag bare class names in a
  top-level declaration -- `class_tag_edits` was called only from
  `emit_split`. The fixture used a method-local static instead. Fixed as #246
  (gap U), and the fixture now uses the real spelling.

**U. Gap A was only half fixed, and the half left open was invisible to the
test suite.** Fixed (#246). `class_tag_edits` was called from `emit_split`
only; the single-file `emit()` had **no `declaration` arm at all** and did not
tag a function signature either, so both of gap A's positions stayed broken
there. `emit()` patched the original text, so anything no arm claimed survived
verbatim -- which is how an untagged `static OZHeap *sHeap;` reached the C
compiler.

No shipped output was ever wrong: every real build goes through the CLI, hence
`emit_split`. The cost was entirely in what could be *tested*. The whole Rust
suite drives `oz_static::transpile()`, so until this fix **no Rust test could
use a file-scope object declaration** -- the shape `samples/gpio_demo`
(`static GPIOOutput *led;`), `samples/heap_alloc` (`static OZHeap *sHeap;`)
and all three singletons are built on. That is the mechanism by which gaps A
and D were each diagnosed against a sample, fixed, and never locked in by a
test; and it is why gap T's singleton fixture had to be written around a
method-local static. That fixture now uses the production spelling.

The asymmetry itself is the lesson, and it is the third finding of its kind:
gap R recorded `staticbar` and `emit::collect_local_decls` disagreeing about
what counts as a local, and the fix there was to make them share one
definition. Two emitters that disagree about what valid output looks like will
keep producing this shape of bug — which is what eventually got the walk
merged; see the end of this entry.

Now filed as **#254**, so the mechanism is tracked somewhere a person is
assigned rather than only described here. That issue records the concrete cost
— the two arm lists, `EmitCtx` constructed six times over — and the fact that
made it worse than ordinary duplication: every Rust test drove `emit()` while
every real build drove `emit_split()`, so the path with test coverage was not
the path that shipped.

That issue's first task was an audit, since all three findings had been
stumbled into rather than looked for. **It is done and found no remaining
divergence**: `tests/emitter_agreement.rs` drives both emitters over every
top-level node kind the walk matches on, plus `@synchronized` and a
construct outside the static subset, and compares diagnostics and symbol
presence — not text, since the two deliberately place text differently.

Worth knowing from it: the asymmetry has bitten in *both* directions, which
cuts against reading `emit_split` as simply the more complete one. #246 was
`emit` missing a `declaration` arm entirely; gap C's seventh cause was
`emit_split` *dropping* a top-level struct that `emit` kept by not touching it.
Both directions now have a case.

The audit lowered the urgency and did not close the case: it guarded the
*known* node kinds, so a new kind added to one walk and not the other still
slipped past until someone added a case.

**The structural fix has since landed, and the mechanism is gone.** There is
one `emit::walk_top_level` returning per-origin buckets, and two *assemblers*
over it: `emit_split` (one `.h`/`.c` pair per origin — the CLI, and so every
real build) and `emit` (one translation unit — what `transpile()` exposes and
the Rust suite drives). A node kind is now handled in exactly one place and
reaches both by construction, which is the property gap R, #246, #250 and #251
each restored by hand for one case. `EmitCtx::new` replaces the eighteen fields
that were spelled out at six call sites, three per emitter — the reason #250's
fix had to be written twice.

What decided the shape was a measurement #254 itself asked for: `emit()` has no
consumer but the test suite (`main.rs` calls `transpile_split_with_options`
only), so the choice was between sharing a renderer and retiring the walk
outright. Sharing it keeps a convenient one-string API for tests; retiring the
*walk* rather than the entry point gets the same guarantee without porting
fifteen test files to a file-writing harness.

**The shipped path was proved not to move**, which is the whole risk of a
refactor like this: every file `oz2c` generates is byte-identical before and
after — 820 across the 73 corpus cases, and 342 across the samples' real ARM
twister build. The manifest is excluded, as the RISC-V comparison excludes it,
because it lists absolute paths. (Gap X, found by that comparison and fixed in
the same change, then moved 146 of those files by deleting a stray `;` from
each. Nothing else moved: **0 lines added anywhere**, 184 removed, all of them
a bare `;` or a blank line left behind by one.)

Three things did change in the single-file output, each of them the split
assembler's existing behaviour winning: a full `struct Tag { … };` goes to the
companion header rather than staying in `source_c`, top-level trivia groups
ahead of the bodies rather than sitting where it was written, and
inter-construct whitespace comes from the assembler rather than the original
text. No test assertion needed changing, which is a weaker statement than it
sounds — the assertions are `contains` checks, and what moved was placement.

`tests/emitter_agreement.rs` survives with a smaller claim, stated in the file:
it can no longer compare two walks, so it guards the two *assemblers* and the
presence of each node kind in both. A green run means "both assemblers surface
every kind these cases name", not "two implementations agree".

Two adjacent gaps surfaced while writing the tests, both left open and filed,
because each is about *type tracking* rather than about emitting the tag:

- **Writing the tag by hand cost the variable its type tracking.** Fixed
  (#251), and the cause is one step further down than this entry first
  guessed. `collect::extract_type_and_stars` has a `struct_specifier` arm that
  reports `struct Widget` rather than `Widget` -- deliberately, because an
  incomplete struct type can only ever be spelled with the keyword. But
  `file_scope_vars` then tested that string for membership in the known-class
  set, which is keyed on bare names, so the tagged declaration was silently
  skipped and a send through it reported an `id` receiver. Stripping the
  prefix before the membership test fixes it; the bare name is also what
  `render_type` wants, since it re-adds the tag.

  The prediction gap R suggests -- that the *local* collector has the same
  hole -- was checked and is false: `collect_local_decls` has no membership
  gate at all, so `struct Widget *w = ...;` always resolved. So this was a
  genuinely one-sided disagreement, which makes it the third instance of the
  same pattern rather than a fourth variant of it: two places answering "is
  this an object declaration?" differently, and only one of them wrong.

  What kept it from being a user-visible problem is that there is no reason to
  write the tag by hand -- oz_static adds it. What made it worth fixing anyway
  is that the two spellings mean the same thing in C, and a rule that depends
  on which one you wrote is not a rule.
- **A free function's parameters were not type-tracked.** Fixed as #250; see
  gap V.

**V. A free function's parameters were not in scope for its own body.** Fixed
(#250). `emit`'s `function_definition` arm seeded its scope from
`file_scope_vars` alone and never from the parameter list, so

```objc
static int readWidget(Widget *w) { return [w n]; }
```

was rejected -- `cannot statically resolve the receiver type for selector 'n'
(receiver type is 'id')` -- while the identical method
`- (int)readWidget:(Widget *)w` resolved, because `render_method_definition`
has always inserted `sig.params`.

This is the third finding in the same shape as gap Q, where the static bar
turned out never to scan a plain C function at all: the free-function path
keeps getting a *reduced* version of what a method body gets, and each time
the missing piece is invisible until someone writes the ordinary code. Passing
an object to a helper and sending it a message is not an exotic construct, and
the diagnostic gave no hint that the parameter rather than the send was the
problem.

`collect_function_params` inserts every parameter, not only the object-typed
ones, because that is what a method does -- and two paths that disagree about
what a scope contains is the mechanism behind #246 and gap R both. It is
applied in `emit` *and* `emit_split`, which build their `EmitCtx` separately;
#246's fix had to be made twice for the same reason.

Three things worth recording:

- **Adding parameters to `ctx.locals` cannot make ARC release a borrowed
  one.** Checked before relying on it: `managed_object_locals` looks for
  `declaration` nodes *inside the body*, and a parameter is a
  `parameter_declaration` outside it, so it can never be picked up as owned.
  Releasing a reference never taken is a double free -- gap L in reverse, and
  the reason this needed checking rather than assuming.
- **Parameters are seeded before `collect_local_decls`,** so a body
  declaration of the same name shadows the parameter rather than the reverse,
  which is C's own rule.
- **The bar is unchanged.** An `id` parameter still cannot receive a send: its
  class is genuinely unknown, and seeding a scope must not turn "unknown type"
  into a quiet guess. Pinned by its own case, since it is the one thing here
  that must keep *failing*.

**W. `@synchronized` excluded nothing between cores, and was indistinguishable
from no lock at all.** Fixed. Found by building the first sample where two
threads touch the same object, on the first board with two cores.

It lowered to a spinlock declared *inside the block*, on the caller's own
stack, initialized fresh on entry:

```c
oz_spinlock_t _oz_sync_lock_L1551_C2_1;
oz_spin_init(&_oz_sync_lock_L1551_C2_1);
oz_spinlock_key_t key = oz_spin_lock(&_oz_sync_lock_L1551_C2_1);
```

Two threads therefore locked two different locks. Nothing keyed on the
object's identity, though `@synchronized(self)` names it.

**Measured on two cores, same sample, three configurations:**

| Lock | Result |
| --- | --- |
| none at all | `count=2023 expected=4000` |
| per-block (what shipped) | `count=2015 expected=4000` |
| per-object (now) | `count=4000 expected=4000` |

The first two rows are the finding. The old lock was not weak, it was absent.

**Why it survived this long is the more useful part.** `k_spin_lock` calls
`arch_irq_lock()` unconditionally, so on a single core the critical section
really is serialized -- by disabling interrupts, for a reason unrelated to
which lock was taken. Every board tested until now was single-core, and no
sample or test had two threads reach the same object. It was correct by
accident everywhere anyone looked.

This was **not** an undiscovered oversight: `render_synchronized_statement`'s
own doc comment described the per-block lock as deliberate, said plainly that
it buys "an interrupt-disabled critical section, not mutual exclusion keyed on
`obj`", and recorded that the per-object alternative had been tried and
rejected because a `k_spinlock` is not recursive. What was missing was
evidence, and the reason the trade looked acceptable was that its cost could
not be seen on any board in use.

**The fix, and why it is not the rejected one.** The lock is now a root-struct
field (`oz_sync_lock`, added only when the program uses `@synchronized`,
mirroring the conditional `oz_prop_lock`), plus `oz_sync_owner` recording which
thread holds it. A re-entrant `@synchronized` on the same object *does not
attempt the second acquire* -- it sees itself as owner and skips both lock and
unlock:

```c
int held = (obj->oz_sync_owner != oz_current_thread());
if (held) { key = oz_spin_lock(&obj->oz_sync_lock); obj->oz_sync_owner = oz_current_thread(); }
...
if (held) { obj->oz_sync_owner = 0; oz_spin_unlock(&obj->oz_sync_lock, key); }
```

`held` is a per-block local, so nesting unwinds correctly at any depth with no
depth counter: inner blocks never acquired, so they never unlock. That is what
makes this different from the rejected design -- the spinlock is never
re-locked, which is the actual constraint, rather than made recursive, which it
cannot be. Recursion now matches Objective-C's own semantics instead of being
a hazard.

The alternative -- a plain per-object lock without owner tracking -- was
implemented first and would have **hung on target** in a shape the suite
already exercises: `behavior_synchronized.rs`'s `[n runNested:n]` passes the
same object as both receivers, so `@synchronized(self)` then
`@synchronized(other)` is one object twice. It passed on host regardless,
because the host PAL's `oz_spin_lock` is `(void)lck; return 0;`. A textual
check for identical receiver spellings does not catch it either, the two
spellings being `self` and `other`. That test now passes for the right reason,
and `samples/smp_shared` runs the re-entrant path on two cores with a real
spinlock.

Three things worth recording, two of them mistakes:

- **The obvious version of the sample proved nothing.** Written as a tight
  three-statement read-modify-write, the *unlocked* build also passed, with a
  perfect 40000: QEMU's TCG emulates cores in coarse translation blocks and
  never interleaved a critical section that short. A 200-iteration volatile
  spin holding the window open is what separates the numbers above. Without
  that negative control the sample would have looked like strong evidence for
  a lock that did nothing -- and would have concealed this gap rather than
  finding it.
- **`oz_spinlock_key_t key = 0;` does not compile on Zephyr.** That type is
  `k_spinlock_key_t`, a *struct*. Same family as the `oz_spinlock_t lock = {0}`
  failure recorded under "On target" -- assuming a PAL type is scalar. The PAL
  gained `oz_spin_key_none()` on both backends, so "no key yet" has a spelling
  that does not depend on the type.
- **The refcount half of the sample cannot be falsified.** `rc_after=1` held
  even in the unlocked build, because retain/release go through real atomics
  rather than a plain read-modify-write, and there is no equally cheap way to
  un-atomic them for a control. Those lines confirm the atomics behave under
  two-core load; they do not prove the atomicity is load-bearing the way the
  count line proves the lock is. Stated in the sample too, so nobody reads more
  into a green run than it carries.

**X. Every generated program carried a bare `;` at file scope, which is not
valid ISO C.** Fixed (#254, folded into the same change). Found by comparing
generated output byte for byte, which is a measurement taken for a different
reason — nothing was failing, and nothing was going to.

Several arms consume a specifier node whose grammar span stops short of its
trailing semicolon, so the semicolon arrives at the passthrough arm as a
top-level node of its own and was copied through. Three producers of that
shape, one of them in every program: `@compatibility_alias NSObject OZObject;`
in `include/oz_sdk/Foundation/OZObject.h`, plus a full `struct`/`union`
definition and an `enum` definition. **51 of the samples' generated files and
146 of the corpus's** had one. A fourth producer, of a different shape and
visible only on target, outlived this fix — see gap Y.

An empty declaration at file scope is a constraint violation, not a style
question — but diagnosing it needed `-Wpedantic`, which Zephyr does not pass
and which the `-Wall -Wextra` sweep behind gap S did not either. So gap S's
claim that generated C is `-Wall -Wextra` clean was true and this was invalid
anyway, which is the useful thing to notice: **"warning-free under the flags
we pass" is a narrower statement than "valid C"**, and the two had been read
as the same. Since gap Y the corpus is compiled with `-std=c17
-pedantic-errors`, so this shape now fails a test rather than waiting to be
noticed.

Fixed at the passthrough arm rather than in each arm that leaves one. That is
the single place every unclaimed node goes, so a future arm gets the same
treatment without knowing to ask, and nothing is lost — a top-level `;`
carries no information in any C dialect. Pinned by
`emitter_agreement::no_bare_semicolon_at_file_scope`, which puts those three
producers in one source and was confirmed to report 4 stray semicolons with
the guard disabled.

**It had a fourth producer, and only on target.** That test cannot reach it —
in the generated text the `;` is attached to a macro call, so only the
preprocessor separates them — and reading it as covering "all producers" is
what would make the gap look closed. See gap Y.

**Y. Nothing measured whether generated C is *valid*, and the first
measurement found gap X still alive on Zephyr.** Fixed (#266): the corpus
compile check now passes `-std=c17 -pedantic-errors` and `just test-pedantic`
asks the same of the samples with the real ARM toolchain.

Gap X is the reason this was worth doing and gap S's history is the reason it
started with a count rather than a fix. Before it, PARITY.md's own definition
of **compiles** — "the generated `.c` files pass the host compiler" — meant
*compiles as GNU C with the flags Zephyr happens to pass*. `c17` is what
Zephyr itself pins (`CONFIG_STD_C17`; nothing here selects
`CONFIG_GNU_C_EXTENSIONS`), so the corpus is now asked exactly what the target
asks and no more.

**The counts, which are not close to each other.** The corpus is at **0**
across all 71 cases, which is why it is a gate
(`-pedantic-errors`) rather than a report. The samples were at **29 sites** on
ARM. A host sweep could not have found any of the ones that mattered, and that
asymmetry is the entry's substance.

"Gate" was narrower than it sounded when this entry was written: **CI did not
run the Rust suite at all** — `ci.yml` never invoked `cargo test`, so this
gate and every other assertion in `corpus_parity.rs` held on a maintainer's
machine and were unverified on every PR. Fixed in #269, which also let
`hw-build-check` fail (it was `continue-on-error: true`, and so reported
`pass` whatever the build did). Found while landing this change, which is
itself the pattern gap S and the "Not verified" section keep recording: the
claim that goes stale is the one about what has been checked.

One consequence of #269 belongs here rather than there, because it is about
the AST oracle this file describes. The Clang dump that answers "is this ivar
an object the class owns?" and "does this method exist?" — see "Clang as the
authority" below — was being produced in CI by **clang 18.1**, Ubuntu's
preinstalled one, because the SDK was installed without its LLVM component
and `objz_find_clang()` fell past its SDK entry to `PATH`. The tested version
is the SDK's 19, and the same job separately installed clang 20 and never
used it. Nothing is known to have gone wrong because of it; what is worth
recording is that the compatibility warning designed to catch exactly this
printed on every run for the life of the workflow, unread in a 1400-line log.
A warning about a silently-substituted oracle is not a check.

**The survivor was the item pool, and it was correct on host.**
`OZ_MEM_BLOCKS_DEFINE(oz_item_pool, ...);` is a PAL macro that was
self-terminating on *one* backend: on Zephyr it expands to
`SYS_MEM_BLOCKS_DEFINE`, whose body ends in `;` (`sys/mem_blocks.h:156`), so
the `;` at the call site became a bare one at file scope — while the host
backend's macro ended in `}` and needed exactly that `;`. **The same emission
was valid on host and a constraint violation on target**, so no host check
could ever see the difference, and unlike gap X's other three producers this
one reached only programs that build an item pool, which is how it outlived
them. Both PAL macros are self-terminating now, matching Zephyr's own
convention; the four call sites in `tests/pal/` and the Python backend's
emission of the same line (`templates/oz_dispatch.c.j2`) follow. Pinned by
`emitter_agreement::item_pool_definition_has_no_trailing_semicolon`, which
asserts on the text rather than by compiling — compiling is the instrument
that could not see it, and at the text level the `;` is attached to a macro
call, so only the preprocessor separates them.

**Two ways to measure this and get zero, both of which read as good news.**
Recorded at length because each one wasted a pass here, and because gap S's
first re-measurement failed the same way:

- **`-Wpedantic` cannot go in `EXTRA_CFLAGS`.** It reaches Zephyr's own
  sources, where it does not merely warn: `subsys/mem_mgmt/mem_attr.c` fails
  with `error: zero or negative size array`, so all 13 samples stop building.
  The sweep therefore builds each sample entirely normally and recompiles only
  the files under `oz_static_generated/`, from the exact command
  `compile_commands.json` records.
- **CMSIS switches `-Wpedantic` off for the rest of the translation unit.**
  `modules/hal/cmsis_6/CMSIS/Core/Include/core_cm3.h:28` opens with
  `#pragma GCC diagnostic ignored "-Wpedantic"` — no `push`, no `pop` — so
  once anything reaches `zephyr/kernel.h`, which every generated TU does,
  pedantic diagnostics stop being reported for everything after it. This is
  not a subtle degradation: injecting a bare `;` **and** an empty struct into
  generated files produced **zero** diagnostics, while `-Wall`'s
  `-Wunused-variable` still fired from the same file, and a single
  `#pragma GCC diagnostic pop` restored both. So the sweep compiles each TU
  through a wrapper that pulls Zephyr in first, re-enables the flag, and only
  then includes the real TU — which covers generated *headers* too, unlike
  inserting the pragma at the top of the `.c`.

  Worth stating plainly: an ARM `-Wpedantic` sweep written the obvious way
  reports a clean result on output that is not clean. That is the same failure
  mode as `tests/zephyr/` exercising no transpiler — a green run whose subject
  is not what the reader thinks.

**What remains, and why it is now a gate.** 10 sites, each in
`scripts/objz_pedantic_sweep.py`'s `KNOWN_PEDANTIC` with its reason, and
that list asserts every entry *still* occurs, so fixing one forces an
update — `KNOWN_CC_FAILURES`'s discipline, for the same reason.

| Sites | What | Whose |
| --- | --- | --- |
| 6 | `ISO C99 requires at least one argument for the "..."` — `GPIO_DT_SPEC_GET`, `ZBUS_CHAN_DEFINE` | inside Zephyr's own macros, invoked from sample passthrough C. No spelling of those calls avoids it |
| 2 | same, from `ZBUS_OBS_DECLARE` | inside Zephyr's `FOR_EACH_NONEMPTY_TERM`. Added with the declaration `samples/zbus_service` needs because its OZM-wrapped listener leaves Clang no symbol for `ZBUS_CHAN_ADD_OBS` to check |
| 2 | `extra ';' outside of a function` after `ZBUS_CHAN_DEFINE(...)` | Zephyr's documented idiom — every Zephyr zbus sample writes it, and with `ZBUS_OBSERVERS_EMPTY` the terminator macro emits nothing and the `;` becomes *required*. Removing it would make the source depend on those channels keeping their observers |

The 18 that used to dominate this table were one cast in `src/OZTimer.m`,
2 sites × 9 samples, and they are gone: #267 retired the class rather than
giving its PAL helper a second signature (gap AB).

**It was already a gate in substance and nobody was running it.** The
sweep exits non-zero unless what it finds matches `KNOWN_PEDANTIC`
exactly — a new violation fails, and so does a baseline entry that stopped
occurring. What it lacked was CI: it ran only on a maintainer's machine,
which is exactly how three new sites reached `main` in #275 unmeasured.
There is a `pedantic-gate` job now. #269 is the standing lesson and this
is its third instance: a check CI does not run is a check that holds
nowhere.

**One of those three was a real fix rather than a baseline entry**, which
is worth separating. The `;` after `ZBUS_OBS_DECLARE(...)` was an empty
declaration: that macro is `FOR_EACH_NONEMPTY_TERM(_ZBUS_OBS_EXTERN, (;),
...)` and terminates each declaration itself. Dropping it removed the
site. That is the *opposite* of `ZBUS_CHAN_DEFINE`, whose `;` this table
records as required — same subsystem, same shape, different terminator
behaviour, and the only way to tell was to read the macro.

**A provenance rule was considered for the remaining 8 and cannot be
built.** The intent was to classify a diagnostic as the target's when its
cause lies inside a target macro, and gate on the rest. GCC gives nothing
to classify with: these are *preprocessor* diagnostics, reported at the
invocation with **no "in expansion of macro" note at all** — checked on
the real output. So a rule would have to guess from the source line, and a
rule that guesses wrong suppresses real findings silently. The baseline
with a stated reason per entry is the honest instrument, and it is the one
that caught #275's regression.

**Z. Blocks were lowered to function pointers everywhere except the top
level, where three positions reached the C compiler with the `^` intact.**
Fixed (#272). Not a weaker type this time but text no GCC target can parse:
blocks are a Clang extension rather than ISO C, and
`arm-zephyr-eabi-gcc -std=c17` reports `expected ')' before '^' token`.
Confirmed against the real toolchain, and `clang -fno-blocks` reports
`blocks support disabled` on the same text.

Every position routed through `collect::render_type` was already correct — an
ivar becomes `void (*_ivarBlk)(int)`, a method parameter
`void (*b)(struct k_timer *)`, a local `void (*local)(int)`. The three that
were not are the ones `emit::walk_top_level` assembles by *patching the
original text*, where no edit lowered a block type:

| Position | Emitted as |
| --- | --- |
| a file-scope block variable | `static void (^g_blk)(int);` |
| that variable's block-literal initializer | `= ^(int v) { ... };` |
| a free function's signature, prototype and definition alike | `static void take_cb(void (^cb)(int))` |

Each is valid Objective-C — checked against
`clang -x objective-c -fobjc-arc -fblocks` — so each was a real
valid-in / invalid-out defect rather than a shape nobody may write. Nothing
in the repository writes any of them, which is why they went unnoticed. The
fourth instance of the family gaps Q, V and R record, and #246 / #250 / #251
before it: the top-level or free-function path getting a reduced version of
what a method body gets.

`block_pointer_edits` lowers a declarator's `^` to `*` and
`top_level_block_edits` hoists a literal to a named function, both applied at
the passthrough arm as well as the function-definition arm — the one place
every unclaimed node goes, which is what gap X's bare-`;` fix chose and for
the same reason. The static bar now runs over a hoisted body too, a position
that had no scan at all: the top-level twin of the free-function scan #234
added.

**Nothing else moved**, checked the way #254 checked its refactor rather than
assumed: both binaries were run over all 73 corpus cases and the generated
files diffed, excluding `oz_static_manifest.txt` because it lists absolute
paths. **10 differing lines, all 5 of them one banner comment reworded** —
see the third note below — and no other line anywhere. Three positions gained
a lowering that nothing in the corpus writes, so a clean diff is what should
have happened; it is worth having measured rather than reasoned, since the
edits are applied at the arm every unclaimed node passes through.

Three things worth recording, two of them about the instrument:

- **Running the output cannot see this defect on a Mac, and nearly hid it.**
  The Rust suite's `compile_and_run` uses the host `cc`, which is Apple
  clang, and clang enables blocks by default — so a surviving `^` compiles
  there as a perfectly good Clang block. One test drafted as compile-and-run
  alone **passed with the fix disabled**, because the declaration and its
  initializer were both left as blocks and agreed with each other. Every test
  now asserts on the generated *text*, and `samples/transpiled_blocks`
  carries the two writable shapes so the ARM build is the check that means
  something. Same shape as gap Y's finding that an ARM `-Wpedantic` sweep
  written the obvious way reports clean on output that is not.
- **The idiom this was filed for is reachable, but only through a macro that
  discards its block argument on the Objective-C side.** #272 was opened to
  make a Zephyr definition macro take an inline block, which would dissolve
  #267 by retiring `OZTimer` altogether -- which is what happened; see gap
  AB. Writing Zephyr's macros *directly*
  does not work: `struct zbus_observer::callback` is a
  `void (*)(const struct zbus_channel *)`, and **Objective-C refuses
  block-to-function-pointer conversion in every position** — by cast or by
  initialization, with ARC or without: `error: initializing 'void (*)(int)'
  with an expression of incompatible type 'void (^)(int)'`. Clang is not
  optional here, since `cmake/oz_static.cmake` dumps one AST per source as
  oz2c's ownership oracle (gap N) and the Python backend compiles the same
  file, so a source Clang rejects is not an option however well oz_static
  lowers it.

  **`OZM` is the escape, and it is implemented (#272), not hypothetical.**
  `include/oz_sdk/Foundation/OZMacro.h` defines `OZM(...)` as *empty* for
  Clang, and `include/platform/oz_platform.h` defines
  `OZM(target, ...) target(__VA_ARGS__)` for the C compiler:

  ```objc
  OZM(ZBUS_LISTENER_DEFINE, lis_print_temp, ^(const struct zbus_channel *chan) {
          ...
  });
  ```

  It works because **a macro is the only construct whose argument
  Objective-C leaves unparsed**: an argument whose parameter is absent from
  the replacement list is discarded rather than expanded, so it need only
  lex, and `^` is a valid punctuator. Clang therefore never type-checks the
  block. By the time the C half expands, `top_level_block_edits` has already
  turned the literal into the name of a hoisted function — so the argument
  really is a function pointer, which is what `Z_TIMER_INITIALIZER`'s
  `.expiry_fn = expiry` and `zbus_observer::callback` need, an address
  constant. One name serves every target macro: no per-primitive wrapper, no
  second arm to keep in step, and the call site still names the macro it
  means.

  **Both halves are pure preprocessor, and the transpiler rule this entry
  first described is gone.** It began as `emit::ozm_edits`, a rewrite in the
  emitter; `#define OZM(target, ...) target(__VA_ARGS__)` does the same work
  by substituting `target` and letting the replacement list be rescanned, so
  ~90 lines came out of the emitter and OZM stopped being a naming
  convention the compiler had to know. The transpiler's remaining
  contribution is the part only it can do: the hoist.

  The two halves are in *separate files*, each unconditional, and that is
  forced rather than chosen — each side reaches exactly one of them.
  Objective-C never includes the PAL (checked: zero mentions in the
  preprocessed output of a file importing `Foundation.h`), and `OZMacro.h`
  declares no Objective-C, so gap C's rule gives it no generated output pair
  and its definition never reaches the C side. The rewrite did not care,
  having removed the invocation before any C compiler saw it; the
  preprocessor version does.

  Two things measured and deliberately not done: no `OZM_DEFER` level, the
  canonical extra rescan, because the single-level form expands a plain
  target and an aliased one alike; and no support for `OZM(MACRO)` with
  nothing after the name, which leaves `__VA_ARGS__` empty — a C23
  extension that `-std=c17 -pedantic-errors` rejects, and the corpus is
  gated on those flags.

  A named spelling is available without a second mechanism, for anyone who
  prefers it: `#define OZM_K_TIMER_DEFINE(...) OZM(K_TIMER_DEFINE,
  __VA_ARGS__)` is one line, works on both sides because `OZM` already
  does, and can be written in the source itself since oz_static passes a
  `#define` through. A *generic* prefix strip is not possible — the
  preprocessor has no operation that takes an identifier apart, which is
  precisely why the macro's name has to be an argument.

  Nothing more Objective-C-ish was available, and that is a consequence
  rather than a preference: anywhere Clang actually type-checks the
  expression the conversion is refused, so an attribute on a named C
  function discards the inline block, `+initialize` calling `k_timer_init`
  hits the same wall, and the language has no file-scope declarative form
  beyond `@interface`. The macro is the only door.

  **Measured on target, not just transpiled.**
  `samples/zbus_service/src/main.m` now writes its zbus listener as an inline
  block and its generated C reads
  `ZBUS_LISTENER_DEFINE(lis_print_temp, oz_block_L1806_C43_1);`. Under
  twister on `mps2/an385` it prints `+ [listener] Temperature: 11` — real
  zbus invoking the hoisted block.

  **Only that line needs `OZM`.** `ZBUS_CHAN_ADD_OBS` is pure C and stays
  pure C: it expands to `.obs = &_obs`, so it needs `lis_print_temp` to exist
  for Clang, which the discarded definition does not provide — and Zephyr's
  own `ZBUS_OBS_DECLARE` is exactly the idiom for that. It expands to
  `extern const struct zbus_observer`, which agrees with the `const`
  definition the generated C gets, so it is written unconditionally rather
  than under `#ifdef __OBJC__`. This file first said the pair had to be
  discarded together; wrapping a line with no Objective-C in it also meant
  Clang stopped checking that line, for no gain.

  **The AST check this entry originally claimed does not hold, and finding
  that out was worth more than the claim.** It said the dump "comes back
  non-empty, which is the check that matters, since a source Clang rejects
  fails silently". Non-empty proves nothing: Clang writes a partial AST on a
  fatal error and exits non-zero, and the dump is taken with
  `2>/dev/null || true`. Run by hand, `zbus_service`'s own dump command
  fails with `fatal error: 'TemperatureService.h' file not found` — and has
  all along, for a reason unrelated to `OZM`. See gap AA.

  An `OZM(...)` that somehow reached the C compiler unrewritten is a
  `_Static_assert(0, ...)` rather than an expansion to nothing — dropping a
  listener silently would leave a program that builds and does not work, and
  oz_static never degrades quietly.

  This document said the idiom was "not writable at all" when gap Z was first
  written, which was wrong: a fact about the macros Zephyr happens to ship,
  generalised into a claim about Objective-C. What holds is the narrower one
  — a macro that *consumes* the argument on the ObjC side cannot be used, so
  Zephyr's own macros need `OZM` in front of them.

  Three costs, none small enough to leave unsaid. A hoisted block captures
  nothing (`staticbar` rejects captures), so such a callback reaches its
  context only through the API's own channel — `k_timer_user_data_get`,
  `zbus_chan_const_msg` — which is what `OZTimer`'s `_userdata` ivar wrapped
  and the constraint Zephyr's own C callbacks already live under. Whatever
  the macro *declares* is invisible to Clang, since the whole invocation is
  discarded there: code naming the object outside another `OZM(...)` must
  declare it under `#ifdef __OBJC__`, which passes through to the generated C
  where the real macro defines it. And **the rewrite is oz_static's**, so
  under `CONFIG_OBJZ_BACKEND_PYTHON` the Objective-C arm is the only one
  there is — the invocation expands to nothing and the callback is never
  registered, in a program that still builds. A silent behavioural
  difference on the outgoing backend rather than a build failure.

  That last one costs `samples/zbus_service` nothing, which is worth checking
  rather than assuming: it already cannot build under the Python backend, for
  an unrelated defect of that backend's own (it emits Clang's diagnostic
  spelling of an anonymous enum into a header — see the code-size section).
  Any *other* sample adopting `OZM` would be making the trade for real.
- **A source Clang rejects fails silently, which is worse than a hard
  failure**, and is why the constraint above is a real one rather than a
  nuisance. The dump is taken with `2>/dev/null || true`, and the "no usable
  Clang AST" warning fires only when *every* dump is unusable — so one such
  file loses just its own ARC facts, and ARC then skips its `id` ivars rather
  than releasing them. A leak, with a green build. The same shape #269
  recorded: a warning about a silently-substituted oracle is not a check.
- **A banner in the generated output had to change, and it is the only thing
  that did.** `render_block`'s comment read "hoisted out of its enclosing
  method", true of every caller until a file-scope literal became one — there
  is no enclosing method to name. It says "hoisted from a block literal" now:
  where the literal was, not what enclosed it. Worth noting because it is the
  same stale-claim shape this document keeps recording, in the one place a
  reader of the generated C would meet it.

**AA. The Clang AST dumps were silently truncated — on every RISC-V sample,
and on five samples' own files everywhere.** Fixed (#274). Found while
checking a claim gap Z made about `zbus_service`, not by anything failing,
and it turned out to have **two independent causes**. The one the issue was
filed on was the smaller.

`_objz_build_ast_flags` (`cmake/ObjcClang.cmake:509`) collects include
directories from the **`zephyr_interface` target only**. A sample's own
`include_directories(include)` — or `include_directories(app PRIVATE include)`
— reaches neither, so a dump of that sample's `.m` stops at the first
`#include` of its own header:

```
samples/zbus_service/src/main.m:10:10:
        fatal error: 'TemperatureService.h' file not found
```

Clang still writes the AST it built up to that point and the wrapper swallows
the status (`2>/dev/null || true`), so the file is large — 51 MB, all of it
SDK and Zephyr declarations — and contains **none of the sample's own
`@implementation`s**. Verified on a second sample: `heap_alloc`'s `App.m` and
`main.m` each fail the same way while all eleven `src/*.m` dumps are clean,
which is the tell — the SDK's files need only `oz_sdk` includes, which the
flags do carry.

Five samples have their own `include/` and are affected: `gpio_demo`,
`heap_alloc`, `hello_category`, `zbus_objc`, `zbus_service`.

**Latent, not active.** The dump is the only authority on whether an `id`
ivar is an object the class owns (gap N), and without it oz2c stays
conservative and skips every `id` ivar — correct, but a leak. None of the
five declares an `id`-typed ivar, so nothing leaks today. What is real is
that the oracle those builds appear to supply is absent, and `--ast` was
added (gap N) precisely so the production build would stop being the one
place the facts were missing.

**The second cause was larger: the dumps named no `--target`.**
`_objz_get_clang_target_triple()` has existed all along and
`_objz_build_ast_flags` never called it, so every dump was parsed as the
*build machine* — 64-bit pointers on an arm64 Mac — and Zephyr's arch
headers then reached for intrinsics the host has no declaration of.
Measured on one `src/OZTimer.m` dump for `qemu_riscv32` (that file has
since been retired, #267): **20 errors without
the triple, 1 with it.** Twenty is Clang's default `-ferror-limit`, at which
point it emits `fatal error: too many errors emitted, stopping now` and
stops — so **every RISC-V dump was truncated**, 26 of 150 files reporting it
once the diagnostics were kept. The one error that remains is
`__oz_timer_setup` being undeclared, which is ordinary and truncates
nothing (#267).

So the include-path hole cost five samples their own `@implementation`s, and
the missing triple cost RISC-V everything past the first arch header. Both
are fixed; both were invisible.

**After: 0 truncated dumps out of 165 on `mps2/an385` and 0 of 150 on
`qemu_riscv32`**, with 13 of 13 and 12 of 12 samples still passing.

**And no generated C moved.** Every sample's output was diffed pre-fix
against post-fix — **0 differing lines across all 13** — which is what makes
the "latent, not active" claim above a measurement rather than an argument:
restoring the oracle changed no decision, because no affected sample has an
`id` ivar for it to decide about.

Three things this says about the instrument, which is why it sits here
rather than only in an issue:

- **A non-empty AST dump is not evidence Clang accepted the file.** Gap Z's
  first draft used exactly that check and it was wrong. The status is
  discarded by the wrapper, so the only honest check is to re-run the dump
  command and read its exit code.
- **Nor is a non-zero exit evidence the dump is unusable**, which cost a
  pass here to learn. Clang exits non-zero for an ordinary error and then
  carries on; only a `fatal error` stops it. Failing on exit status alone
  rejected dumps that had always been complete. The criterion is
  `fatal error`, because that is the one that truncates.
- **The existing warning could not catch any of it.** `oz_static.cmake`
  warns only when *no* dump at all is usable; a truncated one is a file with
  content, so it counted as usable. #269 recorded the same shape one step
  less severe — the oracle being silently the wrong clang rather than
  silently truncated — and concluded that a warning about a substituted
  oracle is not a check. The check now lives in its own script, run at
  build time between the dumps and oz2c, and it is fatal by default
  (`-DOBJZ_ALLOW_PARTIAL_AST=ON` to downgrade it).

**Why the check runs at build time and not at configure time**, which is
itself a thing worth knowing: the dump script runs twice. The configure-time
run exists only to discover the output file list, and Zephyr's *generated*
headers do not exist yet on a pristine build, so anything reaching
`zephyr/kernel.h` fails there with
`fatal error: 'zephyr/syscall_list.h' file not found`. That is expected and
harmless — a file name does not depend on an ARC fact. The build-time run is
ordered after `zephyr_generated_headers` and is the one whose dumps reach
the shipped C, so it is the one checked. Putting the check at configure time
broke every pristine build, which is how this was discovered.

**AB. OZTimer is retired; the cast it existed around is gone rather than
worked around.** Fixed (#267). The issue was scoped as a
`__oz_timer_setup` signature decision — give the helper two faces,
`#ifdef __OBJC__`, blocks for Clang and function pointers for the C both
backends emit. That was implemented and verified, and then not used,
because `OZM` (gap Z) makes the helper unnecessary rather than fixable:

```objc
OZM(K_TIMER_DEFINE, demo_timer, ^(struct k_timer *t) { ... }, NULL);
```

Zephyr's own macro, an inline block, no wrapper class and no bridge. So
`src/OZTimer.h`/`.m` are deleted, `OZTimer.h` leaves the `Foundation.h`
umbrella, and **`__oz_timer_setup` is deleted from both copies** — the
Zephyr PAL and the behaviour tests' Zephyr stand-in. The 18 pedantic
sites it contributed to every sample pulling in Foundation, 2 × 9, go with
it: the samples sweep is at 10 sites, from 26.

`samples/transpiled_blocks` carries the replacement and runs it on target,
which is the point — a real `k_timer` firing a hoisted block, printing
`Timer fired: 1` under twister on both boards. A one-shot timer, so the
count is deterministic rather than a function of how long QEMU took.

**What it cost, stated rather than absorbed.** Two corpus cases —
`foundation/timer_basic` and `foundation/timer_zephyr` — tested OZTimer
and are deleted with it, so the shared behavior corpus is **71 cases, not
73**, and `just test-cross-backend` is 71 of 71. That corpus is the Python
pipeline's own suite, so its `just test-behavior` is 71 too. A ztest
(`tests/zephyr/src/test_timer.c`) and a Rust file
(`behavior_foundation_timer.rs`, 3 tests) go as well, and
`tests/zephyr/generated/` was regenerated without them. Nothing was
rewritten to preserve the count: a test of a retired class has nothing to
test, and the capability it covered — a timer firing a block — is covered
on target instead, which is stronger than the host stub it used to run
against.

**What callers lose.** OZTimer wrapped `k_timer` with an ARC-managed
`_userdata` ivar. A hoisted block captures nothing, so a callback needing
per-instance context now reaches it through `k_timer_user_data_get`, as
Zephyr's own C callbacks do. That is a real ergonomic step back for the
managed case, and the trade the retirement makes: one less class, one less
bridge, and the ISO C violation gone rather than relocated.

## On target (Zephyr under QEMU: mps2/an385, qemu_riscv32, qemu_cortex_a53/smp)

The check that was missing, and the one that mattered most. Every
measurement above this section is a host measurement; this one uses the real
ARM toolchain, real `k_mem_slab`, real spinlocks and Zephyr's own warning
set. RISC-V is covered in its own subsection below.

```
just test                       # west twister -T samples/ -p mps2/an385
just project_dir=samples/arc_demo rebuild && just run    # one sample, QEMU
```

`just test` is the real harness: twister builds each sample, runs it under
QEMU, and matches the console output against the `regex:` list in that
sample's own `sample.yaml`. It is stricter than a plain `west build` in two
ways that both mattered — it adds `-Werror`, and it checks output rather
than exit status.

**All 13 single-core samples build for ARM**, and `arc_demo`'s output under QEMU is
byte-identical to the Python backend's.

**13 of 13 twister configurations pass on ARM** — every sample it selects, built, run under
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

### A second architecture: RISC-V (qemu_riscv32)

**12 of 13 samples build, run under QEMU and pass their own `sample.yaml`
checks on `qemu_riscv32`** (#230). `gpio_demo` is the thirteenth and is
correctly excluded: the board has no `led0`/`sw0` device-tree aliases, which
`GPIO_DT_SPEC_GET(DT_ALIAS(led0), gpios)` requires, so its `sample.yaml`
allows `mps2/an385` only. Nothing was fixed to get here — the runs passed
first time.

```
just test-riscv                 # west twister -T samples/ -p qemu_riscv32
just test-boards                # both boards, so neither can hide a regression
```

Two things about this measurement are worth stating precisely, because both
were nearly recorded wrong.

**Execution was never actually blocked; `platform_allow` was.** #230 recorded
builds only, and read the obstacle as twister filtering everything out
because "every `sample.yaml` pins `platform_allow: mps2/an385`". That was not
so — only 7 of 13 pinned anything, `hello_category` and `transpiled_led`
already allowed `qemu_riscv32`, and six samples pinned no platform at all.
So **8 of 13 already built, ran and passed on RISC-V with no changes
whatsoever**; they had simply never been pointed at the board. Four of the
five genuinely ARM-pinned samples (`arc_demo`, `heap_alloc`, `zbus_objc`,
`zbus_service`) gained `qemu_riscv32`, which is the entire change this
verification needed. The mechanical obstacles #230 warned about — `west build
-t run` blocking forever and orphaned `qemu-system-riscv32` children
colliding on `qemu.pid` — never arose, because twister manages the guest
lifecycle itself. Driving twister was the answer to a problem recorded as
needing hand-run QEMU.

**The generated C is byte-identical across the two architectures: 304
generated `.c`/`.h` files over all 12 shared samples, zero differing lines.**
#230 had checked five samples; this is the whole set. That is the design
intent holding — the PAL absorbs the target and nothing
architecture-specific reaches the transpiler's output — and it is why a
RISC-V failure would have had to come from the PAL, the SDK headers or the
board rather than from codegen.

That diff has to exclude two things, and including either makes it look
spectacularly false: `oz_static_manifest.txt` lists absolute output paths,
and `oz_static_generated/ast/` holds the Clang AST dumps, which are oz2c's
*inputs* and are produced per target triple. Compared naively, the twelve
samples show ten million differing lines, essentially all of it AST JSON.
Recorded because the first attempt here did exactly that and the number is
alarming enough to be believed.

Which backend ran was verified rather than assumed, as on ARM: all 12
resolve `CONFIG_OBJZ_BACKEND_STATIC=y`, produce `oz_static_generated/`,
mention `oz2c` in the build log and mention `oz_transpile` nowhere.

Two of the passes carry weight beyond "it ran". `arc_demo` passes its
*ordered* ARC expectations, which pin `Sensor dealloc (value=100)` before
101 is allocated — so gap Q's release-then-allocate ordering, and the
single-slab-slot claim resting on it, hold on a second architecture and not
only on ARM. `heap_alloc` passes, so the `--heap-support` path and
`+allocWithHeap:` (gap I) are exercised there too.

What RISC-V does **not** add: `struct k_spinlock` is empty on
`qemu_riscv32` as well, because `CONFIG_SMP` and `CONFIG_SPIN_VALIDATE` are
both off there as on `mps2/an385`. The other shape of that struct — with
`locked`/`owner`/`tail` — stays unexercised on both. That needs an SMP board,
not a second architecture — see the SMP subsection below, which is where that
gap was closed and where it turned out to be hiding a real defect. Nor is
this a code-size comparison: `qemu_riscv32` reports no FLASH region at all,
being RAM-only, so its figures are not comparable with the ARM sweep's flash
numbers (#231 covers size, same-architecture).

### Two cores: qemu_cortex_a53/smp

**9 of 9 selected configurations pass on `qemu_cortex_a53/qemu_cortex_a53/smp`**
(`just test-smp`), with `CONFIG_SMP=y` and `CONFIG_MP_MAX_NUM_CPUS=2` verified
in every build. A third architecture (aarch64) and the first genuinely
concurrent one.

The board needed nothing installed: it is in this Zephyr tree, the SDK already
had `aarch64-zephyr-elf`, and six samples passed on it with no changes at all.
So the "needs an SMP board" note above was accurate about the requirement and
misleading about the cost.

**This is where `@synchronized` turned out not to work.** The full account is
gap W; the short version is that it locked a fresh spinlock on the *caller's
stack*, so two cores locked two different locks, and it was measurably
indistinguishable from no lock at all.

Two harness findings came with it, both the same shape as `heap_alloc`'s
ordering timeout:

- **`arc_demo`'s expectations encoded single-core scheduling.** Its extra
  thread runs on core 1 *concurrently* with `main` and prints before main's
  second line, so an `ordered: true` list requiring "Demo Extra thread
  started" after "Demo main complete" could never match. The program was
  correct throughout. Its `sample.yaml` now has separate scenarios: the
  single-core boards keep the strict cross-thread ordering, which is
  meaningful there, and SMP gets two scenarios each ordered *within* one
  thread. Deliberately not relaxed to `ordered: false` — that would have
  discarded the release-then-allocate ordering gap Q's single-slab-slot claim
  rests on, which holds on any number of cores.
- **`integration_platforms` cannot live in `common:` once scenarios differ by
  platform.** A platform named there but absent from a scenario's own
  `platform_allow` is a hard twister error, not a filter. The first attempt at
  the above broke `mps2/an385` — and RISC-V filtered correctly, so testing
  only the boards of interest would have shipped it.

What SMP still does not cover: `CONFIG_SPIN_VALIDATE` is off there too, so
Zephyr's own lock-misuse assertions remain unexercised. And QEMU is not
hardware — see the caveat under "what this default rests on".

## The Python backend still passes its own suites

Making oz_static the default changed shared files -- `platform/oz_platform.h`
and its two backends, `oz_sdk/Foundation/OZHeap.h`, the host Zephyr stubs,
and four sample sources. Those are the Python pipeline's inputs too, so its
own suites were re-run rather than assumed unaffected:

| Suite | Result |
| --- | --- |
| `just test-transpiler` (`tools/oz_transpile/tests/`) | 539 passed |
| `just test-behavior` (`tests/behavior/`) | 71 passed |
| `just test-adapted` (`tests/adapted/`) | 40 passed |

All three green, so nothing in the shared surface regressed for that
backend. Those are its *host* suites, though, and they are not the whole
question: a Zephyr build of the samples under
`CONFIG_OBJZ_BACKEND_PYTHON=y` is a separate check that no routine gate
runs, and it is where gap Z's addition to `transpiled_blocks` turned out to
have broken that sample for this backend -- see "Code size against the
Python backend". Green here does not cover it.

## Behavior corpus (71 cases)

`tests/behavior/cases/*/*.m` is the Python pipeline's own behavior suite,
driven through oz_static by `tools/oz_static/tests/corpus_parity.rs`
rather than being re-implemented as separate fixtures.

- **71 of 71 transpile.** Enforced with no allowlist.
- **71 of 71 produce compiling C, as ISO C17 with no constraint
  violation** — `-std=c17 -pedantic-errors` since gap Y, which is what
  makes this line mean more than "the host compiler accepted it".
  **`KNOWN_CC_FAILURES` is empty**, and forced to be: the test asserts a
  listed case *still* fails. It held `foundation/timer_basic` and
  `foundation/timer_zephyr` until #267, both on that issue's
  function-pointer-to-object-pointer conversion; both cases are gone with
  OZTimer (gap AB), so the cast they came from exists nowhere. Its
  previous last entry was `memory/heap_alloc.m`, which failed for two
  reasons, both since fixed: `struct oz_heap_inner` was defined by both
  `OZHeap.h` and `platform/oz_platform.h` — each guarded on
  `OZ_HEAP_INNER_DEFINED`, which neither then defined — and it needed the
  `allocWithHeap:` path, now emitted under `--heap-support`. Note the
  corpus compiles with `-DOZ_PLATFORM_HOST` and *no*
  `-DOZ_HEAP_SUPPORT`, so the guard fix carries it, not the heap
  configuration.

That allowlist asserts each listed case *still* fails, so fixing one
without updating the list also fails the test; it cannot decay into
silently skipped cases. Empty is therefore a stronger statement than a
passing suite with entries.

**The two entries it held are worth remembering, because they were never
a regression.** Every generated program using OZTimer carried the cast,
and it became visible only once the flags asked for ISO C (#266) and once
CI actually ran the suite against gcc (#269). Apple clang does not
diagnose that conversion at all, so a maintainer's machine reported the
corpus clean — which is why `cc_diagnoses_fptr_to_object_pointer()`
probes the compiler rather than trusting its name, and why the "still
fails" half of the allowlist is enforced only where the compiler agrees
there is something to fail on. Two compilers disagreeing about whether
the corpus compiles is worth knowing on its own account. That probe is
now unexercised, the cast being gone; it stays because the asymmetry it
guards against is a property of the compilers, not of that one cast.

Rust test suite: 272 passing, 0 failing, with `RUSTFLAGS=-D warnings`.
Three fewer than before #267: `behavior_foundation_timer.rs` went with
OZTimer.

### Behavioral parity: 71 of 71

Transpiling and compiling say the input was understood and the output is
real C. They say nothing about what the code *does*. `just
test-cross-backend` (`tests/tools/cross_backend.py`) closes that: it runs
each case through **both** backends over the same Unity driver and diffs
the results.

| Outcome | Cases | Meaning |
| --- | --- | --- |
| MATCH | **71** | Identical Unity results — same tests, same outcomes |
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

Since gap Q there is a second way a local is owned, and it is not a property
of its initializer: one ARC manages as a *strong* variable
(`emit::managed_object_locals`) owns whatever it ends up holding, because every
store into it is made owning. `Counter *c;` and `Counter *c = nil;` qualify on
that basis with no owning initializer at all. The asymmetry above still holds —
membership requires that every store be one the emitter can make owning, so an
unrecognised shape leaves the local unmanaged rather than half-managed.

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
found by the ARM build. It emptied `corpus_parity.rs`'s
`KNOWN_CC_FAILURES` — and that list asserts a listed case *still* fails, so
emptying it was forced rather than chosen. The list has two entries again
since #269, both #267's cast; see the corpus section above for why that is
a tightened instrument rather than a regression.

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

### The compile check used to need `-DOZ_HEAP_SUPPORT`

Without it, five otherwise-fine samples hit the same
`redefinition of 'oz_heap_inner'` described above, because
`Foundation.h` pulls in `OZHeap.h`. The generated header contained exactly
one definition — the collision was between SDK header content and the PAL,
not something oz_static emits. Worth knowing before reading a bare
`cc` failure as a codegen bug.

Superseded by the guard fix (defect 1 above): every site now sets
`OZ_HEAP_INNER_DEFINED`, so the first one seen wins and the flag is no
longer needed to avoid the collision.

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

**What this default rests on, stated plainly.**

- The 71-case behavior corpus **matches** under both backends — run, not just
  transpiled, with Unity results diffed (`just test-cross-backend`).
- **All 13 samples it selects build for ARM and run under twister**, each one's console
  output matched against its own `sample.yaml` — an oracle independent of the
  Python backend. That includes the three needing kernel or device-tree
  infrastructure (`arc_demo`, `gpio_demo`, `zbus_objc`), which no host build
  can exercise at all.
- **12 of 13 also run under twister on `qemu_riscv32`** (#230), a second
  architecture and a second toolchain, with the generated C byte-identical to
  the ARM build across all 304 generated files. `gpio_demo` is excluded by the
  board's missing device-tree aliases, not by anything in oz_static.
- **Two cores, on `qemu_cortex_a53/smp`**: 9 of 9 selected configurations pass
  with `CONFIG_SMP=y` and 2 CPUs, including `samples/smp_shared`, where two
  threads contend on one object's `@synchronized` lock and its refcount. This
  is the only coverage that can distinguish a working lock from a no-op one,
  and it is how gap W was found.
- Of the samples a host build can run, all are clean under AddressSanitizer
  and UndefinedBehaviorSanitizer with leak detection on.
- Generated C is `-Wall -Wextra` clean (gap S), so a new warning on target is
  visible rather than lost in noise — and Zephyr compiles with `-Werror`.
- **Validity is measured separately, because it is a different claim** (gap
  Y). The corpus compiles under `-std=c17 -pedantic-errors` — the standard
  Zephyr itself pins — with an empty allowlist, so a constraint violation in
  generated output fails a test. The samples are gated on the same question
  with the real ARM toolchain, at **10 sites** whose causes are all inside
  Zephyr's own macros, each in `KNOWN_PEDANTIC` with its reason. It was 26
  until #267 retired OZTimer, whose one cast accounted for 18 of them, and a
  report until the same change put it in CI. Gap X's history is why this is
  listed apart from the line above rather than folded into it.

What that still does not cover, and why the escape hatch stays:

- **QEMU is not hardware.** Neither board is real silicon, so nothing here
  says anything about real `k_mem_slab` contention, real interrupt-disabled
  critical sections, or timing. No physical board has been used (#231).
- **Code size is now measured** against the Python backend, and it is close:
  **+1.3% flash overall**, smaller on five samples and larger on seven. See
  "Code size against the Python backend" below. It was unmeasured when the
  default was flipped, so this was "unknown" rather than "known-acceptable"
  until now (#231).
- **`CONFIG_SPIN_VALIDATE` has never been on**, on any board, so Zephyr's own
  lock-misuse assertions have never run against generated code. SMP covers the
  populated `struct k_spinlock`; this is the remaining shape of that struct.

`CONFIG_OBJZ_BACKEND_PYTHON=y` remains the way back for any target this
breaks.

## Code size against the Python backend

Both backends, same board (`mps2/an385`), same sources, same optimization
(`-Os`, Zephyr's default for these samples). Flash is `text + data` and RAM is
`data + bss`, read from each `zephyr.elf` with `arm-zephyr-eabi-size`. Twister's
own `rom_size`/`ram_size` fields come back `None` unless size reporting is
enabled explicitly, which is why the ELF is read directly — worth knowing
before trying to reproduce this from a twister report.

One row no longer reproduces at all, and the reason is a regression this
document has to own. `transpiled_blocks` gained a file-scope block and a C
function taking one when gap Z landed -- and **the Python backend has no
lowering for either**, which is the very gap Z fixed in oz_static. So that
sample no longer builds under `CONFIG_OBJZ_BACKEND_PYTHON`:

```
main_ozm.c:74:13: error: expected identifier or '(' before '^' token
   74 | static int (^scale_by_three)(int) = ^(int v) {
```

It joins `zbus_service` as a sample only oz_static can build, taking the
comparison from twelve rows to eleven. Found by running the Python-backend
sweep while verifying #274; it was missed when gap Z landed because the
routine gates (`just test`, `just test-boards`) only exercise the *default*
backend, so a second-backend build failure is invisible to all of them. The
row's figures also predate the sample's growth. Left as the measurement
actually taken rather than adjusted by estimate.

**Leaving it that way is a decision, not an oversight.** The alternatives
were to move those two shapes into a sample already unbuildable under that
backend, or to give them one of their own; both were declined on the
standing rule that the Python pipeline is a reference and not something to
extend or design around. It is the outgoing implementation, oz_static is the
default, and a sample exercising constructs only the default backend lowers
is a fair thing for this repository to contain. The cost is stated plainly
here rather than absorbed: eleven comparable rows instead of twelve, and one
fewer sample proving the Python backend still builds.

Reproduce with `west twister -T samples/ -p mps2/an385` and again with
`-x CONFIG_OBJZ_BACKEND_PYTHON=y`, then size the ELFs.

| Sample | Flash static | Flash python | Δ flash | RAM static | RAM python | Δ RAM |
| --- | --- | --- | --- | --- | --- | --- |
| pool_demo | 13300 | 13804 | -504 (-3.7%) | 6211 | 6571 | -360 |
| transpiled_led | 13776 | 14184 | -408 (-2.9%) | 6359 | 6659 | -300 |
| hello_world | 13936 | 15280 | -1344 (-8.8%) | 6203 | 6527 | -324 |
| mem_demo | 16816 | 15772 | +1044 (+6.6%) | 6807 | 6723 | +84 |
| transpiled_blocks | 17376 | 16280 | +1096 (+6.7%) | 7231 | 7087 | +144 |
| arc_demo | 17552 | 16492 | +1060 (+6.4%) | 8051 | 7959 | +92 |
| heap_alloc | 18864 | 18208 | +656 (+3.6%) | 15171 | 13035 | +2136 |
| transpiled_generics | 19020 | 18168 | +852 (+4.7%) | 7323 | 7143 | +180 |
| transpiled_literals | 20112 | 19236 | +876 (+4.6%) | 7351 | 7139 | +212 |
| gpio_demo | 24984 | 26056 | -1072 (-4.1%) | 8370 | 8950 | -580 |
| hello_category | 26832 | 24700 | +2132 (+8.6%) | 6971 | 6867 | +104 |
| zbus_objc | 47070 | 48238 | -1168 (-2.4%) | 21524 | 21848 | -324 |
| zbus_service | 51767 | — | — | 14841 | — | — |
| **total (12)** | **249638** | **246418** | **+3220 (+1.3%)** | **107572** | **106508** | **+1064** |

**The answer is "close, with no clear winner".** oz_static costs **1.3% more
flash** in total and is smaller on five of the twelve comparable samples,
larger on seven, over a range of -8.8% to +8.6%. Nothing here would have
changed the default decision either way, which is the useful thing to be able
to say — it was previously unknown rather than known-acceptable.

On RAM the total (+1064 B) is misleading and should not be quoted. All of it is
`heap_alloc`, where the oracle is wrong rather than smaller: **exclude that one
sample and oz_static uses 1072 B _less_ RAM across the other eleven**, smaller
on five of them and never more than 580 B apart. See the `heap_alloc` entry
below.

Two entries are worth more than the total:

- **`zbus_service` cannot be compared at all, and the reason is a
  Python-backend defect.** It emits Clang's *diagnostic spelling* of an
  anonymous aggregate straight into a header --
  `enum (unnamed enum at /abs/path/TemperatureService.h:17:2) tag;` -- which is
  not C, so the sample does not build under that backend. oz_static compiles
  the same header. Recorded because it is the one place the two backends differ
  on whether a sample is buildable at all, and it is the outgoing one that
  fails.
- **`heap_alloc`'s +2136 B of RAM is the largest single delta, and it is the
  price of being correct.** Investigated, and it is not a footprint cost at
  all. `samples/heap_alloc/src/App.m` writes, inside `-init`:

  ```objc
  static char appHeapBuffer[2048];
  _heap = [[OZHeap alloc] initWithBuffer:appHeapBuffer size:sizeof(appHeapBuffer)];
  ```

  oz_static keeps the `static`, so the buffer lives in bss --
  `appHeapBuffer.0`, 2048 B, which is 2048 of the 2116-byte bss difference and
  essentially the whole delta. **The Python backend drops the `static`** and
  emits `char appHeapBuffer[2048];`, a plain stack array
  (`oz_generated/App_ozm.c`), whose address it then hands to `OZHeap` and
  stores in the ivar. Once `-init` returns, that heap is backed by a dead
  stack frame, and every later allocation from it writes into whatever now
  occupies that region -- a use-after-return.

  Two things make it worse than a leak. The array's address escapes into the
  ivar, so the compiler cannot elide it: the frame really is 2048 bytes, in a
  function reached from a `z_main_stack` of 1024 bytes (both builds), so it
  also overflows the main stack. And the sample still *passes* its
  `sample.yaml` checks under that backend, which is the whole reason the
  defect is invisible from the harness.

  So the honest reading of this row is inverted: oz_static is not 2136 bytes
  worse, it is 2048 bytes of correctly-owned storage against 2048 bytes the
  oracle does not own. Another entry for the standing rule that the Python
  pipeline is a reference and not an authority.

What this does *not* say: nothing here is a *performance* comparison, and the
figures are one board at one optimization level. #231's other half -- running
on real hardware -- is still open.

One of #231's own predictions is now stale and would mislead a reader of the
issue: it lists "oz_static's `@synchronized` allocates a per-block spinlock on
the stack" as a difference that should show in size. Since gap W that is no
longer how it works -- the lock is a field on the object -- so for `pool_demo`
the comparison is a per-object spinlock plus owner pointer against the oracle's
per-block `OZSpinLock` instance.

This paragraph previously read "every measurement in this document is a host
measurement ... no sample has been built on target through this backend". That
was true when the default was flipped and stopped being true once the
cross-build landed — while contradicting both the "On target" section above it
and the first line of "Not verified" immediately below. Left as a caution: the
claim most likely to go stale is the one about what has *not* been checked yet,
because the work that falsifies it is filed somewhere else.

## Not verified

**The Zephyr cross-build is now run, on three boards** — see "On target"
above. ARM runs 13 of the 14 samples; `qemu_riscv32` runs 12, the one it drops
excluded by that board's missing device-tree aliases rather than by anything in
oz_static (#230); `qemu_cortex_a53/smp` runs 9 of 9 selected with two CPUs.
What is still not covered: **no real board has been used** — all three are
QEMU — and `CONFIG_SPIN_VALIDATE` has never been enabled anywhere. Code size
against the Python backend was listed here too and is measured; see that
section. Validity was listed nowhere at all, and is now checked on both sides
(gap Y).

This entry is the one to distrust on principle. It has now been wrong four
times in the same direction — first claiming no target build existed after the
cross-build landed, then claiming no RISC-V sample had run when eight of them
would have passed the moment anyone pointed twister at the board, then
claiming no code-size comparison existed when this file carries one, and
claiming no SMP board had been used when the board was in-tree and the
toolchain already installed. Each time the document understated what was
already reachable, because the work that falsifies it gets recorded somewhere
else.

Two of the four are worth a second look for the opposite reason, though: SMP
and validity were the ones whose absence was concealing a live defect rather
than merely lagging behind — `@synchronized` excluding nothing between cores
(gap W), and gap X still alive on Zephyr (gap Y). "Not yet checked" and
"checked and fine" are not close together, and this section spent months
implying the second while meaning the first.

Both were also *absent* from this list rather than wrong in it, which is the
harder failure to notice: a claim recorded here can go stale and be corrected,
while a question nobody has asked leaves no trace at all. Validity is the
clearer case — the word **compiles** at the top of this file had a definition
that quietly excluded it.

**Every sample is run under twister** on at least one board, on QEMU. Each is built, executed,
and its console output matched against its own `sample.yaml` — a real oracle,
and one independent of the Python backend. QEMU still says nothing about real
`k_mem_slab` contention, real interrupt-disabled critical sections, or
timing.

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

## Follow-ups

Filed rather than folded in, each with the reason it was kept separate:

| Issue | What |
| --- | --- |
| #226 | Static, no-heap reflection and `@selector`. Needs its own design pass; oz_static rejects them today with a located error. |
| #227 | Host-portable samples. Only three samples genuinely need Zephyr (`K_THREAD_DEFINE`, device tree, zbus) — stubbing `printk` alone moved four others to running on host. |
| #230 | Verify on RISC-V (`qemu_riscv32`). **Done** — 12 of 13 samples build, run under QEMU and pass their own `sample.yaml` checks; generated C byte-identical to ARM across all 304 generated files. `gpio_demo` stays ARM-only, needing device-tree aliases the board lacks. Repeatable as `just test-riscv`. |
| #231 | Compare code size between backends, and run at least one sample on real hardware. **Size half done** — +1.3% flash overall, and on RAM oz_static is 1072 B *smaller* once `heap_alloc` is excluded, where the oracle drops a `static` and backs its heap with a dead stack frame. See "Code size against the Python backend". Hardware is untouched and needs a board. |
| #238 | Objective-C inside a `#define` *body* is emitted verbatim, so the generated C does not compile — the other half of #234, split out because a macro body is one opaque `preproc_arg` token and needs its own approach. Detector prototyped: 0 of 40 real macro bodies flagged. |
| #254 | `emit()` and `emit_split()` duplicated the top-level walk and had disagreed four times (gap R, #246, #250, #251). The mechanism behind three of this file's gaps, rather than a gap of its own. **Done** — one `emit::walk_top_level`, two assemblers over it, so a node kind is handled in exactly one place; `EmitCtx::new` replaces the six hand-spelled constructions. The refactor left generated output byte-identical across 820 corpus and 342 sample files; gap X, found by that comparison and fixed alongside, then removed a stray `;` from 146 of them and added nothing anywhere. The audit that preceded it (`tests/emitter_agreement.rs`) survives with a smaller claim — it guards the two assemblers, not two walks. |
| #266 | Nothing checked generated C for *validity*, only for warnings. **Done** — `corpus_parity.rs` compiles with `-std=c17 -pedantic-errors` (the standard Zephyr pins) and it is a gate, the count there being 0 across the corpus; `just test-pedantic` asks the same of the samples on ARM. It reported 26 sites when it landed and is a **gate at 10** since #267 — that change removed 18 by retiring OZTimer, fixed one by dropping a redundant `;`, and put the sweep in CI, which is what a gate needs to mean anything. The count came first, as the issue asked, and it found gap X's fourth producer — an item-pool `;` that was valid on host and a constraint violation on Zephyr, so no host check could see it. It also found that the obvious way to sweep `-Wpedantic` on target reports zero on output that is not clean: CMSIS disables the flag for the rest of every Cortex-M TU. See gap Y. |
| #269 | CI never ran the Rust suite, and `hw-build-check` could not fail. **Done** — a `rust-tests` job runs all 262 tests plus `RUSTFLAGS=-D warnings`, so `corpus_parity.rs`'s `-pedantic-errors` gate is now enforced on every PR rather than only locally; `continue-on-error: true` is gone from the one job that cross-compiles for a board, verified safe first by reading ten runs' step conclusions. Two things found on the way and fixed in the same change: the AST oracle in CI was clang **18.1** rather than the tested 19, because the SDK was installed without `-l` and `objz_find_clang()` fell through to `PATH` while the job separately installed an unused clang 20 — one shared SDK install with LLVM now feeds cmake, `OZ_CLANG` and a `PATH` symlink alike, and `-DOBJZ_REQUIRE_TESTED_CLANG=ON` makes a repeat fatal; and `west.yml` said `revision: main`, so every CI run built against whatever upstream main was that morning, now pinned to **v4.4.2**. |
| #267 | Generated C converted a block's function pointer to `void *` (`(void*)(expBlock)`, `src/OZTimer.m`), which ISO C forbids in either direction — 18 of the 26 sites `just test-pedantic` reported, and the reason it was a report rather than a gate. **Done**, and not the way it was scoped. The plan was a `__oz_timer_setup` signature decision, two faces under `#ifdef __OBJC__`; that was built and verified and then not used, because `OZM` (#272) makes the helper unnecessary rather than fixable — `OZM(K_TIMER_DEFINE, my_timer, ^(struct k_timer *t) { ... }, NULL)` calls Zephyr's own macro with an inline block. So **OZTimer is retired**: the class, its header, its place in the `Foundation.h` umbrella, and `__oz_timer_setup` in both copies are gone, taking the 18 sites with them and leaving the sweep a gate at 10. The costs are stated in gap AB rather than absorbed: two corpus cases deleted (71, not 73, and cross-backend with them), a ztest and three Rust tests gone, and a callback needing per-instance context now reaching it through `k_timer_user_data_get` where OZTimer wrapped it in an ARC-managed ivar. Also corrected here: the helper never existed "on both PAL backends" as this row and both allowlists said — it was in the Zephyr PAL and in the behaviour tests' Zephyr stand-in, the host PAL having no timer at all. |
| #272 | Blocks were lowered to function pointers everywhere except the top level, where a file-scope block variable, its block-literal initializer and a free function's block parameter each reached the C compiler with the `^` intact — text no GCC target can parse, though each shape is valid Objective-C. **Done** — see gap Z. Filed while scoping #267 and taken first because it produces *invalid* C rather than merely non-conforming C. It also delivered what it was filed for: Zephyr's own `ZBUS_LISTENER_DEFINE`/`K_TIMER_DEFINE` cannot take an inline block, because Objective-C refuses block-to-function-pointer conversion in every position and Clang has to parse the same file for the AST oracle — so the same PR adds **`OZM`**, discarded unparsed by Clang and expanding to the target macro in the generated C, where the literal is already a hoisted function name. One name for every target macro, no per-primitive wrapper. Both halves are pure preprocessor since #267, which deleted the emitter rule OZM shipped with (~90 lines) — the transpiler's contribution is the hoist alone. `samples/zbus_service` writes its zbus listener as an inline block on that basis and passes on ARM. |
| #274 | The Clang AST dumps were silently truncated, with two independent causes: `_objz_build_ast_flags` took include directories from the `zephyr_interface` target only, so no sample's own `include/` reached its dump (five samples, losing their own `@implementation`s); and it named no `--target`, so every dump was parsed as the build machine and Zephyr's arch headers exhausted Clang's 20-error limit (**every RISC-V dump**). **Done** -- 0 truncated of 165 dumps on ARM and 0 of 150 on RISC-V, with generated C byte-identical pre- and post-fix across all 13 samples, which is what makes the impact latent rather than active. A truncated dump is now fatal at build time rather than counted as usable. See gap AA. |
