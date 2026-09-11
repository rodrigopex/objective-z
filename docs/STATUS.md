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
`include/oz_sdk/Foundation/OZObject+Protocol.h`, is reached through `OZObject.h`
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

### Every position that carries a type through (#326, #336, #367)

The third family, and the one that has now produced four bugs with a
single sentence behind all of them: **a method's signature and a free
function's, a block literal's, and a C struct's field list are four
separate walks over the same question**, and lowering a type was
implemented in one of them at a time.

There is nothing shared to fix. `render_method_definition` lowers a
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
0.161 s each; re-measured by timing `oz_static.cmake`'s own generated
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
that local ARC-managed and emits `oz_static_release((struct OZObject *)(result))`
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
  default `-cDescription:maxLength:` (#354) looked like it would cost only
  programs that use `%@`, on the reasoning that `--gc-sections` drops the
  protocol dispatch otherwise. Measured, it costs **~360 B on every program
  that calls `OZLog` at all** -- `samples/hello_category`, which contains no
  `%@`, grew 26456 -> 26820 B. `src/OZLog.c:82` calls the dispatch from
  inside `oz_log`'s body and the format string is parsed at *run time*, so
  the `%@` branch is always present and the whole chain behind it stays
  reachable: the dispatch, `OZObject_cDescription_maxLength_` (4 B to
  176 B, of which 110 is formatting the address), the synthesized
  `oz_static_class_name` (40 B) and one name string per class.

  "The linker will drop it" is a claim about reachability, and reachability
  is decided by the call graph rather than by what the program appears to
  use. The corollary is the useful half: with
  `CONFIG_OBJZ_DEFAULT_DESCRIPTION=n` the call itself is compiled out, and
  then the linker does strip all of it -- `oz_static_class_name` is absent
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

## Why this is not Clang's ARC, and what that costs (#351)

Worth writing down because the gap looks like an omission and is mostly a
constraint, and because the one part that *was* an omission was a
use-after-free.

Three differences are forced by the target and are correctly decided:
there is no autorelease pool (`@autoreleasepool` lowers to a plain
compound statement), so ARC's `objc_autoreleaseReturnValue` /
`objc_retainAutoreleasedReturnValue` return convention cannot exist and a
returning function must pick +1-to-caller or borrowed and declare it
consistently; there is no zeroing `__weak`, which is a hard located error
because nothing can zero a weak reference without a runtime; and there is
no `ObjCARCOpt`.

That third one is the load-bearing difference. **Clang emits retain and
release naively at every binding and deletes the redundant pairs in an
LLVM pass.** `oz2c` emits C for GCC, and nothing downstream elides
anything, because `oz_static_retain`/`oz_static_release` are ordinary C
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
(`tools/oz_static/tests/ivar_store_ordering.rs`).

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
emits C for GCC, where `oz_static_retain`/`oz_static_release` are ordinary
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
(`OZ_CLANG`, `-DOBJZ_REQUIRE_TESTED_CLANG=ON`), and `cmake/oz_static.cmake`
already dumps one AST per source. It carries precisely the facts ARC
decides from: `__strong` / `__unsafe_unretained` qualifiers, and the
transfer points marked `ARCProduceObject`, `ARCConsumeObject` and
`ARCReclaimReturnedObject`. `astinfo.rs` reads only `ObjCIvarDecl` today,
so every local qualifier and every cast kind is parsed and discarded --
that is head-room, not a design limit.

Two things measured about it, so the next reader does not have to guess:

- **It settles questions about ARC that memory gets wrong.** Modern Clang
  permits `__strong` members in C structs; a `static` local is `__strong`
  with static storage duration; a file-scope object pointer is `__strong`.
  All three were asserted incorrectly from recall during the #359 audit and
  corrected by dumping the AST for the shape.
- **It cannot answer every question, and one of them looks like it should.**
  At a call site, a protocol send, a `+1` class send and a `+0` class send
  are marked *identically* -- `ARCReclaimReturnedObject` on all three --
  because ARC's callee autoreleases and its caller always reclaims. That
  convention is sound only because of the pool this target does not have,
  so the AST distinguishes nothing there. It *does* distinguish the
  implementors: a method returning an owned reference carries
  `ARCConsumeObject` inside it, one returning a borrowed ivar does not.
  So the AST can classify each implementor more reliably than the CST can
  -- but which implementor runs is a runtime fact, and no oracle removes
  the need for unanimity.

**Since #385 the dump is required, not optional.** `oz2c` refuses a source
that declares a class with no `--ast` behind it -- a hard, located error at
the first class keyword, consistent with the rule that this backend never
silently degrades. What it used to do instead was print `no AST dumps` and
carry on, and the consequence of carrying on is not a failed build but a
leak: without a dump `owned_object_ivars` falls back to a syntactic rule
that cannot recognise an `id`-typed ivar as an object, so nothing releases
it.

The practical constraint that used to block this was the Rust suite: ~500
tests drove `oz_static::transpile` with no AST at all, so the primary gate
exercised the fall-back while every shipped path exercised Clang's answer.
`tests/common/mod.rs` now writes each case's source to a real `.m` and
dumps it, and every other producer was already in place -- so all five
paths agree:

| path | dumps | which clang |
|---|---|---|
| `cmake/oz_static.cmake` (Zephyr) | one per entry `.m` + per `src/*.m` | `objz_find_clang()` |
| `tests/tools/compile_and_run.py` (behaviour + adapted) | one per case | `scripts/objz_clang.py` |
| `tools/oz_static/tests/common/mod.rs` (Rust suite) | one per compile-and-run case | `scripts/objz_clang.py` |
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

The audit is now `tools/oz_static/tests/ownership_matrix.rs`, asserting the
refcount shape of every sink, with the two remaining defects asserted to
*still* be defective the way `KNOWN_CC_FAILURES` is -- so fixing one fails
the test and forces the list to change, and everything not listed is
believed correct rather than merely unexamined.

## A seventh, and the audit that found it (#400)

#400 walked the dimension the four earlier audits did not: **which selectors
and constructs create or consume a reference**, the set every other ownership
answer is built on. Six areas, fourteen cells, now standing as
`tools/oz_static/tests/selector_ownership_matrix.rs`.

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

    (tmp = makeThing(), v = Thing_n(tmp), oz_static_release(tmp), v)

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
	oz_static_release((struct OZObject *)(t));
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
`oz_static_release((struct OZObject *)(n))` against a `long` today —
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
	oz_static_release(t);   /* break only exits the switch */
	break;
}
Thing_n(t);                 /* freed */
oz_static_release(t);       /* and again */
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
| ivar, file-scope variable | yes | **no** | **2** | 1/4, then `nil` |
| array element, varying index | **no** | n/a | loop bound | unbounded |
| local ARC declined to manage | yes | **no**, nothing releases | loop bound | unbounded |

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

