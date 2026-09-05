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
| Rust suite (`cargo test`) | **331 tests**, `RUSTFLAGS=-D warnings` clean. The primary gate |
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
  returning incompatible types.** `OZ_PROTOCOL_SEND_<sel>` is emitted per
  selector *name*, so a pointer or by-value aggregate mismatch between
  implementors is a located error (#290). Differing *arithmetic* returns are
  accepted, and the shim then declares whichever implementor came first
  rather than the common type C's own conversions would give -- `OZArray`
  returns `unsigned int` for `-count` where another class may return `int`.
  That imprecision is knowingly permitted: C converts correctly, and the
  alternative would move the emitted signature for every such selector.
  `instancetype` is exempt, since the shim already collapses those to
  `void *`.
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
  heuristic.
- **The version is `tools/oz_static/Cargo.toml`**, bumped in the same commit
  as the change it describes. The repo-level `VERSION` file is retired.
