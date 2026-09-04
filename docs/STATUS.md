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
| Rust suite (`cargo test`) | **288 tests**, `RUSTFLAGS=-D warnings` clean. The primary gate |
| Behaviour corpus | **71/71** transpile, compile and run — gcc/clang × `-O0`/`-O2`, plus ASan, UBSan and LeakSanitizer |
| Corpus ISO C validity | Gate at **0** under `-std=c17 -pedantic-errors` |
| Adapted upstream tests | **40/40** (LLVM, GNUstep, Apple, ObjFW, mulle-objc) |
| Samples on ARM (`mps2/an385`) | **13/13** built and run under twister |
| Samples on RISC-V (`qemu_riscv32`) | **12/12** — `gpio_demo` needs device-tree aliases the board lacks |
| Samples on two cores (`qemu_cortex_a53/smp`) | **9/9**, the only place `@synchronized` faces real contention |
| Samples on real silicon (nRF52833DK) | **13/13** flashed and run; `smp_shared` cannot, needing two cores |
| Kernel lock validation | `CONFIG_SPIN_VALIDATE` silent on ARM (13/13) and SMP (9/9) |
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
- **Reflection and `@selector`** are rejected with a located error, not
  supported — see #226. Objective-C in a `#define` body likewise (#238).

## How measurements mislead

The most reusable thing the old document held. Every entry below is something
that reported success while the thing it named was broken.

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