Both stores now route through one predicate, and
`staticbar::overlapping_unless_released_first` asks it rather than
re-deriving an answer. Two of the three shapes need no temporary and are
accepted; `_x = [_x copy]` is `+1` but reads the ivar, so it keeps the
temporary and stays refused. That last part is not just an over-rejection
being lifted -- the refused shape hoists its temporary through
`ctx.pre_stmts`, which a loop lifts out of the loop entirely, so accepting it
would miscompile rather than merely exhaust the pool. The rejection was
load-bearing for one shape and wrong for the other two, which is why the
narrowing and the store fix had to be the same change.

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

Two boundaries worth keeping in view. A **constant** subscript names the
same element every iteration, so it overlaps at two rather than
accumulating — a rule that called every subscript unbounded would say
something false about it. And `[[Foo alloc] init]` is **one** object, so it
is reported at the outer send: `-init` consumes its receiver's reference
and hands it back, and reporting the inner one would start the escape walk
from an expression that is not the thing stored.

## Standing design rules

- **Never silently degrade.** Anything outside the supported subset is a hard,
  *located* error. This is deliberate, not a gap someone forgot to fill.
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
  touches it.** `oz_static_release` checked `_meta.immortal` before its
  decrement and its comment stated the rule -- "their refcount is not tracked
  either". `oz_static_retain` incremented with no check at all, so the rule
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
  `OZIteratorProtocol` and `OZSingletonProtocol` in headers named to match. They
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
- **The version is `tools/oz_static/Cargo.toml`**, bumped in the same commit
  as the change it describes. The repo-level `VERSION` file is retired.
