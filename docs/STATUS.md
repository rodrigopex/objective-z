# oz_static status

What `oz2c` does, what has been measured, and what has not. A status record,
not a claim of readiness.

This replaces `tools/oz_static/PARITY.md`, which was a *parity* document —
written while a second implementation existed (the Python pipeline in
`tools/oz_transpile`, which it called "the oracle") and grown to some 2,900
lines of gap-by-gap history. That backend is retired, so there is nothing left
to be at parity with.

**The old file is still readable, and code comments still cite it.** Roughly
thirty places in the tree refer to "gap R", "gap Y" and so on. Those are the
*reasons* many decisions were taken and are deliberately left in place:

```sh
git show python-backend-final:tools/oz_static/PARITY.md    # gaps A-AH
git log --oneline -- tools/oz_static/PARITY.md             # how each was found
```

## Vocabulary

These words are used precisely and are not interchangeable. The distinctions
are not pedantry — each one was added because a weaker reading had already
hidden a defect.

- **transpiles** — `oz2c` exits 0 and writes output. The input was understood.
- **compiles** — the generated `.c` passes the host compiler as
  `-std=c17 -pedantic-errors`: ISO C17, no constraint violation. The flags are
  part of the definition and were added late. Without them the word meant
  "compiles as GNU C with whatever the compiler defaults to", which is how a
  bare `;` at file scope lived in every generated program while satisfying
  every use of the word.
- **links** — the generated objects link into a binary. Strictly more than
  compiling: a call to a method declared but defined nowhere compiles fine
  against its prototype and fails only at link, so a compile-only sweep once
  reported OK for three samples that could not be built.
- **runs** — the binary was executed, exited 0, and its console output matched
  every line the sample's own `sample.yaml` requires, in order. That file is
  the author's statement of correct behaviour, so it is a real oracle.
- **builds for ARM** — `west build -b mps2/an385` succeeded with the real
  cross-toolchain. Strictly more than compiling on host, and the gap is not
  small: it once found five defects in twenty minutes that a full day of host
  checks had not.
- **matches** — *expired.* It meant "run under both backends, identical
  results", which needed two implementations. Every use of it in history was
  true when taken and cannot be retaken. Nothing replaces it; a second suite
  over one transpiler is not a second opinion.

## Where it stands

| Subject | Status |
| --- | --- |
| Rust suite (`cargo test`) | **362 tests**, `RUSTFLAGS=-D warnings` clean. The primary gate |
| Behaviour corpus | **74/74** transpile, compile and run — gcc/clang × `-O0`/`-O2`, plus ASan, UBSan and LeakSanitizer |
| Corpus ISO C validity | Gate at **0** under `-std=c17 -pedantic-errors` |
| Adapted upstream tests | **40/40** (LLVM, GNUstep, Apple, ObjFW, mulle-objc) |
| Samples on ARM (`mps2/an385`) | **14/14** built and run under twister |
| Samples on RISC-V (`qemu_riscv32`) | **13/13** — `gpio_demo` needs device-tree aliases the board lacks |
| Samples on two cores (`qemu_cortex_a53/smp`) | **10/10**, the only place `@synchronized` faces real contention |
| Samples on real silicon (nRF52833DK) | **13/13** flashed and run; `smp_shared` cannot, needing two cores. `reflection_demo` builds for it and has not been run there — no board was attached when it landed |
| Kernel lock validation | `CONFIG_SPIN_VALIDATE` silent on ARM (14/14) and SMP (10/10) |
| Generated C warnings | `-Wall -Wextra` clean across all samples |
| Pedantic sweep on target | Gate at **10 sites**, every one inside Zephyr's own macros |
| Zephyr integration (ztest) | **18 cases in 5 suites** over committed oz_static output |

## What is not verified

Stated precisely rather than as "everything works":

- **No hardware run happens in CI, and none can** — there is no board on a
  GitHub runner, so `just test-hardware` holds only where the hardware is. The
  one gate in this repo with that property.
