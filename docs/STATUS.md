# oz2c status

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
| Zephyr integration (ztest) | **18 cases in 5 suites** over committed oz2c output |

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
- **`-getDescription:maxLength:` is `int (char *, size_t)`** -- `snprintf`'s
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

**A protocol-typed receiver needs `<OZObjectProtocol>` (#307).** None of the
above can be sent to an `id<P>` unless `P` adopts oz_sdk's base protocol:

```objc
@protocol PXToggleable <OZObjectProtocol>
- (void)toggle;
@end
```

Clang resolves a message to `id<P>` against `P` and its super-protocols and
nowhere else — the root class is unreachable from such a type — so without the
adoption `[indicator conformsToProtocol:...]` is
`error: no known instance method for selector 'conformsToProtocol:'` however
plainly `OZObject` declares it. Real Objective-C has the same rule and
`<NSObject>` is the same answer. `OZObjectProtocol` lives in
`include/oz_sdk/Foundation/OZObjectProtocol.h`, is reached through `OZObject.h`
(which defines the `BOOL` its methods return), and is adopted by `OZObject`,
`OZIteratorProtocol` and `OZSingletonProtocol`. Adopting it demands nothing of a
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
oz2c on the source. That is a real gap, not a detail.

One shape of it has since moved to the right side of that line, and only
because the name was reserved rather than the signature checked -- see
`id` is a reserved word below.

### What the author can still pin down

The return type, by writing it on the literal -- `^uint32_t(int seed) { ... }`
is carried into the hoisted function (#303). Worth knowing because it is the
only way to type a callback that does not return `int`: with no return type
written, oz2c takes the enclosing block-pointer declaration's if there
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
Objective-C's untyped object pointer, and oz2c rewrites it as one
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

    oz_release((struct OZObject *)self->_leaves);

-- the array cast to an object pointer, so the refcount is read out of the
first element's pointer value. That is corruption rather than a leak, and
nothing failed to compile. It was reachable only because the extent was
missing: with no extent there was no way to know an array was an array, so
the ivar looked like a single object everywhere.

Worth keeping for the general point: a fix that supplies missing information
can expose every decision that was made without it. The release path, the
subscript lowering and the store path were all wrong in the same direction,
and all three only became *visible* once the extent existed.

### The third spelling, and then the fourth (#405, #423)

The sharpest instance so far, because the *test suite* was the thing
asserting the defect.

#405 made a strong ivar store release its previous value before evaluating
the new one where the new one cannot read it, and narrowed
`staticbar::LoopEscape::OverlappingStore` to match — routing the
`identifier` and `field_expression` spellings of a store's destination
through one predicate, `overlapping_unless_released_first`. There is a
third spelling. `subscript_expression` kept answering `OverlappingStore`
unconditionally, so

```objc
/* oz-pool: Foo=1 */
for (i = 0; i < 4; i++) {
        _arr[0] = [Foo make];
}
```

was refused as needing "two slab slots rather than one" while
`emit::render_strong_array_element_assign` had, since the same #405, been
emitting `(release(self->_arr[0]), self->_arr[0] = <new>)` for it — one
slot, and measured at 4/4 objects on a pool of one (#423).

Two things are worth keeping from it.

The first is that `tools/oz2c/tests/loop_allocation_bounds.rs`
**asserted the over-rejection as correct behaviour**, in a case named
`a_constant_index_overlaps_rather_than_accumulates` whose message explained
why a constant index "overlaps at two rather than accumulating". It was
written when that was true of every subscript and merged alongside the
change that stopped it being true of one of them. So the suite was not
merely failing to catch the defect, it was pinning it in place: fixing
#423 required *inverting* a green assertion, which is a different and
slower thing to notice than a missing test. The case is still there, with
the same program, now running four iterations on one slot instead of
asserting the refusal.

The second is what looking for a *fourth* site found, which was wrong in
the opposite direction. The `field_expression` arm extracted its
destination with `find_last_identifier`, and in `self->_ivar` the field is
a `field_identifier`, not an `identifier` — so it answered `self`. The bar
then asked `emit::classify_store` whether the right-hand side mentions
`self` while the emitter keyed the real store on `_ivar`, and the two
agree on every spelling but one:

```objc
for (i = 0; i < 4; i++) {
        self->_ivar = [_ivar dup];      /* accepted */
}
```

`[_ivar dup]` does not mention `self`, so the bar called it release-first
and accepted it; the emitter saw a right-hand side that reads `_ivar`, kept
the hoisted temporary, and `ctx.pre_stmts` put that temporary **above** the
loop:

```c
struct OZObject *_oz_prev_L381_C3_1 = (struct OZObject *)(self->_ivar);
for (i = 0; i < 4; i++) {
        (self->_ivar = Foo_dup(...), oz_release(_oz_prev_L381_C3_1));
}
```

— captured once while the ivar was still nil, and that same stale pointer
released on every iteration. So the rejection this issue was filed to
*relax* was, at one spelling, not firing when it had to. An over-rejection
and a miscompile from one cause, which is the tell itself: the cause is not
"the subscript arm was forgotten", it is that four spellings were each
answering the question themselves. All four now go through
`staticbar::assigned_slot_name`, which mirrors `emit::assigned_ivar_name`
(it cannot call it — that needs an `EmitCtx` the bar does not have).

**The emitted C above is history, and #424 is why.** That lowering pushed
the temporary's initialiser through `ctx.pre_stmts`; the shared
`render_overlapping_strong_store` now pushes a bare declaration and assigns
inside the comma expression, so a loop lifts something that evaluates
nothing. Landing after this issue, it means the gap this section describes
now shows up as a nil from the second iteration rather than as a stale
release. The gap and the fix are unchanged — a bar that calls a store
release-first while the emitter lowers it through a temporary disagrees
with the emitter about how many slots the shape needs — but anyone reading
the block above should not go looking for it in the current output. See "A
temporary an expression needs is declared through `ctx.pre_stmts` and
assigned inside the expression" under Standing design rules.

A third defect fell out of giving the subscript arm the same gate as the
others: the bar asked `scope.class_ivars`, which is *every* ivar, where
both emitters gate on `Program::owned_object_ivar_names`, which is fewer.
An `__unsafe_unretained` ivar is not a strong slot — releasing a borrow is
the double free the qualifier exists to prevent, so the store lowers to a
plain C one that releases nothing — and a store into one was being accepted
with `OverlappingStore`'s reasoning, "raise the pool and both live copies
fit". There is no second live copy, because there is no release, and no
pool size bounds that loop. It is `Accumulates` now, for both the scalar
and the array spelling.

### The advice that could not be taken (#425)

Filed separately and fixed on the back of the above, because the answer
changes once #423 has narrowed the arm — which is the point worth keeping.

`OverlappingStore`'s message ended: "Raise this class's pool (a
`/* oz-pool: <Class>=2 */` directive, or `--pool-sizes`) or bind it to a
local declared before the loop". Only the second half works. `staticbar`
has no pool awareness at all — it never reads `PoolSizes` — so it cannot
know the directive was added, and the issue's own example is refused
identically at `Foo=1`, `Foo=2` and `Foo=8`. Measured, and now asserted:
`raising_the_pool_does_not_lift_an_overlapping_store` compares the three
diagnostics and requires them to be the same string.

The issue's *preferred* answer was to make the rejection pool-aware and
accept an `OverlappingStore` whose class has two or more slots, on the
grounds that #405 had noted this becomes sound once the hoisted temporary
is gone for the shapes that do not need one. Whether that answer is right
changed twice in the space of two issues, and the sequence is the
interesting part.

#423 narrowed the arm, so the shapes that release first are accepted at one
slot and never reach it. What is left refused is the complement —
`LocalStore::Unsupported`, the store that reads its own destination, which
the emitter lowers through a temporary. At that point pool-awareness was
**unsound**: `ctx.pre_stmts` hoisted the temporary's initialiser out of the
loop, so it read the destination once while still nil and released the same
stale pointer every iteration. Accepting it on a pool of two would not have
fitted two live objects; it would have miscompiled.

Then #424 landed, for its own reasons, and removed that hazard generally —
the shared lowering declares the temporary through `pre_stmts` and assigns
it inside the comma expression, so a loop lifts something that evaluates
nothing. **Pool-awareness is therefore sound now**, and measured: with the
predicate relaxed in a throwaway build, `_thing = [_thing dup]` over four
iterations runs correctly on a pool of two — five allocations, five frees,
no nil — and is short of slots on one. Two objects are briefly live, one
slab slot cannot hold both, and a second slot is exactly what fixes that.

So the diagnostic fix stands but its justification does not. The remedy is
withheld because the check **cannot act on it**, not because the remedy
would be wrong: `staticbar` never reads `PoolSizes`. The message says that
and no more; it used to say the shape "would be wrong at any pool size",
which was true when written and is not true now. Making the arm pool-aware
is a behavioural change and wants its own issue — it is the one place in
this area where a bigger pool is the genuine answer.

**That issue was #433, and it landed, so the paragraph above is now history
too.** The check reads `PoolSizes`, an `OverlappingStore` whose class has
two or more slots is accepted, and the remedy is offered again — by the arm
that resolved the class and read its size, and by no other. See "The day the
check could read the size (#433)". What this section got right was not its
conclusion but its *condition*: it said which fact would have to change for
the advice to be correct again, and that is the sentence that let the
reversal be recognised as one rather than argued about.

So the general lesson is not "the message was wrong". It is that a remedy
offered in a diagnostic is a claim about the checker's own behaviour, and
this one had never been true. It was also *pinned* by a green assertion —
`diags.contains("oz-pool")`, with the comment "and how to fix it, since
the shape is bounded -- just not at one" — which is the same failure mode
as #423's inverted case, in the same file, found in the same pass.

`Accumulates` carried the same false remedy for a different reason: "size
the pool for the loop's own bound" names a number this pass does not know
and that no directive could express for a dynamic trip count. Both now
carry only what works, and say outright that the pool will not help.
`Returned` never had the advice.

Pool advice elsewhere is untouched and remains correct: `pools.rs` and
`companion.rs` diagnose real slab sizing and exhaustion, where raising the
pool *is* the fix. The rule is narrower than "don't mention the pool" — it
is that the check giving the advice has to be the check that can act on it.

### A test helper fixed twice, in two of its three copies (#418)

Three test files carry a private `function_body(source_c, name)` that pulls
one generated C function out of the output by name. Two of them --
`explicit_ivar_store.rs` and `ownership_matrix.rs` -- carry a correction and
a comment explaining it: `find` reaches the *prototype* in the companion
interface block first, so the span returned begins at a declaration and runs
to whatever `\n}` comes next, which is text from a different function
entirely. `ownership_matrix.rs` records that this once made a correctly
balanced function look like it leaked. `return_alias_escape.rs` was never
updated.

It went unnoticed for as long as the span it wrongly returned happened not
to contain the strings its assertions look for. #418 added
`int oz_retain_count(id obj);` to the spliced `OZObject.h`, and
`a_returned_ivar_is_not_retained` -- which asserts the *absence* of
`oz_retain` in `Holder_held` -- went red on a substring of a
declaration in a header it was never meant to read. The test had been
passing by luck, and a name change is what collected the debt.

The general point, and it is the one this section is about: **when a helper
carries a correction, the correction is a fact about the helper, not about
the file it happens to live in.** Two copies knowing something the third
does not is the tell. And an assertion of the form "this string does not
appear in this span" is only as good as the span -- a negative assertion
over a span that is too wide cannot fail for the right reason and cannot be
trusted when it fails for the wrong one.

### A block literal borrows its enclosing body's context (#339, #342)

The other shape of the same tell, and the more instructive one, because
here the *cause* was shared rather than the symptom. A `block_literal` is
rendered on the enclosing body's `EmitCtx` rather than owning a fresh one
-- block bodies deliberately share the enclosing flat name scope. So
everything that context holds *pending* on behalf of the enclosing body is
visible to the block's own statements, and each such thing needs a
boundary the enclosing method never needed:

- the **return type** a `return` types its temporary from. Left as the
  enclosing body's, a block returning a `Widget *` inside an `-(int)`
  method declared its temporary `int` (#339).
- the **ARC scopes** a `return` unwinds. Walked to the bottom, the block's
  `return` released the enclosing method's locals from inside the hoisted
  function -- `error: 'outerKeep' undeclared`, no valid C at all (#342).
- the **`@synchronized` unlocks** a `return` replays, which had the same
  defect for the same reason and was found by looking for it once the
  second case named the cause (#342).

Two of the three were filed as separate bugs three days apart, and the
third was never filed. Worth stating as the rule the next one falls under:
when a construct is rendered on someone else's context, every field of
that context is a boundary question, and finding one wrong is reason to
enumerate the rest rather than to fix the one. `ArcScope::is_block_body`
draws the boundary where there is scope structure to mark;
`ctx.sync_cleanups` is a flat list of text with none, so it is saved and
cleared instead. Both restore on the way out, since the enclosing body
still owes what it owed.

### Every position that carries a type through (#326, #336, #367, #531, #537)

The third family, and the one that has now produced seven bugs with a
single sentence behind all of them: **a method's signature and a free
function's, a block literal's signature and its body's scope, a C struct's
field list, an ivar list and a `for`-in header are separate walks over the
same question**, and lowering a type was implemented in one of them at a
time.

There was nothing shared to fix, for the first four. (#531 and #537
partly overturn that, and say how below: the text-driven callers now share
one normalization in `render_type`, and a block literal's parameters share
one seeder with a free function's. The reasoning here was right about the
positions it described and is left standing for them.)
`render_method_definition` lowers a
parameter and a return type; the `function_definition` arm builds a fresh
`EmitCtx` and does its own; `render_block` computes the hoisted function's
signature itself; and the `struct_specifier` arm pushed the author's bytes
verbatim into the companion header. So each defect is real work, and each
one looks like an isolated oversight from the outside:

| Position | Spelling it did not lower | Issue |
| --- | --- | --- |
| a block literal's parameter | a class name needing a `struct` tag | #326 |
| a free function's return type | the same, plus the return temporary's type | #336 |
| a C struct's field | a class name -- `unknown type name 'Thing'` | #367 |
| a free function's parameter | `id<Proto>` -- `expected ')'` | #367 |
| an ivar in the emitted struct | `id<Proto>` -- `expected identifier or '('` | #531 |
| a `for`-in loop variable | `id<Proto>` -- the same | #531 |
| a block literal's parameter, *inside the body* | its declared type, lost to `id` | #537 |

The rule worth keeping is the one that would have found the last two from
the first two: **when a construct's type is lowered somewhere, ask which
other syntactic positions hold a type, and check each of them** -- not
because the code is shared, but precisely because it is not.

Two properties of this family make it slow to find and easy to
mis-diagnose:

- **The transpile succeeds.** Nothing is unsupported and nothing is
  rejected; the type simply arrives in the output as written, and the *C
  compiler* is what refuses it. That is the one outcome this backend is
  supposed to have designed out -- anything outside the subset is a hard,
  located error -- so a test that only asserts on emitted text is blind to
  it. Every case in `unlowered_spellings.rs` compiles the generated C for
  that reason.
- **The grammar's filing is not the obvious one.** `id<Marker>` is a
  `typedefed_specifier` holding an `id` node beside a
  `protocol_reference_list`, not a `generic_specifier`, so a predicate that
  compared the whole node text to `"id"` saw `id<Marker>` and said no. A
  file-scope `struct box { ... };` is a bare `struct_specifier` rather than
  a `declaration`, and an *earlier* arm claims it -- so the first attempted
  fix was dead code. Three guesses preceded each of those, and each was
  settled by parsing the fragment and dumping the tree rather than by
  reading the grammar.

Fixing #367's second half in the shared predicate, rather than at the call
site that reported it, immediately made two lowerings reach the same bytes:
`block_pointer_edits` already lowered the `id` inside `void (^cb)(id)`, and
the signature-wide pass now did too. `apply_edits` refuses overlapping
edits, which is correct -- that is how a genuine conflict is caught -- so
exact duplicates are dropped at the call site instead of by weakening the
assertion.

**#531 and #537 add a dimension the first four did not have, and it is the
reason "check every position" kept missing these two: a position can hold its
type as a CST node or as flattened text, and the two need different
answers.** The `<Proto>` stripping was never in `render_type` at all -- it was
in the CST reader that feeds it, `extract_type_and_stars_inner`'s
`typedefed_specifier` arm. So every caller that hands `render_type` a *node*
got the fix for free in #367, and every caller that hands it a *string* did
not, silently:

- `forin_binding` joins the loop header's type nodes with
  `node_text(...).join(" ")`, so `for (id<PXCalibratable> d in c)` arrived as
  the literal text `"id<PXCalibratable>"`, fell through the verbatim return
  and emitted that as C.
- `collect_ivar_lowering_edits` is a verbatim text copy with targeted edits,
  and its edits only matched a *direct* `type_identifier` child and an `id`
  inside a function-pointer parameter list. `id<Proto>` is neither, so the
  angle brackets were copied into the emitted struct.

Normalizing in `render_type` itself covers every text-driven caller at once,
which is the version of "fix it in the shared predicate" that this half of the
family admits. The test that it changed no *reader*'s verdict is worth keeping
in mind for the next one: `forin_binding`'s own `type_text` is left alone, so
`generics::check_forin_header` still sees the author's spelling when it asks
`is_class`.

**#537 is in this family by symptom and not by mechanism, and that is worth
separating.** The other six are lowerings -- a type reaching the output
unlowered. #537 is a *scope* omission: `render_block` used the parameter list
only to render the hoisted signature's text, and never put the parameters in
`ctx.scope`, so inside the body a parameter fell to `render_expr`'s
`unwrap_or("id")` and a send to it was rejected as an unresolvable `id`
receiver however carefully it had been typed. The transpile *fails* here rather
than emitting bad C, so it is the one member of the family the "the C compiler
is what refuses it" property above does not describe.

It is the same omission `collect_function_params` fixed for a free function in
#250, one position over -- and that function's own doc comment already said so
about *its* predecessor ("the free-function path kept getting a reduced version
of what a method body gets"). The seeding is now shared between the two, which
is what makes a block parameter and a free-function parameter answer the same
way by construction rather than by coincidence.

A block parameter is also the first thing in this family that genuinely
**shadows**: the hoisted function's `prefix` is not the enclosing body's
`prefix`. So the seeding records what it displaced and puts it back, including
*removing* a name that was unbound before -- without which a block's parameter
stays visible to the enclosing body after the literal, under the block's type.
Four of the eight tests in `block_parameter_scope.rs` exist for that half
alone, and two of them are negative: a name must not leak out, and two
literals must each see only their own parameter.

### A diagnostic that named its fallback, and the sibling it hid (#551, #557)

Two issues, one `match` arm. `emit`'s unresolvable-receiver rejection answered
every receiver it could not resolve with one sentence -- "cannot statically
resolve the receiver type for selector 'X' (receiver type is 'id')" -- and for
two different causes that sentence named the *consequence* rather than the
reason:

  - `[ value]` (#551). tree-sitter recovers a lone-term send as the receiver
    plus a **MISSING** identifier, so the selector was the empty string: the
    message read `selector ''` and the remedy read "declare the receiver as the
    class that implements `''`". There is nothing in that an author can act on.
  - `[Ghost alloc]`, where `Ghost` came from a `@class` (#557). A forward
    declaration is invisible to the class graph, so the receiver's type degraded
    to `id`. The author had spelled a class name and was told the receiver is
    `id`.

In both, `id` is what the type *became*. And oz2c fires before Clang ever sees
the file -- the resolve pass runs first -- so the generic answer was the only
one either author was ever going to get, where Clang would have named the cause
("is a forward declaration").

**The sibling is the part worth keeping.** #557 was filed on `[Ghost alloc]`,
where the receiver *is* the class name and the type arrives as `id`. A send
through a *variable* of that type -- `Ghost *g; [g tick];` -- reaches the same
arm with `receiver type is 'Ghost*'`: one cause, a second spelling, and no
issue filed on it. It was found by asking the question the ARC defects of
2026-09 earned -- key on the reference, never on the form it was written in --
and a fix keyed on the `id` spelling would have passed #557's own reproduction
while leaving the second live.

What generalises: **a diagnostic can be located, specific, and still name the
wrong thing**, when what it reports is the state the failure left behind rather
than the state that caused it. Two independent sweeps make the point together.
The px-app torture suite graded these diagnostics highly on exactly the
property that was fine -- *"every refusal was located. Not one produced an
unlocated message, and not one silently generated bad C"*, across 26 probes
(#540). The front-end mutation sweep, asking a different question of the same
diagnostics, filed four cause-naming defects: #549, #550, #551 and #557.
Location is a property of the renderer; cause is a property of the branch that
picked the message, and a suite that measures one says nothing about the other.

One bound on the coverage, since it is easy to read the fix as wider than it
is. `staticbar::check_malformed_sends` only sees nodes tree-sitter *built* as a
`message_expression`. `[]` and `[self :1]` are not those -- both come back as
`ERROR` nodes and reach no send walk at all, which is why `MUTATIONS.md` grades
M06 and M10 as caught by Clang downstream rather than by oz2c. "Every malformed
send is refused by oz2c's own diagnostic" is true of the shapes that parse as
sends, and that is a smaller set than the shapes an author can write.

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

**What one dump costs, measured per file rather than inferred from the
total (#385).** The row above says 2.9 s for 18 dumps, which reads as
0.161 s each; re-measured by timing `oz2c.cmake`'s own generated
per-source scripts, `samples/hello_world` on `mps2/an385` produces its 11
dumps in **2.52 s serial, mean 0.229 s/dump** (0.143 s for `OZSpinLock.m`,
0.318 s for `OZDefer.m`, 9.1 MB of JSON). So the order of magnitude in the
table holds and the per-file figure is ~0.2 s. Ninja runs these as
independent edges, so the wall-clock cost is that divided by the core
count.

**A dump without Zephyr headers is 15x cheaper**, which is the number that
decided #385. The behaviour corpus and the Rust suite dump against
`tests/behavior/include/stubs` rather than `zephyr/kernel.h`: 81 corpus
cases dump in **1.43 s serial, mean 0.018 s** (min 0.012, max 0.033,
142 MB). Nearly all of the 0.2 s above is the header closure, not the
Objective-C.

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

### The freed-slot poison cannot survive the free (#452, #445)

`_oz_free` stamps `OZ_CLASS_ID_FREED` into `_meta.class_id` before returning a
slot, so that `oz_retain`, `oz_release` and the dealloc switch can recognise a
stale pointer instead of treating it as a live object. **The stamp is emitted
and cannot be read back.** The reason is the root struct's layout:

```c
struct OZObject {
	struct oz_metadata _meta;   /* class_id is at offset 0 */
	oz_atomic_t oz_refcount;
};
```

`_meta` is the first member, so `class_id` occupies the first bytes of every
object — and those bytes belong to the allocator the instant the block is
returned. Zephyr's `k_mem_slab_free` writes the free-list pointer into the
block -- `*(char **) mem = slab->free_list;` at `mem_slab.c:307`, verified
in `deps/zephyr` -- and glibc writes a tcache `next` pointer there. Either
overwrites the stamp as part of the free it is meant to outlive.

Measured rather than reasoned about, which is how it was found. Probed on
macOS while landing #452: an ivar set to `0x11111111` and the body poisoned
with `0xA5` read back after the free as `class=? v=00000003` -- neither the
marker nor the poison survived, and the `3` is allocator bookkeeping showing
through. The prediction that the marker *would* be readable was written into
#452's handoff as its load-bearing note and was falsified by this probe; the
reasoning had not accounted for the layout.

Two consequences worth keeping:

- **The over-release trap names the class only for a *live* over-release.**
  Driving a live object's refcount to zero and releasing again gives
  `over-release of Widget`. Releasing an *already freed* object cannot name
  the class, because the class is no longer knowable from the object.
  `refcount_traps.rs` stages the first on a live object and deliberately does
  not stage the second.
- **Post-free detection needs a mechanism the free cannot reach.** A side
  table keyed by address, a generation counter held outside the block, or an
  explicit decision that it is AddressSanitizer's job on host and unavailable
  on target. #445 assumed the third was unacceptable ("nothing on target sees
  a leak at all") without the layout constraint in view.

The general shape, which is why this sits here: **an instrument placed inside
the thing it is watching is destroyed by the event it is watching for.** The
stamp was not wrong, and no test of its *emission* could have caught this --
`poison_emission.rs` asserts the store is generated and passes. Only reading
the value back after the free shows it.

#### The fourth option: the word just past the link (#490)

The three options above are all *outside* the block, and the list had a gap:
**a word inside the block that the allocator does not write.** Measured on
target rather than reasoned about, `oz_refcount` is one, on every Zephyr
target, by construction:

| | `sizeof(char *)` | `offsetof(oz_refcount)` | `class_id` after free | refcount after free | body poison |
|---|---|---|---|---|---|
| `mps2/an385` | 4 | **4** | 588 (`?`) | **intact** | intact |
| `qemu_cortex_a53` | 8 | **8** | 376 (`?`) | **intact** | intact |
| arm64 macOS host | 8 | 4 | 5 (`?`) | clobbered | **clobbered** |

`k_mem_slab_free` writes its link over exactly `[0, sizeof(char *))`.
`oz_atomic_t` is Zephyr's `atomic_t`, which is a `long`; `sizeof(long) ==
sizeof(char *)` on both ILP32 and LP64; `_meta` is 4 bytes; so the `long`'s
alignment rounds its offset up to precisely `sizeof(char *)` — the first word
the link cannot reach, on either width. Not luck, and not a coincidence to be
re-measured per board. The host backend is the exception and stays one: its
`oz_atomic_t` is `_Atomic(int)`, so the refcount sits at offset 4 under an
8-byte write, and macOS malloc took the body as well. ASan is the host answer.

So `_oz_free` now stamps `OZ_REFCOUNT_FREED` there instead of `0`, and
`oz_retain`/`oz_release` check for it. What that buys, and it is more than
naming a fault:

**The trap was not merely uninformative on target — it did not run.** This
section said a release of a freed object gives `over-release of ?`. Measured
on `mps2/an385`, it gives *nothing*: the free-list link was `0x2000324c`,
bit 12 of a link **is** `_meta.immortal`, and that bit read back as **1**, so
`oz_release` returns at the immortal check *above* the trap. On
`qemu_cortex_a53` the link was `0x40062978` and the same bit read back **0**,
so there the release would have reached the trap. The two bits are what was
measured; the abort on one board and not the other follows from them and from
`oz_release`'s shape rather than from a second observation. One fixture, two
verdicts, decided by an address — the same shape as the glibc-vs-macOS
disagreement `refcount_traps.rs` documents, and reached here by a different
route. The sentinel check runs above every read of `_meta` precisely so no
clobbered bit can route around it, and
`poison_emission.rs::the_freed_check_precedes_every_read_of_meta` is what
holds that position.

`0` was the wrong marker for a second reason, independent of the ordering: it
is also what a live immortal object holds and what an object mid-teardown
holds, and `<= 0` is the over-release trap's own condition. One value cannot
distinguish three states.

What is **still** uncovered on target, stated as a verdict rather than left
to be discovered: a use of a slot that has since been **reallocated**. The
marker lives in the block, so the next `_oz_alloc` clears it — which it must,
or every object after the first free would trap. Catching that needs a side
table or a generation counter, and neither exists. Option 3 stands for *that*
case; it no longer has to stand for the double free.

The instrument's own coverage was the other half of the gap. Nothing under
`tests/zephyr/` defined `OZ_DEBUG_REFCOUNT`, so every line of #452's C had
never run on a board; the suite now compiles with it, and
`tests/zephyr/src/test_freed_slot.c` measures the survival and the layout
that guarantees it. The two halves are deliberately split: the Rust tests
prove the trap fires and where it sits, staged on a live object so they are
defined everywhere, and the ztest proves the marker is there to be read.
Either alone passes while the feature is broken.

### The audit's first run reported two things about itself (#453)

`oz2c --check-arc` diffs oz2c's ownership decisions against the marks
Clang wrote. Run over the corpus the day it was written, both of its
findings were about the audit rather than about the transpiler -- which is
the argument for running an instrument over a corpus you already believe
is correct, because that is the only condition under which a finding is
diagnostic of the instrument.

**`[t copy];` reported as a position with no handler.** A `+1` dropped at
statement level sits in no expression position, so Clang marks the consume
against the enclosing `CompoundStmt` -- and `arc::discarded_owning_value`
has handled exactly that since #322. The tool's position-to-handler column
is a declared list, and the list was incomplete. That is the only direction
it is allowed to fail in, and the reason the tool says in its own output
that the column is declared rather than read from `emit.rs`.

**`OZObject.oz_prop_lock` reported as "no Clang answer".** It is
synthesized onto the root class by `collect::resolve_properties` and
appears in no source file, so no dump can describe it -- and the report was
sending a reader to look for it. The general fix was already available in
the oracle: `knows_class` separates *the dump covered this class and not
this ivar*, which means oz2c invented the ivar, from *the dump never saw
this class*, which is the only one of the two that is a gap. Naming
`oz_prop_lock` would have left every property backing store to be
rediscovered.

A third, in the same family, in the tool's own test helper: two tests
auditing the same corpus case keyed their temp directory on the source
stem, so each removed the other's dump mid-run. Under `cargo test`'s
default parallelism that presented as the audit reporting nothing, not as
a collision.

And the claim the tool shipped in its first version was **#453's own**,
repeated without checking: that at a call site a `+1` class send, a `+0`
send and a protocol send are marked identically. They are not, and **#478
had already corrected it** in "Where tree-sitter and the Clang AST each
sit" above -- along with the implementor half, which #478 reached with
sharper counterexamples than the ones found here. Nothing below is new
knowledge; it is an independent confirmation from a second direction, kept
because the per-send table is what the audit prints and a table is easier
to re-check than a sentence. Measured with the pinned clang
(`clang-19`, `$ZEPHYR_SDK_INSTALL_DIR/llvm/bin`), one send per row:

| send | mark |
|------|------|
| `[Thing alloc]` -- family `+1` class | `ARCConsumeObject` |
| `[a copy]` -- family `+1` instance | `ARCConsumeObject` |
| `[Thing factoryThing]` -- **non-family** `+1` class | `ARCReclaimReturnedObject` |
| `[a borrowed]` -- `+0` instance | `ARCReclaimReturnedObject` |
| `[s supply]` -- protocol | `ARCReclaimReturnedObject` |

What collapses together is a non-family factory and a `+0` send, which is
#361's question -- so #361 stays unanswerable, for a different reason than
the issue gives. The difference is not pedantic: "the marks say nothing at
a call site" would have made that whole section of the audit look pointless
when the marks are in fact a redundancy check on the family rule.

**The issue body still asserts both of the corrected claims**, and an issue
is the spec a reader arbitrates against. #453 is commented with the two
corrections and the measurements rather than rewritten, so the original
text stays legible next to what replaced it.

### Every host gate green over C that is not C (#428)

`__unsafe_unretained` reached the generated `.c` from ten positions and
**Apple clang accepts it in plain C mode.** Verified directly, with no
flag able to change the answer:

```sh
$ printf '__unsafe_unretained struct S *p;\nint main(void){(void)p;return 0;}\n' > uu.c
$ cc -c uu.c                            # Apple clang 21 -- accepted, silent
$ cc -std=c17 -pedantic-errors -c uu.c   # still accepted
$ cc -Weverything -Werror -c uu.c        # still accepted
$ aarch64-zephyr-elf-gcc -fsyntax-only uu.c
uu.c:1:20: error: expected ';' before 'struct'
```

So `cargo test` (602), the 81-case behaviour corpus, the 40 adapted cases,
ASan, UBSan, `just test` on ARM, `just test-riscv`, `just test-zephyr` and
`just test-pedantic` were all green on macOS, and `rust-tests` failed on
`ubuntu-latest` at the first compile -- because that job's `cc` is gcc,
which answers `'__unsafe_unretained' undeclared`.

This is a **new axis** of "host green is not enough", and the one already
recorded does not cover it. The existing lesson is host-versus-target: a
construct valid on the host and a constraint violation on Zephyr (gap Y's
item-pool `;`), answered by running the board sweeps. This one is
**macOS-clang-versus-Linux-gcc**, and *both* compilers here are hosts. Every
board sweep passed too, because the sample corpus contains no
`__unsafe_unretained` and the qualifier only became reachable when #428 put
it on locals in the Rust fixtures -- which no board gate compiles.

What it costs, and what to do instead: on this machine there is a real GCC
in the Zephyr SDK (`~/.local/zephyr-sdk-*/gnu/*/bin/*-gcc`, GCC 14.3.0),
and it was the only local compiler that could see the defect -- 15
diagnostics naming the qualifier before the fix, 0 errors after. Reach for
it whenever a change puts a new *spelling* into the generated C, rather
than trusting `cc`. And note which check would have caught it without any
compiler at all: an assertion that no ARC ownership qualifier survives
outside a `/* original */` provenance comment, which is now the first of
the two assertions in
`unlowered_spellings::arc_qualifiers_are_stripped_from_every_emitted_position`.
A text match is weaker than a compile everywhere except where the compiler
is wrong about the language.

### A CI filter that never once said no, and read as success (#409)

#387 added a `changes` job so a pull request that cannot affect a target build
stops paying for the five ARM/Zephyr jobs. It ran on thirteen pull requests and
skipped nothing. The cause was one line:

```sh
git fetch --quiet --depth=1 origin "$base"
files=$(git diff --name-only "origin/${base}...HEAD" 2>/dev/null) || {
        say true "could not diff against the base ref"
```

`...` needs a **merge base**, and a depth-1 fetch leaves none. There was a second
cause underneath, found only by running the shape locally: `actions/checkout`
makes a *single-branch* clone, whose refspec covers the pull request's own ref
alone, so the fetch wrote `FETCH_HEAD` and never
`refs/remotes/origin/<base>` -- `origin/main` was not a valid object name, so the
diff had nothing to fail *at*. Either cause alone produces the same verdict, which
is why the first one found looked like the whole answer.

Three things worth keeping.

- **A fail-open is invisible by construction.** Failing open is right here: an
  unnecessary job costs minutes, a skipped necessary one lets a regression
  through. But `affects=true` from a dead diff and `affects=true` from a diff that
  found `emit.rs` print the same line, and the reason went to stderr, which
  nothing reads. The fix annotates an unexplained fail-open with `::warning::`, so
  the run summary carries it. **A safe default still needs to be distinguishable
  from the answer it imitates.**
- **The instrument was never the problem, and #409 said it was.** The issue
  claimed -- and #414 repeated in this document -- that `gh pr checks` renders a
  skipped job as `pass`, so the bug could not be seen. Measured on run
  34609454252 with gh 2.97.0, it prints `skipping` with a duration of `0`,
  plainly distinct from the `pass` beside every job that ran; the jobs API
  reports `conclusion: skipped` for the same jobs. **Both instruments would
  have shown it.** Nobody ran either, for thirteen merges. A blamed tool is a
  comfortable explanation for an unrun check, and it was the wrong one here.
- **Naming an experiment is not running it.** #387 wrote "the first docs-only PR
  after this merges is what shows them skipped" and #390 repeated it. Both were
  correct about what would settle it; neither settled it, and #406 became that
  pull request thirteen merges later -- inside a day -- without anyone looking.
  The same pair shipped a cache that cached nothing (#391) the same way.

And the measurement the filter was justified by was itself short. #387 costed the
gated jobs at 805s and #409 at 940s; the real figure on run 34595210212 is
**1887s**, because `zephyr-integration` is gated too and neither figure counted
its 947s.

**The experiment, run deliberately rather than waited for.** This entry's own
pull request was the docs-only change that settled it -- `docs/STATUS.md` and
nothing else, matching no pattern. Run 34609454252:

```
changes     success, 6s   reason: 1 changed file(s), none under a path that
                          reaches a target build
hw-build-check      skipped
pedantic-gate       skipped
spin-validate       skipped
zephyr-integration  skipped
```

The five gated jobs that cost #406 1887s cost this pull request nothing. A
skipped matrix job collapses to one entry, which is why `hw-build-check`'s two
legs appear once.

Thirteen merges of "the next docs-only PR will show this" took one deliberate
pull request to answer. **The cheapest experiment in this document is the one
nobody ran.**

### A blast-radius sweep that covered two thirds of what it claimed (#400)

Every codegen PR of 2026-09-11 quoted "81 of 81 corpus cases byte-identical" and
described it as both corpora. It was the behaviour corpus alone. The sweep globbed

```python
glob('tests/behavior/cases/*/*.m') + glob('tests/adapted/cases/*/*.m')
```

and the adapted corpus has **no `cases/` level** -- its 40 files live at
`tests/adapted/<source>/*.m`. The second glob matched nothing, contributed nothing,
and said nothing about it. 81 is exactly the behaviour count, so the total looked
right and the wrong number was the one that agreed with expectations.

Worse than the omission: had the sweep covered them, the answer would not have
been "all identical" either. `tests/adapted/gnustep/nil_msgSend_types.m:22` reads
`id result = [nilObj init];`, and `-init` is an owning send, so #400's fix makes
that local ARC-managed and emits `oz_release((struct OZObject *)(result))`
at scope exit -- correct, safe on nil, and a real difference. The honest figure
was 120 of 121 with one expected change, not 81 of 81 with none.

Two rules fall out, both cheap:

- **Assert the corpus size before trusting a corpus result.** A glob that returns
  the number you expected from one half is indistinguishable from a glob that
  worked.
- **A byte-identical result is a claim about what you globbed**, so state the
  populations by path, not by the word "corpora".

Related in kind, and found the same day: the selector survey in #400 first
counted sends across `tests/*/cases/*.m` and samples only, missing the ~50 Rust
test files whose ObjC is inline. `-release` has 1 send in the first population
and **142** in the one that was skipped.


### A blast-radius sweep that agreed with itself about nothing (#433)

The same sweep, one issue later, reported **121 of 121 refused by both
binaries** — no output to compare, and therefore no diff. Read as a result
that is a clean bill of health: nothing changed. It was a broken invocation.

The flags were held in a shell variable and interpolated unquoted. In `zsh`
an unquoted parameter expansion does **not** word-split, so all seven flags
arrived as one argument and every case failed to parse, in both binaries
identically. Re-run with a proper array: **121 of 121 byte-identical**, zero
newly refused, zero newly accepted.

Both numbers are "the two binaries agree". Only one of them is about the
code. The distinguishing question is not whether the two sides match but
**whether the harness did the work at all**, so a sweep needs a cell that
fails when nothing ran — here, the count of cases *either* binary accepted,
which was zero and should have been 121. An all-refused sweep and an
all-identical sweep are the same green and opposite evidence.

### A blast-radius sweep whose own harness produced the diff (#424)

The sweep for #424 built the `origin/main` binary in a scratch copy of the
tree and ran both binaries over the corpora, the samples and the SDK's own
`src/*.m`. It reported **nine SDK sources changed**, each with the whole
`@implementation` emitted twice in main's output and once in the fix's -- a
structural difference no ARC change can produce, which is the only reason it
was not believed.

The cause was in the harness. Main's binary was given main's tree as its `-I`
and `--impl-dir` root while the input `.m` came from the *other* tree, so
`imports.rs` saw `Foundation/OZSpinLock.h` under two paths, failed to recognise
it as already resolved, and spliced the implementation a second time. Running
each binary inside its own self-consistent tree -- the two verified
byte-identical with `diff -r` apart from the change under test -- gave **140 of
140 transpiled inputs byte-identical**, and zero cases where one binary
accepted and the other refused.

Two things to carry forward. A cross-tree comparison must hold *everything*
about the invocation constant except the binary, and the root a compiler
resolves includes against is part of the invocation, not scenery. And a
before/after diff that shows a change too large for the hypothesis is evidence
about the harness, not about the code -- the nine "regressions" were the
harness describing itself.

### Two ways the case for literal dedup was overstated (#372)

The issue was filed arguing footprint, and the footprint argument did not
survive measurement. Both errors are worth keeping, because both look like
diligence.

**`grep '@"'` counts `%@`.** It reported seven boxed string literals in
px-keyboard, the only real application. All seven are `%@` format specifiers
inside plain C strings passed to `OZLog` -- the pattern matches the quote that
*follows* `%@`. px-keyboard contains no boxed literal at all, so the change
that was justified by application footprint saves that application nothing. The
count needs `(?<!%)@"`.

**A duplicate across two files is only a duplicate if they link together.**
Counting distinct-file occurrences across the behavior corpus suggested another
96 bytes available from cross-origin dedup. Every corpus case is its own
program, so those are duplicates across programs that are never linked, and no
dedup can merge them. The real cross-origin figure is zero: the only
multi-origin program in the tree is px-keyboard, which has no literals.

What was actually available: 3 instances across every sample, 72 bytes, and
9 across the behavior corpus. The reason to make the change was that
`@"a" == @"a"` was false where Objective-C says it is true -- which measurement
had nothing to say about.


### A "BSS win" that was 61% not in BSS (#419)

#419 asked for the BSS saved on `samples/heap_alloc` for `mps2/an385` by no
longer reserving a slab for a class nothing slab-allocates. Measured with
`arm-zephyr-eabi-size` on the two ELFs, built from committed binaries and
with no rebuild between them:

| | text | data | bss | dec |
|---|---|---|---|---|
| before | 18536 | 520 | 14487 | 33543 |
| after | 18448 | 324 | 14363 | 33135 |

**124 bytes of BSS, and 196 bytes of `.data`.** A `k_mem_slab` is two
objects, and they land in different sections: the block buffer is
uninitialised (`_k_mem_slab_buf_oz_slab_X`, `B`) and the control structure
is initialised (`oz_slab_X`, `D`, 28 bytes each). Seven slabs went, so the
`.data` half is 7 x 28 and is the *larger* half. Answering the question as
asked -- reading the `bss` column -- would have reported 124 bytes and
understated the RAM saved by more than half.

`nm -S --size-sort` is what makes this checkable rather than inferred: it
names both symbols per slab and their sizes, and 8+16+16+20+20+20+24 = 124
accounts for the BSS column exactly, 7 x 28 = 196 for the data column
exactly. A total that decomposes is a measurement; one that only matches in
aggregate is a coincidence waiting to be found out.

The general form: **a section name in a question is a hypothesis, not a
specification.** Where the bytes went is a property of the linker, not of
the issue text, so measure every section and say which ones moved.

### A substring grep called a dead field live (#371)

`OZObject.h` declared `int _refcount` that nothing read. The live refcount is
`oz_refcount`, synthesized by `companion.rs` into the same struct, so
`grep -rn _refcount` matched the live field and reported the dead one as used.
An ivar audit built on that grep cleared both fields; the same audit matching
whole identifier tokens flagged them immediately.

The field was hard to see by reading, too. In the header it sits where the
refcount belongs and is named what the refcount would be named, and
`emit.rs`'s "`_refcount` stays a sibling" comment -- written about
`oz_refcount` -- reads as a defence of it. Two independent things had to be
misread the same way, which is why it survived to be found by a size
measurement rather than by review.

`tests/no_dead_ivars.rs` is the standing check, and it matches tokens.

**#417 asked for that name back, and it was refused (item 3).** The issue reads
`oz_refcount` as "the one root-object field without the underscore its sibling
`_meta` has" and asks for `_refcount` -- which is the name of the field deleted
above, for being dead. Three things in the tree say why it cannot come back: the
paragraph you are reading; `tests/no_dead_ivars.rs`, whose stated reason for
matching whole identifier tokens is precisely that `_refcount` must not match
inside `oz_refcount`, so the rename would have made the test's own rationale
incoherent; and `emit.rs`'s note that the field is spelled in full *because* the
dead one sat beside it. `include/runtime_legacy/Foundation/Object.h` still
carries an `atomic_t _refcount`, so the collision is live rather than
historical.

The premise behind the item is narrower than it looks. CLAUDE.md's
underscore rule governs **ivars an author writes**, not fields the transpiler
synthesizes; `_meta` and `oz_refcount` are both synthesized, so the
inconsistency between them is real, but the `oz_` is deliberate and
load-bearing rather than an oversight. Renaming `_meta` the other way is worse
still: three behaviour drivers assert on `obj->base._meta.class_id` and were
once unbuildable purely because of that spelling. Item 3 is closed on that
reasoning, not implemented.


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
- **A scratch directory two runs share reports a failure that looks
  entirely local.** `tests/corpus_parity.rs` built its paths from fixed
  names under `$TMPDIR` and cleared them on entry, so a `cargo test` in a
  second worktree deleted the directory this one was still writing into.
  What comes out names a corpus case, a generated file and a missing
  header -- `fatal error: 'slab_reuse_after_free.h' file not found` -- and
  suggests nothing external at all. It also fails in the *other*
  direction, which is worse: a run whose directory is recreated by a
  neighbour can pass on the neighbour's output. Same collision #315 fixed
  for twister, where two sweeps shared `/tmp/twister-out`; the justfile's
  `outdir` has been keyed on the checkout ever since and the Rust suite
  simply never was (#343). A scratch path has to name the checkout, and a
  pid-keyed one has to be removed on `Drop` or it is a leak per run
  instead.
- **Unused code is not free when the call site is unconditional.** The
  default `-getDescription:maxLength:` (#354) looked like it would cost only
  programs that use `%@`, on the reasoning that `--gc-sections` drops the
  protocol dispatch otherwise. Measured, it costs **~360 B on every program
  that calls `OZLog` at all** -- `samples/hello_category`, which contains no
  `%@`, grew 26456 -> 26820 B. `src/OZLog.c:82` calls the dispatch from
  inside `OZLog`'s body and the format string is parsed at *run time*, so
  the `%@` branch is always present and the whole chain behind it stays
  reachable: the dispatch, `OZObject_getDescription_maxLength_` (4 B to
  176 B, of which 110 is formatting the address), the synthesized
  `oz_class_name` (40 B) and one name string per class.

  "The linker will drop it" is a claim about reachability, and reachability
  is decided by the call graph rather than by what the program appears to
  use. The corollary is the useful half: with
  `CONFIG_OBJZ_DEFAULT_DESCRIPTION=n` the call itself is compiled out, and
  then the linker does strip all of it -- `oz_class_name` is absent
  from the ELF and the same sample builds to **26456 B, byte for byte the
  pre-#354 baseline**. So the gate is what makes the cost optional; the
  original reasoning was not wrong about `--gc-sections`, it was wrong
  about what was reachable.

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
- **`grep -c` measures declarations, not dispatch.** #421 reported `%@` on an
  `OZMutableString` printing `<OZMutableString: 0xADDRESS>`, evidenced by
  `grep -c getDescription` answering 0 for both its header and its
  implementation while `OZString`, `OZArray`, `OZDictionary` and `OZNumber`
  each have their own. The count is correct and the conclusion is not:
  `companion.rs` resolves each class's protocol-dispatch arm by walking the
  superclass chain, so `OZMutableString`'s arm reads
  `return OZString_getDescription_maxLength_(...)` and `%@` has always printed
  the contents. Measured on `mps2/an385` -- `ms=hello world` -- both as its
  own type and through an `id`, with and without any `OZString` literal in the
  program, since a literal is what would otherwise be the reason `OZString`'s
  implementation is in the binary at all. **The absent-method reading of an
  inheriting class is the address, and the address is what an unrun report
  reproduces**: the shape #421 describes is real, but only in a tree where
  `OZString` has no `-getDescription:maxLength:` either, which is what
  deleting it from both files was needed to produce.
  What the report did find is that nothing pinned any of this.
  `behavior_foundation_mutable_string.rs` now does, on the dynamic path and on
  the dispatcher arm, so the next reader of that issue does not close it by
  adding an override that duplicates the superclass's -- code in every
  Foundation translation unit, bought with nothing.

## Why this is not Clang's ARC, and what that costs (#351)

Worth writing down because the gap looks like an omission and is mostly a
constraint, and because the one part that *was* an omission was a
use-after-free.

Three differences are forced by the target and are correctly decided:
there is no autorelease pool -- `@autoreleasepool` used to lower to a plain
compound statement and is a hard located error as of #430 -- so ARC's
`objc_autoreleaseReturnValue` / `objc_retainAutoreleasedReturnValue` return
convention cannot exist and a returning function must pick +1-to-caller or
borrowed and declare it consistently; there is no zeroing `__weak`, which is a hard located error
because nothing can zero a weak reference without a runtime; and there is
no `ObjCARCOpt`.

That third one is the load-bearing difference. **Clang emits retain and
release naively at every binding and deletes the redundant pairs in an
LLVM pass.** `oz2c` emits C for GCC, and nothing downstream elides
anything, because `oz_retain`/`oz_release` are ordinary C
functions: the counter is an atomic RMW, which may not be removed or
reordered, and the decrement gates a call to `-dealloc`, which is an
observable effect. Measured with the Zephyr ARM GCC at `-O2` on a
three-line aliasing function: **8** instructions as emitted today, **12**
with a retain/release pair and the ops out of line as they really are, and
**62** with the ops `static inline` in the same translation unit -- three
`LDREX`/`STREX` loops and six `dmb ish`, because inlining drags the whole
release state machine (immortal check, deallocating check, class switch)
into every call site. Under whole-program `-flto` with a no-op dealloc,
GCC inlined the allocator and the use and elided **zero** refcount
traffic. There is no attribute that means "this pair cancels".

So the sparse model is not laziness; it is the only affordable shape. The
right way to read `arc.rs` is as **ARC's optimizer, hand-written at the
source level** -- emit the traffic that is necessary, elide the rest.

Which locates the actual defect precisely. The rule the module stated was
"every local this decides to release must be provably +1", and that is
*provenance*. It says nothing about **escape**: whether the reference is
still reachable after the scope under another name. ARC never needs escape
analysis, because retain-on-binding gives every name its own reference and
makes the question moot -- so declining to pay that atomic means
inheriting the analysis, and the analysis had not been written. `Thing *b
= a; return b;` released `a`, the only reference there was, and handed the
caller a freed object, while the same module reported the function as
returning `+0` so nothing owned it either. Both halves have to be asked.

Two mechanisms, because the two shapes are knowable to different degrees,
and this is the general lesson rather than a detail of one fix:

- A **syntactic** alias is resolvable, and resolving it is free:
  `alias_chain` follows plain-identifier initialisers to the local that
  owns the reference, and that local is kept instead of the alias. No
  refcount traffic, output byte identical.
- An **opaque call** is not resolvable at any price. Nothing can know
  whether `passthrough(a)` returns `a`, another object, or nothing, so
  there is no analysis to write -- the returned value has to be retained,
  which is exactly what ARC does and what the Clang AST already marks
  `ARCReclaimReturnedObject`. Confined to that case, it changed nothing
  across all 118 corpus and adapted cases.

The same root cause had a second site, found by looking for it rather than
by being reported: `render_strong_ivar_assign` matched only the
bare-identifier spelling of an ivar, so `self->_ivar = value` fell through
to a plain C store with no retain and no release of the old value, while
`_ivar = value` -- the identical operation -- was correct (#352). One
missing retain produced three defects in sequence: the storing method's own
scope-exit release destroyed the object immediately, the ivar was left
dangling, and the synthesized dealloc released that freed block a second
time. Two spellings with opposite ownership behaviour over a choice that
says nothing about ownership.

Worth stating as the rule both share: **an ownership decision keyed on a
syntactic form is a decision waiting to be wrong**, because a second
spelling of the same operation escapes it. #351 keyed on the returned
*name*, #352 on the shape of the assignment's left side. Both fixes work by
routing every spelling through one function -- `alias_chain` and
`assigned_ivar_name` -- rather than by adding a branch for the spelling
that was missed.

And the reason it reached `main`: the instruments were fine and pointed
elsewhere. `leak-check` (LSan) and `sanitizers` (ASan) run the whole
behavior corpus, and on the host PAL `oz_slab_alloc` is real
`malloc`/`free`, so ASan reports this shape as a `heap-use-after-free`
immediately. No case in the corpus aliased an owned local. A coverage gap,
not an instrument gap -- and the reason
`tests/behavior/cases/arc/return_alias_escape.m` now exists.

One further trap, recorded because it made the bug look benign: the
use-after-free is **silent unless the use touches the freed memory**. With
the accessor returning a constant instead of reading an ivar, the same
defect produced clean ASan output and correct-looking program output. The
sample that started this printed `Hello, world from object` after
`Deallocating` and exited 0.

## An object is allocated once, and may be initialised more than once

`+alloc` is the slab get: `{Class}_oz_alloc()` is `oz_slab_alloc` plus a
`memset`, the `class_id`, and `oz_refcount = 1`. It hands back a
fully-formed instance, which is why a bare `[X alloc]` with no `-init` is
legitimate here and is in fact the commonest allocation spelling in the
tree. `-init` is an ordinary method that happens to be named `init`.

So `-init` can be sent twice, and after #405 it costs **nothing** in slab
terms: a strong ivar store releases its previous value before evaluating
the new one, so the peak is `N` slots however many times the initialiser
runs -- not `N+1`, and not `2N`. Sizing needs no headroom for it. Measured
on a one-slot pool: `first=1`, `second=1`
(`tools/oz2c/tests/ivar_store_ordering.rs`).

**The contract that falls out: `-init` must be idempotent.** What the store
ordering buys is memory safety, not semantic safety, and three shapes are
outside it:

- **A raw `malloc` into an ivar.** `src/OZMutableString.m` allocates its
  character buffer directly; before #405 `-initWithCString:` overwrote
  `_data` without freeing it, so a second initialisation leaked the first
  buffer and `-dealloc` could not make up for it. Fixed there by freeing
  first, which is a no-op on the first run because `+alloc` zeroed the ivar.
- **A Zephyr primitive.** `gpio_add_callback_dt`
  (`samples/gpio_demo/src/GPIOInput.m`) links a node into a list, so a
  second add corrupts it. `k_work_init`, `k_timer_init` and friends are the
  same shape.
- **A one-shot subsystem call.** `bt_enable()` and `settings_load()`
  (`px-keyboard/src/PXBLEController.m`) answer `-EALREADY` or redo work.

None of those is a sizing problem and none is detectable in general -- any
C call in an initialiser could be non-idempotent. So this is stated as a
contract rather than enforced as a rejection, which was the other option
considered and dropped: a `staticbar` refusal would have to key on
"contains a C call", and the shape it was invented for turned out not to
need refusing at all.

For the record, Clang has no opinion here to match. `-Weverything
-fobjc-arc` and `clang --analyze` are both silent on a double `-init`,
including the loop form; what Clang polices is the initialiser graph
*inside* a class, through `-Wobjc-designated-initializers`.

## The hybrid model: what this backend's ARC is

The section above says why this is not Clang's ARC. This one says what it
*is*, because "scope-based ARC" undersells it and invites the wrong kind of
change.

**It is ARC's optimizer, hand-written at the source level.** Clang emits
retain and release naively at every binding and deletes the redundant
pairs in an LLVM pass; that pass is the only reason ARC is cheap. `oz2c`
emits C for GCC, where `oz_retain`/`oz_release` are ordinary
functions whose counter is an atomic RMW and whose decrement gates a call
to `-dealloc` -- so nothing downstream elides anything, measured at every
setting including whole-program LTO. The elision therefore has to happen
here, and it is the whole value of `arc.rs`. Which is also why it has to be
sound in one direction: a leak is a bug, a double free is corruption, so an
unrecognised shape is treated as borrowed.

### Two questions, not one

Every release decision answers both, and for a long time only the first was
asked:

- **Provenance** -- is this reference `+1` by shape? (`is_owning_expr`,
  `binds_ownership`, `created_by`)
- **Escape** -- is it still reachable after this scope, under another name
  or in another slot?

`Thing *b = a; return b;` is provably `+1` in `a`, and releasing `a` is
still wrong. That omission alone produced #351, #352, #359 and #360.

### The two mechanisms, and the limit on the second

1. **Resolve it statically -- free.** Where the CST says what happened,
   read it and emit nothing. `alias_chain` follows plain-identifier
   initialisers to the local that owns the reference, and that local is
   kept instead of the alias. No refcount traffic; output byte-identical.
2. **Retain where provenance cannot be established -- one pair, only
   there.** Nothing can know whether `passthrough(a)` returns `a`, another
   object, or nothing, so `return_needs_retain` retains the returned value
   and the caller owns it.

**Mechanism 2 works at a `return` and nowhere else, and the reason is
worth keeping.** Retaining there creates a *new* reference the caller can
own, which makes the unknown irrelevant. At a *call* site the question is
whether an existing reference was transferred to you -- and the `+1` and
`+0` cases need one release and zero releases, while adding a retain
shifts both by one. The difference is invariant, so no local action is
correct for both. That is why #361 is answerable only by mechanism 1, or
by making the unknown impossible: adopt the implementors' unanimous
answer, and refuse a genuine disagreement, since a protocol whose
ownership contract depends on the implementation cannot be used correctly
by any caller.

### Where tree-sitter and the Clang AST each sit

**tree-sitter is the primary frontend and mechanism 1's source of truth.**
The CST is syntactic, exact, and available on every run, which is what
makes static resolution free.

**The Clang AST is the corroborating oracle, and depending on it is now
sanctioned** -- the Zephyr SDK ships clang, CI pins its version
(`OZ_CLANG`, `-DOBJZ_REQUIRE_TESTED_CLANG=ON`), and `cmake/oz2c.cmake`
already dumps one AST per source. It carries precisely the facts ARC
decides from: `__strong` / `__unsafe_unretained` qualifiers, and the
transfer points marked `ARCProduceObject`, `ARCConsumeObject` and
`ARCReclaimReturnedObject`. Since #453 `astinfo.rs` reads all of it --
every transfer mark and every ownership qualifier, each attributed to a
resolved source position -- and `oz2c --check-arc` is the audit that diffs
those against oz2c's own verdicts. It read only `ObjCIvarDecl` before
that, so every local qualifier and every cast kind was parsed and
discarded.

**Reading it was four measurements, not a parse.** Each one changed the
design, and none of them is guessable from the JSON:

| what | measured | consequence |
|------|----------|-------------|
| Locations are delta-encoded | of 1,389 positions in a 701 KB dump, **10** name a file, 333 a line, 1,056 an offset alone | resolution is a stateful fold in Clang's print order, not a per-node read |
| The mark node has no location | every mark rides an `ImplicitCastExpr`, which carries `range` and no `loc` | a mark's position is its nearest enclosing node's, inherited downward |
| Node ids repeat | a dump of one three-line method yields 6 mark nodes at **5** distinct ids -- a `VarDecl` is printed twice, once in the method's decl list and again under its `DeclStmt` | dedupe by `id`, or every binding is counted twice, and bindings hold 49 of 107 in-file marks |
| Most marks are elsewhere | **45 of 152** marks over the `arc`, `memory` and `lifecycle` corpora are in `src/*.m`, all one shape | filter by file, or the report's largest row is SDK boilerplate |

Two location shapes a hand-written fixture would have got wrong, both
found by counting them across 22 real dumps rather than by reading the
parser: **888 of 32,465 locations are macro-nested** (`spellingLoc` /
`expansionLoc` nested objects rather than flat fields), which resolve to
the expansion because that is the position a reader can act on -- in this
repo that moves `+ (instancetype)alloc { return nil; }` off the `return`
keyword and onto `nil`, which is the macro; and **2,050 are empty `{}`**,
which inherit rather than drop the mark.

The cost, since #299 made this module's memory the thing to protect:
**34 ms against 35 ms** on a 49 MB dump of a real Zephyr build, same node
count, 78 MB peak resident dominated by the file text. Carrying `loc`,
`range`, `id` and `castKind` on every node is free because serde still
skips every field not named, and because `Loc` needs no lifetime -- `file`
appears on ~10 nodes in a whole dump.

Two things measured about it, so the next reader does not have to guess:

- **It settles questions about ARC that memory gets wrong.** Modern Clang
  permits `__strong` members in C structs; a `static` local is `__strong`
  with static storage duration; a file-scope object pointer is `__strong`.
  All three were asserted incorrectly from recall during the #359 audit and
  corrected by dumping the AST for the shape.
- **It cannot answer every question, and the two sentences this bullet used
  to make about *which* questions were both false.** Corrected 2026-09-13 by
  direct measurement against the pinned clang (#453's audit); the claims had
  stood since #385 and nothing had re-derived them.

  What the marks actually discriminate is **method-family membership**. A
  family `+1` carries `ARCConsumeObject` -- `[Probe alloc]` and
  `[[Probe alloc] init]` both do -- while a `+0` instance send, a protocol
  send and a *non-family* `+1` class send all carry
  `ARCReclaimReturnedObject`. So the three are **not** marked identically,
  and what Clang tells apart is exactly the set `arc::create_rule_family_of`
  already computes from the selector -- and computes with a return-type guard
  Clang lacks (`docs/ARC.md` s 3.1 -- s 1.2 is nil-safe release, and this
  cited it until #453's implementation checked). So `ARCConsumeObject` at a
  call site is a
  **redundancy check** on the family rule, not new knowledge. What Clang does
  not tell apart is a non-family factory from a `+0` send, which is #361's
  question -- so #361 stays unanswerable from the AST, for a different reason
  than this bullet used to give.

  It is also **silent on implementor return-ownership**, contrary to the
  claim that an owned return carries `ARCConsumeObject` and a borrowed one
  does not. Measured, that is false in both directions:
  `- (id)borrowAfterAlloc` returns a borrowed ivar and carries
  `ARCConsumeObject` (from an unrelated sub-expression), while
  `- (id)ownedViaOpaqueC { return cReturns(); }` returns a reclaimed `+1`
  and carries none. And `ARCProduceObject` sits on the return of *every*
  object-returning method, owned or borrowed alike -- it is
  `objc_autoreleaseReturnValue`, which `docs/ARC.md` s 1.3.3 already rules
  `N/A` here. The AST therefore classifies implementors no better than the
  CST does, and the need for unanimity stands on its own.

  Census over repo-owned source (155 `.m` files, deduped by `file:line:col`):
  489 transfer marks -- 202 `ARCReclaimReturnedObject`, 194
  `ARCConsumeObject`, 93 `ARCProduceObject` -- of which `astinfo.rs` read
  **none**; and 411 ownership-qualified declarations of which it read **35**
  (8.5%), all of them `ObjCIvarDecl`.

  **That census is the *before* picture: #453 reads all of it**, every
  transfer mark and every ownership qualifier on a `VarDecl`,
  `ParmVarDecl`, `FieldDecl` or `ObjCIvarDecl`. Two notes for whoever
  re-runs the count, because neither is visible in the figures:

  - **The dedup keys differ, so the two censuses are not comparable even at
    equal scope.** This one keys on `file:line:col`; `astinfo.rs` keys on
    Clang's node `id`, because the thing being deduplicated is a node
    *printed twice* (a `VarDecl` appears in its method's decl list and again
    under its `DeclStmt`). Two genuinely distinct marks can share a
    `file:line:col` -- `Slot *s = [Slot alloc];` carries the produce and the
    consume that takes it -- so the `file:line:col` key can undercount where
    the `id` key does not.
  - **Not every qualifier in that 411 was written by an author.** ARC infers
    `__autoreleasing` on an indirect parameter, so
    `+ (id)arrayWithObjects:(const id *)objects` -- which writes no
    qualifier -- dumps as `const __autoreleasing id *`. All 18
    `__autoreleasing` occurrences in this repo's dumps are that one shape,
    and `git grep __autoreleasing -- '*.h' '*.m' '*.c'` matches **nothing**.
    Clang does not *override* a written qualifier: five lines below,
    `OZArray.h:28`'s `objects:(__unsafe_unretained id *)stackbuf` dumps as
    `__unsafe_unretained`. So the inference fills a gap rather than
    contradicting source, and `astinfo::QualifierScope` records which of the
    two a qualifier is -- the declaration's or its pointee's. An audit that
    conflates them tells an author they wrote a qualifier #448 refuses when
    their file contains none.

**Since #385 the dump is required, not optional.** `oz2c` refuses a source
that declares a class with no `--ast` behind it -- a hard, located error at
the first class keyword, consistent with the rule that this backend never
silently degrades. What it used to do instead was print `no AST dumps` and
carry on, and the consequence of carrying on is not a failed build but a
leak: without a dump `owned_object_ivars` falls back to a syntactic rule
that cannot recognise an `id`-typed ivar as an object, so nothing releases
it.

The practical constraint that used to block this was the Rust suite: ~500
tests drove `oz2c::transpile` with no AST at all, so the primary gate
exercised the fall-back while every shipped path exercised Clang's answer.
`tests/common/mod.rs` now writes each case's source to a real `.m` and
dumps it, and every other producer was already in place -- so all five
paths agree:

| path | dumps | which clang |
|---|---|---|
| `cmake/oz2c.cmake` (Zephyr) | one per entry `.m` + per `src/*.m` | `objz_find_clang()` |
| `tests/tools/compile_and_run.py` (behaviour + adapted) | one per case | `scripts/objz_clang.py` |
| `tools/oz2c/tests/common/mod.rs` (Rust suite) | one per compile-and-run case | `scripts/objz_clang.py` |
| `tests/smoke/run.py` | one | `scripts/objz_clang.py` |
| `scripts/regen_zephyr_tests.py` | one per source | `scripts/objz_clang.py` |

`objz_clang.py` exists because the two Python harnesses started their search
at Homebrew and never looked in the SDK, so on a machine with the SDK's
LLVM installed they dumped with a *different* clang from every CMake build.
That is #269 one layer down, and a shared locator is the only thing that
keeps five call sites agreeing.

Two exemptions, and both are stated rather than discovered:

- **`--manifest-only`**, the configure-time run. It exists to discover the
  generated *file list*, which no AST fact affects, and CMake calls it
  before Zephyr's generated headers exist -- so `zephyr/kernel.h` dies on
  `fatal error: 'zephyr/syscall_list.h' file not found` and a dump is not
  merely wasteful there but impossible. Requiring one would also reinstate
  the 742 MB per configure that #299 removed.
- **`--allow-missing-ast`**, the escape hatch, for a hand transpile whose
  header closure will not parse where it is being run. Named for what it
  permits so it cannot read as "skip the AST to go faster": it transpiles
  with the narrower rule, which leaks every `id`-typed ivar.

`Options::require_ast` is off in `Options::default()`, deliberately:
`transpile(source)` is a pure function over a string with no file behind
it, and the ~130 tests that assert on emitted text for one construct have
no `.m` for Clang to read. `expect_reject` is the same case from the other
side -- many of those sources are not valid Objective-C at all, so Clang
has no answer to give and the rejection happens before any ownership
question arises.

Making the harness dump found a real defect the moment it ran, which is the
argument for having done it: two `OZDefer` fixtures declared their ivar as
`struct OZDefer *_cleanup` -- the *generated C* spelling, in Objective-C
source. tree-sitter's fall-back rule treated that as owned and the tests
passed; Clang says a `struct` tag is not an object pointer, and it is
right. The real corpus case those two were ported from
(`tests/behavior/cases/foundation/defer_basic.m`) writes `OZDefer
*_cleanup`, so the fixtures were the outlier, and the gate had been
green on an answer the shipped path never gives.

### The rule that falls out

**Key ownership on the reference, never on a syntactic form.** Every defect
in this family was a decision keyed on spelling -- the returned name, an
assignment's left-hand form, an array store's receiver, the kind of slot.
Each fix routes every spelling through one function (`alias_chain`,
`assigned_ivar_name`), and that is the only thing that has stopped the next
site appearing. When one site is found wrong, enumerate the rest rather
than fixing the one: that is how #352 was found, and the three defects in
#359.

`ownership_matrix.rs` is the standing check -- every sink's refcount shape
asserted, known defects asserted to *stay* defective -- so what is believed
correct is a claim someone verified rather than one nobody examined.

## The ownership audit (#359, #360, #361)

Prompted by the question the fixes for #351 and #352 raised and did not
answer: those two were found by report and by enumeration, so what else is
there? Every **sink** a `+1` reference can reach was walked -- 25 shapes --
and each checked for both failure directions, released-while-reachable and
never-released.

Nineteen were correct: an ivar store in either spelling, an owned array
element in the bare spelling, array elements released when the array dies,
returning the owner, plain/loop/nested locals, local reassignment, a plain
C call argument, a variadic argument, a property store through the
synthesized setter, `@synchronized` on an owned local, a collection-literal
element, a discarded `+1` method result, a discarded C-factory result, a
direct global assignment, both #351 shapes, and a statically dispatched
call judged per class. Two escapes are closed by refusal rather than
tracking -- a block capturing a local is a located error, and so is a
struct field as a message receiver -- which is as good as tracking them and
is now pinned, because if either starts being *accepted* it becomes an
untracked sink.

Four defects, all one family: **three kinds of strong storage exist outside
an ivar and none was tracked** (#359). A file-scope global assigned from a
local was a reachable use-after-free -- reading the slot back compiles. The
same global reassigned leaked the previous value. A `static` local failed
twice over, destroyed at scope exit and then released again on the next
call. A C struct field was the same wrong free. Two more sites of the
ownership-by-spelling cause remain: `self->_arr[i]` (#360), and a `+1`
returned through protocol dispatch, which leaks (#361) -- in the safe
direction, and improvable, since `arc::analyze` can ask whether every
implementor of the selector agrees.

Three things worth keeping from how the audit went wrong before it went
right:

- **The measuring tool failed first.** The extractor that pulled one
  function out of the generated C stopped at the first `}` in column zero,
  which is not the end of any function containing a nested block -- so
  `@synchronized` on an owned local looked like a leak. It is balanced, and
  always was. A false finding from a broken instrument reads exactly like a
  real one.
- **Two claims about ARC were wrong from memory and right from the AST.**
  Modern Clang permits `__strong` members in C structs (non-trivial C
  structs), and a `static` local is `__strong` with static storage
  duration. Both were checked by dumping the AST for the shape rather than
  recalled.
- **The live application was not affected, and only checking showed why.**
  `px-keyboard` has five file-scope object globals, all singletons, and
  every one assigns the `+1` *directly*; the defect needs the reference to
  pass through a local first.

The audit is now `tools/oz2c/tests/ownership_matrix.rs`, asserting the
refcount shape of every sink, with the two remaining defects asserted to
*still* be defective the way `KNOWN_CC_FAILURES` is -- so fixing one fails
the test and forces the list to change, and everything not listed is
believed correct rather than merely unexamined.

## A seventh, and the audit that found it (#400)

#400 walked the dimension the four earlier audits did not: **which selectors
and constructs create or consume a reference**, the set every other ownership
answer is built on. Six areas, fourteen cells, now standing as
`tools/oz2c/tests/selector_ownership_matrix.rs`.

Everything behaved correctly except one cell, and it is the seventh consecutive
ownership decision keyed on a syntactic form rather than on the reference:
`managed_object_locals` asked `stars == 1 && program.is_class(type_text)`, and
`id` carries no `*` in source -- it is already a pointer. So an `id`-typed local
holding a `+1` was never ARC-managed, and `id c = [t copy];` deallocs once where
`Thing *c = [t copy];` deallocs twice. Same reference, same statement, different
spelling of the declared type.

As with #398, the correct rule was already written down in the same file: the
dynamic-dispatch return check reads
`class_name_from_type(ty).is_some() || ty.trim() == "id"`. ARC's local scan did
not consult it. Two of the seven have now been "a correct rule a few lines from
a check that never asks it", which is worth watching for directly rather than
rediscovering.

**Why this matrix counts nothing.** `ownership_matrix.rs` counts `(allocations,
retains, releases)`, and counting cannot see *which* pointer a release names --
which is exactly what #398 got wrong, and why a row there would have passed
before and after that fix. These rows assert observed stdout instead, with
`-dealloc` printing, so an unreleased reference is visible and a
released-through-the-wrong-pointer one crashes. It is also #376's lesson: its
eager-allocation defect had matching counts and was invisible to a counting
matrix.

**Two traps this audit walked into itself**, both recorded because neither
announced itself:

- Every cell first passed the same stem to `compile_and_run`, which keys its
  build directory on the stem. Twelve cells collided on one binary and each
  reported the *first* cell's output -- the matrix agreed with itself about
  nothing while reporting four of five groups failing for invented reasons.
- The one cell that expects *no* dealloc could have passed on a failed
  allocation, since nil produces the same output as "the author owns it". The
  fixture's `-copy` allocates, so exhausting a slab is reachable. It asserts a
  non-null receiver first; every other cell is protected by the dealloc it
  asserts, because a `d` cannot print for an object that was never allocated.

**What it establishes, and the claim's price.** Everything not in that file is
believed correct about selector and construct ownership -- a claim worth making
only while the list stays complete. A new selector or construct that can create
or consume a reference needs a row.

## A sixth decision keyed on spelling (#398)

`arc.rs` asked `selector.starts_with("init")` in **five** places, and that
prefix matches every ordinary method whose name merely begins with those four
letters: `-initialValue`, `-initialCount`, `-initialised`, `-initializeCache`,
`-initialState`. Three of the five then read the receiver's `+1` as handed back
through the return value, so `[[Thing alloc] initialValue]` treated the *`int`*
as the reference and released it. Signal 11 on the host.

This is the sixth ARC defect in a row keyed on a syntactic form rather than on
the reference -- after the returned name (#351), a scalar ivar store's left side
(#352), an array store's receiver (#360), the kind of slot (#359), and the
receiver's static class (#365). The fix has the same shape every time: one
function that every spelling routes through. Here that is `is_initialiser`,
which asks what the method *returns* rather than how it is spelled.

Two details worth keeping:

**The rule was already written down two lines away.** `consider_method` reads
"Only an object-returning method can hand back ownership" and tests
`return_type.contains('*')` -- but `is_owning_selector` returned early above it,
so `-initialValue` never reached the test that would have rejected it. Both now
share one `returns_object_pointer`. A correct rule sitting next to a check that
short-circuits past it is not a rule.

**Unanimity across the program, not a lookup on the receiver's class.** #365 is
the standing lesson that the implementation which runs may be an override with a
different contract. So an `init`-prefixed selector qualifies only if *every*
declaration of it returns an object pointer; disagreement answers "not an
initialiser", which leaks rather than corrupts.

## Where the same fix twice was the tell, again (#361, #365)

The ownership audit found the protocol-dispatch leak (#361) and stopped
there. Implementing it turned up a second defect in the same function --
worse, and unreported -- which is the same lesson the array-ivar entry
above records, arrived at a different way.

`emit::dynamic_dispatch_call` routes a send through the `class_id` switch
for **two** reasons, and only one of them had been examined:

- the receiver pins nothing down (a bare `id`, or protocol-qualified).
  Ownership was `+0`, so a `+1` from the implementation that ran leaked
  (#361).
- the receiver's class is known but a subclass overrides the selector. The
  call is correctly dynamic; ownership came from the **static** class, so
  a caller holding a `Base *` released what a `Derived` override still
  owned (#365) -- heap-use-after-free, the corrupting direction, against
  this document's own standing rule.

Both are one question -- *which implementations can this send reach?* --
over different sets, and one answer covers both: poll them and require
agreement. What makes it worth recording is the argument for why there is
nothing better available:

**No caller-side action resolves a disagreement.** A `+1` result must be
released exactly once and a `+0` one never, so the two cases differ by one
release. Adding a retain shifts *both* by one and leaves the difference
exactly where it was. That is why the retain-when-unprovable mechanism
#351 uses at a `return` does not generalise: retaining there creates a
*new* reference the caller can own, which makes the unknown irrelevant,
whereas at a call site the question is whether an *existing* reference was
handed over. The choice is therefore static resolution, or making the
unknown impossible -- and a contract that depends on which subclass turns
up cannot be satisfied by anyone, so refusing it is the honest answer
rather than the aggressive one.

**Clang cannot answer it either**, which is worth knowing before anyone
reaches for the AST here. A protocol send, a `+1` class send and a `+0`
class send all carry the identical `ARCReclaimReturnedObject`: ARC's callee
autoreleases and its caller always reclaims, a convention that is sound
only with the pool this target does not have. The AST *does* distinguish
the implementors -- a method handing over an owned reference carries
`ARCConsumeObject` inside it -- but which implementor runs is a run-time
fact, and no oracle removes the need for unanimity.

One more thing the fix had to get right, and it is the reason the emitted C
did not move at all: the refusal is scoped to **object-returning**
selectors. Ownership is meaningless for a `void` or scalar result, and
refusing those would reject ordinary polymorphism -- `-poke` overridden by
three subclasses is what dynamic dispatch is *for*.

## Where the same fix twice was the tell, a third time (#424)

#359 brought the two strong slots that are not ivars under ARC -- a file-scope
object and a `static` local -- and wrote the store once, as
`emit::render_strong_local_assign`, on the argument that the three kinds of
slot differ only in whether a scope releases them. The argument is right. The
store did not actually follow it, because `render_strong_local_assign` answered
`None` for one of the three shapes `emit::classify_store` distinguishes, and a
`None` there falls through to `pass_through` -- a plain C store.

That was sound for a managed **local**: `managed_object_locals` drops a
candidate whose stores include a `LocalStore::Unsupported` one, so such a local
is never managed at all and never reaches the store. `static_object_locals` and
`is_file_scope_object` have no such filter. So

```objc
	static Thing *cached;

	cached = [Thing alloc];   /* released the previous -- managed */
	cached = [cached copy];   /* plain C store -- leaked the previous */
```

was managed for one store and not the other, and nothing anywhere said so. The
two halves of #359 -- *which slots are strong* and *how a store to one is
lowered* -- were keyed on different things, and only the first had been swept.

Two shapes reach the gap, and the second is worse than a leak:

- `cached = [cached copy]` is `+1` but reads the slot, so the release cannot
  come first. Nothing released the previous value: a leak.
- `g_slot = pick(a)` is a `+0` **call**, and `classify_store` puts it in the
  same arm for the same reason it cannot be lowered release-first. It took no
  retain either, so the caller's own release freed an object the slot still
  pointed at. `g_slot = a` -- the identical reference, spelled as an
  identifier -- has retained since #359. The spelling was the whole difference.

The fix is the one this document already prescribes: route every spelling
through one function. `render_overlapping_strong_store` is the ivar arm moved
out and parameterised on the slot's C lvalue, which is the only thing the four
strong slots differ in. **The filter was deliberately not copied to the other
two slots**, even though it is the smaller change: it would have dropped those
slots' *other* stores back to unmanaged, trading a declared leak on one store
for a silent one on all of them.

### The loop hazard the shared lowering had to fix to be shareable

The ivar arm could not be moved as it stood. It pushed the previous value's
capture into `ctx.pre_stmts` **with its initialiser**, and `pre_stmts` are
drained by the enclosing *top-level* statement -- so the capture is lifted
above an enclosing loop, above a braced body included. Measured on `main`:

```c
struct OZObject *_oz_prev_L397_C3_1 = (struct OZObject *)(self->_kid);
for (int i = 0; i < 3; i++) {
	(self->_kid = pick(a), oz_retain(...), oz_release(_oz_prev_L397_C3_1));
}
```

One pointer, read once before the loop, released three times.

The comment on that arm said the shape was unreachable inside a loop because
`staticbar::LoopEscape::OverlappingStore` refuses it. It is not, and the reason
is worth keeping: the bar only walks outward from a **`+1`** expression, and
half of what lands in this arm is `+0`. A rejection that covers the shape
by accident of what the scan visits is not a guard.

`render_comma_operand_expr` had already solved this for #376 and written down
why: **declare** the temporary through `pre_stmts`, **assign** it inside the
comma expression. A declaration with no initialiser evaluates nothing, so
lifting it above a loop costs nothing and reorders nothing, while the capture
stays where the source put it. The shared lowering does that, which closes the
ivar defect as a side effect of being shareable at all -- and without relaxing
the bar, which is still right to refuse a `+1` of this shape for its own
reason: two objects are briefly live, so one slab slot is not enough.

## Which positions ask the ownership question (#355 and after)

The audit behind #359 walked every **sink** a `+1` reference can reach —
a local, an ivar, an array element, a global, a `return`. This one walked
every **position a `+1` expression can appear in**, which turned out to be
a different question with a worse answer.

Only two arms of `render_expr` ever reached
`emit::collect_owning_operands`: `expression_statement` and
`declaration`, plus `for_header_owning_operands` for a loop
initialiser. Every other position that can hold an expression asked
nothing at all. Fourteen probed, **nine wrong**:

| Position | Was | Now |
| --- | --- | --- |
| strong-ivar setter argument | correct | unchanged |
| discarded result | correct | unchanged |
| unbraced `if` body | correct | unchanged |
| declaration binding a borrowed result | **use-after-free** | retained |
| plain C call argument | **leak** | released |
| `return` of a borrowed result | **leak** | retained, caller owns |
| `if` condition | **leak** | released |
| `switch` value | **leak** | released |
| `while` / `do` condition | **leak per iteration** | released in-expression (#376) |
| `for` condition / update | **leak per iteration** | released in-expression (#376) |
| `&&` / `||` right operand | **leak**, then eager | released, and only when reached (#376) |
| ternary arm | eager allocation | released, and only when taken (#376) |
| `+1` in a C call in a loop condition | **leak per iteration** | released in-expression (#376) |
| `for` header declaration | **leak** | released after the loop (#376) |

### The two halves, and why they split there

The split is not by syntax but by **how often the operand is evaluated**.

A position evaluated exactly once can have its operand hoisted into a
temporary beside the statement, which is what the group machinery does.
A position evaluated conditionally or repeatedly cannot: hoisting *moves
the allocation*, so it runs once where the source runs it per iteration,
or on a branch the source never takes. The fix for those is not a
different release site but a different shape — a comma expression over a
**declaration-only** temporary:

    (tmp = makeThing(), v = Thing_n(tmp), oz_release(tmp), v)

The declaration is hoisted through `ctx.pre_stmts` and only the
assignment and the release stay inside. That split is why this needed no
new hoisting machinery at all: a declaration with no initialiser
evaluates nothing, so lifting it above a statement — even above a loop —
costs nothing and reorders nothing.

One predicate decides which shape a position gets
(`emit::conditionally_evaluated`), and it is needed in **both**
directions. Where nothing hoists, the operand leaks. Where a statement
arm does hoist, hoisting is eager: `if (x && [makeThing() n] > 0)` was a
leak until the `if`-condition arm reached it, and then became balanced
but allocating whether `x` held or not. So the same question that adds
the comma form also has to *decline* the hoist, and the two mechanisms
are exclusive by construction —
`emit::collect_owning_operands_in` skips exactly what
`emit::unhoisted_owning_operands` picks up.

Eager allocation is the failure that survives a refcount audit: the
counts balance, so nothing in `ownership_matrix.rs` could see it. Only
observing the side effect catches it, which is why
`operand_comma_expr.rs` has a factory that prints and asserts on the
number of evaluations rather than on the number of releases.

The `for`-header **declaration** belonged to neither half, and the next
section is about why.

`staticbar` narrowed how much of this was reachable **at the time**, and
that mattered for testing it: the direct `alloc` spelling of each loop
case was refused outright, while a factory *call* — whose `+1` is created
inside the callee — reached the emitter and leaked. Written with `alloc`,
the known-defect rows added to `ownership_matrix.rs` failed as refusals
and hid the real hole, which is the vacuous-test trap that file's own
header warns about, walked into while writing it.

**#345 removed both halves of that**, so this paragraph is history rather
than current behaviour: the bar now asks whether the reference outlives
the iteration, of any `+1` however produced, and it accepts a bare `alloc`
in a controlling expression because the temporary is released inside the
iteration.

### The half that was neither: a header's own declaration (#376)

The `for` header declaration sat on the wrong side of that split, and
saying why is the useful part. It *is* evaluated exactly once, so nothing
about when it allocates was ever wrong and it needed no comma form. What
it needed was somewhere for the **name** to live: `owned_locals_of` is
reached from `arc_note`, for a `declaration` whose parent is a
`compound_statement`, and a header declaration's parent is the
`for_statement`. No scope could see `t`, so the reference was created and
abandoned.

So the header is **rewritten** rather than the statement wrapped —
`emit::render_for_header_owned_declaration`:

```c
{
	struct Thing *t = makeThing();
	for (; i < 1; i++) { ... }
	oz_release((struct OZObject *)(t));
}
```

That is the general shape of the difference: an *operand* can stay where
it is and be named by a temporary beside the statement, but a
*declaration's* name has to move for anything to be able to release it.
Wrapping and rewriting are not the same tool, and the split above reads
as though they were.

Two details carry the correctness, and each was disabled in turn to see
which test failed. The group is a real `ArcScope`, or a `return` inside
the loop jumps straight past the trailing release and the leak returns on
exactly the path an early exit takes. And its `ArcScope::start_byte` is
the `for_statement`'s **own**, so `releases_up_to_jump_target` — which
releases the scopes that began strictly *inside* the construct being left
— leaves it alone for a `break` or `continue`, while the trailing release
still runs when the loop finishes. Given the loop body's offset instead,
both jumps segfault: a double free and a use-after-free.

Its two guards turned out to be complementary rather than defensive, and
only measuring said so. `owned_locals_of_in` answers **provenance**, so
`for (Thing *t = [owned itself]; ...)` is left alone — without it, that
frees what `owned` still names. `arc::declares_pointer` answers the
**type**, so `for (long n = (long)makeThing(); ...)` is left alone —
without it the emitted release casts a `long` to an object pointer, since
`binds_ownership` looks through a non-bridging cast (#332). A plain
`for (int i = 0; ...)` is declined by both and comes out byte-identical.
Removing either broke exactly one test, and a different one each time.

Worth recording as a defect this uncovered rather than fixed: the
**statement-level** twin, `long n = (long)makeThing();`, emits
`oz_release((struct OZObject *)(n))` against a `long` today —
`owned_locals_of_in` has no type check of its own, which is why the
header arm has to supply one.

`staticbar` narrowed how much of this was reachable **at the time**, and
that mattered for testing it: the direct `alloc` spelling of each loop
case was refused outright, while a factory *call* — whose `+1` is created
inside the callee — reached the emitter and leaked. Written with `alloc`,
the known-defect rows added to `ownership_matrix.rs` failed as refusals
and hid the real hole, which is the vacuous-test trap that file's own
header warns about, walked into while writing it.

**#345 removed both halves of that**, so this paragraph is history rather
than current behaviour: the bar now asks whether the reference outlives
the iteration, of any `+1` however produced, and it accepts a bare `alloc`
in a controlling expression because the temporary is released inside the
iteration.

### The failure that was not a leak

`Thing *z = [h take:makeThing()];` where `-take:` hands its argument back
is the one shape here that **frees a live object** rather than keeping a
dead one. The operand goes into a statement-scoped temporary released at
the end of the statement; `z` is not owned, correctly, because `-take:`
is borrowing; so nothing retains it and the release drops the only
reference. `[z n]` then segfaults, measured on the host, in the argument
spelling and the receiver one (`[makeThing() itself]`) alike.
`Thing *s = [[Builder new] result];` is the same shape in ordinary code.

The fix is a retain on the bound slot — what ARC emits for a `__strong`
slot, and correct whether or not the value aliases: aliased, the retain
covers the release; not aliased, the retain and the scope-exit release
cancel on a different object. What makes it the right answer rather than
merely a working one is that `z` becomes an **owned local**, so every
later question about it falls to machinery that already exists and is
already tested.

Two things had to be true for that not to make things worse:

- **The retain is narrow.** `int m = [h count:makeThing()];` keeps the
  tight statement-end release. This is not tidiness: a slab holds one
  slot per *allocation site*, so holding a value past its statement can
  exhaust a pool a statement-scoped release would have recycled. An
  earlier draft of this work claimed site-based sizing made deferral
  free, and the measurement that disproved it was the leak probe —
  `keep(makeThing(1)); keep(makeThing(2));` printed "an object", then
  "nil".
- **The emitter and the analysis read one predicate.** Retaining while
  `arc::return_hands_back_ownership` still reported the function `+0`
  turned the use-after-free into a leak: the caller was handed a `+1`
  nobody released. `arc::hoists_owning_operand` is called by both sides
  for the same reason `return_needs_retain` is (#351), and the type check
  each position needs is left to that position — a blanket widening of
  `binds_ownership` would have had a scope release an `int`.

### `break` inside a `switch` (found by the above, older than it)

Adding an `if`/`switch` arm made a `switch` get wrapped in an operand
group, and the group's release ran twice. The cause had nothing to do with
operands and was live on `main`:

```c
struct Thing *t = ...;
switch (i) {
case 0:
	oz_release(t);   /* break only exits the switch */
	break;
}
Thing_n(t);                 /* freed */
oz_release(t);       /* and again */
```

`ArcScope::is_loop_body` conflated "a `break` stops here" with "this is a
loop body". A `switch` body is a `break` boundary but **not** a
`continue` one, and a loop body is both, so no single flag could express
it. A use-after-free *and* a double free, in a `switch` inside a loop with
an owned local.

The flag is gone. A jump now releases exactly the scopes that **began
inside the construct it leaves**, which is a byte comparison against that
construct's own start offset — `ArcScope::start_byte`. It gets `break`,
`continue` and the operand group's own boundary right at once, and it is
shorter than what it replaced. Worth stating as the general form: when a
flag has to be set differently for two jumps out of the same construct,
the flag is standing in for a question about *structure* that can be asked
directly.

## What bounds an allocation inside a loop (#345)

The slab holds **one slot per allocation site**, not per live object. So
an allocation reached inside a loop is sound exactly when each iteration's
instance dies before the next begins — and `staticbar` asked that question
through three proxies: is the selector literally `alloc`, is the result
bound to a fresh per-iteration local, is it stored into an ARC-managed
one.

All three were approximations, and the rule was wrong in **both**
directions at once.

### Measured, on a one-slot pool, four iterations each

| destination | reused? | previous released *before* the next allocation? | slots | run |
| --- | --- | --- | --- | --- |
| managed local | yes | **yes** | 1 | 4/4 objects |
| ivar, file-scope variable, store cannot read it | yes | **yes** (since #405) | 1 | 4/4 objects |
| ivar, file-scope variable, store reads it | yes | **no** | **2** | 1/4, then `nil` |
| array element, constant index, store cannot read it | yes | **yes** (since #423) | 1 | 4/4 objects |
| array element, constant index, store reads it | yes | **no** | **2** | refused |
| array element, varying index | **no** | n/a | loop bound | unbounded |
| local ARC declined to manage | yes | **no**, nothing releases | loop bound | unbounded |
| ivar that is not an owned strong slot | yes | **no**, nothing releases | loop bound | unbounded |

The first two rows were one row reading "**no**, 2 slots, 1/4 then `nil`"
when #345 measured them, which is what the prose below is arguing with; the
two array rows were likewise one refusal until #423 split them the same
way.

The ivar overlap was called **inherent** here, and it is not. The claim was
that the new value has to be evaluated before the old one is released, or
`_ivar = [_ivar retain]` would free a live object -- true only of a store
that **reads** the ivar. `_ivar = [Thing alloc]` does not, and
`emit::render_strong_local_assign` had been releasing first on exactly that
condition for locals since #234, through `emit::classify_store`, while
`render_strong_ivar_assign` never consulted it and evaluated the new value
first unconditionally. So a strong ivar needed two slab slots for a store a
local served with one, and a class whose `-init` allocates into an ivar could
not be initialised twice on its own sizing: measured `first=1`, `second=0` on
a one-slot pool (#405).

Every store now routes through one predicate, and
`staticbar::overlapping_unless_released_first` asks it rather than
re-deriving an answer. Two of the three shapes need no temporary and are
accepted; `_x = [_x copy]` is `+1` but reads the ivar, so it keeps the
temporary and stays refused. That last part is not just an over-rejection
being lifted -- the refused shape hoists its temporary through
`ctx.pre_stmts`, which a loop lifts out of the loop entirely, so accepting it
would miscompile rather than merely exhaust the pool. The rejection was
load-bearing for one shape and wrong for the other two, which is why the
narrowing and the store fix had to be the same change.

"Every store" is #423's correction, and worth stating as such: when this
was written it meant the ivar store and the local store, and the sentence
read "both stores". The *destination spellings* were two of four — a
`subscript_expression` and a `self->_ivar` were each still deciding for
themselves, in opposite directions. They reach the predicate through
`staticbar::assigned_slot_name` now; see "The third spelling, and then the
fourth" above.

### The over-rejection

`[[Foo alloc] poke]` in a loop was refused although nothing escaped: the
receiver's `+1` is held in a temporary and released after the send, inside
the loop body (#340), and the same is true of an argument (#328), a
discarded result (#322) and a controlling expression (#376). Proved by the
one spelling the old rule could not see — the same shapes through a
factory ran **8 allocations on a single slot** with every object live.

### The under-rejection, which was the worse half

Because the trigger was the literal selector `alloc`, every other way of
producing a `+1` went straight past: a class's own `+new`, `-copy`, and
any analysis-derived factory. So

```objc
for (i = 0; i < 4; i++) {
	_arr[i] = [Foo make];      /* accepted */
}
```

built, ran, and printed `arr[0] = object` followed by three `nil`s. The
`alloc` spelling of the identical program was refused. A program that
silently stores nils is the outcome the rule exists to prevent, and it was
reachable by writing the allocation one function away.

### The rule now

One question — *does this reference outlive the iteration, and if it is
kept, is the previous one released before the next is allocated?* —
answered of any expression `arc::is_owning_expr` calls `+1`, which is
available because `walk_for_reject` runs from `emit`, after `arc::analyze`.

`emit::LoopEscape` names the three ways the answer is no, and the
diagnostic says which applies rather than listing workarounds for a reason
that may not hold. The old message asserted the reference "escapes the
iteration" even for shapes where it plainly did not.

Three boundaries worth keeping in view.

A **constant** subscript names the same element every iteration, so it is
an ivar store for this purpose rather than an unbounded one — a rule that
called every subscript unbounded would say something false about it. Which
half of the ivar answer it gets is then the same question as for any other
spelling: released first, so one slot, where the store cannot read the
element; two, and refused, where it can (#423). It was refused either way
until then, and this document said "it overlaps at two", which was only
ever true of the second.

An ivar the emitter does **not** manage as a strong slot —
`__unsafe_unretained`, or a type Clang did not call an owned object — is
not an overlap at all. Nothing releases the previous value there, so the
loop accumulates and no pool size bounds it; telling the author to raise
the pool would be advice that cannot work (#423).

And `[[Foo alloc] init]` is **one** object, so it
is reported at the outer send: `-init` consumes its receiver's reference
and hands it back, and reporting the inner one would start the escape walk
from an expression that is not the thing stored.

## Walking the specification (#447)

Four audits had already walked this ground: every **sink** a `+1` can reach
(#359), every **position** a `+1` expression can appear in (#355), every
**selector and construct** that creates or consumes one (#400), and every
**store destination** (#423). Each was prompted by the previous one raising
"so what else is there?", and each found defects. The dimension none of them
walked is **the specification itself** -- which of ARC's normative rules hold
here, rule by rule.

That is now [docs/ARC.md](ARC.md), with one verdict per rule, and
`tools/oz2c/tests/arc_conformance.rs` pinning the two verdicts that are
claims about behaviour rather than descriptions of code.

**What it produced before it had written a single row.** Six defects, three of
them memory corruption, all from Objective-C that `clang -fobjc-arc
-Weverything` accepts with zero diagnostics:

| # | direction | keyed on |
|---|---|---|
| #459 | use-after-free | an identifier's text, in a set of names that outlives the body it describes |
| #458 | use-after-free | a selector's exact text, where ARC matches a method *family* and its attributes |
| #460 | use-after-free *and* leak | one predicate for three bridging casts that mean three different things |
| #461 | leak | a store destination the four strong-slot shapes do not cover (`*out = ...`) |
| #448 | silent degrade | two positions for a qualifier that needs covering in all of them |
| #450 | leak | a fixed point that watches one of the two sets it grows |

Three lessons, and the third is the one worth carrying:

- **The verdict categories did the work, not the prose.** Forcing every rule
  into `IMPLEMENTED` / `DELEGATED` / `REFUSED` / `N/A` / `GAP` is what made the
  gaps visible: each is a rule that had no verdict, and writing "what does
  oz2c do here?" next to "what does ARC require?" is a question the
  existing prose never asked in that form.
- **`DELEGATED` is the most valuable category and the most fragile.** Most of
  ARC's front-end rules need no oz2c implementation because
  `-fobjc-arc` refuses them first, and #443 already asserts the flag is on
  every path. But the flag being present is not the same as the *refusal*
  still happening -- a Clang upgrade or a changed triple can retire one
  silently. So `arc_conformance.rs` runs the probes and asserts the
  diagnostics, and a second test asserts each probe is *also* accepted
  **without** `-fobjc-arc`, because the first draft of four rows matched
  `no visible @interface ... declares the selector 'retain'` -- an ordinary
  unknown-selector error that has nothing to do with ARC. Those rows passed
  and tested nothing.
- **An instrument aimed at leaks would have caught none of these.** #451's
  exit-time census sees an object that was never freed; #452's refcount trap
  sees a release below zero. Five of the six above free an object *too early*
  or not at all in a place neither instrument watches, and a dealloc counter
  reads the **right** count when an object is freed too soon. What found them
  was comparing behaviour against a written-down requirement, one rule at a
  time -- which is a different kind of instrument, and cheaper than all of
  them.

One measurement trap specific to this work, recorded because it cost an hour:
**#459 contaminates any probe that shares a local name with another method in
the same fixture.** An early `__bridge_transfer` probe appeared to emit the
correct release and did not -- the release came from a preceding method's
owning `t`. Any fixture for #458, #460 or #461 has to use distinct local names
or it passes for the wrong reason.

## A strong-local decision belongs to the body it was made in (#459)

The first defect #447's matrix turned up, and the most reachable ARC defect
this project has had: **two ordinary methods, and reordering them is the whole
difference.**

```objc
- (void)make { Thing *t = [[Thing alloc] init]; (void)[t tag]; }
- (int)borrow:(Thing *)arg { Thing *t = arg; return [t tag]; }
```

`-borrow:` released the caller's reference. Written the other way round it was
correct.

`EmitCtx::arc_managed_locals` and `arc_managed_slots` hold **names**, and
`owned_locals_of` consults them by name alone. `collect_local_decls` computed
the right answer per body -- `managed_object_locals(body, ..)` is properly
scoped -- and then `extend`ed a context-wide set that was never cleared.
Bodies render in source order, so only a *preceding* body could contaminate a
later one, which is exactly the order dependence.

Three things worth keeping.

**The containment is not where I expected, and only measuring showed it.** A
plain C function builds its own `EmitCtx::new(...)`, so its set starts empty
and free functions were never affected; every method in an `@implementation`
shares one context. The regression test for the free-function case was written
expecting to fail without the fix and passes either way -- it is kept as a
control, because if free functions are ever moved onto the shared context that
test is what notices they have joined the contaminated set.

**Reset in the one function, not at each call site.** The first fix added a
`collect_local_decls_for_body` wrapper at the two top-level sites and left the
`block_literal` site alone. It worked, and it is one added caller away from
reintroducing the bug -- the same argument `collect_local_decls`'s own doc
comment already makes for driving both passes from one place. The landed fix
replaces the sets inside that function, so every caller gets it. `ctx.scope`
is deliberately *not* reset alongside them, and the asymmetry is the point:
that map answers "what C type does this identifier have", which a block body
needs of its enclosing body; these two answer "does ARC manage this slot",
which no block body needs, because capturing an enclosing local is a located
error and a `__block` local is excluded from the managed set anyway.

**The blast radius was measured by asking which sources *could* differ, not by
diffing everything.** The fix can only change output where an object-typed
local name is declared in one body and again in a later body of the same file.
A tree-sitter scan over all 81 behaviour cases, 40 adapted cases, 34 samples
and SDK sources, and px-keyboard's 8 found **12** such files; transpiling those
12 with and without the fix produced **byte-identical** output, and the scan
proves the remaining 151 sources cannot be affected. That is a stronger
statement than a byte count over everything and cost one incremental rebuild:
the reuse is real and the later declarations are all owning, so nothing in the
tree was relying on the stale name.

Worth noting what the reuse means for the corpora as a gate: eight behaviour
cases *do* declare the same object local in two bodies and none of them
declares a borrowed one second, which is why 121 cases under ASan and LSan
were green over a use-after-free reachable in two lines.

## The create rule is a family, and the attributes that could contradict it (#458)

`CREATE_RULE_SELECTORS` was matched with `contains(&selector)` -- an exact
string comparison against six spellings -- while its own doc comment claimed
it was "matching Objective-C's own naming rule (the create rule)". It was
not. ARC matches a **family** (spec § 3.1): a selector is in one when its
first component *is* the family name, or begins with it and the next
character is not a lowercase letter. `-newThing`, `-copyThing` and
`-copyWithZone:` are all `+1` and were read as `+0`.

The tenth ownership decision keyed on a syntactic form, and the form here is
the selector's exact text.

### Widening it alone would have made things worse

Two attributes exist precisely to contradict the family a selector is
spelled into, and oz2c reads neither. `ns_returns_not_retained` on a
create-rule selector is the corrupting case, and it was already reachable
before any widening:

```objc
- (Thing *)copy __attribute__((ns_returns_not_retained))
{
        return _shared;        /* a borrowed ivar */
}
```

ARC reads the attribute and says `+0`, so the caller owes nothing.
oz2c exact-matched `copy`, called it `+1`, and released at scope exit
-- freeing an object the caller never owned while the ivar still held it.
`heap-use-after-free` under ASan, from a source
`clang -fobjc-arc -Weverything` accepts with **zero** diagnostics.

So the two halves are one change: the five ownership attributes
(`ns_returns_retained`, `ns_returns_not_retained`, `ns_consumed`,
`ns_consumes_self`, `objc_method_family`) are refused with a located error,
and *that* is what makes the family rule safe to widen. Refused rather than
implemented on #430's precedent -- a spelling whose meaning is a mechanism
the backend does not have must not be quietly accepted -- and reading them
properly needs the Clang AST parsed for attributes, which is #453. Nothing
in `src/`, `include/`, `samples/`, `tests/` or px-keyboard uses any of the
five, so the refusal refuses nothing that exists.

`objc_precise_lifetime` and `objc_externally_retained` are deliberately
**not** in that set, and a test pins the boundary: they constrain ARC's
freedom to move traffic rather than reassigning ownership, and every release
here is already precise, so ignoring them changes no answer. Their defect is
that they reach the generated C unlowered, which is #461's.

### The guard is the whole safety argument, and the corpus held the counterexample

Widening what counts as owning is the dangerous direction, so the family
rule is guarded by what the method returns -- exactly as `is_initialiser`
has been since #398.

A scan of all **305** distinct selectors in the tree found exactly one that
the family rule newly reaches:
`tests/adapted/mulle_spec/retain_release_balance.m` declares
`- (int)allocOk`. "alloc" followed by `O` is in the `alloc` family by
spelling, and treating it as `+1` hands `oz_release` an `int` --
which is #398 verbatim, where `[[Thing alloc] initialValue]` released the
integer 42 and dereferenced it. Signal 11.

**Clang is no help here, and that is worth knowing.** § 3.2 says an
alloc-family method must return a retainable object pointer, and
`clang -fobjc-arc` accepts `- (int)allocOk`, `- (int)newCount` and
`- (int)copyFlag` with no diagnostic at all -- measured, not assumed. So
this guard is oz2c's own and cannot be delegated.

The scan is the method worth copying: before widening any name-based rule,
enumerate every name in the tree the widening newly reaches, rather than
reasoning about which ones it might.


## Three bridging casts, three meanings, one predicate (#460)

`arc::is_bridging_cast` names `__bridge`, `__bridge_transfer` and
`__bridge_retained` and treats them identically -- as a signal to hold
ownership *back*, so none of the three ownership questions looks through
them. That is exactly right for the first and wrong for the other two, in
opposite directions:

- **`__bridge_retained`** hands a `+1` to the C side, so ARC retains. No
  retain was emitted, and the local was still released at scope exit --
  leaving C holding a freed slot. `heap-use-after-free` under ASan. The
  stale read *succeeded* first and printed the right value, which is the
  trap recorded above: a use-after-free is silent until the allocator
  reuses the block.
- **`__bridge_transfer`** takes a `+1` over from C, so ARC releases it. No
  release was emitted. A leak.

Both from sources `clang -fobjc-arc -Weverything` accepts with zero
diagnostics. The eleventh ownership decision keyed on a syntactic form, and
the form here is a *set* of three spellings collapsed into one answer.

**Refused rather than implemented, and the choice was measured before it
was argued.** Across `src/`, `include/`, `samples/`, `tests/` and
px-keyboard the tree has **zero** uses of either kind; all six bridging
casts are plain `__bridge` and all six are correct --
`px-keyboard/src/PXLEDController.m:61,143` round-trips `self` through a
Zephyr `k_timer` user_data, and `samples/smp_shared/src/main.m:207-208`
casts through `__bridge` to drive a refcount by hand via the C API #437
made a deliberate escape hatch. So the refusal refuses nothing that exists
and converts two silent memory bugs into build errors, which is the
#430 → #458 precedent. Implementing them properly needs new emission and
is a product question -- is CF-style hand-off to C supported? -- rather
than a correctness one, so it is sequenced behind #462's respelling of the
emitted ABI as its own issue.

**The refusal and the opacity are complementary, not redundant**, and this
is the part worth keeping. `is_bridging_cast` still lists all three, and
narrowing it to the one surviving spelling would be the quiet mistake: an
*ordinary* cast is looked through on purpose (#332,
`arc::value_behind_casts`), so a bridging cast that fell into that path
would have its operand's ownership read as the binding's, and
`Thing *t = (__bridge Thing *)[Thing alloc];` would count `+1` twice. A
test asserts the round trip through `void *` yields exactly one dealloc
rather than two, because that is the only way to say it from outside a
private predicate.

**And two diagnostics, not one.** `__bridge_retained` names the
use-after-free; `__bridge_transfer` names the leak. An author who reads
"use-after-free" for a leak learns the wrong thing about their own code,
which is the same standard #425 set for a remedy: a diagnostic makes a
claim, and the claim has to be the true one for that input.

## The fourth jump, and two expectations that did not survive measurement (#454)

Every release here is emitted explicitly at each exit — there is no
`__attribute__((cleanup))` and no unwinding — so each kind of exit needs its
own arm. Normal scope end has `arc_exit`, `return` has
`render_return_statement`, `break` and `continue` have `render_loop_jump`.
`goto` had none, and `is_jump_statement` counted it anyway, which
*suppresses* the trailing scope release on the reasoning that a jump emits
its own. So the release was suppressed and never replaced.

**The root cause was neither half of that.** `needs_translation` lists
`break_statement`, `continue_statement` and `return_statement` to force the
emitter to descend into a subtree, and `goto_statement` was absent. So
`if (n) { goto done; }` — an Objective-C-free subtree — was copied verbatim
and no renderer saw the jump at all. Adding `render_goto` without that entry
leaves the arm **unreachable for the only spelling that matters**, which is
what the first attempt here did. `return_statement` is on that list because
#283 hit precisely this shape: a `return` nested in an ObjC-free `if`,
skipping its loop's release, found by LeakSanitizer and invisible to a test
that only checked return values.

Both halves are load-bearing: four of the six tests fail without either, and
the two that survive both are the controls.

### Two expectations that did not survive

Worth recording because both were stated confidently, relayed onward, and
wrong — and because the measurements were cheap in each case.

**It needs no scope graph.** I expected the release set to require a scope
*graph* rather than the emitter's `Vec<ArcScope>` stack, on the reasoning
that a `goto`'s target is arbitrary where a `break`'s is structural. It does
not: `goto_statement` carries its label as a `statement_identifier` and
`labeled_statement` carries the matching one, so the set is "live scopes that
do not also contain the label" — `releases_up_to_jump_target`'s existing
`start_byte` comparison with a different target search. One rule covers both
jump directions.

**The backward-`goto` hazard does not exist.** `staticbar::LOOP_KINDS` is
`["for_statement", "while_statement", "do_statement"]`, so the loop-escape
bar cannot see a loop built from a backward `goto`, and `pools.rs` counts one
slot per allocation site without asking whether the reference outlives the
iteration. That reads like an unbounded-allocation hole. It is not one: the
scope that allocates also *closes* inside the loop body, so it releases per
iteration and one slot suffices — measured at three iterations on a one-slot
pool, three deallocs, no nil. The non-finding is pinned as
`a_backward_goto_forming_a_loop_needs_one_slot` so it is not re-derived.

### What Clang refuses, so the emitter need not

Probed with this project's own flags rather than recalled:

| shape | `clang -fobjc-arc` |
|---|---|
| forward `goto` out of a scope holding an owned local | accepts |
| backward `goto` re-entering an allocating scope | accepts |
| `goto` **into** a scope, skipping a declaration | **rejects** |
| `goto` **over** a declaration at the same level | **rejects** |

The two rejections are ARC § 2.6.6 (`cannot jump from this goto statement to
its label`) and they are the genuinely hard shapes. So what reaches the
emitter is exactly the two cases one rule handles — another `DELEGATED`
verdict earning its place in [docs/ARC.md](ARC.md).

### One instrument failure, for the record

The first draft emitted the bare name `t` where a release belonged, because
it mirrored `releases_up_to_jump_target`'s body but not its last line — that
function ends in `release_lines(&names, ctx)`. Both sides are
`Vec<String>`, so the type checker had nothing to say; only reading the
generated C did. The same shape as the extractor that made `@synchronized`
look like a leak: when a helper and its caller agree on a type and disagree
on a meaning, the compiler is not the instrument.

## Two resolvers, one receiver, disagreeing silently (#481)

`arc::collect_declared_types` matched `"declaration" | "parameter_declaration"`.
An Objective-C **method** parameter is a `method_parameter` node, and that
kind appeared **zero** times in all of `arc.rs` — against four times each in
`staticbar.rs` and `collect.rs`. `arc.rs` was the only module that did not
know it existed.

**What that cost is not what it looks like.** The emitter resolves a
parameter receiver perfectly well — `ctx.scope` is seeded from
`collect::extract_method_sig` — and emits a **static** call. Only `arc`
failed, so `message_target` answered `None`, `dispatch_ownership` fell
through to polling every reachable implementor, and where two classes
declare the selector and disagree the poll answered `Ambiguous`, which reads
as borrowed and emits no release.

So a **statically dispatched send took its ownership answer from an
ambiguous poll over classes it can never reach.** The `Ambiguous` refusal in
`emit::dynamic_dispatch_call` could not save it: that guards a *dynamically*
dispatched send, and this one was static. The two halves resolved the same
receiver differently and nothing noticed.

### The three conditions, and why the first repro did not reproduce

The defect needs all of:

1. the receiver is a **method parameter**, and
2. the selector is **not** a create-rule name, and
3. **two or more** classes declare it and **disagree** about ownership.

The issue's original repro was `- (void)use:(Thing *)t { Thing *c = [t copy]; }`
and it releases correctly *without* the fix: `copy` is an exact entry in
`CREATE_RULE_SELECTORS`, so `creates_reference` short-circuits
`dispatch_ownership` before the poll and the receiver's class is never
needed. Condition 2 fails outright. An unambiguous analysis-derived factory
does not reproduce either — one implementor means the poll agrees.

**A test written from that description would have passed on unfixed code.**
Both non-reproducing shapes are now pinned as controls, and the check that
matters is the split: removing the fix fails exactly one of four tests, and
the three that pass either way are the controls. That is what makes the
three-condition claim measured rather than asserted.

### The question this does not answer

The fix removes the disagreement for this shape. It does not answer whether
`arc` should resolve receivers **at all**, or should ask the emitter's
already-correct answer. Two resolvers for one question is the drift this
repo has paid for twice: #405 made the bar and the emitter share one
predicate precisely so they could not differ, and #435 made
`staticbar::message_selector` delegate to `emit::parse_message` after it had
answered the same question wrongly for years. This is the third instance of
that shape, and it is an architectural call rather than a fix.

## The day the check could read the size (#433)

#425 ended by naming the condition under which its own conclusion would
expire: the pool would be the right remedy again "the day this check can
read the size". #433 is that day, and the interesting part is that **what
changed was an argument, not a sentence.**

The chain, because each link was true when it was written:

1. The message once claimed the shape would be wrong at *any* pool size.
   True while `ctx.pre_stmts` hoisted the temporary's **initialiser** out of
   the loop: a bigger pool could not move it back in, so accepting the shape
   would have miscompiled rather than merely exhausted the slab.
2. #424 replaced that with a bare declaration plus an assignment inside the
   comma expression. A declaration with no initialiser evaluates nothing, so
   the hazard went — and pool-awareness became *sound* while remaining
   unimplemented.
3. #425 narrowed the message to the fact that survived: raising the pool does
   not lift **this rejection**, because the check never reads `PoolSizes`.
4. #433 made it read them. The narrowed fact is now false too, and the
   advice is back.

No step rewrote a claim that was wrong when made. Each expired because the
code underneath it moved. That is worth separating from the usual case,
where a diagnostic is corrected because it never was true: the guard against
the second is review, and the guard against the first is **citing the
condition that would end the claim**, which #425 happened to do.

### Two questions that look like one class question

The first cut of this fix resolved the wrong class and would have shipped
accepting nothing the issue was filed to accept.

`allocated_class` — written for the *diagnostic*, to say "an allocation of
'Thing'" — answers **which class is named as the receiver of the
allocation**. For the issue's own example, `_thing = [_thing dup]`, it
answers nothing: `_thing` is an ivar, not a class. The capacity question is
a different one — **which slab does the new object come from** — and for a
send that is the declared return type of the selector. `Foo *_thing =
[_thing dup]` draws from `Foo`'s slab although no `Foo` is named anywhere in
the expression.

Keyed on the first, the relaxation was sound, built clean, passed 672 tests
and relaxed **nothing**. The tell was that `raising_the_pool_does_not_lift_an_overlapping_store`
— the test whose whole purpose was to pin the behaviour being changed —
still passed. A green suite after a behavioural change is not evidence the
change is safe; it is first a question about whether the change happened.

So the bar now resolves the stored class through `emit::find_defining_class`
and `emit::method_return_type`, and the two resolvers are collapsed into
one: the diagnostic names the class it can actually promise something about,
which is the same class the acceptance is decided on. `allocated_class` is
gone.

### Advice that is the check's own finding

#425's lesson was that a remedy in a diagnostic is a claim about the
checker's behaviour. The structural guard added here is stronger than
watching the wording: the pool is offered **only** by the arm that has
already resolved the class and read its size below two (`PoolAdvice`). The
advice is a consequence of the check rather than a sentence beside it, so it
cannot drift from what the check will honour.

The two arms that cannot offer it say which fact they are missing rather
than falling silent:

| arm | why no size is offered |
| --- | --- |
| `PoolAdvice::ClassUnresolved` | the slab could not be named — an `id` return type, a C factory — so no size can be promised |
| `PoolAdvice::LoweringCannotUseIt` | `render_strong_array_element_assign` answers `LocalStore::Unsupported` with a located error and **no temporary**, so no size makes the shape work |

The second is why the relaxation is scoped rather than general. #424 gave
the *shared* lowering a liftable temporary; it deliberately did not reach
the array-element one, and that was verified in the tree for this change
rather than carried over from the issue's text.

### One question, both directions through the wrappers

`stored_class` walks **down** through parentheses, casts and a ternary whose
arms share a slab — the same wrappers `loop_escape` already walks **up**
through. A question answered differently on either side of a cast is a
question about the spelling rather than about the reference, which is the
standing rule of every ARC defect since #351. A ternary with a *borrowing*
arm still resolves to nothing, which is why the case that exercises that
shape kept its meaning unchanged.

### What this leaves open

`walk_for_reject` has arms for `message_expression`, `array_literal` and
`dictionary_literal`, and **none for `call_expression`**. A plain C factory
that reads its own destination inside a loop — `_thing = copy_of(_thing);` —
is therefore never asked the loop question at all, at any pool size. That is
pre-existing and untouched by this change, and it is the same shape as every
defect since #355: a position nobody asked the question in. Filed separately
rather than folded in.

## A qualifier read as text, forty lines from one read as a node (#488)

`emit::is_static_declaration` reads the `storage_class_specifier` **node**, and its own
comment says why: so it "sees only the storage class and not a `static` appearing
anywhere else in the text". Forty lines above one of its callers, the ownership
qualifier was read as a substring of the whole declaration:

```rust
if is_object && !node_text(node, src).contains("__unsafe_unretained") {
```

Five sites did that. The cost is a **leak**, and it needs one token in a cast:

```objc
Foo *a = (__unsafe_unretained Foo *)[Foo make];
```

`a` is `__strong` — the qualifier belongs to the cast's type, not to the declaration —
so ARC releases it at scope exit. The substring search saw the token, dropped `a` from
the managed set, and emitted no release. The control differing by that one token emits
`oz_release`, and nothing else in the two outputs differs.

### What separates the cast is not depth

The instinct is "read the node instead of the text", and that is not enough on its own.
The qualifier reaches a variable from three source positions, and tree-sitter puts the
`type_qualifier` node in two different parents:

```text
__unsafe_unretained Foo *a;   declaration > type_qualifier
id __unsafe_unretained g;     declaration > type_qualifier        (after the specifier)
Foo *__unsafe_unretained b;   declaration > init_declarator > pointer_declarator > type_qualifier
Foo *d = (__unsafe_unretained Foo *)0;
                              declaration > init_declarator > cast_expression > ... > type_qualifier
```

A check on one child position covers some spellings and not others, which is the shape
of every ARC defect since #351. What actually separates the cast from the three that
qualify the variable is **which side of the `=` it falls on**, so
`collect::declares_qualifier` descends the declarator freely and stops at the
initialiser's value.

### Reading the node is necessary and not sufficient

The obvious fix is "read the node instead of the text", and stopping there leaves a
second leak untouched. C scopes the two qualifier positions differently:

```objc
__unsafe_unretained Foo *a, *b;   /* both unretained */
Foo *__unsafe_unretained a, *b;   /* `a` unretained, `b` __strong */
```

A qualifier among the *declaration's* own children introduces every declarator, so it
applies to all of them. One inside a *declarator* applies to that declarator alone. The
substring search answered the whole declaration -- and so does a per-declaration node
check, identically -- which made `b` a borrow. `b` holds a `+1` from its own
initialiser, so nothing released it: one object freed where ARC frees two, measured.

So `collect::qualifies` takes the declaration **and** the declarator, and all five call
sites moved the check inside their declarator loops. The instrument lesson is the pair,
not the first half: read the node, *and* narrow the scope to what the language scopes it
to. A node-precise check at the wrong granularity is still a check on the wrong thing.

### One defect can mask another, and an unreachable site is not a safe one

This is the part worth carrying forward. Four of the five sites were un-backstopped —
`collect.rs`'s ivar scan is not, because `model::owned_object_ivar_names` returns
Clang's AST answer and `continue`s before `unretained_ivars` is read (`model.rs:367`).
Of those four, **only two are demonstrable**:

| site | status |
| --- | --- |
| `owned_locals_of` | the leak above; a test that fails against the substring search |
| `managed_object_locals` | demonstrable; likewise |
| `retained_bindings` | byte-identical C either way in every shape tried — the binding it would retain is elided, because the owner outlives the borrow in the same scope |
| `static_object_locals` | **masked by a different defect** |

The last one is the interesting failure. Reaching its qualifier check needs the token
inside the declaration's initialiser, and a `static` local's initialiser must be a
constant expression in C, which leaves exactly one shape:
`static Foo *slot = (__unsafe_unretained Foo *)0;`. That shape loses its release-first
store — and so does `static Foo *slot = (Foo *)0;`, measured, with no qualifier anywhere.
A plain cast already unmanages the slot before any qualifier is consulted, so the
qualifier check is unobservable behind it.

Two conclusions, and the second is the one that is easy to get backwards:

- **A site with no possible test is not thereby a site with no defect.** It can be a
  site whose defect is hidden by an earlier one on the same path. The absence of a
  failing test is evidence about reachability, not about correctness.
- So all four were changed and **two are recorded as unproven**, in the test file's own
  header. Changing a site for consistency is right; counting it as a closed hole because
  the suite stayed green is the thing this repo keeps paying for.

### And a "fails loud" that was loud for the wrong reason

The audit's severity note had the dangerous direction failing at the C compiler: a
macro-spelled qualifier reaching the generated C, where GCC rejects
`__unsafe_unretained`. Measured, it never reaches GCC. `UNRETAINED Foo *p` arrives as a
declaration whose *type* is `UNRETAINED`, so a send to it is a located `oz2c` error —
and the declaration passes through **entirely unlowered**, `Foo` never becoming
`struct Foo`. Still loud, one stage earlier, and for a reason that does not depend on
which compiler is downstream. Worth correcting rather than accepting, because "GCC
catches it" would have credited a backstop that never sees the file.

## Standing design rules

- **The heap has one name per layer, and the layers are the point (#417).** The
  issue this closes counted "seven names for one concept" and called
  `oz_heap_obj_alloc` calling `oz_heap_alloc_obj` the worst pair in the tree.
  Those two are gone -- `ff6616d` retired the anagram and #462 respelled the
  bridge -- and what the count was really seeing is a four-layer path where each
  layer legitimately needs its own name. Written down here because the names are
  now right and nothing said so, which is how a later reader decides to
  "simplify" one of them:

  | layer | name | takes |
  |---|---|---|
  | Objective-C class | `OZHeap` | — |
  | generated accessor | `OZHeap_oz_inner` | `struct OZHeap *` |
  | generated bridge | `oz_heap_alloc` / `oz_heap_free` | `struct OZHeap *` / `void *` |
  | PAL | `oz_heap_alloc_obj` / `oz_heap_free_obj` | `struct oz_heap_inner *` |
  | system fallback | `oz_sys_heap_alloc` / `oz_sys_heap_free` | `size_t` / `void *` |

  The **parameter type is what distinguishes them**, not the verb: the bridge
  takes the class pointer a user holds, the PAL takes the inner store only the
  companion can reach, and the fallback takes neither. `oz_heap_alloc` calling
  `oz_heap_alloc_obj` is a prefix pair rather than the anagram it replaced, and
  that closeness is the accepted cost of one C-side prefix (#462) -- collapsing
  either into the other means a caller passing the wrong one of two pointer
  types that are both `void *`-compatible in generated C.

  `struct oz_heap_inner` is defined in **four** headers, which looks like the
  dead-header duplication #417 found elsewhere and is not: `oz_platform_host.h`
  and `oz_platform_zephyr.h` each `#define OZ_HEAP_INNER_DEFINED` before
  defining it, and `OZHeap.h` and `oz_platform.h` are `#ifndef`-guarded on that
  macro. Whichever header a translation unit reaches first wins and the rest
  skip. Checked rather than assumed, because the shape is indistinguishable from
  a real collision until you read all four guards.

- **`oz_class_name` in a public header is correct, and was not always (#417).**
  The name was `oz_static_class_name` -- a *generated-namespace* spelling
  hand-written into public `OZObject.h`, which #417 filed as the only one a
  human wrote. #462 did not delete the declaration; it removed the violation by
  retiring that namespace, so the name now sits in `oz_`, which is where a
  public C declaration belongs. `OZObject.h:204` declares it and
  `src/OZObject.m:100` calls it, and both should stay. The header has to carry
  it because Clang resolves the call while dumping the AST, before any generated
  header exists (#418's invariant).

- **A claim that depends on the code should name the condition that would end
  it (#425, #433).** A diagnostic, a doc paragraph or a test comment that
  states *why* something is refused is a claim about the current
  implementation, and implementations move. #425 wrote "raising the pool does
  not lift this rejection, because this check never reads `PoolSizes`" and
  added that the remedy would be right again "the day this check can read the
  size". #433 made it read them, and the reversal was recognisable *as* a
  reversal in one reading -- no archaeology, no argument about whether the
  earlier author had been wrong. They had not been; the condition they named
  had simply been met.

  The cost of omitting the condition is not that the text goes stale. It is
  that a later reader cannot tell a claim that expired from a claim that was
  never true, and those want opposite responses: the first is updated, the
  second is a defect to go looking for siblings of. Prefer "X is refused
  because Y" over "X is wrong", and say what would make Y false.

  The corollary for tests: the assertion that pins such a claim is the thing
  that will fail when the condition is met, and that is its job. `#425`'s
  `raising_the_pool_does_not_lift_an_overlapping_store` failing was the
  signal #433 had landed, not an obstacle to landing it -- but only because
  it asserted the *measured behaviour* (three identical diagnostics) rather
  than the reasoning behind it.

- **`oz2c` names the tool. `oz_`/`OZ_` names the code. Nothing is named after
  the crate.** The transpiler answered to two names for most of its life:
  `oz2c` as the binary and the justfile recipe, `oz_static` as the crate, the
  directory, the cmake module, four emitted ABI functions, the per-class id
  macro, the generated dispatch header, the output directory, the diagnostic
  prefix and the banner it signed its output with -- 1851 occurrences over 183
  files (#462). Two layers now, not three: `oz_`/`OZ_` for C and `_oz_` for
  per-class internals, with `oz2c` reserved for *files the tool produces*
  (`oz2c_dispatch.h`, `oz2c_generated/`, `oz2c_build.py`). A new emitted
  symbol takes `oz_`.

  Three things this cost, each worth more than the rename:

  **It reversed a merged decision, and that is allowed to happen.** #417 had
  just given the heap bridge the crate prefix, arguing that a name the PAL
  *declares* and the companion *defines* belongs to a namespace of its own; it
  landed with a test asserting exactly that. #462 overrules it, so
  `oz_heap_alloc` now sits one qualifier from the PAL's `oz_heap_alloc_obj` --
  closer than #417 wanted, and the accepted price of one prefix per layer. The
  word-order rule #417 was really fixing survives untouched. Reintroducing a
  third prefix to restore the distance is the thing not to do.

  **A blind substitution of a guard test can invert it.** #417's test asserted
  `!out.contains("<retired>_heap_alloc_obj")` -- a name no spelling could
  produce, there to catch a replace that caught the PAL alongside the bridge.
  Substituting the prefix turns it into an assertion against
  `oz_heap_alloc_obj`, the PAL's real symbol, which generated C genuinely
  calls: vacuously true becomes false. It was rewritten by hand, and the
  replacement needed a fixture #417's did not -- `companion.rs` emits two heap
  arms, and a program declaring no `OZHeap` gets `return oz_sys_heap_alloc(size)`,
  which never names the PAL pair at all. Asserting against that output failed,
  correctly, on the first run.

  **`\b` is the wrong anchor on the left.** 23 occurrences sat behind `\t`
  inside emission string literals in `companion.rs` and `emit.rs`, and the
  trap flag sat behind `-D`; both are word characters, so a left-anchored
  pattern skips them. The result is an emitter where some sites emit the old
  name and some the new -- which compiles, and fails at link. Anchor on the
  right instead (`oz_static_release\b`), which also stops `_obj` suffixes
  being rewritten. Found by counting occurrences, not by reading the diff.

  The one place the old name survives on purpose is this file, which is the
  archive: `tools/oz_static/PARITY.md` is cited three times, twice as runnable
  `git show` / `git log` commands against the retired `python-backend-final`
  branch, and a rename would turn two working commands into broken ones.
  `tools/oz2c/tests/naming_tool_identity.rs` enforces the rest, and excludes
  this file for that reason and no other.

  **A `-D` flag is the one interface the rename could not carry forward.**
  Every other renamed name is a compile or link error at the old spelling, so
  a stale caller is told. A macro tested with `#ifdef` is not: a build passing
  `-DOZ_STATIC_DEBUG_REFCOUNT` after #472 compiles clean, links clean, and
  runs with the instruments absent -- the one failure mode an instrument must
  not have, because the flag's whole purpose is to be trusted when it is on.
  `OZ_DEBUG_REFCOUNT` (#452) is the current spelling and `CONFIG_OBJZ_DEBUG_REFCOUNT`
  the Kconfig option that supplies it; the Kconfig help points here rather than
  spelling the retired name itself, so the guard above stays strict. This is
  also why `OZ_TRAP_POOL_EXHAUSTION` kept a name it had outgrown, and why
  renaming either one belongs in a release note rather than in a diff alone.

- **Never silently degrade.** Anything outside the supported subset is a hard,
  *located* error. This is deliberate, not a gap someone forgot to fill.
- **A diagnostic's remedy has to be findable, not merely present.** #456 gave
  every rejection a real `file:line:col`; the text was still one line with the
  diagnosis and every remedy fused into it. The `-retain` rejection ran to 90
  words and carried *three* distinct fixes -- let ARC manage the lifetime, opt
  the slot out with `__unsafe_unretained`, read the count with
  `oz_retain_count` -- and a reader had to find the imperative clause
  inside the prose. So `help` is a `Vec`, not an `Option`, and a `note` tier
  carries the reason, which is neither diagnosis nor remedy (#457).

  Two things the renderer turns on that are easy to get wrong. **Columns are
  display width, not bytes.** 58 of the 81 behaviour cases are tab-indented, so
  a caret padded one space per byte lands four columns short of an 8-column tab
  -- this is the common case here, not an edge one, and the shown line has its
  tabs expanded so the two rows agree however a terminal draws a tab. **A span
  wider than its line is clipped**, with a marker: a rejection can cover a whole
  method body, and underlining forty lines buries the `help:` that says what to
  do.

  And the change that made the split safe rather than a rewording risk:
  `Display` carries the tiers. A remedy moved out of `message` into `help` is
  still visible to anything asking whether the remedy was offered, which is what
  ~400 substring assertions in the suite ask. Without that, splitting the text
  would have failed tests whose subject was the remedy, and the temptation would
  have been to weaken the assertions rather than keep the text.

- **Located means a position in a file the author can open, not a position in
  the buffer the compiler walks.** Every pass reads one `#import`-spliced
  buffer and raises diagnostics at offsets into it, and for the whole life of
  the feature `Diagnostic` reported those offsets directly: a 9-line
  `Keyboard.m` with 33 spliced origins had its defect reported at **line 1989**,
  and no file in the project has a line 1989 (#456). Half of "hard, located
  error" was therefore not being delivered, by a rule nobody had written down
  because it read as obviously true.

  What makes it worth a rule is where the fix already was.
  `imports::SourceMap::source_position` answers this exactly, by binary search
  over pre-indexed segments -- it had been built for `#line` directives and no
  diagnostic path ever called it. So the gap was not a missing capability but
  an unconnected one, which is the shape that survives longest: nothing fails,
  and the output looks like a real compiler's.

  Two things the fix turns on, both easy to get wrong the other way. **Carry an
  offset, never a line.** `parse::repair_bare_macro_statements` overwrites an
  ASCII whitespace byte in place, so it preserves every byte offset while
  *eating a line* whenever that byte is a newline -- a line derived from the
  repaired buffer is quietly wrong, and only for files that were repaired.
  **Resolve where the map lives, not where the diagnostic is built.**
  `Options::source_map` carries a map only to switch `#line` directives on, so
  resolving through that field would emit directives into every build; `main.rs`
  owns the resolution because it owns the resolution the map came from.

  And say nothing rather than something plausible: three whole-program checks
  have no node to blame (`attach_ast`, an unknown `--pool-sizes` class, an
  unsizable slab cycle). They keep `(1, 1)` and report *no file*, so a reader
  can tell "unanchored" from "anchored here". A test asserts that absence,
  because the tempting failure is to hand them the entry file and make an
  unanchored diagnostic look located.
- **A remedy in a diagnostic is a claim about the checker, so the check that
  offers it has to be the check that can act on it.** The loop-escape rejection
  told authors to raise the class's pool for four releases while having no pool
  awareness at all -- it never reads `PoolSizes`, so the same program was refused
  identically at `Foo=1`, `Foo=2` and `Foo=8` (#425). Note what makes the rule
  bite: a bigger pool is, since #424, the *genuinely correct* remedy for that
  shape -- two objects are briefly live and a second slot fits them, measured at
  five allocations and five frees on a pool of two. The advice was still wrong to
  give, because this check cannot read the size and so cannot act on it. Being
  right about the fix is not enough; the checker offering it has to be able to
  honour it, or the author changes their source and nothing happens. Two tests
  hold the line -- one compares the diagnostic across three pool sizes and
  requires one string, the other holds all three escapes to the rule so a fourth
  cannot be added without answering it. This is narrower than "don't mention the
  pool": `pools.rs` and `companion.rs` name it for real sizing and exhaustion,
  where it is the fix and they can act on it.

  The same rule has a second edge, and the fix for #425 walked straight into it
  before landing: **a remedy must also be writable in ARC source.**
  `Accumulates` was briefly reworded to advise "release each instance before the
  next iteration allocates", which no author can do — ARC is always enabled
  (`-fobjc-arc` on every path that produces the Clang AST oracle) and an explicit
  `[x release]` is a Clang error, so such a source never reaches `oz2c`. So an
  unactionable remedy had been replaced with an unwritable one — the same defect
  twice, caught by review rather than by a test, which is why there is now a test
  asserting no loop-escape diagnostic tells the author to release anything.

  One sentence of that entry has since been overtaken and is corrected here
  rather than left to contradict the code: it read "That
  `emit::released_by_hand` exists, and that `oz2c` tolerates manual
  retain/release as a feature of its own, is not a licence to recommend it."
  Both halves are gone. #428 made every one of those sends a located error and
  deleted `released_by_hand`, so there is no tolerance left to mistake for a
  licence — the rule below is the one that now says so.
- **Key ownership on the reference, never on a syntactic form, and route every
  spelling through one function.** Eight defects in a row came from a decision
  keyed on a form (#351, #352, #359, #360, #365, #398, #400, #423). #423 is the
  one to read for how it recurs after a fix: #405 routed two of the four
  destination spellings of a store through one predicate, and the two it left
  were wrong in *opposite* directions -- a `subscript_expression` over-rejecting
  a shape the emitter already lowered release-first, and a `self->_ivar`
  accepting one it lowered with a hoisted temporary. Fixing the site that was
  reported is not the fix; enumerating its siblings is.
- **A keyword whose mechanism does not exist is refused, even when the
  behaviour is right.** `@autoreleasepool` was accepted and *worked*: `emit`
  dropped the token and ran the same `arc_enter`/`arc_exit` bookkeeping as
  `render_scoped_block`, so the block was an ordinary ARC scope and
  everything inside was released at the closing brace. It is a hard located
  error now (`staticbar::check_autoreleasepool`, #430), because the keyword
  promises deferred reclaim at a drain point and nothing here can defer:
  there is no `-autorelease` -- ARC forbids the send, one of the five in the
  rule below -- so no reference can ever be *pending*, and no pool object
  exists outside `src/runtime_legacy/`. In Cocoa the difference is
  observable; here it is unreachable, because the mechanism that creates it
  is refused.

  Two things this cost, both worth stating. The construct was **five
  samples**, and each fix was deleting one token -- the braces stay and the
  output is byte-identical, which is what made the rejection mechanical
  rather than risky; measure that claim rather than assuming it, as
  `behavior_autoreleasepool` now does by keeping the old leak regression
  respelled. And **README's advice was wrong twice**: it recommended the
  keyword "in loops that create temporary objects", contrasting a loop whose
  1000 temporaries supposedly lived until return against one that drained a
  pool per iteration. The mechanism did not exist *and* the two loops
  behaved identically, because a `for` body is a scope and ARC already
  released each iteration's object at its end. A document describing a
  mechanism the code does not have will describe it wrongly; that is the
  second reason to refuse rather than document (the first being #418's
  year-long `__objc_refcount_get`).

- **ARC is the only ownership model, and the five selectors it owns cannot be
  written.** A send of `retain`, `release`, `autorelease` or `dealloc` is a
  hard, located error, and so is *declaring or defining* one of the first three
  (#428). Every Clang path in this project passes `-fobjc-arc` --
  `cmake/oz2c.cmake`, `cmake/ObjcClang.cmake`,
  `tests/tools/compile_and_run.py`, `tools/oz2c/tests/common/mod.rs`,
  `scripts/regen_zephyr_tests.py`, and `scripts/objz_check_compile_db.py`'s
  `REQUIRED_FLAGS` -- under which each of those sends is a compile error.
  oz2c parses with tree-sitter rather than Clang, which is the only
  reason they were ever reachable; `emit::released_by_hand` was then built to
  *accommodate* one, so that ARC stood back from any local the author
  released. Its own doc comment called that "a feature of its own", which is
  what made it a second ownership model rather than a tolerance.

  It reached memory corruption. `managed_object_locals` consulted the
  predicate and `static_object_locals`/`is_file_scope_object` did not, so a
  hand release into a `static` slot emitted **two** releases of one reference
  -- signal 11 on the host. Adding the missing filter to the other two paths
  was the first proposal and was the wrong direction: it would have extended
  the second model, and a third slot kind added later would have reintroduced
  the bug. Rejecting the input removes the decision instead.

  Three consequences worth stating rather than leaving to be rediscovered:

  - **Rejecting `[super dealloc]` required synthesizing the chain it drove.**
    `oz_release`'s dispatch called `find_defining_dealloc(class)` and
    nothing else, so only the *most-derived* `-dealloc` ever ran and a
    superclass's own body ran only because a subclass spelled the send.
    Without `companion::dealloc_chain` there would have been no spelling that
    runs a superclass's cleanup and no error saying so -- this rule's own
    violation. A `-dealloc` override is therefore still supported and is the
    one exception in both directions. Every `[super dealloc]` in the tree
    resolved to an empty function, so the three deletions in `samples/` were
    behaviour-identical; the chain is what keeps that true for a superclass
    that has a body.
  - **`retainCount` went the same way, one issue later (#436).** #428 left it
    out because it takes and gives no ownership, so it is not a second
    ownership model -- sound, and an answer to a different question. ARC
    forbids the *send* regardless of ownership, which a probe settles where
    an argument could not: declared or undeclared, Clang under `-fobjc-arc`
    answers `ARC forbids explicit message send of 'retainCount'`. Adding it
    is what made the list's rule statable in one line, **exactly what Clang
    refuses**, with nothing weighed per selector -- hence
    `ARC_FORBIDDEN_SELECTORS`, since `retainCount` is forbidden without being
    owned and the old name could not say that. Reading a refcount never
    depended on the message spelling: `oz_retain_count()` is the plain
    C call #418 made the single entry point. The divergence this bullet used
    to record is gone.
  - **The C API is a deliberate escape hatch, decided rather than tolerated
    (#437).** `oz_retain` and `oz_release` are declared in the
    generated companion header, so a `.m` file's plain C can drive a refcount
    by hand. **ARC governs Objective-C; it has no opinion about a C call**, so
    the rejection is a rule about the source language and not an enforced
    invariant -- and that is the chosen answer, not an admission. Narrowing
    the exports to `oz_retain_count` alone was the alternative and was
    rejected for a concrete reason worth recording: **there is no ARC-legal
    Objective-C spelling that drives one shared object's refcount up and down
    without also serialising on a slot.** A strong-slot store is ARC's way to
    change a refcount and serialises on the slot as well, so it stops being a
    refcount test. A two-core refcount-contention test -- which is what
    `samples/smp_shared` exists for -- therefore cannot be written in
    Objective-C at all, and the migrated fixtures whose subject *is* the
    runtime's arithmetic have nowhere else to go either. Using the C API means
    taking ownership manually and deliberately, in a file that has stepped
    outside the language ARC governs.

  **Reversals (#428), stated out loud.** Three fixture assertions were
  reversed or deleted rather than adapted:

  - `static_bar_rejects::releasing_unretained_ivar_in_dealloc_accepted`
    asserted that `[_seen release]` on an `__unsafe_unretained` ivar was
    *accepted*, on the grounds that nothing releases such an ivar
    automatically. It is now rejected, under the opposite name: the qualifier
    means "this slot does not participate in ARC's retain/release", not
    "manual sends are legal on it", and Clang refuses the send whatever the
    qualifier.
  - `arc_strong_locals::manual_release_suppresses_arc` is **deleted**. Its
    whole subject was `released_by_hand`, and it asserted the *absence* of
    ARC's management. Reversing it would have duplicated
    `bare_declaration_gets_arcs_implicit_nil` in the same file.
  - `selector_ownership_matrix` loses **three** consume-set rows, with the
    record left where they were. One of them expected *no* dealloc for a
    balanced `[t retain]; [t release];` -- a leak, asserted as design,
    because one manual release handed the predicate the whole local.
    `staticbar::check_dealloc_body` is removed with them: the general rule
    covers what it rejected, and its advice (declare the ivar
    `__unsafe_unretained`) is no longer true.

  **Coverage genuinely lost, and not papered over.** Two things nothing
  exercises any more:

  - `arc::created_by`'s outright exclusion of `-retain` as a pass-through,
    in all four positions `arc_leak_regressions` covered it in: a discarded
    result, behind a `(void)` cast, in an argument, and as a receiver. The
    arms stay in `arc.rs` as defence; they have no reachable input.
  - the dealloc **re-entrancy guard** (`_meta.deallocating`). Reaching it
    needs a release *during* the object's own teardown, and the only way to
    write that was `[self retain]; [self release];` inside `-dealloc`: a
    retain cycle cannot do it, because holding the object means its refcount
    never reached zero in the first place.
    `behavior_lifecycle::dealloc_reentrant_guard` keeps the coverage by
    calling `oz_retain`/`oz_release` directly, which is exactly
    what the two sends lowered to -- so the guard is still exercised, but no
    longer by anything an ARC-legal Objective-C program can write.

  **Blast radius, measured.** Both corpora (81 behaviour + 40 adapted) through
  the pre-change `oz2c` and this one, with `corpus_parity.rs`'s flags: **94 of
  121 byte-identical, 0 newly rejected, 0 newly accepted.** Every line of the
  27 diffs is an added `*_dealloc((struct * *)self)` chain call -- 107 added
  lines, 0 removed, nothing else. A rejection should change nothing that
  compiled correctly before, and the only movement is the chain the rejection
  required.

- **A release is only ever emitted for an expression that is an object
  pointer.** Both operand sites used to take the expression's own type where it
  ended in `*` and the root pointer otherwise, reasoning that "`id` is the one
  spelling that is not a C type and nothing else non-pointer can be an object".
  The first half is right and the second was a hole wide enough for `int`: the
  root cast made any type compile, so a `+1` wrongly claimed for
  `[[Thing alloc] initialValue]` released the integer 42 and dereferenced it
  (#398). The fallback is now allowed only for `id`, `instancetype` and a bare
  class name, and anything else is a located error naming the type -- a hard
  error rather than a skipped release, because reaching there with a non-object
  means the analysis decided something untrue, and covering for that means
  either a leak or a wrong free. Verified to bite on its own: with the
  selector fix reverted and only this check in place, the same program stops
  faulting and fails to transpile instead. It would have caught #380, which is
  the same shape reached through a cast.
- **Identical boxed literals in one origin are one object.** That is
  Objective-C's own guarantee, and `-isEqual:`'s opening
  `if (self == anObject)` depends on it: with an instance per *occurrence* the
  fast path missed and `@"a" == @"a"` was false (#372). The collapse happens
  once, at the end of `walk_top_level`, so both assemblers see collapsed
  literals rather than each keeping its own copy of the rule. Two things it
  must not get wrong: the rename has to reach every bucket that can carry the
  expression's text -- bodies, hoisted blocks, `__block` statics, and the
  generated header -- because a hoisted block keeps its own copy and renaming
  only bodies leaves a reference to a definition just dropped; and the rewrite
  is whole-identifier, since `_oz_str_L1_C1_1` sits inside
  `_oz_str_L1_C1_11`.
- **A declared ivar is not free.** It becomes a field in the generated struct
  whether or not anything reads it, and in the root class it becomes a field in
  every object of every class -- `OZObject._refcount` cost 4 bytes per instance
  program-wide while being unreachable. Removing it and `OZString._hash` took
  `struct OZObject` from 12 bytes to 8 and `struct OZString` from 24 to 16, a
  third of every string (#371). Note the second edit each removal needs: the
  boxed-literal initializer in `emit.rs` named `._hash = 0`, so deleting the
  header line alone would have failed every program containing a string
  literal, on an initializer for a member that no longer exists.
  `tests/no_dead_ivars.rs` now fails on any Foundation ivar the SDK's own
  sources never touch, so this is enforced rather than remembered.
- **An invariant about an object's header has to hold on every side that
  touches it.** `oz_release` checked `_meta.immortal` before its
  decrement and its comment stated the rule -- "their refcount is not tracked
  either". `oz_retain` incremented with no check at all, so the rule
  held in one direction and the count of every immortal object climbed for the
  life of the program (#373). Nothing caught it for two reasons worth
  remembering. The nine tests in `behavior_immortal_literals.rs` all exercised
  *release*, and all nine pass with the fix reverted -- a suite named after the
  invariant tested one half of it. And the wrong value was invisible: an
  immortal object's lifetime does not depend on its refcount, so the drift
  changed no behaviour and surfaced only through `retainCount`, which nothing
  asserted on an immortal receiver. **When a check is added to one side of a
  header field, enumerate every function that touches that field** -- there
  were seven, and reading all seven is what showed `retain_count` reporting a
  word nobody maintained. The payoff was not the wasted atomic: a writer is
  what makes an object non-`const`, so removing the last one is what let a
  boxed literal move from `datas` to `.rodata` and stop costing RAM at all.
- **A strong slot's store is keyed on the store's shape, never on the kind of
  slot.** There are four -- an ivar, a managed local, a `static` local, a
  file-scope object -- and they differ in exactly one thing: the C lvalue the
  store names. Everything else about ownership is identical, so the lowering is
  one function (`emit::render_overlapping_strong_store` for the shape that
  needs a temporary, `render_strong_local_assign` for the two that do not) and
  the caller supplies the lvalue text. #359 made this argument and wrote the
  *membership* test three times and the store once; #424 was the store's own
  arm having been written for one slot kind and silently answering `None` for
  the others. When a slot kind is added, the question to ask is not "does it
  have a store" but "which arm of `classify_store` does it reach, and does
  something upstream guarantee it cannot reach the others".
- **A temporary an expression needs is declared through `ctx.pre_stmts` and
  assigned inside the expression.** `pre_stmts` are drained by the enclosing
  *top-level* statement, so anything pushed there is lifted above an enclosing
  loop -- above a braced body too, which is the part that surprises. Pushing a
  declaration *with* its initialiser therefore evaluates it once, outside the
  loop, and every iteration then operates on a stale value: #234 released nil
  four times that way, and the ivar store released one pointer three times
  (#424). A declaration with no initialiser evaluates nothing, so lifting it
  reorders nothing. The split is `render_comma_operand_expr`'s (#376) and it is
  the general answer, not a special case -- the alternative, a self-contained
  braced group, is only available where a statement is
  (`render_owning_operand_statement`).
- **A leak is a bug; a double free is memory corruption.** ARC therefore fails
  toward leaking: an unrecognised shape is treated as borrowed. Widening what
  counts as owning is the dangerous direction and must be exact rather than
  heuristic.

  **Read that as a statement about `is_owning_expr`'s bias, not about the
  system's behaviour** -- which is the correction #447 forced. Walking the ARC
  specification turned up three defects in the *corrupting* direction at once
  (#458, #459, #460), and none of them came from widening what counts as
  owning. They came from three different decisions keyed on a syntactic form
  where ARC keys on something else: a selector's exact text against ARC's
  method *family* (#458), an identifier's text against a set of names that
  outlives the body it describes (#459), and one predicate for three bridging
  casts that mean three different things (#460). The bias protects the shapes
  the analysis declines to recognise. It does nothing for a shape the analysis
  recognises *confidently and wrongly*, and that is where every corrupting
  defect so far has lived.

  Note what that rule does *not* say: recognising a shape as +1 is only half
  the job, and a *recognised* one still leaked for as long as nothing bound it
  (#322). It also does not say that one reading of ownership serves
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
  none of the three. An **argument** is the third site the question is asked
  from, and it asks the discard question rather than a separate one of its own
  (`owning_argument_value`, #328): whether the callee retains the argument or
  only borrows it does not change the caller's obligation, so all that is
  left to decide is whether the reference is new. Getting *that* wrong is the
  corruption direction again -- the naive `is_owning_expr` reading undoes a
  manual `[e retain]` and frees `[u init]`'s receiver twice, and one Rust
  test on the emitted C is the whole of what catches it, because an
  over-release is invisible to a dealloc counter (a refcount already at zero
  returns early) and to a slot count (the host slab clamps `num_used`). A
  **receiver** is the fourth and last of these positions, and the first where
  the discard question is necessary but *not sufficient*
  (`receiver_owning_value`, #340). `[[Foo alloc] poke];` abandons a `+1` that
  no name and no discarded value reaches, so nothing released it; but
  `[[Foo alloc] init];` hands that same reference out through its return
  value, where #322's arm already releases it, so releasing the receiver as
  well is the second free. The selector therefore has to be consulted
  alongside the receiver, and only the four selectors that consume or hand
  back the receiver's *own* reference -- `init...`, `retain`, `release`,
  `dealloc` -- are excluded. Being an *owning* selector is not the same
  thing: `-copy` and an analysis-derived factory return a fresh object, so
  the receiver's `+1` really is abandoned and both references are released.
  The sweep through these four positions is complete as a question of
  *which position* a `+1` sits in. What is left is a question of *where the
  statement sits*, and the answer differs by how often the enclosing code
  runs. A `for` header's **initialiser** runs once, so its operand's
  allocation is lifted above the loop and released after it -- the whole
  loop wrapped in a braced group, since a header cannot take a statement
  group the way #328's two positions do (#341). The **condition** and the
  **update** run per iteration, so hoisting either would allocate once
  where the source allocates every time round; both still leak,
  deliberately, and a Rust test on the emitted C is what keeps a later
  widening from sweeping them in. So does an owning operand of a plain
  **C** call, also deliberately, since a C callee cannot retain what it
  keeps.

  Worth stating as a rule rather than three cases: hoisting is correct
  exactly where the hoisted-from code runs once. That is the *opposite* of
  the constraint that shaped #328, which avoided `ctx.pre_stmts` because
  hoisting out of a loop **body** would run an allocation once instead of
  per iteration -- and it is why #341 is a separate arm rather than a
  widened guard on #328's.
- **An escape hatch is only real in the configuration it was kept for.**
  `CONFIG_OBJZ_DEBUG_LINES` was made `default y if DEBUG` rather than
  `depends on DEBUG` so that `y` stayed reachable by hand in a release build,
  on the argument that a field fault is exactly what has to be resolved
  against the `.m` (#358). The hatch could not answer that: resolving a
  shipped image's addresses in `.m` terms needs the directives to have been
  in the build that shipped, and a build that turns `CONFIG_DEBUG` on to get
  them is at `-Og` with asserts -- a different image. What the soft default
  did leave reachable was `y` in the one configuration where the directives
  are pure cost, 38% of the bytes of generated C nobody will step through
  (#395). So it is `depends on DEBUG` now, and the reasoning on both sides
  sits in the Kconfig help rather than only the half that survived.

  The general form, since a Kconfig default is cheap to argue about and hard
  to measure: **state which build the option is for, then check that build
  can actually reach it.** Both directions were measured here rather than
  reasoned about -- forced `=y` without `CONFIG_DEBUG` transpiled 20 `#line`
  directives before the change and zero after, where Kconfig now refuses the
  symbol with an unmet-dependency warning; `CONFIG_DEBUG=y` gets all 20 and
  runs. The image, as documented, moves the *other* way: 26816 bytes with
  directives against 26904 without, a `.m` name being shorter than a
  generated `.c` path.
- **Every public name the SDK exports carries the `OZ` prefix — protocols
  included.** The three protocols were `ObjectProtocol`, `IteratorProtocol` and
  `SingletonProtocol` until #401, which made them `OZObjectProtocol`,
  `OZIteratorProtocol` and `OZSingletonProtocol`. The headers did *not* match,
  which this document claimed they did until #413: all three were still spelled
  `OZ…+Protocol.h`, Objective-C's **category** syntax, for files containing only
  an `@protocol` block -- and two of them named classes that do not exist, so a
  reader grepping `OZIterator` found nothing but the protocol. They are
  `OZObjectProtocol.h`, `OZIteratorProtocol.h` and `OZSingletonProtocol.h` now.
  They
  were the one family a *user* writes into their own declarations
  (`@interface Foo: OZObject <OZSingletonProtocol>`), so they were also the one
  family that told a reader nothing about where it came from. A new protocol is
  named `OZ…Protocol` from the start; there is no deprecation path to fall back
  on, because `@compatibility_alias` covers classes and not protocols.

  What made the rename safe to do mechanically is that the fixtures read the
  real headers through `include_str!` rather than retyping them
  (`tests/common/mod.rs`, whose own doc says a hand-copied protocol of the same
  name would keep passing if the header were renamed). An incomplete rename is
  then a compile error, or the `assert!` inside `replace_line_containing` when
  the `OZObject.h` splice marker stops matching — not a green run against a
  fixture with no protocol in it.

  One site was neither: `companion.rs`'s `SINGLETON_PROTOCOL` string, compared
  through `class_conforms_to` to set `_meta.immortal`. A missed literal there
  makes the comparison return false and singletons mortal, with no diagnostic
  at all — the failure mode a rename has that a rename is not supposed to have.
  **When a rename touches a name the compiler compares as data rather than
  resolves as an identifier, find the comparison and prove a stale one fails**;
  three tests in `behavior_immortal_literals.rs` do, which was established by
  restoring the old spelling and watching them go red rather than by assuming.
- **`get` on a selector means it writes through a caller's pointer.** Objective-C
  reserves the prefix for that shape -- `-getBytes:length:range:` -- so
  `-getDescription:maxLength:` carries it and `OZHeap`'s `-usedBytes`, which
  returns a value, does not. `OZString`'s `-cString` keeps its own spelling for
  the same reason: it hands back a pointer to storage that already exists, so
  nothing is written and `get` would be a lie. The asymmetry is the rule working,
  not an oversight, and it is stated in `OZObject.h` beside the declaration
  because a later reader would otherwise "fix" it (#413).
- **`OZNumber` is an embedded fixed-point type, not an `NSNumber` clone.**
  Widths are spelled and *sized*: `+numberWithUnsignedInt8:`, never Cocoa's
  `+numberWithUnsignedChar:`. What a caller needs from this class is the width,
  and `@compatibility_alias NSNumber` is a convenience rather than a contract, so
  fidelity to Cocoa's C-type names loses to explicitness every time (#413). The
  two platform-width forms it keeps are documented as the exception they are.
- **A selector compared as data needs a test that goes red when it drifts.**
  `docs/STATUS.md` already said a rename touching such a name must have one
  (see the `SINGLETON_PROTOCOL` case above), and #413 found the rule stated but
  unenforced for `model.rs`'s `ALWAYS_DYNAMIC`. That one is worse than
  `SINGLETON_PROTOCOL` because it fails **conditionally**: `is_protocol_selector`
  is consulted first, so every program whose root adopts `OZObjectProtocol` --
  which is every fixture in the suite -- still gets its dispatcher, and only a
  minimal hand-rolled root silently loses one. Restoring the old spelling left
  all 568 other tests green. The test that closes it
  (`behavior_dispatch::description_dispatches_dynamically_without_a_protocol_declaring_it`)
  needs both conditions at once: a root adopting nothing, and exactly one
  implementor, so neither the protocol arm nor the multi-implementor arm can
  answer for it. **When a literal is unprotected, the shape that reaches it is
  usually the one no fixture builds.**
- **The create-rule selectors are one constant, not two lists.**
  `arc::is_owning_selector` and `arc::creates_reference` each carried their own
  copy until #413, and the cost of that is already recorded in `arc.rs`:
  `+allocWithHeap:` missing from one made `samples/heap_alloc` leak every object
  it allocated, with no diagnostic, because ARC fails toward leaking. Adding a
  selector to one and not the other is the same defect waiting to happen, so both
  now read `CREATE_RULE_SELECTORS`.

  A regression test for an owning selector has to bind the allocation **bare**.
  `[[Widget dynamicAlloc] init]` cannot see the defect: the outer `-init` is
  itself an owning selector, so the binding is `+1` whatever the receiver's
  provenance was. Written the natural way first, the #413 test passed with the
  selector removed from the list -- vacuously. `Widget *w = [Widget dynamicAlloc];`
  is what makes it fail.
- **Every `CONFIG_OBJZ_*` a compiled source reads must be declared in
  `Kconfig`, and an `#ifndef` fallback beside it is what makes a missing one
  invisible.** `src/OZLog.c` carried
  `#ifndef CONFIG_OBJZ_LOG_BUFFER_SIZE / #define ... 128`, sized its stack
  buffer from it, and `OZLog.h` documented the option to users; `Kconfig`
  declared it nowhere (#420). Three things about that shape are worth keeping.

  The fallback is not a safety net, it is the concealment: it makes the code
  *read* as configurable, so a maintainer greps the source, finds the symbol
  used and defaulted, and never asks the separate question of whether
  `Kconfig` declares it. A plainly hardcoded `128` would have been honest.

  What the user gets is not the warning the option's absence sounds like.
  Zephyr's `kconfig.cmake` turns Kconfig warnings into `error: Aborting due
  to Kconfig warnings`, so `CONFIG_OBJZ_LOG_BUFFER_SIZE=256` in a `prj.conf`
  fails the *configure* step with "attempt to assign the value '256' to the
  undefined symbol OBJZ_LOG_BUFFER_SIZE" -- a hard failure naming the user's
  spelling and not the documentation that was wrong. A documented-undeclared
  option is therefore a broken build for anyone who believes the
  documentation, not a silently ignored setting. The same machinery makes a
  `range` a gate rather than advice: `=2048` fails with "user value 2048 ...
  outside the active range ([32, 1024])".

  The enforcement is structural rather than a test. Dropping the `#ifndef`
  leaves the read bare, and `src/OZLog.c` is added to a build only by
  `oz2c.cmake` under `CONFIG_OBJZ` while the option sits inside
  `if OBJZ`, so the symbol exists wherever the file does -- and removing the
  `Kconfig` entry now stops the build at
  `src/OZLog.c:33:18: error: 'CONFIG_OBJZ_LOG_BUFFER_SIZE' undeclared`, which was
  verified by removing it. For that one symbol there is no test to decay,
  because the property is that nothing supplies a default but `Kconfig`.

  The rule as a whole is enforced rather than remembered:
  `tests/kconfig_options_declared.rs` fails on any `CONFIG_OBJZ_*` that
  `src/`, `include/` or the three CMake files name without `Kconfig`
  declaring it. Two of its choices carry the lesson. Comments in `src/` and
  `include/` are **not** stripped, because a header comment promising an
  option to users is a promise and that was half of #420 -- the option was
  documented in `OZLog.h`'s Doxygen block; with the entry removed the test
  names both `OZLog.c` and `OZLog.h`. Comments in the CMake files *are*
  stripped, because those name retired symbols on purpose to record the
  retirement, and requiring them to exist would be requiring it undone.

  The sweep that followed found no second instance, which is the useful half
  of the answer. Thirty-four distinct `CONFIG_OBJZ*` spellings appear in the
  tree against the eight `Kconfig` declares, so twenty-six are undeclared --
  and twenty-three of those occur only under `src/runtime_legacy/`,
  `include/runtime_legacy/` and `tests/objc-reference/`, which no
  `CMakeLists.txt`, cmake module or justfile recipe reaches, making them
  references in dead code rather than reads. The three with a site outside
  those trees are all prose recording a retirement: `CONFIG_OBJZ_BACKEND` in
  `oz2c.cmake` and `CLAUDE.md` on the dispatcher that went with the
  Python backend, `CONFIG_OBJZ_BACKEND_PYTHON` in `Kconfig`'s own comment
  saying it no longer exists, `CONFIG_OBJZ_FLAT_DISPATCH` inside README's
  "Legacy Runtime Reference" block -- and each of them again in this paragraph,
  which is why a sweep run after reading this will keep counting three.
  `CONFIG_OBJZ_LOG_BUFFER_SIZE` was the only one a compiled source read. The
  reverse direction found one:
  `CONFIG_OBJZ_BACKEND_STATIC` is declared, `default y`, and read by nothing
  at all -- `CMakeLists.txt` gates on `CONFIG_OBJZ`, `oz2c.cmake` never
  tests it, and `main.rs:5`'s comment claiming it is "wired into CMake by
  cmake/oz2c.cmake" is the opposite of what that file does.

- **`__objc_` is retired as a prefix, and a leading double underscore is never
  ours to spell.** C reserves it to the implementation, so every name under it
  was undefined behaviour waiting for a toolchain to claim the spelling.
  Synthesized and internal names are `oz_` (C-side, companion-wide) or `_oz_`
  (per-class). There were three layers until #462: the companion's own names
  carried `oz_static_`, the transpiler's crate name at the time, on the
  reasoning that a
  name the tool synthesizes is distinguishable from one a human writes. That
  is now retired -- `oz2c` names the *tool*, and only files it produces carry
  it (`oz2c_dispatch.h`, `oz2c_generated/`). Code gets `oz_`/`OZ_`.
  `__objc_refcount_get` was the last survivor in the live tree and
  went in #418; the prefix remains only in `runtime_legacy/`, which is not
  compiled, and in one `#define` bridge in `tests/tools/oz2c_build.py` that
  exists so behaviour drivers written against the retired Python pipeline's ABI
  stay unmodified. Neither is a precedent. `CLAUDE.md` had documented the
  opposite -- "Internal functions: `__objc_` prefix" -- which #418 made outright
  false, so the enumeration a rename needs runs over the *documentation* as well
  as the code.
- **One concept gets one public name, and a second name for it is usually a
  signature problem wearing a naming problem's clothes.** `__objc_refcount_get`
  and the refcount reader did the same thing, and the reason there were two
  is the part worth keeping. (That reader was spelled `oz_retain_count`
  when #418 settled this, and is `oz_retain_count` since #462 retired the
  `oz_static_` prefix; #418's own commits and diffs read the older name
  throughout.) It took `struct <root> *`, and
  `include/oz_sdk/Foundation/OZObject.h` has to declare whatever Objective-C
  source calls -- Clang resolves the call while dumping the AST, before any
  generated header exists -- while being unable to name a generated struct. So
  the second name existed purely to have an `id`-typed parameter. Collapsing the
  two meant changing the *signature*, not deleting a line: the companion emits
  `int oz_retain_count(id obj)` now, `id` is `void *` in generated C, and
  every internal caller keeps passing a root-struct pointer and converts
  implicitly.

  The invariant that falls out, and it has no local test of its own outside
  #418's: **a declaration the SDK header and the companion both carry must
  agree exactly.** The SDK headers are *spliced into* generated C, so the two
  land in one translation unit -- identical, they are redundant and legal;
  differing in one parameter type, they are a conflicting declaration and
  nothing in the program compiles. That is a whole-program failure from a
  one-word edit, and it is invisible to any fixture that does not splice the
  header in question.
- **`get` on a selector means it writes through a caller's pointer, and that
  rule reaches plain C functions too.** The selector half is recorded above
  (#413). `__objc_refcount_get` was the SDK's last `get`-prefixed *reader*, and
  retiring it is what made the rule true of every name the SDK exports rather
  than of selectors alone (#418).
- **"How many?" and "any at all?" are two questions, and a floor is where the
  second one goes to hide.** `PoolSizes::for_class` ended `.unwrap_or(0).max(1)`
  because `K_MEM_SLAB_DEFINE(..., 0, ...)` is not a usable slab -- so the size
  question had no way to say "none", and every program reserved a `k_mem_slab`
  plus one instance of static storage for every Foundation class it never
  allocates. The fix was not a better count but a second question,
  `ever_slab_allocated`, which the emitters read the way `item_slots` of zero
  has always been read: emit nothing (#419).

  **Key it on site *presence*, never on the count.** `Scan::resolve` gives a
  site inside an uncalled *class-method* body a multiplicity of zero, and that
  is correct -- it is what stopped every program sizing `OZNumber` at 16 for
  seventeen uncalled factories. So `counted` is 0 for a class whose
  `[X alloc]` is right there in the source, and keying the elision on `counted`
  drops that class's slab while leaving its emitted, externally callable
  factory allocating from a slab that does not exist. The two readings agree on
  every program in the corpus and disagree on exactly that one shape, which is
  why `a_site_in_an_uncalled_class_method_still_gets_a_slab` exists.

  What the floor was really covering for, found by removing it: **a
  hand-written C caller the transpiler cannot see.** Eight behaviour cases
  broke, all of them Unity drivers calling `X_alloc()` for a class their `.m`
  never allocates. The floor paid for that everywhere to serve those eight, and
  the right place is the harness, which *can* see the driver --
  `tests/tools/compile_and_run.py` now scans the `_test.c` and passes
  `--pool-sizes`, which is what that flag is for. Worth noting how the eight
  presented: the emitted trap named the class, the selector and the flag that
  fixes it, and the diagnosis took one line of output. That is the whole case
  for a named trap over a returned nil.

  Two of the eight were a separate defect the floor had been hiding since long
  before: the harness read `_parse_pool_sizes(m) or _default_pool_sizes(m)`, so
  a case carrying `/* oz-pool: OZObject=1 */` -- meaning "size OZObject at one"
  -- silently dropped the default for every *other* class in the file. It is
  merged now, directive winning per class.

  Measured, `samples/heap_alloc` on `mps2/an385`: 11 slabs to 4, 320 bytes of
  RAM back (124 BSS in block buffers, 196 `.data` in seven 28-byte
  `struct k_mem_slab` control blocks) and 88 bytes of flash. The `.data` half
  is the larger one and is easy to miss by measuring `bss` alone.
- **An allocator that can fail must name the failure, and one switch must cover
  every allocator that can.** The slab path has had
  `OZ_TRAP_POOL_EXHAUSTION` since the pools work; the heap path had a
  bare `return (struct {name} *)0;`, so heap exhaustion travelled exactly the
  way slab exhaustion used to -- `EXC_BAD_ACCESS` inside a function with
  nothing to do with the cause. #419 gave it the same trap, under the same
  macro, with two arms so the message can say *which* heap ran out.

  **It is opt-in, and the argument for default-on was the interesting half.**
  Heap exhaustion is a runtime condition rather than a sizing mistake, so
  unlike a pool there is no number to go and fix, which is a real reason to
  treat the two differently. Three reasons it still loses. Nil-on-failure is
  *one* contract across both allocators: a build that wants to keep it has to
  keep it on both, and that is also the only way the failure path stays
  testable -- `alloc_failure_enomem` tests the slab's, and a default-on heap
  trap would leave the heap's untestable. `[Cls dynamicAlloc]` behaving
  differently from `[Cls alloc]` on the identical failure is two spellings of
  one concept disagreeing, which is the defect #418 was about. And the
  complaint is that the failure is *unnamed*, not that it is survivable;
  naming it is the fix, a second policy is not.

  The macro's name now covers a heap as well as a pool, which is a small lie in
  a name accepted deliberately: renaming it breaks every build already passing
  it, and a `-D` flag is the one interface here with no deprecation path.

- **File scope in an SDK `.m` is the Foundation's shared namespace, not that
  file's.** Text ahead of an `@implementation` is spliced into the *generated
  header* for that origin, and every Foundation translation unit includes those
  headers -- so anything defined there is defined for all of them. Two
  consequences, both learned the expensive way. A function has to be
  `static inline` rather than `static`: `_oz_write_default_description` as plain
  `static` was unused in all but one of the units it landed in, Zephyr builds
  with `-Werror`, and it failed `-Wunused-function` on every sample whose
  generated set includes a file that does not call it -- which the two
  single-purpose samples it was first tried on happened not to be (#354). And a
  macro has to be project-namespaced: `src/OZMutableString.m` carried
  `#define NULL ((void *)0)`, which redefined a standard library macro for the
  whole Foundation (#422).
  That second one is the harder shape to catch, because **nothing it can be run
  through reports it.** It was `#ifndef`-guarded and its replacement text was
  token-identical to libc's, so it never expanded, never warned, and changed no
  byte of any output -- reading the generated header, compiling it, running it,
  and `just test-pedantic` (the gate a real redefinition *would* trip) were all
  green with it in place. Only the text says it, so
  `tests/sdk_spliced_file_scope.rs` reads the text: a `#define` in a spliced
  prelude must start `OZ`, which admits the `OZ_Q31_HELPERS` idempotency
  guard around `OZNumber.m`'s `static inline` helpers and rejects anything owned
  by someone else, and no SDK `.m` may define a standard library macro anywhere.
  The rule read `OZ`/`_OZ` until #417. The one guard spelled `_OZ_` was the only
  user of that alternative, and a leading underscore followed by an uppercase
  letter is reserved to the implementation in C -- the same undefined behaviour
  the retired `__objc_` prefix was removed for (#418). Renaming the guard left the
  alternative with no users, so it went with it rather than sitting there inviting
  the next one.
  Keep the guard's own first draft in mind when writing another like it: it
  found the prelude with `src.find("@implementation")`, which matched the word
  inside the comment explaining the splice and cut the prelude off one line
  above the `#define` it was written to catch. It reported clean on the exact
  input it was built against. **A text guard is not evidence until it has been
  made to fail.**
- **The version is `tools/oz2c/Cargo.toml`**, and a PR **declares the kind of
  bump without carrying the number** -- `fix(oz2c):` a patch, `feat(oz2c):` or
  a `!` break a minor while pre-1.0. The number is applied just before merge,
  as the final commit of whichever PR is next to land (#499, and
  `docs/WORKING.md` is authoritative). The repo-level `VERSION` file is
  retired.

  This line said "bumped in the same commit as the change it describes" until
  #539, which is what #499 *reversed* -- and the reasoning is worth keeping
  rather than just the rule. Carrying the number is asserting a value the
  branch cannot know: the repo rebase-merges, so the number is a function of
  the kind **and the merge order**, and 8 of the 9 recorded version incidents
  were *created* by a branch carrying one. 12 of 50 commits in a single day
  touched this file, one per PR, so every merge forced a rebase on every other
  open PR.

  The new rule opened one failure of its own, and `.github/version-omission.sh`
  is the gate for exactly it: the number became a separate, final, skippable
  commit, and it was skipped six times in about a day. It was skipped twice
  more during the #527-#542 batch -- #544 and #546 both merged without theirs,
  and the next PR's bump absorbed the debt each time, which is the convention
  working rather than failing.

- **Two casts with identical text, answering different questions -- do not
  merge them (#532).** `render_return_statement` now casts a returned value to
  the method's declared return type when the value's static class is a strict
  descendant of it, because inheritance here is struct embedding and C has no
  implicit subclass-to-base pointer conversion. The emitted text is
  `return (struct OZString *)(built);`.

  `send_to_resolved_class` and `render_message` emit a cast that reads
  *exactly the same* and is not the same rule: theirs is keyed on
  `MethodSig::returns_instancetype` and is covariance on `instancetype` at the
  **call** site (OZ-003). Neither can answer the other's question -- one asks
  "is this class a descendant of the declared return type", the other "did the
  callee declare `instancetype`, and which class did the send resolve to". A
  reader who notices the duplicated spelling and factors them together will
  produce something that is wrong for one caller in a way no test names,
  which is why this is written down rather than left to be rediscovered.

  Two things about the shape that are easy to get backwards:

  - **Strict descent, in one direction only.** A value already of the declared
    type needs no cast, and `Program::is_descendant_of` excludes the class
    itself, so asking it *is* the whole test. The opposite direction -- a base
    pointer returned where a subclass is declared -- is a narrowing the author
    has to write, and casting it silently would suppress a diagnostic instead
    of emitting one.
  - **There are two exits and both emitted it uncast.** The plain `return` is
    rebuilt in place; a `return` with an ARC release owed in the same scope
    goes out through the cleanup path, which declares `{ret_ty} {tmp} =
    {value};` and so produced the *same* defect as an initialiser rather than
    as a return. Disabling the fix makes the two tests fail with two different
    Clang diagnostics -- `returning ... from a function with result type` and
    `incompatible pointer types initializing` -- and that difference is the
    evidence they are genuinely separate paths rather than one path reached
    twice.

  The blast radius is nil on code that was already correct: 121 of 121 corpus
  cases transpile byte-identically before and after, and
  `tests/zephyr/generated/` does not move. That number means something only
  because the same harness, given an upcast return, *does* report a
  difference -- a sweep that cannot see the change it is measuring reports
  agreement about nothing (#424, #433).
## A mutator's first question is its receiver's state (#542)

`_oz_alloc` memsets a fresh slab slot, and **nothing in this tree requires an
initialiser before a mutator** -- no `staticbar` refusal, no generated guard, no
gate. So for every Foundation class, `[Klass alloc]` followed directly by a
mutating send is a *reachable* shape, and the mutator runs against ivars that
are all zero. That is a standing rule about how SDK methods must be written,
not a note about one class: **a mutator that reads its own ivars has to be
correct for the all-zero receiver, because it can be given one.**

`src/OZMutableString.m` was not, in three ways at once, which is why they were
filed and fixed together:

- **A capacity-doubling loop seeded at `_capacity`.** Both mutators wrote
  `size_t newCap = _capacity; while (newCap < needed) { newCap = newCap * 2; }`.
  At `_capacity == 0` the body is `0 * 2 == 0`, so the condition can never go
  false. Worth naming the failure mode precisely: **an unbounded hang, not a
  crash.** On target there is no allocator to fail and no fault to trap, so the
  thread simply stops making progress -- which is strictly worse than a crash,
  because a crash names its cause. Both sites now floor the seed at 16, the
  minimum every initialiser in the file allocates, so a floored grow lands on
  the capacity an initialised instance would have had.
- **`memcpy(dst, NULL, 0)`.** `-appendCString:`'s grow path copied the old
  contents with `memcpy(newBuf, _data, _length)`, and on the zero-capacity path
  `_data` is NULL and `_length` is 0. ISO C requires the pointer arguments to
  every `<string.h>` function to be valid *even at a length of zero*, so this
  is undefined however reliably real implementations tolerate it. Guarded.
  `free(NULL)` on the next line is defined and needs no guard -- the asymmetry
  is real and worth not "tidying" into consistency.
- **A guard that checked the argument and not the receiver.** `-setString:`'s
  nil branch did `((char *)_data)[0] = '\0'`, a write through NULL. The obvious
  reading of that defect -- "no nil check" -- is wrong and sends a reader to
  the wrong line: the argument check was present and correct, and is the reason
  control is in that branch at all. **State which operand a missing check is
  missing on.** The fix records `_length = 0` and returns rather than
  allocating, because allocating in a `void` method reintroduces exactly the
  silent-degrade this file is already criticised for (below).

### The test has to construct the un-initialised receiver

A case that starts from `[[OZMutableString alloc] initWithCString:...]` cannot
reach any of the three, which is why eleven existing assertions over this class
covered none of them. The regression coverage is `[OZMutableString alloc]` with
no `-init`, and that shape does survive ARC: assigned to a local or to a strong
ivar it lives to the end of its scope, so the mutating send really does run
against the zeroed slot. (Reported here because the question -- whether
scope-based ARC releases a bare `+alloc` at end of full-expression -- is asked
every time someone tries to write this kind of test. It does not.)

**The pre-fix behaviour is a hang, so "the test fails without the fix" needed
arranging rather than assuming.** Two halves, and the difference between them
is the runner:

- `tests/behavior/cases/foundation/mutable_string_basic.m` needs nothing
  special: `tests/behavior/conftest.py` bounds the outer process at 60s and
  `tests/tools/compile_and_run.py` bounds the binary at 30s, so a regression
  is reported as a `TimeoutExpired` failure.
- `tools/oz2c/tests/behavior_foundation_mutable_string.rs` calls `alarm(3)` in
  the generated program before the risky sends, and that call is load-bearing.
  `common::compile_and_run` runs the built binary through `Command::output()`
  with **no timeout**, so a regression in either loop would wedge `cargo test`
  -- and with it the `rust-tests` gate -- instead of failing it. Under SIGALRM
  the process dies, `output()` returns, and the harness's `status.success()`
  assertion fires. Any future test in the Rust suite whose failure mode is
  non-termination needs the same treatment.

Each of the three fixes was reverted on its own, with the other two in place,
to check that no one of them carries the others: dropping the `-setString:`
floor times out at 30s in the corpus run, dropping the nil guard exits 245.

### Two things deliberately left alone

- **Both mutators still `return` silently when `malloc` fails**
  (`-appendCString:` and `-setString:`). That is a real violation of "this
  project never silently degrades", and it is #107's design guidance for a
  revived `OZMutableData` rather than part of this fix -- a `void` mutator has
  nowhere to report to, so fixing it is an API decision, not a patch.
- **The raw `malloc`/`free` in this file.** Allocation is supposed to route
  through the PAL, but that is policy: no document in this tree states it, no
  gate checks it, and live sites exist, this file among them. Converting it is
  a separate decision with its own blast radius.

## One discarded bit, two bugs, and a predicate that had to be named (#529, #530)

`collect::class_header` answered "what kind of `@interface` is this?" with
`Option<String>` -- the category's name, or `None`. Objective-C has **three**
shapes, and in tree-sitter-objc's grammar the category name is an *optional
field of one shared node*, so `@interface Foo ()` is an ordinary
`class_interface` whose only direct-child identifier is `Foo`:

| source | old answer | actual kind |
|---|---|---|
| `@interface Foo : Bar` | `None` | primary |
| `@interface Foo ()` | **`None`** | class extension |
| `@interface Foo (Name)` | `Some("Name")` | category |

The parenthesis tokens are the only evidence an extension is one, and the
function **computed exactly that** -- as a local `saw_paren` -- and then threw
it away. That single discarded bit is the root cause of both issues, which is
why they are one change: #529 is the *extension = primary* collision and #530
is the *category = class* collision, two arms of one three-way decision.

### The predicate is `may_declare_ivars`, and both obvious spellings are wrong

An extension **is** part of the class: it may declare ivars, and its properties
do get backing storage and a synthesized accessor. A category may declare
neither -- it has no storage of its own, and adding a field to the extended
class changes that class's layout behind the back of every translation unit
that includes its header. So:

- **Testing for parentheses strips storage from the extension**, which is the
  one parenthesised shape that owns its ivars.
- **Testing `category.is_some()` is right today and silently wrong tomorrow.**
  It worked only because an extension came back `None`; it would have inverted
  the moment the `Option` became a three-way enum, with no test to catch it.

`InterfaceKind::may_declare_ivars` exists as a *named method* for that reason,
rather than an inline `matches!` at each of the six call sites. `declares_class`
is the second such predicate: only the primary declaration brings a class into
existence, which is what lets pass 1 record a bare `@interface Foo ()` as a
*use* of `Foo`. Before this, an extension on an undeclared class reached the
#501 check as a primary interface and so **fabricated** the class -- a
`struct Ghost` with no superclass, a second root class, from source declaring
no such thing. The check could not see it because by the time it ran, the class
it was looking for existed, having been invented three hundred lines earlier.

### The clobber was worse than the duplicate, and the report had it backwards

#529 reports a second `struct` with a different layout. The more dangerous half
is one line in pass 2: `info.own_ivars = ivars;` -- an **assignment**, where the
`@implementation` arm fifty lines below has always appended. Invisible while an
`@interface` was a class's only ivar-declaring block, and a silent catastrophe
once an extension also reached it, because the extension *replaced* the
primary's ivar list. An extension declaring one ivar left the class owning only
that one. **An extension declaring none -- the common shape, adding only
private methods -- left the class owning nothing**, which loses every ARC
release on dealloc and drops every ivar out of method scope. Restoring the
clobber fails `extension_declaring_no_ivars_leaves_the_primary_ivars_intact`,
and that is the test to read first.

GCC caught the duplicate only because both structs landed in one header. Split
across two translation units it would have compiled clean and the two would
have disagreed about where each ivar lives.

### Two independent sites would each have materialised the field

`resolve_properties` records a property's backing ivar, and
`render_interface` synthesizes a field for any property whose ivar is not in
the node's own text. **Either one alone reintroduces #530**: disabling the guard
in either fails
`behavior_category::category_property_gets_no_backing_ivar_and_one_definition`.
The accessor `MethodSig`s are still synthesized in both cases -- a category
property genuinely *declares* its accessors, and dispatch needs those
signatures to route a send or a `.` access to the category's own definition.
Only the storage and the body that would read it go away, and `ivar_name` stays
populated so nothing downstream has to handle a second kind of `None`.

The block filter that was already there (`!is_category_impl`) is not enough,
and the gap between the two is the whole of #530: **it asks which block is
being rendered, and the question is which block the property came from.** A
category's properties merge into the extended class's `ClassInfo`, so by the
time the primary `@implementation` renders, its list holds the category's
alongside its own with nothing left to tell them apart. Hence `PropertyOrigin`
on the property itself.

Replacing that block's per-node `defined_here` with the program-wide
`ClassInfo::defined_selectors` fixed a third case in passing, in the other
direction: a property declared by the primary `@interface` whose accessor is
hand-written in a *category* block was getting a synthesized definition here as
well -- the same duplicate-symbol shape, and the right answer had been sitting
unused in the model all along.

### A fix that traded one link error for another, until it didn't

Removing the double *definition* leaves the case #530 says "deserves a
diagnostic": a category property whose accessors nothing defines. Without a new
check that is not "nothing happens" -- it is
`undefined reference to 'Sensor_diagnosticCode'`, which is the same complaint
both issues file under *diagnostic quality: none from oz2c*, at the far end of
the pipeline. `reject_undefined_category_accessors` is that check, located at
the `@property`.

Clang only *warns* here, and a warning is right for a runtime that can carry a
selector nothing implements: the send fails at runtime, on that object, if it is
ever made. The generated C has a symbol or it does not, so no such deferral is
available -- and there is no non-fatal diagnostic channel here to use even if
there were. `@dynamic`, the other half of Clang's advice, promises the accessor
arrives at runtime, and nothing here has a runtime.

### A merged assertion reversed, and the shape that made it look right

`behavior_category::category_property_synthesizes_accessors_once` asserted the
#530 behaviour was **correct**, and passed. It was wrong on both counts, and
the reason it looked right is worth keeping: **its category `@implementation`
was empty**, so there was no second definition to collide with -- the single
shape in which this defect presents as a feature. #530's own note is the one to
hold onto: had the names not collided, reads would have returned the dead
field. The linker error is what saved it.

Real Objective-C rejects that source too. It is now a located refusal, and the
one test is three: the supported shape runs, the structural claim is asserted
on the emitted text, and the old shape is refused.
## One omission, two issues, and the fix that would have been worse (#534, #535, #539)

`EmitCtx` carried no notion of **which side of the class** the body being
rendered belonged to. `render_method_definition` computed
`sig.is_class_method`, used it three times to build the signature, and threw
it away; so `render_expr`'s `self` arm answered `struct C *` and its `super`
arm answered `struct Super *` for every body in the program, class methods
included. Every class-side send off either then entered `render_message`'s
*instance* branch and asked `find_defining_class` for a `-` method.

That is the whole of #534 and #535. They read as two bugs -- one about
`[[self alloc] init]`, one about `[super familyDepth]` -- and they quote the
**same message**, `class 'X' has no method matching 'sel'`, which is what says
they are one. The tell was there in both reports and neither of us read it.

### The diagnosis in the issue was wrong, and the wrong diagnosis was the expensive part

#535 is titled *"a class-method super send is looked up only in the immediate
superclass, while instance super walks the chain"*, and it explains itself with
a 5-level chain and a note that a one-level chain "never reaches it". Both
halves are false. `find_defining_class` walks the whole chain and always did;
what it filters on is `is_class_method`, so a class-side `super` arriving as an
instance lookup missed at **every** link. `class_side_resolution.rs`'s
`class_method_super_reaches_the_immediate_parent` is that correction pinned
down: `Base` declares and implements `+depth`, `Sub` overrides it with
`[super depth] + 1`, and before this change it failed with
`class 'Base' has no method matching 'depth'` -- the immediate parent, which
the issue's own severity note says is the case that works.

Believing the title would have bought a walk-the-chain patch in
`find_defining_class`, which already walks the chain; the test proving it
would have been the 5-level fixture, which fails for the real reason and
would have gone green for the wrong one. **The issue said "stops at one
level" because one level is where the reporter stopped testing.** A fixture
at the boundary a report declares safe is cheap, and it was the whole
diagnosis here.

### The fix that transpiles, links, and returns nil

`pools::alloc_receiver_class` requires the receiver **text** to be a literal
class name. `self` is not one, so `[self alloc]` counted no site -- and since
#419 that is not merely a slot count. `slab_sites` never learns the class,
`ever_slab_allocated` answers no, `for_class` returns 0, and the emitters read
0 as *emit no `k_mem_slab` at all*.

So fixing `self`-as-class in `emit.rs` alone turns #534's **hard located
error** into a factory that transpiles clean, links clean, and hands back
**nil on its first call**. Strictly worse than the bug it fixes, and invisible
to every gate that reads generated C rather than running it -- which is most
of them, and was the whole set this change would otherwise have been reviewed
against.

Two resolvers, one receiver, disagreeing silently (#481) is the same shape and
has its own section above. The lesson that section did not carry, and this one
adds: **when the emitter learns to lower a new spelling, the passes that
*size* what it lowers have to learn the same spelling in the same commit, or
the new capability arrives with its allocation missing.** `emit.rs` and
`pools.rs` now read one predicate, `emit::new_is_synthesized`, rather than two
copies of the same comparison -- because the two answering differently is
precisely the nil above.

### Why `+new` could not be written in Objective-C

#539 asks for `+new` on `OZObject`, and observes that its natural body is
`[[self alloc] init]`, so #534 has to land first. It does -- and the natural
body still does not work, for a reason unrelated to #534.

**A generated class method takes no receiver parameter.** `[Sub inherited]`
and `[Base inherited]` compile to the identical C call, with nothing passed
that could tell them apart. So `+ (instancetype)new { return [[self alloc]
init]; }` in `src/OZObject.m` renders **once**, with `self` fixed at
`OZObject`: `[Gadget new]` becomes `(struct Gadget *)(OZObject_new_cls())`, a
`Gadget *` pointing into an `OZObject`-sized slab slot, and `-init` writes
past the end of it. That is `samples/heap_alloc`'s failure under
`+dynamicAllocWithHeap:`, recorded in `render_message` since #413.

`+new` is therefore declared in `include/oz_sdk/Foundation/OZObject.h` with
**no body**, and resolved at the send site to the receiver's own allocator
plus the receiver's own `-init` -- which is how `+alloc`, `+dynamicAlloc` and
`+class` already work, and why all four are special cases in `render_message`
rather than methods. `[Gadget new]` runs `Gadget`'s `-init` out of `Gadget`'s
slab; a class declaring its own `+new` keeps its own body, which
`tests/behavior/cases/arc/owning_argument.m` has depended on (a `+new`
returning a bare `[Thing alloc]`, no `-init`) since long before `+new` was
inheritable.

### What `self` on the class side is, and is not

It resolves to `ctx.class_name`: the class whose `@implementation` **lexically
encloses** the send, not the dynamic receiver. There is no receiver to ask.
So the inheritable factory #534 wants -- `[[self alloc] init]` on `Base`,
called as `[Sub factory]`, yielding a `Sub` -- **still does not inherit**;
it allocates a `Base`. What #534 buys is that the canonical spelling
compiles, and that WA-009's hand-written class name is no longer required to
say the same thing. The remaining gap is a property of the static subset, not
of this fix, and it is the same gap `+new` had to be resolved at the send site
to avoid.

`self` is consequently only meaningful as a **receiver** in a `+` method. As a
value it has no representation at all: `Class` is the `class_id` integer, no
class object exists, and the identifier renders to a bare class name.
`return self;` in a class method was already broken before this change --
it emitted a reference to a `self` parameter that class methods do not have --
and afterwards it would have emitted `return Sensor;`, which is not C. Either
way the failure landed on the C compiler with no Objective-C line attached, so
`reject_self_as_value_in_class_method` makes it a located refusal that says
what `self` means on the class side.

### A doc comment changed the generated output (#539)

Found by measuring #539's blast radius, and it is the reason that
measurement is not a formality. The `+new` declaration added to
`include/oz_sdk/Foundation/OZObject.h` came with a doc comment explaining why
it has no body, and the comment used a concrete example class -- `[Gadget
new]`. Four of the 121 corpus cases then gained an `#include` that nothing in
them needed:

```c
/* Foundation/OZObject.c, the SDK root class's own translation unit */
#include "oz2c_dispatch.h"
#include "OZObject.h"
#include "init_sets_fields.h"   /* <- the *user program's* header */
```

`emit`'s `body_includes` is built with `mentions_identifier` over each
origin's **source text**, and source text includes comments. All four
affected cases declare a class called `Gadget`, so the SDK's `OZObject`
origin "mentioned" a class owned by the program's origin and pulled in its
header. Nothing in `OZObject.c`'s body changed -- the include was
unnecessary as well as unintended.

Three things worth taking from it:

- **The effect is harmless and the direction is not.** The header is
  `#pragma once`-guarded and nothing used it, so it compiled and ran
  everywhere. But it is the SDK's translation unit including the
  application's, which is the dependency arrow inverted, and it appears or
  not depending on what the *consumer* happens to name a class.
- **Any capitalised word in an SDK doc comment is a potential class name.**
  The comment now describes the shape without naming a class, and says why
  in the comment itself -- which is the only place a later editor will look
  before adding an example back.
- **The explanation caused the problem it explained.** The first attempt at
  that note said "an earlier draft said `[Gadget new]`", which put the
  identifier straight back into the text and left the includes exactly where
  they were. When a check's subject is text you are also writing *about*,
  your own prose is in the corpus.

The measurement that caught it nearly did not. The first sweep built the old
and new *binaries* and ran both against the current tree -- so both arms saw
the new `OZObject.h`, the declaration was present in each, and the diff came
back 121 of 121 identical. That is a clean bill of health from a sweep that
had held one of the two variables fixed. This change moves a spliced SDK
header as well as the transpiler, and the arms have to differ in both or the
number means nothing (#400, #424, #433 are the same lesson three times over).
With both varied, every one of the 1201 differing lines is either the `+new`
declaration or a line-number rename, and *that* is the reviewable claim.

## Fourteen walks, one invariant, and the hang that had no stderr (#547)

`@interface A : A` is a one-character typo. Before this it turned `west build`
into a memory-exhaustion event: oz2c grew at roughly 500 MB/s and was killed by
the supervisor with an **empty stderr** -- no file, no line, no construct
named. Measured on `main`: 1.2 GB for the self-reference and 4.0 GB for the
mutual pair, both still climbing when an 8-second timeout cut them off.

**The count is the finding.** `grep -c 'superclass.clone()'` over `src/` gives
**fifteen** ancestry walks, every one shaped `while let Some(name) = cur { ...
cur = info.superclass.clone(); }`, and exactly **one** carries a visited set:
`companion.rs`'s `visit`. The other fourteen run forever on a cycle. Note
`class_conforms_to` looks guarded and is not -- its `seen` set guards the
*protocol* walk nested inside it, not the superclass walk below; a heuristic
that matched "a visited set somewhere in this function" scored it as safe and
was wrong. Read the loop, not the function.

Which of the fourteen you land in is an accident of program shape. A sampling
profiler named it: `Program::owned_object_ivar_names`, pushing a `String` per
step into a `Vec` that is never read, which is why the symptom is RSS rather
than a spin.

### The fix is one check, not fourteen guards

`lib.rs` gates the pipeline on `collect`'s diagnostics *before* arc, generics,
pools or emit run. So rejecting the cycle in `collect` makes every later walk
acyclic **by invariant**, which is strictly stronger than fourteen independent
visited sets: a fifteenth walk added next year inherits it, and no author has
to remember. It sits beside the check that already stops an *unresolved*
superclass reaching those same walks -- the two are the same guarantee about
the same field, one for a missing key and one for a cyclic one.

One diagnostic per *cycle*, not per class in it. A two-class cycle reported
from both ends is one defect twice: fixing either class's superclass fixes
both, and the second message sends the reader looking for a second problem.

### What this does not fix, against the issue that assumed it would

#554 predicted that "whoever fixes #547's cycle (a visited-set in this same
walk) can linearize it in the same pass, since a cycle-safe walk that still
re-walks from the root keeps the quadratic behaviour." That reasoning is sound
and its premise does not hold here: **this fix never enters the walks.** It
rejects at `collect`, so the walks are byte-for-byte unchanged and still
O(depth squared). Measured after the fix, `emit`/depth² is flat at ~8.7e-7
across depths 200, 800 and 1500 -- the same trend #554 reported. So #554 is now
*more* independent than when it was filed, not less, and the two should not be
bundled on the strength of that sentence.

### A regression test whose failure mode is a hang

The three cycle cases do not fail without the fix, they **hang**: confirmed by
disabling the check, at which point `mutual_superclass_cycle_rejected_once` ran
past 60 seconds and was SIGKILLed rather than returning a failure. That makes
them the same class as #542's grow loops, and the same caution applies -- a
timeout in `static_bar_rejects` means this check, not a slow machine.

The accepting control is the one that earns its place: a deep *acyclic* chain
must still transpile. A visited set that rejected a revisited **name** rather
than a name revisited **on the current path** would pass all three cycle tests
and break every real program with a shared ancestor.

**And a note on how the counterfactual was run, because it went wrong.** The
save, the sabotage, the test and the restore were one unguarded shell command.
The restore did not take, and the next two commands ran against sabotaged
source while being reported as a clean tree. Nothing was lost, and the result
happened to be the right one, but the lesson is the same as every other entry
here: a restore is a claim, and it needs an explicit absence check --
`grep -c 'if true ||'` returning 0 -- before anything downstream is believed.

## The move of a strong lvalue, and what mimicking ARC actually costs (#527)

`Reading *taken = _slots[i]; _slots[i] = nil; return taken;` -- the idiomatic
queue pop -- handed the caller a **freed block**. The load retained nothing, so
the store's release was the last one on the reference. With
`CONFIG_OBJZ_DEBUG_REFCOUNT=y` the caller read `0xA5A5A5A5`; with the
instruments off it read a plausible `11`, which is why it survived a passing
on-target run.

`docs/ARC.md` 2.5.5 had it as `UNEXAMINED` -- "no construct in the accepted
subset moves a slot". **That row is the lesson, not the fix.** It was true of
the *subset* and false of what the transpiler *accepted*: the construct
compiled, ran, and miscompiled. An `UNEXAMINED` label describes the analysis
and says nothing about the third outcome, and the third outcome is the
dangerous one.

### The answer is Clang's, and Clang's optimizer is the part that does not come

Measured from Clang's own AST: a `__strong` local initialised from a `__strong`
lvalue is `cinit destroyed` -- it retains, and the scope releases -- and the
return is `ARCProduceObject`, so the caller receives a retained reference.
**The load retains.** That is the whole of it.

What cannot be copied is how Clang affords it. Clang emits the pair at every
binding and deletes the redundant ones in `ObjCARCOpt`; there is no such pass
here, and nothing downstream elides an `oz_retain`/`oz_release` pair because
they are ordinary C functions over an atomic counter whose decrement gates
`-dealloc` (see "Why this is not Clang's ARC"). So retaining at every bind
from a strong lvalue would charge **six live sites** permanently --
`OZArray`'s element access and enumeration, `OZDictionary`'s key and value
access -- every one a borrow whose slot is never overwritten, and all of them
the SDK's hottest paths.

**Keyed on the store instead**, which is exactly the condition under which the
elision is unsound: the local retains only when the same body also writes the
slot it read. All six borrows pay nothing. Measured cost where it does apply,
arm-zephyr-eabi-gcc 14.3.0, cortex-m3: **9 -> 11 instructions at `-Os`, 10 ->
11 at `-O2`** -- and zero live sites in the corpora, the samples or `src/`
reach it, so the present cost of this change is nothing at all.

### Two halves, one predicate, and the leak that proved it

The retain is half the fix. The other half is that the reference now belongs to
the local, so either the scope releases it or a `return` hands it on -- and
`arc::return_hands_back_ownership` has to agree, because it is what tells a
*caller* to release.

Getting one half first produced both failure modes, in order:

- **Managed set only:** the scope-exit release was emitted with no retain to
  balance it, turning one use-after-free into a **double free**. The invariant
  it violated is written down at `for_header_owned_declaration`: "a borrowed
  initialiser is left alone -- releasing one is a use-after-free on whatever
  still names the object."
- **Both emit halves, `arc.rs` untouched:** emit retained and suppressed the
  escaping local's release, while `arc` still reported the method `+0`, so no
  caller released and the object **leaked**. That is #351's disagreement
  exactly, one direction over -- there, emit released an owner this same
  function called borrowed.

So the two read one predicate rather than agreeing by assertion, which is the
same resolution #351 reached: `emit::moved_slot_locals` decides the retain and
`arc::return_hands_back_ownership` asks the same question for the caller.

### What this is not

Not keyed on the selector's family. Ownership here is computed from the
**body**, so `-takeIndex:` hands back `+1` and its callers release without any
create-rule name -- verified directly: `-buildOne`, no family prefix, already
gets an `oz_release` in its caller. Clang ties `+1` returns to method families
because its pool-backed convention needs the name; this does not, and the
create-rule gating an earlier reading of #527 assumed turned out to be
unnecessary.

Not a change to any store. Suppressing the store's release instead -- the other
way to balance this -- was considered and rejected: it would make two
store-lowering switches conditional on a non-local property in the most
heavily litigated path in the transpiler (#405 made a strong ivar store release
before evaluating, #423 narrowed the arm, five test files pin it), and it
declines to pay the atomic again, which is what #351 records as inheriting the
escape analysis. Adding traffic at one new site is the smaller claim.

## The gate whose reason was true of two passes and false of two others (#540)

A selector collision (#290) is a whole-program *name* check in `generics`. A
`@try` or a block capture is a per-site refusal that `emit` reaches. Neither is
a consequence of the other, and `front_end` returned at the first pass that
produced anything -- so the collision was reported and the refusal three
classes away was invisible. **Fixing the collision revealed it on the next
build. Each rebuild showed one layer.**

The rationale in the code was *"later passes would only report consequences of
the first failure."* That is not wrong; it is **true of two gates and false of
two others**, and the comment did not say which. `Collect` and the AST checks
really do produce a `Program` whose later readers cannot be trusted -- a
`superclass` string that is not a key in `classes`, which emit indexes
directly, giving an unlocated panic naming neither class nor file (#205, #501).
`Generics` and `Pools` produce no such thing.

So those two now defer into `FrontEnd::deferred` and the caller fails on the
union with emit's. The outcome is unchanged -- still hard errors, still no
output -- and only the *timing* moved.

### Two things the experiment found that reading could not

Removing the generics gate alone changed nothing: the **pools** gate two lines
later returned first. Worth knowing before concluding a gate has been removed.

With both gates gone, emit reported the `@try` refusal and the collision
**disappeared** -- because `front_end`'s accumulated `diagnostics` is a local
the `Ok` path never returned, so the masking simply inverted. The fix is not
"remove the gate"; it is "carry the diagnostics forward and merge", and the
difference between those two is a silently dropped error.

### Why running emit over a refused program is safe here, and how that is known

Not by argument. The `expect_reject` corpus is now the standing check: every
shape the front end refuses has emit run over it, and the whole suite passes
with **zero panics**. If a future front-end refusal leaves a `Program` emit
cannot walk, that corpus is where it will show up -- which is a better place
for it than a consumer's build.

### A merged assertion reversed, and its other half kept

`progress_observer::a_failing_pass_ends_the_sequence_there` asserted
`[Repair, Collect, AstIngest, Arc, Generics]` with the message *"generics
rejected, so pools and emit must not be reported"*. It was right about the
pipeline as it stood, and it is now
`a_generics_refusal_no_longer_ends_the_sequence`.

The half that did **not** change is asserted beside it rather than left
implicit: `a_collect_refusal_still_ends_the_sequence` pins the sequence
stopping at `Collect`. The reversal is only safe because that one holds, so the
two belong together -- and if a later change widens the hard gate, that test is
the one that fails first.

## Two declaration defects, and a node type that was not where the grammar said (#538, #549)

Both were silent in oz2c and surfaced from GCC on generated C -- the failure
shape #501, #205 and OZ-001/002/004 all share.

- **A duplicate parameter name** (`- (int)addA:(int)amount andB:(int)amount`)
  reached `oz2c_generated/Foundation/oz2c_dispatch.h` and GCC reported
  `redefinition of parameter 'amount'` against a file the author never opened,
  at a line that does not exist in their source (#549).
- **A variadic ellipsis was dropped, not refused** (#538). That is worse than a
  missing diagnostic, because the *declaration was altered*:
  `extract_method_sig` rebuilds the C signature from its `params` alone, so
  `, ...` could not reappear -- while the comment the emitter writes above the
  function preserved it verbatim. A body that never reaches for `va_start`
  compiles silently as a fixed-arg function; the one that does failed on
  `'va_start' used in function with fixed arguments`, a cause that is a
  *symptom* of the drop.

### The ellipsis is not `variadic_parameter`

`tree-sitter-objc`'s own `node-types.json` has a `variadic_parameter` type, and
matching it here found **nothing**: that type belongs to a plain C parameter
list. An Objective-C method's ellipsis is a bare anonymous `"..."` token child
of the `method_declaration`.

Settled by parsing the fragment and dumping the tree, which is the third time
that has been the only way: `id<Marker>` is a `typedefed_specifier` and not the
obvious `generic_specifier` (#367), a file-scope `struct box { };` is a bare
`struct_specifier` and not a `declaration` (#367 again), and now this. **The
grammar's type list is a catalogue of what the grammar can produce, not a map
of where.**

The token match needs its own control, and this one is not hypothetical: the
same `"..."` appears in a C `parameter_list`, so a walk that did not require a
`method_declaration` parent would refuse `OZLog` and every other variadic C
function in the SDK. `a_plain_c_variadic_function_is_untouched` is that
control.

### Where they are checked is #540's rule, applied

Neither belongs in `collect`'s root scans, and the reason is the one #540 had
just established: a `collect` diagnostic gates the pipeline because the
`Program` may be unwalkable afterwards, and neither of these makes it so. The
class table is fine; one signature is merely wrong.

So they sit with `check_out_parameter_stores`, where diagnostics accumulate
rather than return -- which means an author who writes both a duplicate
parameter and a `@try` sees both. In `collect` they would have become the
earliest masker in the pipeline, which is the thing #540 fixed. Two issues
apart, the second inherited the first's rule without a second argument, which
is what a rule stated once is for.

### A negative assertion that matched its own explanation

`a_duplicate_parameter_name_is_refused` first asserted
`!diags.contains("oz2c_dispatch.h")`, meaning "the diagnostic points at the
author's file, not a generated one". It failed -- because the diagnostic's own
**note** names that file, explaining where the error used to surface.

The needle matched the prose written *about* the defect rather than the defect.
That is the same shape as a guard whose subject is text you also wrote, and the
fix is the same: assert the property positively. The note's presence is now the
assertion, since a reader who saw the old GCC error needs exactly that sentence
to connect the two.

## The one argument Clang cannot see, and the parser that closed it for us (#548, #553)

`OZFN(...)` expands to `0` for Clang and to `__VA_ARGS__` for C, so its
argument is invisible to Clang **by construction** -- and `OZMacro.h` gives the
reason it must be: to reach a static initializer the expansion has to be a null
pointer constant, so the block goes unparsed. That makes oz2c the only gate on
it, and oz2c validated nothing.

Four shapes, of which one is much worse than the other three:

- **A block missing its closing brace was silently completed and hoisted.**
  tree-sitter recovers by *inserting* the `}`, so oz2c received a well-formed
  `block_literal` ending at the macro's `)`, hoisted it, wrote the hoisted name
  back into the macro, and exited 0. The emitted function body was not the one
  the author wrote and nothing said so -- the same "quietly shortened" failure
  #494 removed for sends, still live inside a macro argument. **Outside** one
  the identical mutation is caught, because there Clang sees it.
- `OZFN()` and `OZFN(42)` in a typed slot reached GCC as errors against
  generated C.
- `(void)OZFN(42)` was accepted by everybody.

### Ask the parser, not the braces

A recovered node is marked `is_missing()`, so "did the author close this block?"
has an exact answer with no lexing of our own. Two details cost a pass each:

- The inserted `}` is a **descendant** of the `block_literal` -- it belongs to
  the `compound_statement` inside it -- not a direct child. A direct-child test
  found nothing and left the headline case unreported, and the fix was found by
  dumping the tree. That is now the fourth time the tree has had to be dumped
  because the obvious node was the wrong one (`typedefed_specifier` not
  `generic_specifier`, a bare `struct_specifier` not a `declaration`, `"..."`
  not `variadic_parameter`, and this).
- The descent stops at a nested `block_literal`, which reports itself on its own
  visit. Without that an outer block is blamed for an inner one's missing brace,
  and both are reported for the same defect.

### The control that failed first, and why it was the test's fault

The accepting control -- a well-formed `OZFN` block that must still compile --
was first written on Zephyr's `K_TIMER_DEFINE` and failed. Not because the check
over-reached: the transpile succeeded and hoisted correctly, and the **host stub**
for `K_TIMER_DEFINE` cannot compile a hoisted callback. A control that fails for
a reason unrelated to its subject is worse than no control, because the obvious
reading is that the change is broken.

Rewritten on the function-pointer-field shape `ozfn_escape.rs` uses, which does
host-compile. The real macro's evidence is `just test-boards`:
`sample.transpiled_blocks`, `sample.objz.zbus` and `sample.objz.zbus_service`
all use `OZFN` and all pass on both boards.

### #553: the same macro's comment overstated its own hazard

`OZMacro.h` warned that an `INPUT_CALLBACK_DEFINE` token-paste collision lands
"on the AST-dump path, where a truncated dump silently costs ivar ownership
facts". `cmake/oz2c.cmake` already separates the two cases and this is the
harmless one: a **fatal** Clang error stops the dump, so declarations after it
are absent from a file that still looks complete -- that is the case that costs
ownership facts. An **ordinary** error like a redefinition does not truncate
anything. Clang recovers, the dump is complete, oz2c's wrapper prints
`file:line:col` with a macro-expansion trace, and the build fails loudly.

The advice was right and the reason was wrong, which is the more expensive half:
it sends a reader looking for silent data loss that better error handling had
already closed. Corrected rather than deleted, with the distinction stated, so
the next reader inherits the fatal-versus-ordinary rule instead of rediscovering
it.

## Two halves of a class, three wrong ends (#566, #567, #568)

`collect` gathers `@interface` declarations and `@implementation` definitions
into the same `ClassInfo` and, until now, never asked whether the two described
the same class. Three issues, one gap, and not one of them surfaced where the
mistake was.

This file's own **links** entry already names the shape: *"a call to a method
declared but defined nowhere compiles fine against its prototype and fails only
at link, so a compile-only sweep once reported OK for three samples that could
not be built."* That sentence was written about a *sweep*. It is also a
statement about what oz2c let through, and the three issues below are the
author-facing half of it.

### Each one surfaced at the wrong end

- **#567** — an `@implementation` with no `@interface` did not fail. It
  *fabricated* the class: pass 1 accepted either node kind and `class_header`
  answered `Primary` for both, so the class got a `ClassInfo`, a slab, and a
  slot in the shared dispatch. But `companion.rs` synthesizes `Foo_oz_alloc`
  and `Foo_oz_free` from the **`@interface`**, so the dispatch's `oz_release`
  called a deallocator nothing defined. This is the same defect one construct
  over from #529's class extension, and worse: `oz_release` is the single
  release path every ARC'd program calls, so `-dead_strip`/`--gc-sections`
  cannot reach it.

- **#566** — a declared selector with no body (M76), or a body whose selector
  differs from the declaration's (M79), emitted a call to a mangled symbol that
  appears nowhere in the source. Whether the program built then depended on the
  *caller*: with a reachable call site the linker says
  `undefined reference to 'MT76Probe_missing'`, and with none, `--gc-sections`
  drops the enclosing function and the build is clean. One typo, two outcomes.

- **#568** — a declaration and a body disagreeing about the return type let
  the declaration's spelling win *silently*, because the implementation arm
  de-duplicates against `info.methods` and simply declined to push. So
  `method_return_type` answered `int` for a body returning an object, `arc`
  claimed a `+1` reference on an `int`, and the author was told
  `This is an ownership-analysis bug rather than a problem with this source --
  please report it with the snippet`. Ordinary malformed input was asking for a
  transpiler bug report, and it fired *before* the Clang AST dump, so Clang
  never got to say `conflicting return type in implementation of 'value'`.

### The link failure was measured, not argued

#567's guard rests on a claim about the linker, so it was run rather than
reasoned about. A driver that never names the class, `-dead_strip` on:

```
Undefined symbols for architecture arm64:
  "_P77_oz_free", referenced from:
      _oz_release in oz2c_dispatch.o
```

That matters because a **merged test asserted the opposite.** #501 left behind
`implementation_with_no_interface_still_accepted`, whose comment read: pass 1
inserts the class in its own right, "so it has a `ClassInfo`, its methods are
emitted, and Clang only warns -- **there is no divergence to correct**." The
first three clauses are true and the conclusion does not follow: the test
checked the method *body* and never the dispatcher. It is the same mistake
`project_oz_undeclared_override_renames_silently` records -- read the generated
dispatcher, not the build log -- and the same shape as #529's
`category_property_synthesizes_accessors_once`. Reversed, with the reversal
stated in the test.

#501's own scoping stays right: its guard is about the *category*, which pass 1
skips. What changed is that the neighbouring shape it declined to catch turns
out to be a defect of its own.

### Where each check sits, and why the placements differ

- **#567 in `collect`, gating.** Beside the category (#501) and extension
  (#529) checks: one construct, three spellings, one refusal.
- **#568 in `collect`, gating**, at the exact point the implementation arm
  declines to push. Gating is on #540's criterion rather than by default: this
  *is* the case where later passes read an inconsistent `Program`. One selector
  has two return types, the table holds one, and every consumer downstream --
  `arc`'s ownership, emit's casts, the dispatch signature -- reasons from a type
  the body does not have. Comparison is on the **resolved** C spelling, so
  `instancetype` against `Foo *` on `Foo` agrees and only a real disagreement is
  reported.
- **#566 in `emit`, at the send.** Not at the declaration, which is what the
  issue asks for -- and that rule refuses the SDK.

### #566's stated fix would have refused Foundation

The issue asks for "every selector declared in an `@interface` needs a
definition in the matching `@implementation`". Stated at the declaration, that
rejects `include/oz_sdk/Foundation/OZArray.h` and `OZDictionary.h`, which both
declare `countByEnumeratingWithState:objects:count:` -- implemented only in
`src/runtime_legacy/`, which no build file references.
`Program::method_is_defined` exists *because* of that selector, and
`reachable_implementors` and `render_protocol_dispatch` already filter it out of
the dispatch.

On the call it costs nothing, because `for (x in a)` lowers to
`OZ_PROTOCOL_SEND_nextObject` and never to that selector -- so there is no call
to be undefined. That is a claim about the lowering, so
`decl_impl_reconciliation.rs` asserts it, paired with a *presence* check that
the loop still lowers to `nextObject`. An absence assertion alone also passes
when the feature stops being emitted, which is the failure mode #542's four
green guards had.

The check therefore leaves #566's "clean build" case clean, deliberately: with
no emitted call there is nothing undefined, and a declaration nobody calls harms
nobody. What it removes is the divergence, which only ever existed where a call
was written.

### The boundary the first cut got wrong

The first version refused every row of `method_family_ownership.rs`. That suite
declares `@interface Remote` with **no `@implementation` at all** and reads the
emitted `oz_release` text without ever linking -- which is how the create-rule
families are tested.

That is not fixture economy; it is a legal shape. A class declared here and
implemented in another translation unit, or by hand-written C providing
`Remote_ping()`, is invisible to the whole-program model, and
`method_is_defined`'s own doc already draws that line. So the check turns on
`ClassInfo::has_primary_implementation`: a declared selector with no body is an
omission only when the class's own implementation is present to have omitted it.
A category or class extension does not count.

The distinction is exactly #566's own wording -- a definition *"in the matching
`@implementation`"* -- and the narrowing is now a test of its own rather than a
distant suite that happens to depend on it.

A body with **no** declaration stays legal in both directions: that is an
ordinary private method (#566's M75), `defined_selectors` holds it, and this
check asks the question the other way round.

### Not the fixtures' fault, four times over

Four of `type_constraints.rs`'s cases failed, and the split was clean: every
accepting fixture (`compile_and_run`) declared `@interface User : OZObject`,
and every rejecting one (`expect_reject`) omitted it. The authors already knew a
bare `@implementation` does not link -- that is *why* the ones that actually
compile all carry the interface. The four rejecting fixtures were
under-specified, not evidence against the check, and each got the declaration
the accepting ones already had.

### Gates

`cargo test` 862 passed / 0 failed; `just test-behavior` 87 passed, and 87
again under `--sanitize address,undefined`; `just test-adapted` 40 passed;
`just test-boards` 16 suites on ARM and 14 on RISC-V, all passed, tallied from
`twister.json` rather than the recipe's exit code; and
`scripts/regen_zephyr_tests.py` left `tests/zephyr/generated/` untouched.

`just test-pedantic` was not run, and the omission is deliberate rather than an
oversight: the rule is to run it for any change that emits new *unconditional*
C, and this change emits no C at all. Every string it adds is diagnostic text --
checked by grepping the added `format!`/`push_str`/`write!` lines, not assumed.

## Three answers, not one, for the `@`-keywords (#563, #564)

The static subset accepted a *positive list* of `@`-keywords -- boxed
numeric and boolean literals, `@selector`, `@protocol(...)` as
`-conformsToProtocol:`'s argument -- and refused a few more by name
(`@try`/`@catch`, `@synchronized`). A keyword in **neither** list was
neither accepted nor refused: tree-sitter parsed it, no pass had a case for
it, and `emit`'s catch-all copied the source text through. The first thing
in the toolchain that understood it was GCC, saying `stray '@' in program`
about a line the author did write, in a file they never saw.

### The answer is per keyword, and that is the finding

The tempting fix is a category -- "refuse every unrecognised `@`-keyword"
-- and it is wrong, in both directions at once:

- `@import` is unrecognised and must **not** be refused. Modules are a
  front-end feature, Clang's own AST dump rejects it, and
  `oz2c-challenges/MUTATIONS.md` grades M68 `CLANG` -- delegated, and
  correctly. A category-shaped check takes it with them.
- `@class` is unrecognised and must not be refused *either*, because a
  forward declaration is not an operation. There was never anything to
  lower; what was missing was a case (#564).

So the surface is now three-way, and the split is not derivable from "is
this implemented":

| | keywords | why |
|---|---|---|
| **refused** | `@encode`, `@throw`, `@available`, `@defs`, handler-less `@try` | no meaning in this backend to lower to |
| **supported** | `@class` | not an operation |
| **delegated** | `@import` | a front-end feature; Clang is the right gate |

### Two of the five overrule a `CLANG` grade, deliberately

`MUTATIONS.md` grades M69 (`@defs`) and M70 (handler-less `@try`) as
`CLANG`, meaning the C front end is expected to refuse them -- and it does.
The decision to refuse them in oz2c anyway is not a disagreement about what
Clang does; it is about *which* Clang. Both are caught only by the **AST
dump**, and `oz2c` runs before it. So the diagnosis depends on a step that
`--allow-missing-ast` can skip, and on a tool invoked for its facts rather
than as a gate. Refusing in oz2c makes the answer independent of whether
the dump happened.

The three `GENERATED-C` grades (M64, M66, M67) need no such argument: those
are valid Objective-C, Clang accepts them, and *nothing* before GCC ever
looked.

### Three of five node kinds were not what their names suggest

Read out of a tree dump, not out of the grammar's type list:

- `@encode(int)` -> `encode_expression`, `@throw` -> `throw_statement`,
  `@available(...)` -> `available_expression`. Dedicated nodes, as expected.
- **`@defs(P)` -> `at_expression`.** This grammar has no `@defs` rule at
  all, so it arrives as the same generic node that carries `@42`, `@YES`
  and `@protocol(...)`. `@protocol(P)` is byte-for-byte the same tree with
  `protocol` where `defs` sits, so the **callee identifier text is the only
  thing separating an accepted construct from a refused one**. A check on
  the node kind would have refused every boxed literal in the tree.
- **A handler-less `@try` -> `ERROR`.** tree-sitter builds a
  `try_statement` only once a `@catch`/`@finally` follows, so the existing
  refusal never saw a bare one. Matched as an `ERROR` whose *first child*
  is the `@try` token -- narrow on purpose, since a general `ERROR` arm
  would turn every parse failure in the file into an exceptions diagnostic.

That is the fifth and sixth time in this tree that the obvious node was the
wrong one, after `typedefed_specifier` not `generic_specifier`, a bare
`struct_specifier` not a `declaration`, `"..."` not `variadic_parameter`,
and `block_literal`'s inserted brace being a descendant rather than a child.

### The position mattered more than the node kind

`@defs` is reported by the corpus at **file scope**, and the obvious home
for these checks -- `walk_for_reject`, where `try_statement` and
`synchronized_statement` already live -- is entered *only* with a method or
function **body** (`staticbar.rs`'s two call sites both pass one). A
body-scoped arm would have looked like a fix and left the reported case
untouched.

Worse, the two halves of that one construct already disagreed before this
change: `@defs` inside a body **was** refused, by the generic
`at_expression` catch-all with a generic message, while the same construct
at file scope was not refused at all. So the fix is one walk over the whole
tree, and `walk_for_reject` now *defers* the `@defs` shape so that one
construct yields one named diagnostic wherever it appears.

### #564 was half-built already

`@class` support turned out to be mostly present: #557 added
`Program::forward_declared`, `is_forward_declared_only`,
`collect::forward_declared_classes` and `emit::forward_declared_receiver`,
so that a *send* through such a name could blame the forward declaration
instead of reporting a receiver degraded to `id`. What was missing was
everything about the name as a **type**:

- the `@class` line itself, copied through verbatim -- now consumed and
  replaced by the C spelling of the same statement, `struct Other;`, which
  is legal with nothing ever defining `struct Other`. That is precisely the
  property `@class` has. One `class_declaration` carries every name in
  `@class A, B;`, so it emits one tag per name.
- the use sites, which had no `struct` tag. Both spelling paths now ask
  `Program::spells_with_struct_tag`.

That predicate is deliberately **not** a widening of `is_class`.
`is_class` has fourteen callers in `emit` alone and most are asking
something a forward declaration cannot answer -- does this class have a
slab, an allocator, ivars, a dispatch slot, a place in `class_order`?
Widening it would hand `pools`, `companion` and `arc` a class with no
shape. What a forward declaration settles is only the spelling.

`nil` needs no lowering here, which was checked rather than assumed:
`#define nil ((id)0)` with `typedef void *id`, so `struct Other *o = nil;`
is a null pointer constant assigned to an object pointer and passes
`-std=c17 -pedantic-errors` clean.

### A green gate over code it had never seen

`just test-pedantic` passed immediately after #564 -- and proved nothing,
because **no sample in the tree used `@class`**, so the sweep had never
compiled a tag declaration. Same failure mode as a violation behind an
`#ifdef` the sweep does not define: the gate is green over code it cannot
reach.

`samples/class_forward` fixes that permanently rather than caveating it.
The sweep now reports `class_forward   0 site(s)` -- enumerated, compiled,
clean -- and `just test-boards` runs it on both architectures.

The sample is built around a class that is forward-declared and **never
defined**, which is the only shape that exercises the new path: a name with
a real `@interface` later in the file is an ordinary class, and
`is_forward_declared_only` is false for it. It also carries an **ivar** of
that type, because that is a different lowering path from a local
(`emit`'s bare-ivar edit, not `render_expr`'s `type_identifier` arm) and
nothing else in the tree exercises it.

### Gates

`cargo test` 874 passed / 0 failed; `just test-behavior` 87 passed, and 87
again under `--sanitize address,undefined`; `just test-adapted` 40 passed;
`just test-boards` 17 suites on ARM and 15 on RISC-V, all passed, with
`sample.objz.class_forward` green on both, tallied from `twister.json`;
`just test-pedantic` 10 known sites and 0 for the new sample;
`scripts/regen_zephyr_tests.py` left `tests/zephyr/generated/` untouched.