- **One board of one SoC family is not "hardware" in general.** Nothing says
  anything about a part with different flash timing, tighter RAM, or an MPU
  configuration these samples do not exercise.
- **`smp_shared` has never run on silicon.** The DK has one core, so every
  claim about `@synchronized` under real contention rests on emulated cores.
- **Nobody has pressed the button.** `gpio_demo`'s GPIO callback *registers*
  on hardware, which is asserted; the block running on a real pin change is
  checked by hand.
- **No independent implementation.** Behaviour is checked against the sources'
  own expectations, not against a second transpiler.
- **Objective-C in a `#define` body** is rejected with a located error, not
  supported (#238).
- **One dynamically-dispatched selector cannot be shared by two classes
  whose return types disagree at all.** `OZ_PROTOCOL_SEND_<sel>` is emitted
  per selector *name* and declares one return type, so any disagreement is
  wrong for at least one implementor -- a located error (#290).
  `instancetype` is exempt, since the shim already collapses those to
  `void *`. *Located* covers both declaration forms: the error points at the
  second `@property` as readily as at the second method, which it did not
  until #297 -- the helper walked for a method node, found none for a
  property, and fell back to `1:1` for precisely the shape #290 was filed
  for.

  Two unrelated classes therefore have to agree on a selector's return type
  merely because they share its name: a `-count` meaning "tally" must match
  a `-count` meaning "container size". That is what dispatch keyed on the
  name alone costs, and it is why the SDK's own size APIs are uniformly
  `size_t`.

  Having the shim declare the type C's usual arithmetic conversions would
  give, rather than refusing, is **not implementable from the spelling**:
  whether `size_t` is wider than `unsigned int` is target-dependent, so
  ranking them textually would be a guess.
- **`-cDescription:maxLength:` is `int (char *, size_t)`** -- `snprintf`'s
  own shape, which is the function it imitates. The capacity is a `size_t`
  because it cannot be negative; the result stays `int` because the
  alternatives were worse. `ssize_t` is unusable: the Clang AST dump that
  decides ivar ownership runs `--target=x86_64-unknown-linux-gnu` with no
  Linux sysroot, so only *compiler-provided* headers resolve there --
  `<stddef.h>` is Clang's own, `<sys/types.h>` is libc's, and including it
  broke all 74 behaviour cases at the dump step. `ptrdiff_t` compiles but
  means "difference between two pointers", which is not what the method
  returns; an Objective-C interface saying so reads wrong. Anything these
  headers include has to survive the dump path, which is the constraint
  worth remembering.

  The recursive implementations now guard the accumulation --
  `if (written > 0) { pos += written; }`. A negative result was previously
  added straight into a signed `pos` and then used to index `buf`.
- **An array of objects is owned one dimension deep, and only in the three
  store shapes ARC can balance.** A store must be a `+1` value, a plain
  variable, or nil, and its index a literal or a plain variable -- the store
  names its target twice, so the index is evaluated twice. Anything else is
  a located error rather than a plain-C store that silently drops the
  element it overwrote. An owned object array with **two or more dimensions
  is rejected**: the release walks elements, and `a[i]` is then a sub-array,
  so releasing it would cast array storage to an object pointer. Flattening
  with a cast to `Element **` works on every real target and is still
  refused, because reaching across a multi-dimensional array through a
  pointer to its first element is not defined by ISO C (#287). Scalar arrays
  are unaffected at any dimensionality.
- **A top-level macro invocation with no trailing `;` is repaired, not
  parsed.** `ZBUS_OBS_DECLARE(x)` terminates its own expansion, so it is
  written without a semicolon, and tree-sitter then reads it as a *type* and
  absorbs the next construct into one node. `parse::repair_bare_macro_statements`
  writes a `;` over one whitespace byte before any pass reads the text, and
  `walk_top_level` writes the space back on the way out (#288, #289). What is
  *not* verified is the case with no whitespace after the `)` to overwrite:
  the repair is length-preserving because every offset in the file is a span
  into that text, so there is nowhere to put the semicolon, and the
  absorption stands. No sample or test writes that shape.

## Introspection and reflection (#226)

Supported, each half behind its own Kconfig option, both defaulting to `y`.
Nothing here needs a heap or a runtime registry: every answer is read from a
`const` table the transpiler generated, so it lives in flash and costs no RAM.

| Construct | Needs | Cost |
|---|---|---|
| `[Foo class]`, `[obj class]`, `-isMemberOfClass:` | nothing — always available | none; `Class` is the `class_id` every object already carries, so these are a constant or a bitfield read |
| `-isKindOfClass:` | `CONFIG_OBJZ_INTROSPECTION` | `oz_superclass_of[]`, 2 bytes per class, plus a 32-byte walker |
| `-conformsToProtocol:` | `CONFIG_OBJZ_INTROSPECTION` | one 4-byte bitmap per protocol named, plus a 36-byte reader |
| `@selector`, `SEL`, `-respondsToSelector:`, `-performSelector:` | `CONFIG_OBJZ_REFLECTION` | 12-byte record + 4-byte bitmap + 4–10-byte wrapper per selector named, plus 42–62 bytes of helpers |

Measured on the linked `samples/reflection_demo` image for
`nrf52833dk/nrf52833` (Cortex-M4, `-Os`): **318 bytes of flash and 0 of RAM**
for all of it, plus 42 bytes for the two `OZ_PROTOCOL_SEND_*` functions the
reflected selectors forced into existence. 1.4% of that sample's flash.

**A protocol-typed receiver needs `<ObjectProtocol>` (#307).** None of the
above can be sent to an `id<P>` unless `P` adopts oz_sdk's base protocol:

```objc
@protocol PXToggleable <ObjectProtocol>
- (void)toggle;
@end
```

Clang resolves a message to `id<P>` against `P` and its super-protocols and
nowhere else — the root class is unreachable from such a type — so without the
adoption `[indicator conformsToProtocol:...]` is
`error: no known instance method for selector 'conformsToProtocol:'` however
plainly `OZObject` declares it. Real Objective-C has the same rule and
`<NSObject>` is the same answer. `ObjectProtocol` lives in
`include/oz_sdk/Foundation/Object+Protocol.h`, is reached through `OZObject.h`
(which defines the `BOOL` its methods return), and is adopted by `OZObject`,
`IteratorProtocol` and `SingletonProtocol`. Adopting it demands nothing of a
conforming class: every method it declares is defined once, on the root class,
and an inherited implementation satisfies a protocol requirement.

It costs nothing. Its ten selectors become protocol-declared, so
`is_dynamically_dispatched` generates a dispatch function for each — and
`hello_world`'s image is byte-identical across the change (13740 text, 228
data, 5975 bss), because `-ffunction-sections` plus `--gc-sections` collects
every one a program does not call.

Three things about the design are worth knowing before changing it:

- **Tables are gated on use, not on the option.** A program that enables both
  and introspects nothing emits nothing — no table, no helper. Gating on the
  option instead would have added 94 bytes to every build that merely left the
  defaults alone.
- **A `SEL` is a pointer to a `const` record, not to a function.** A selector
  has one implementation per class, so it cannot be a method pointer; and while
  its `OZ_PROTOCOL_SEND_*` dispatcher is a single function, that leaves
  `-respondsToSelector:` — a predicate, not a call — nothing to read, and
  dispatchers have per-selector signatures, so calling one through a
  differently-typed pointer is undefined behaviour the pedantic gate exists to
  prevent. The record holds a responds bitmap and a wrapper of one uniform
  shape, so an indirect call needs no cast, no shape tag and no variadics.
- **Performability is checked at the `@selector(...)`, not at the perform.** A
  selector reachable by a `-performSelector:` must fit that wrapper: at most two
  object-typed arguments, returning void or an object. Which selectors those are
  depends on the program — the literals named at perform sites, or, if any site
  takes its `SEL` from a value, every reflectively-named selector, since nothing
  can then prove which one arrives. `samples/reflection_demo` is the second
  case, and its `-toggle` had to return void because of it.

With either option off, its constructs are hard located errors naming the
option — never silently unavailable, and never degraded to something weaker.

`Nil` has no Objective-C spelling on purpose. `Class` is a pointer to Clang,
which rejects the integer cast under ARC, and defining it as `((Class)0)` for
the AST dump's benefit would make the same comparison mean two different things
there and in the emitted C. The contract is observable without it: a nil
receiver's class matches nothing at all, not even the root class.

## Two escapes for a block where a function pointer is wanted

Objective-C refuses the conversion in every position, so neither is optional:

| | Hides | Use when |
| --- | --- | --- |
| `OZM(MACRO, args...)` | the whole macro invocation | the target macro token-pastes its callback into a symbol name -- `INPUT_CALLBACK_DEFINE` does |
| `OZFN(^{ ... })` | one expression | everywhere else, and it is the better default |

`OZFN` is preferred because `OZM` costs the symbol: with the invocation
discarded, Clang never sees what the macro declares, so a reference to it
needs a hand-written `#ifdef __OBJC__` twin. `OZFN` leaves the real macro to
expand on both sides. It also reaches callbacks `OZM` cannot -- a designated
initializer is not a macro argument, which is the shape Zephyr's
`BT_CONN_CB_DEFINE(name) = { .connected = ..., }` uses (#300).

Where `OZFN` is *wrong*: a macro pasting its callback into a name. The
argument expands to `0` before the paste, so two callbacks in one file both
become `_input_callback__0` and Clang reports `redefinition` -- on the
AST-dump path, where a truncated dump costs ivar ownership facts. No longer
*silently*: since #307 any error in a dump fails the build, not only a
`fatal error`, so this now surfaces as the redefinition it is instead of as a
leak found later. Zephyr's `INPUT_CALLBACK_DEFINE_NAMED` is the way out for a
caller who wants `OZFN` there anyway.

Neither lets Clang check the block. `OZFN` expands to `0` because a static
initializer needs a null pointer constant, and `((blk), 0)` or
`((void)sizeof(blk), 0)` are not ones -- they have the value zero and a
pointer initializer rejects them. So the block goes unparsed either way, and
a signature mismatch surfaces from GCC on generated code rather than from
oz_static on the source. That is a real gap, not a detail.

One shape of it has since moved to the right side of that line, and only
because the name was reserved rather than the signature checked -- see
`id` is a reserved word below.

### What the author can still pin down

The return type, by writing it on the literal -- `^uint32_t(int seed) { ... }`
is carried into the hoisted function (#303). Worth knowing because it is the
only way to type a callback that does not return `int`: with no return type
written, oz_static takes the enclosing block-pointer declaration's if there
is one, and otherwise *guesses* from the body, where any return-with-value
means `int`.

A designated initializer has no such declaration, so `.fn = OZFN(^(int seed)
{ ... })` gets the guess -- and Zephyr's `bt_conn_auth_cb.app_passkey`
returns `uint32_t`, which is how #303 was found. Inferring it from the field
instead is not a gap waiting to be filled; it is closed twice over. The
field's type lives in a `#include`d pure-C header, which `imports`
deliberately leaves verbatim rather than splicing, so the struct never
enters the CST at all. And Clang cannot supply it either, for the reason
just above: `OZFN` expands to `0`, so no block exists at that position to
type.

So: write the return type on any callback block that does not return `int`.
`px-keyboard` needed a named C function for `.app_passkey` before this and
does not now.

### `id` is a reserved word (#317)

Nothing may be *declared* with that name -- not a block or function or method
parameter, not an ivar, not a plain C struct field, not a local. `id` is
Objective-C's untyped object pointer, and oz_static rewrites it as one
wherever a declaration uses it.

Clang accepts the name, because shadowing a typedef with a declarator is
legal C, so transcribing a C callback signature verbatim is how an author
meets this: Zephyr's `bt_conn_auth_info_cb.bond_deleted` is
`void (*)(uint8_t id, const bt_addr_le_t *peer)`, and copying that into an
`OZFN` block emitted `uint8_t struct OZObject *` -- two type specifiers, no
parameter name, and a body still referring to one. `px-keyboard` spells that
parameter `identity`.

Reserving the name was the fix rather than lowering it correctly, because
lowering it correctly is only possible where the CST is still in hand. Once a
parameter list is flat text, a parameter *typed* `id` and one *named* `id`
are the same three characters. Both emit paths key on the grammar now and
would emit correct C, but the rule is what turns the whole class of it into a
located error on the author's own line -- the one thing the gap above says a
block signature otherwise does not get.

Member access is untouched: `sAdvParam.id` reads a field of a struct from a
plain `#include`, which `imports` leaves verbatim, so no foreign declaration
is visible to the check. Zephyr is full of `.id` fields.

## What one cause can look like

Worth keeping because it cost two wrong diagnoses before the right one.
#288, #289 and OZ-004 (#37) were filed as three bugs and were one: a
semicolon-less top-level macro invocation absorbing whatever followed it.
The symptom is decided entirely by what the victim was, and by which emit
arm it needed:

| Victim | Arm it needed | Symptom |
| --- | --- | --- |
| `@implementation` | `class_implementation` | Objective-C copied through verbatim, "stray '@' in program" |
| a second `OZM(...)` | passthrough's block hoist | the block literal survives at its call site, `^` reaches GCC |
| `static Foo *p;` | passthrough's `class_tag_edits` | the class name is not tagged, so `Foo *` is not a C type |
| a plain C function | `function_definition` | **nothing** — that arm renders a body correctly anyway |

That last row is why `samples/zbus_service` built and ran for as long as it
did with its own `main()` absorbed: the victim happened to be the one kind
of node whose proper arm and whose absorbing arm do the same thing.

Two lessons, both paid for:

- **Diagnose on the import-resolved tree, not the file.** `#import` splices
  every header and sibling implementation inline, so grouping and offsets
  differ from the `.m`. A raw-file dump showed a clean `class_implementation`
  and sent the investigation after a protocol-qualified `id` ivar, which
  turned out to be irrelevant. `--dump-cst` prints the resolved tree for
  exactly this reason.
- **A regression test for this class of bug is easy to write vacuously.**
  The absorption needs the *next* line to be call-shaped with three or more
  arguments ending in a number -- `ZBUS_CHAN_ADD_OBS(chan, obs, prio);`.
  With two arguments, or with no second line, tree-sitter recovers on its
  own and the test passes with the fix removed. Both weaker shapes were
  written first and did exactly that.

## Where the same fix twice was the tell

#287 was filed as "an array ivar loses its dimension", and it was that --
but only on one of the two paths an ivar can take. An ivar declared in the
`@interface` was always correct, because `emit::lower_ivar_decl` copies that
declaration through verbatim; only the path that *rebuilds* the field from
`own_ivars` had nowhere to put the extent. That asymmetry is why the bug
looked arbitrary from the outside, and it is the same shape as gap C's
seventh cause and #246: two walks over the same thing, one of them complete.

Fixing it uncovered a second defect that had been sitting behind it. An
owned array of objects was released with

    oz_static_release((struct OZObject *)self->_leaves);

-- the array cast to an object pointer, so the refcount is read out of the
first element's pointer value. That is corruption rather than a leak, and
nothing failed to compile. It was reachable only because the extent was
missing: with no extent there was no way to know an array was an array, so
the ivar looked like a single object everywhere.

Worth keeping for the general point: a fix that supplies missing information
can expose every decision that was made without it. The release path, the
subscript lowering and the store path were all wrong in the same direction,
and all three only became *visible* once the extent existed.

## What the Clang AST oracle costs (#299)

Measured on px-keyboard (8 app sources plus the 10 SDK `src/*.m`), because the
numbers are not the ones anyone guesses and they decide where optimisation is
worth spending.

The oracle is enormously out of proportion to what it answers. Clang serialises
the *entire header closure* to JSON, so one `#include <zephyr/kernel.h>` in a
485-line file produces **117 MB**, and the 18 dumps together are **742 MB** —
against ~40 KB of Objective-C. All 10 SDK dumps together are 6.9 MB, 0.8% of
it; every byte of the problem is the app sources.

What it buys is narrow and load-bearing: **4 of 46 generated files change** when
`--ast` is withheld. It supplies ivar ownership where tree-sitter cannot resolve
the type as an object — `id<PXToggleable> _indicator`, `id _obj` — and without
it those classes lose their synthesized `_oz_release_ivars` and their assignment
retain/release, which leaks. Its live surface is two call sites,
`model.rs`'s `is_owned_object_ivar` and `has_method_body`.

Where it went, and where it is now:

| | before | after |
|---|---|---|
| `oz2c`, 18 dumps | 11.05 s | **0.83 s** |
| of which AST ingest | 95% | — |
| peak resident | 1.30 GB | **317 MB** |
| configure-time transpile | 11.05 s + 2.9 s of dumps | **0.11 s**, no dumps |
| AST written per configure | 742 MB | **none** |
| the ninja transpile edge | 14.9 s, 85% of the build | **2.4 s, 11%** |

Three things did that, in order of effect: optimising the *dependencies* in
`profile.dev` (the hot path was `serde_json`, and oz2c had never been built with
any optimisation at all); dropping the configure-time transpile, which existed
only to discover a file list that no ARC fact affects (`--manifest-only`); and
deserializing into a narrow borrowed struct instead of a `serde_json::Value`
tree, reading one dump at a time.

**`-Xclang -ast-dump-filter` is the remaining lever and is deliberately not
taken.** Filtering each dump to the class its file implements gives 742 MB →
**29 MB** with a *byte-identical* fact set, verified with `--dump-ast-facts`
rather than inferred. But the flag takes one name and cannot be repeated
(verified: the last wins), and **38 live `.m` files implement more than one
class** — 3 samples, 16 behaviour cases, 19 adapted, though none in the SDK or
px-keyboard. Filtering by a single name would silently drop the 2nd..Nth
class's ownership facts, which is the exact silent degradation this project
forbids. It needs a coverage assertion first: require `knows_class` for every
class parsed with ivars, and hard-error naming the class and the `.m` to add.

## How measurements mislead

The most reusable thing the old document held. Every entry below is something
that reported success while the thing it named was broken.

- **A substring is not a definition.** A test asserting the generated C
  "contains `OZ_PROTOCOL_SEND_tick`" passed with the dispatch-generation logic
  removed, because the wrapper that *calls* that function is in the same file.
  Only the companion *header* carries the prototype, and only when the function
  really exists. Assert on the declaration, or on behaviour.
- **A green check whose subject is not what the reader thinks.**
  `tests/zephyr/` globs pre-generated C rather than transpiling, so for years
  its cases said "this committed C runs on Zephyr" and nothing about the
  transpiler — and the C was the *other* backend's output. Ask what a passing
  test actually exercises.
- **A dependency edge that does not exist reports "no work to do".**
  Editing `include/oz_sdk/Foundation/OZSpinLock.h` — the very `id _obj`
  declaration the ownership oracle reads — regenerated nothing, because
  `DEPENDS` listed the caller's `.m` files and not the headers spliced into the
  translation unit. The build said `ninja: no work to do` while the compiled C
  went on releasing an ivar the source now said was `__unsafe_unretained`. A
  header closure is only known after preprocessing, so it has to come from a
  depfile; `DEPENDS` cannot express it (#299).
- **An instrument that cannot see the defect reports zero.** An ARM
  `-Wpedantic` sweep written the obvious way reports a clean result on output
  that is not clean: CMSIS does `#pragma GCC diagnostic ignored "-Wpedantic"`
  with no `pop`, so once anything reaches `zephyr/kernel.h` the diagnostics
  stop. Injecting a bare `;` *and* an empty struct produced zero warnings.
  Prove the instrument can fail before trusting it to pass.
- **A detector that answers from memory.** `nrfjprog --ids` reports probe ids
  it *remembers*, so it named a board that was not plugged in. `nrfutil
  device list` plus the VCOM appearing is the honest test.
- **Host green is not enough.** A day of clean host checks once hid a
  segfault, an MPU fault, a pool leak, a doubly-defined struct, a shadowed
  header and a `-Werror` failure — all six surfaced within twenty minutes of
  the first real cross-build. Compiling proves the input was understood; only
  running proves the output behaves.
- **Agreement is not equivalence.** Cross-backend comparison reported 71/71
  identical results while two real ARC leaks were live, because it compared
  Unity results and not allocation balance.
- **A check CI does not run holds nowhere.** A pedantic gate was a gate in
  substance for weeks while three new violations reached `main`, because it
  ran only on a maintainer's machine. The mirror image is just as real: a
  check that runs *only* locally can silently depend on local state — the
  corpus jobs failed on their first CI run because every machine they were
  developed on already had `oz2c` built.
- **A regression test must fail without the fix.** One written for the
  loop-escape rule was placed in `main()`, which the static bar never scans,
  and passed vacuously. Disable the fix, watch the test fail, restore it.
- **A known-failures list must assert the listed case still fails.**
  Otherwise it decays into silently skipped cases. `KNOWN_PEDANTIC` and
  `KNOWN_CC_FAILURES` both work that way, so fixing an entry forces an update.
- **`git diff` is blind to untracked files.** The freshness check guarding
  generated sources would have passed a change that emitted an *extra* file.
- **The claim most likely to be stale is the one about what has not been
  checked yet**, because the work that falsifies it gets recorded somewhere
  else. The old document's "Not verified" section was wrong four times in the
  same direction, each time understating what was already reachable.

## Standing design rules

- **Never silently degrade.** Anything outside the supported subset is a hard,
  *located* error. This is deliberate, not a gap someone forgot to fill.
- **A leak is a bug; a double free is memory corruption.** ARC therefore fails
  toward leaking: an unrecognised shape is treated as borrowed. Widening what
  counts as owning is the dangerous direction and must be exact rather than
  heuristic. Note what that rule does *not* say: recognising a shape as +1 is
  only half the job, and a *recognised* one still leaked for as long as nothing
  bound it (#322). It also does not say that one reading of ownership serves
  every question. `is_owning_expr` answers "is this +1 by shape"; a result
  bound to nothing needs the narrower `discarded_owning_value`, because
  `-retain` and `-init...` hand back a reference something else already
  accounts for, and releasing those is the corruption direction. A cast runs
  the other way: `is_owning_expr` reads one as borrowed, and both of the
  other two questions look through it. Discarding does, so `(void)[t copy];`
  cannot leak where `[t copy];` does not (#327); *binding* does too, so
  `Thing *t = (Thing *)[Thing alloc];` cannot leak where
  `Thing *t = [Thing alloc];` does not (#332). Both peel through
  `arc::value_behind_casts` and then ask `created_by`, and that second step
  is the whole safety argument -- widening `is_owning_expr` instead skips it
  and was measured to be the corruption direction: `Thing *t = (Thing *)[u
  init];` then releases `u` twice, which ASan reports as a
  heap-use-after-free. So the right combination at a binding site is
  `binds_ownership` -- `is_owning_expr`, plus a non-bridging cast over a
  reference `created_by` calls new -- and not a wider `is_owning_expr`,
  whose answer also decides which methods are owning factories and so what
  every caller of one must release. A *bridging* cast is looked through by
  none of the three.
- **The version is `tools/oz_static/Cargo.toml`**, bumped in the same commit
  as the change it describes. The repo-level `VERSION` file is retired.
