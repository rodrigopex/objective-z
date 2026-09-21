# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Version: `tools/oz2c/Cargo.toml`** — the transpiler carries its own semantic
version. **A PR declares the *kind* of bump and does not carry the number.** The kind
is already in the conventional-commit subject: `fix(oz2c):` is a patch, `feat(oz2c):`
or a `!` break is a minor (pre-1.0). The number is assigned **just before merge**, as
the final commit on the PR that is next to land — because the number is a function of
kind *and merge order*, and merge order is the one thing an author cannot know.

Changed 2026-09-14 after measuring the cost: 12 of 50 commits on `main` in one day
touched this file, one per PR, so **every merge forced a rebase on every other open
PR** — N−1 rebases per merge, by arithmetic. And the property the old rule seemed to
buy does not exist: the version string is **write-only**. `CARGO_PKG_VERSION` appears
nowhere in the crate, there is no `--version` flag, it reaches no generated output, and
no gate or CI job asserts it. Carrying it through review verified nothing while
colliding with every sibling branch. Eight of the nine version incidents recorded in
this repo were *created* by carrying the number, not caught by it. The repo-level `VERSION` file sits at v0.5.99
and no longer moves: it tracked the outgoing Python pipeline through a scheme that tied
`PATCHLEVEL` to an issue id, which is retired. Don't bump it.

**The omission is gated, the number is not** (#512). Making the number a separate,
final commit made it skippable, and it was skipped six times in about a day — #504,
#503, #510, #509, #508 and #523's window. `.github/version-omission.sh` fails when a
`feat`/`fix` or `!` commit *scoped to the crate* is reachable since the last change to
the version and the version still reads the same. It runs on **`push` to `main` only**
(`.github/workflows/version-omission.yml`), because on a pull request the branch is
deliberately number-free and the check would be red on every correct PR;
`.github/version-omission.test.sh` drives its 30 cases from `rust-tests`. A red there
is an outstanding-debt marker, not a blocked pipeline — it gates no merge, **must not
be added to the ruleset's required contexts** (it never runs on a PR, so a required
context would hang every PR forever), and it stays red on each later push until the
number lands. It deliberately does **not** compute the number: that gate was built and
reproduced only 9 of 17 historical transitions, and folding is order-dependent anyway
— one patch and two breaks off 0.98.0 give 0.100.0 as patch-break-break but 0.100.1
as break-break-patch.

Objective-Z is an Objective-C transpiler for Zephyr RTOS, packaged as a Zephyr module
(`zephyr/module.yml`). Converts `.m` sources to plain C — no ObjC runtime needed. Uses the
Platform Abstraction Layer (PAL) for zero-cost Zephyr integration.

**The transpiler is `tools/oz2c/` (the `oz2c` binary, Rust).** It is the only
backend. A second, Python implementation (`tools/oz_transpile/`, a 3-pass Clang-AST
pipeline) was retired: it implemented no construct oz2c lacks, had been unable to
build any sample on target since #267, and its one unique contribution --
`just test-cross-backend`, an independent behavioural oracle -- is gone with it. The
implementation is readable at the **`python-backend-final`** tag rather than kept as an
uncompiled directory; see [docs/STATUS.md](docs/STATUS.md).

## Project instructions

- Use just for build automation
- Use semantic versioning on `tools/oz2c/Cargo.toml` (see Version above)
- All changes must validate by testing. `cargo test --manifest-path tools/oz2c/Cargo.toml`
  is the primary gate; `just test` runs the samples on ARM under twister and `just test-riscv`
  runs them on RISC-V (`just test-boards` does both). Anything touching emitted C also needs a
  real board build and run — compiling only proves the input was understood.

## Issue tracking

**Issues live in this repository — its GitHub issue tracker (`rodrigopex/objective-z`),
tracked on [Project #4](https://github.com/users/rodrigopex/projects/4).** Not in files in
the tree, not in a separate repo. `issues/TEMPLATE.md` is only a body template for filing
one; the `issues/OZ-NNN.md` files that scheme once produced were removed, and re-creating
them splits the record in two — it has happened, and the copies disagreed within a day.

**Reference an issue by its GitHub number (`#226`)** — in commit messages, PR bodies, code
comments and docs alike. **The `OZ-NNN` id scheme is retired:** don't assign new ones and
don't rename an issue into that form. Older commits and closed issues
still carry OZ-NNN ids; leave those as the historical references they are.

## Build Commands

Default board: `mps2/an385` (ARM). RISC-V: `qemu_riscv32`. Requires Zephyr SDK, west, and Clang (for AST analysis). RISC-V requires LLVM Clang (not Apple Clang) — auto-detected from Homebrew.

### Zephyr SDK: LLVM must be installed explicitly

The SDK is componentised — host tools, GNU toolchains and LLVM are separate
downloads — and both `west sdk install` and the SDK's own `setup.sh` install
the GNU toolchains only. LLVM is opt-in, so a default install has no
`clang`, and `objz_find_clang()` (`cmake/ObjcClang.cmake`) then falls
through past the SDK to Homebrew or system clang:

```sh
west sdk install --llvm --version <ver> -b ~/.local   # or: setup.sh -l
```

That puts clang at `$ZEPHYR_SDK_INSTALL_DIR/llvm/bin/clang`, which is
priority 2 in `objz_find_clang()`'s search order and the version the
project is tested against (clang 19). Point the test harnesses at it with
`OZ_CLANG=$ZEPHYR_SDK_INSTALL_DIR/llvm/bin/clang`; both
`tests/tools/compile_and_run.py` and `tests/tools/cross_backend.py` honour
that variable. Without it they pick whatever clang is on `PATH` — Apple
Clang on macOS, which is a different version and only warns.

**Build with `-DOBJZ_REQUIRE_TESTED_CLANG=ON` to make that a hard error.**
CI does, since #269: the AST decides ivar ownership and method definedness,
and it was being produced there by Ubuntu's clang 18.1 for the life of the
workflow because the SDK had been installed without `-l`. The warning that
exists to catch it printed on every run, unread. `objz_find_clang()` checks
the *version* as well as whether the path is the SDK's, so a future SDK
carrying a different clang is caught too.

Versions CI pins, and so the ones to match locally: **Zephyr v4.4.2**
(`west.yml`) and **SDK 1.0.1** with LLVM (`.github/install-zephyr-sdk.sh`).

| Command                    | Description                        |
| -------------------------- | ---------------------------------- |
| `just build` / `just b`   | Build default sample (hello_world) |
| `just rebuild`             | Pristine rebuild                   |
| `just run` / `just r`     | Run in QEMU                        |
| `just flash` / `just f`   | Flash to hardware                  |
| `just monitor` / `just m` | Serial monitor via tio             |
| `just clean` / `just c`   | Remove this checkout's build dir (`rm -rf`; the undo is `just build`) |
| `just clean-rust`          | Remove `tools/oz2c/target` — the largest regenerable object here (1.9 GB) |
| `just clean-samples`       | Remove every `samples/*/build`, their AST dumps and generated C included |
| `just clean-twister`       | Remove **this checkout's** twister output, by name (~1 GB per directory) |
| `just clean-twister-all`   | Every checkout's, orphans included; needs `yes=1`, and prints sizes and mtimes first |
| `just clean-all`           | Everything regenerable this checkout owns; run it before leaving a worktree |
| `just disk-report`         | What this checkout and its worktrees are holding. Read-only |
| `just test` / `just t`    | Run twister on all samples (ARM)   |
| `just test-riscv`          | Same samples on RISC-V (two fewer configurations than ARM: `gpio_demo` and the `CONFIG_DEBUG` scenario are ARM-only). 15 against 17, measured 2026-09-21 (#609); a new sample moves both, so `--dry-run` rather than trust this line — and the dry-run's count *is* the executed count, which #609 established after a test comment claimed otherwise |
| `just test-smp`            | Two cores, `qemu_cortex_a53/smp` — the only board that exercises real lock contention |
| `just test-boards`         | ARM + RISC-V, so neither hides an architecture-specific regression |
| `just test-all-boards`     | All three, including SMP |
| `just test-pedantic`       | ISO C constraint violations in generated C, on target. Reports; the host half is a gate in `corpus_parity.rs` |
| `just test-behavior`      | Behavior corpus, 81 cases; `--compiler`/`--opt`/`--sanitize`/`--check-leaks` |
| `just test-adapted`       | 37 adapted upstream tests |
| `just test-hardware`      | Every single-core sample flashed and run on an nRF52833DK |
| `just smoke`              | Transpile-and-compile smoke test |
| `just test-pal`           | The PAL's own C tests, on the host |
| `just ast-dump file`      | Clang JSON AST dump |

The Rust suite has no `just` recipe — run it directly:

```sh
cargo test --manifest-path tools/oz2c/Cargo.toml
```

Every twister recipe depends on `just oz2c`, so the transpiler is built before
any sample configures. Driving `west twister` directly skips that: build oz2c
first, or every configure step forks its own cargo -- 16 of them on ARM
today -- and a slot can fail to start oz2c at all (#308).

Their output directories derive from `outdir`, which is keyed on the checkout —
`/tmp/twister-out-<checkout>` and suffixed siblings — so two worktrees can sweep
at once without one deleting the other's output mid-run (#315). Override it per
invocation to keep a run aside: `just outdir=/tmp/twister-out-before test`.
Those directories accumulate one per checkout. `just clean-twister` removes
**this checkout's** — the `outdir` name and each suffix in `outdir_suffixes`,
enumerated rather than globbed, because `{{ outdir }}*` over-reaches the moment
one name is a prefix of another (`spinvalidate` already prefixes
`spinvalidate-smp`, and a lane named `oz-477` would sweep `oz-477b`'s output).
`just clean-twister-all` is the every-checkout form: it is the only thing that
reaches output orphaned by a deleted worktree, so it still exists, but it lists
each directory with its size and mtime and refuses without `yes=1`. `just clean`
deliberately leaves both alone.

That `clean-twister` used to be the all-lanes sweep is why `docs/WORKING.md`
carried "Never `just clean-twister`" — a recipe whose documentation was an
instruction not to run it (#521). The instruction is now the implementation.

Build a specific sample: `just project_dir=samples/arc_demo rebuild`
Build for RISC-V: `just board=qemu_riscv32 rebuild`

Each sample uses `ZEPHYR_EXTRA_MODULES` to register the module and enables it with `CONFIG_OBJZ=y` in prj.conf.

## Architecture

### Zephyr Module (root)

- **`zephyr/module.yml`** — Module definition, points cmake/kconfig to root
- **`west.yml`** — West manifest for Zephyr CI integration
- **`CMakeLists.txt`** — Includes `oz2c.cmake` when `CONFIG_OBJZ` is enabled. It
  used to include `oz_transpile.cmake`, which dispatched on `CONFIG_OBJZ_BACKEND`
  between two backends; with one backend there is nothing to dispatch on
- **`Kconfig`** — `CONFIG_OBJZ` master enable, auto-selects `STATIC_INIT_GNU`

### OZ Transpiler (`tools/oz2c/`) — the `oz2c` binary

Primary compilation path: `.m -> tree-sitter CST -> oz2c -> .h + .c`. Generates plain C
compilable by GCC alone. The source text is substituted in place rather than regenerated
from an AST, which is why unexpanded macros survive into the output.

- **`collect.rs`** — CST → `Program`: classes, ivars, methods, types, protocols
- **`emit.rs`** — in-place substitution; expression and statement rendering, plus the
  `#line` directives that point a debugger back at the `.m` (`LineDirectives`, #305)
- **`companion.rs`** — shared dispatch header/source, per-class slabs, allocators, boxed-literal builders
- **`arc.rs`** — ARC. Read it as **ARC's optimizer written at the source level**, not as
  a reimplementation: Clang emits retain/release naively and deletes the redundant pairs
  in an LLVM pass, and nothing downstream of `oz2c` will ever do that, so the eliding is
  this module's whole job. Every release decision answers two questions — **provenance**
  (is this `+1` by shape?) and **escape** (is it reachable after this scope under another
  name?); asking only the first produced #351, #352, #359 and #360. Two mechanisms:
  resolve statically from the CST where possible (free), and retain where provenance
  cannot be established (one pair, and only sound at a `return` — see
  [docs/STATUS.md](docs/STATUS.md), "The hybrid model"). **Key ownership on the reference,
  never on a syntactic form**, and route every spelling through one function. Seven
  defects in a row came from keying on a form instead: the returned name (#351), a scalar
  ivar store's left side (#352), an array store's receiver (#360), the kind of slot
  (#359), the receiver's static class (#365), a selector's `init` prefix (#398) and a
  local's declared-type spelling (#400). Two of those — #398 and #400 — had the *correct*
  rule written down a few lines away in the same file, behind a check that returned
  before reaching it, which is worth looking for directly. The standing records are
  `tools/oz2c/tests/ownership_matrix.rs` (every sink a `+1` reaches, by refcount
  count) and `selector_ownership_matrix.rs` (every selector and construct that creates or
  consumes one, by observed output — counting cannot see *which* pointer a release names,
  which is what #398 got wrong). Everything not in those two files is believed correct,
  so a new sink, selector or construct needs a row.
  Those two records answer *where* a release goes.
  **[docs/OBJECTIVE_C_DIALECT.md](docs/OBJECTIVE_C_DIALECT.md) answers what an author
  may write** -- one row per author-visible construct, one verdict each, gated by
  `tools/oz2c/tests/dialect_ledger.rs`, whose exhaustiveness check is keyed to
  `objc_node_disposition.rs` so a grammar bump cannot add an undocumented construct
  (#583). **[docs/ARC.md](docs/ARC.md) answers which of ARC's rules apply at all** — one verdict per normative rule of the Clang ARC
  specification (implemented / delegated to `-fobjc-arc` / refused / N/A / gap), with
  `tools/oz2c/tests/arc_conformance.rs` pinning the delegated and refused ones. A new
  *rule* needs a row there; a new *site* needs one in the other two. Walking the spec that
  way is what found #458, #459, #460 and #461 — three of them use-after-free, from source
  Clang accepts silently (#447).
  A Clang JSON AST is supplied via `--ast`, and since #385 it is **required**, not
  optional: `oz2c` refuses a source that declares a class with no dump behind it, as a
  hard located error. It carries the `__strong` qualifiers and the
  `ARCProduceObject`/`ARCConsumeObject`/`ARCReclaimReturnedObject` marks, and since
  #453 `astinfo.rs` reads all of it — every transfer mark and every ownership
  qualifier, each attributed to a resolved source position, which is a **stateful
  fold** rather than a per-node read because Clang delta-encodes locations (10 of
  1,389 positions in one dump name a file) and the mark node itself carries none.
  `oz2c --check-arc` is what diffs those against oz2c's own verdicts. tree-sitter
  stays the primary frontend for *syntax*; the ownership oracle is Clang's. Every path that
  transpiles a program now produces one dump per source — `cmake/oz2c.cmake`,
  `tests/tools/compile_and_run.py`, `tests/smoke/run.py`,
  `scripts/regen_zephyr_tests.py` and the Rust harness (`tests/common/mod.rs`) — and
  `scripts/objz_clang.py` is the single place that decides *which* clang, mirroring
  `objz_find_clang()`. Two exemptions, both stated rather than assumed:
  `--manifest-only` (the configure-time run that discovers a file list, which no AST
  fact affects, and which runs before Zephyr's generated headers exist) and
  `--allow-missing-ast` (the escape hatch, which transpiles with the narrower rule that
  skips every `id`-typed ivar — correct, and a leak). `Options::require_ast` is off in
  `Options::default()`, so the pure `transpile(source)` form still works on a string
  with no file behind it
- **`pools.rs`** — slab and element-pool sizing, counted from allocation sites
- **`staticbar.rs`** — accept/reject scan for the static subset, on the **input**
- **`outputbar.rs`** — the dual, on the **output**: the generated `.h`/`.c` parse as C,
  checked on every transpile and a hard error. `emit`'s catch-all copies any construct
  it has no arm for through as the author's bytes — which is what carries Zephyr macros
  and unexpanded `#define`s intact, and also how an unnamed ObjC construct reached GCC
  as `stray '@' in program` with oz2c exiting 0. It reparses rather than grepping
  (every class's banner comment holds its own `@interface`), and stays silent when
  something was already refused. `tests/objc_node_disposition.rs` is the record: all
  191 named grammar kinds classified, count pinned, so a `tree-sitter-objc` bump cannot
  add an unclassified one (#582)
- **`preproc.rs`** — which arm of a `#if`/`#ifdef` is part of the program. oz2c parses
  the raw file, not the preprocessed translation unit, so both arms used to reach every
  pass: a nested `@interface` was copied through unlowered (#573) and checks fired on
  `#if 0` text (#570). The verdicts ride on `Program::preproc` so collect and emit
  cannot answer differently. A conditional around ObjC is resolved at transpile time —
  a class becomes a struct, a dispatch row and a slab, and there is no way to hand GCC
  half a dispatch table — while one carrying only C is passed through untouched
- **`imports.rs`** — `#import` resolution and per-origin provenance, plus the merged-offset
  → (`.m`/`.h`, line) source map the `#line` directives are resolved through
  (`ResolvedSource::source_location`/`source_position`, #305)
- **`generics.rs`** — generic and protocol constraint checking
- **`model.rs`** — `Program`, `ClassInfo`, `Diagnostic`
- **`progress.rs`** — pass boundaries the pipeline reports; no printing, no clock
  (`report.rs` is the binary-side half that formats and times)
- CLI: `--pool-sizes`, `--item-pool-size`, `--heap-support`, `--introspection`,
  `--reflection`, `--line-directives`, `--root-class`, `--ast`, `--allow-missing-ast`,
  `-I`, `--timings`, `--quiet`, `--manifest-only`, `--dump-cst`, `--dump-ast-facts`,
  `--check-arc`, `--no-nil-safe-sends`.
  `--check-arc` is an audit and not a gate — it **always succeeds**, takes sources and
  no outdir, and prints a work queue: the ivar-ownership diff (the one question both
  models answer independently), every ARC transfer grouped by the syntactic position
  it sits in against the `arc.rs` entry point for that position, and a statement of
  what the marks cannot discriminate. A gate would be wrong rather than merely strict:
  Clang retains on binding and `arc.rs` elides, so diffing *release positions* would
  report a discrepancy at every elided site, which is every site.
  `--ast` is required of any source declaring a class; every other feature flag
  (`--heap-support`, `--introspection`, `--reflection`, `--line-directives`) is off unless
  passed — its absence is what the matching Kconfig option's `n` means, and
  `cmake/oz2c.cmake` is what supplies it.
  **`--no-nil-safe-sends` is the one exception, and it is negative** (#528):
  the nil-receiver guards are on by default and this removes them, so
  `cmake/oz2c.cmake` passes it when `CONFIG_OBJZ_NIL_SAFE_SENDS` is `n` rather than
  passing something when it is `y`. The polarity is inverted because the fail-safe
  direction is: a missing `--introspection` removes a feature and refuses the source
  that wanted it, while a missing nil guard is a null dereference that reads whatever
  is at address 0. The `Options` field is `nil_sends_unchecked` for the same reason —
  `derive(Default)` then yields the guarded behaviour, so every test constructing
  `Options` literally gets it without asking. Don't "tidy" either into the positive
  form; that is how the safe default becomes opt-in
- Progress goes to **stdout**; stderr is diagnostics only, because
  `tests/tools/oz2c_build.py` reports its first line as the reason a transpile failed
- Tests: `cargo test --manifest-path tools/oz2c/Cargo.toml`

Three standing design rules, easy to violate with good intentions:

- **It never silently degrades.** Anything outside the supported subset is a hard, *located*
  error. That is deliberate — do not add a soft-diagnostic or best-effort mode.
  Since #582 this is **enforced on the output too**, not just intended on the input:
  every ObjC node kind is lowered or refused, never merely unnamed, and `outputbar`
  refuses generated text that still parses as Objective-C. The rule exists because the
  alternative kept happening — #563, #573 and #574 were each one `@`-keyword arriving
  at GCC as `stray '@'`, filed one issue at a time. Do not add a keyword to the
  passthrough; give it an arm or a refusal.
- **A message to nil is a no-op answering zero, and two guards implement it
  (#528).** Each instance method tests its receiver on entry; each
  `OZ_PROTOCOL_SEND_*` dispatcher tests it before reading
  `self->_meta.class_id`, because that read happens before any method body is
  reached. Both are needed and neither is sufficient. A class method has no
  receiver (`Foo_bar_cls(void)`), so the class side costs nothing. `.text` +0.22%
  and no RAM, measured across 17 configurations on `mps2/an385`; the linker drops
  the unreachable guards, which is why 65 emitted guards cost 832 bytes.
  Before this, a send to nil called through a null `self` and the body read
  `self->_field` — on `mps2/an385` address 0 is flash, so it returned a plausible
  number that drifted between builds rather than faulting.
- **ARC is the only ownership model.** A send of `retain`, `release`, `autorelease`,
  `dealloc` or `retainCount` is a hard located error, and so is declaring or defining
  any of them but `dealloc` (`staticbar::check_manual_memory_sends`, #428 and #436) —
  every Clang path here passes `-fobjc-arc`, under which each is a compile error, and
  that is the whole rule the set follows: **exactly what Clang refuses.** A `-dealloc`
  *override* is the one exception: it is the cleanup hook, and the chain above it is
  called automatically
  (`companion::dealloc_chain`), so `[super dealloc]` is redundant rather than required.
  Reading a refcount is permitted, through `oz_retain_count` -- a plain C call,
  and the only refcount entry point Objective-C source may spell, since `-retainCount`
  joined the forbidden five in #436. `@autoreleasepool` is refused for the neighbouring
  reason (#430): with no `-autorelease` nothing can be pending, so a pool has nothing to
  drain -- write a plain braced scope, which is what it compiled to. See
  [docs/STATUS.md](docs/STATUS.md), "Standing design rules".
- **The Python pipeline is a reference, not an authority.** It has real defects (a
  double-release in synthesized dealloc, item-slot sizing that ignores
  loops); matching them would be a regression dressed as parity. Its lack of variadic
  support used to head that list and no longer belongs there -- see the contract below,
  which oz2c shares with it deliberately.
- **A variadic Objective-C method is refused, with a located error (#538).** Not a gap:
  a dispatch shim declares one concrete signature per selector and
  `-performSelector:`'s wrapper has a fixed shape, so an ellipsis has nowhere to go.
  Until #538 it was *dropped* rather than refused -- the declaration silently became a
  fixed-arg function and a `va_start` in the body failed on GCC. A variadic plain **C**
  function is unaffected and `OZLog` is one (`src/OZLog.c`), which is the counter-example
  a reader reaches for first.

### The retired Python transpiler (`tools/oz_transpile/`)

**Deleted, and readable at the `python-backend-final` tag** rather than kept as an
uncompiled reference directory:

```sh
git checkout python-backend-final     # the last commit that still has it
just test-transpiler                  # its own 539 unit tests, at that tag
just test-cross-backend               # both backends over one corpus, 71/71 MATCH
```

`src/runtime_legacy/` is the cautionary example of the other choice — kept for
reference, not compiled, and now just sitting there.

What it was: a 3-pass Clang-AST pipeline (`collect.py` / `resolve.py` / `emit.py`,
~15,800 lines) that read a Clang JSON AST dump and emitted C from Jinja templates.

Why it went, measured rather than assumed:

- all 71 behaviour and 40 adapted cases *as the corpus stood then* transpile **and
  run** through oz2c, under gcc/clang × -O0/-O2, ASan, UBSan and
  LeakSanitizer (the behaviour corpus is 81 cases now);
- it implemented no construct oz2c lacks — `@try` was in its own
  `_UNSUPPORTED_AST_KINDS`, `@selector`/`@protocol()` appeared only in kind lists with
  no emission rule, reflection selectors were absent entirely, and there was no
  variadic support anywhere, so `OZLog` could never have gone through it (oz2c refuses a
  variadic *method* too, by the contract above; `OZLog` works because it is a C
  function) —
  oz2c has since implemented reflection outright (#226), which it never did;
- Objective-C in a `#define` body crashed it with a `RecursionError`, where oz2c
  rejects it with a located error (#238);
- it had been unable to build **any** sample on target since #267 left a deleted
  `src/OZTimer.m` in its source list, and no gate noticed.

What its removal cost, stated rather than absorbed: `just test-cross-backend` was the
only *independent* implementation to check behaviour against, 71/71 MATCH. Nothing
replaces that. What replaces its role as a gate is the corpora running through
oz2c under sanitizers — which is what found two real ARC leaks (#283) that
cross-backend agreement never caught, since it compared Unity results rather than
allocation balance.

### CMake Build Infrastructure (`cmake/`)

- **`oz2c.cmake`** — builds `oz2c`, dumps one Clang AST per source for ARC facts,
  and emits generated sources into `oz2c_generated/`. Defines
  `objz_transpile_sources()`, the entry point every sample calls — the name is
  unchanged from when it lived in the deleted `oz_transpile.cmake`, because 15 samples,
  px-app and any out-of-tree user call it
- **`ObjcClang.cmake`** — Clang detection (`objz_find_clang()`), target triple mapping, AST analysis flags, compile_commands.json generation for clangd IDE support
- **`scripts/objz_clang.py`** — the same search order for everything outside CMake: the
  two pytest harnesses, the smoke test, `regen_zephyr_tests.py` and the Rust suite all
  ask it which clang to dump with. `OZ_CLANG` → `$ZEPHYR_SDK_INSTALL_DIR/llvm/bin` →
  `~/.local/zephyr-sdk-*/llvm/bin` → Homebrew → PATH, and a hard error naming
  `west sdk install --llvm` if none of them has one

### Platform Abstraction Layer (`include/platform/`)

Zero-cost abstraction for transpiler-generated C:

- **`oz_platform.h`** — ifdef router (`OZ_PLATFORM_ZEPHYR` / `OZ_PLATFORM_HOST`)
- **`oz_platform_zephyr.h`** — Zephyr backend: k_mem_slab, Zephyr atomics, spinlock, printk
- **`oz_platform_host.h`** — Host backend: malloc-backed slab, C11 stdatomic, printf
- **`oz_platform_types.h`** — Shared type definitions

All PAL functions are `static inline` — vanish at -O1+.

### OZ SDK Headers (`include/oz_sdk/`)

OZ Foundation class headers and system shims for Clang AST analysis:

- **`Foundation/`** — OZObject.h, OZString.h, OZNumber.h, OZArray.h, OZDictionary.h, OZLog.h, protocols, Foundation.h umbrella
- **`objc/`** — objc.h (runtime stub)
- **`assert.h`** — System shim for Clang AST (must stay at root for `#import <assert.h>` resolution)

### Transpiler Sources (`src/`)

ObjC implementations consumed by Clang AST analysis:

- **OZObject.m** — Root class
- **OZString.m** — String class
- **OZArray.m**, **OZDictionary.m**, **OZNumber.m** — Collection/fixed-point classes
- **OZLog.c** — Pure C logging support for `%@` object specifier

### Legacy Runtime (`src/runtime_legacy/`, `include/runtime_legacy/`)

Retained as reference for transpiler development. Not compiled — the runtime compilation path has been retired. Includes message dispatch, ARC, refcounting, Foundation classes, and architecture-specific assembly trampolines.

### Test Infrastructure (`tests/`)

- **`tests/behavior/`** — 81 compiled behavior tests across 17 categories (Unity
  framework, host-side), under `tests/behavior/cases/<category>/*.m`
- **`tests/adapted/`** — 37 adapted upstream tests across 6 sources (LLVM, GNUstep,
  Apple, Bucket B, ObjFW, mulle-objc), under `tests/adapted/<source>/*.m` — **note the
  shape: there is no `cases/` level here**, unlike `tests/behavior/`. A blast-radius
  sweep that globs `tests/adapted/cases/*/*.m` matches nothing, reports a clean number
  for the 81 behaviour cases alone, and calls it "both corpora" (#400's PR did exactly
  that). Glob `tests/adapted/**/*.m`, and assert the count is 37 before trusting the
  result
- **`tests/zephyr/`** — 24 Zephyr integration cases in 7 ztest suites (`native_sim` +
  `ztest` + `twister`), over C committed under `tests/zephyr/generated/`. That C is
  **oz2c's output** since the port, so a green run says something about the
  default backend; `scripts/regen_zephyr_tests.py` regenerates it and the
  `generated-freshness` CI job fails if the tree is stale.
  **It compiles with `OZ_DEBUG_REFCOUNT` as of #490, and it is the only thing that
  does** — so this is where the refcount instruments (#452, #490) actually run on a
  board. Before that nothing in the tree defined the macro, and every line of
  #452's C had never executed anywhere: `poison_emission.rs` asserted the stores
  were *emitted* and passed, which is the shape of a working-looking instrument.
  The flag is set app-wide in `tests/zephyr/CMakeLists.txt` rather than per-file,
  because the poison lives in `_oz_free` in the generated sources; scoping it to
  one test would compile that test against a `_oz_free` that stamps nothing.
  `src/test_freed_slot.c` is the case that reads a freed slot back, and its
  counterpart `tools/oz2c/tests/refcount_traps.rs` cannot: what survives a real
  `k_mem_slab_free` is a fact about the allocator, and on arm64 macOS malloc wipes
  the whole block
- **`tests/objc-reference/`** — Legacy runtime tests (reference only, not compiled)

## Working alongside other changes

`docs/WORKING.md` collects the failure modes that cost real time when several
branches are in flight: guards that pass while the property is gone, instruments
that answer a different question than you asked, the version line, rebasing
across a rename, prose as load-bearing, and why a document is a relay. Every
rule there carries the incident that earned it. Read it before running more than
one branch at once.

## Coding Conventions

### C/ObjC Style

- `.clang-format`: LLVM-based, **8-space tab indentation**, Linux braces, column limit 100, `InsertBraces: true`
- Use `/* comment */` for documentation, `/** comment */` for Doxygen (not `//`)
- Always use curly braces with `if`, even single-line blocks
- Avoid `typedef` for structs — use explicit `struct objc_xxx` names (exception: public API types like `id`, `SEL`, `Class` per ObjC spec)
- **Internal and synthesized functions: `oz_` (companion-wide) or `_oz_`
  (per-class). The `__objc_` prefix is retired — don't add one.** A leading double
  underscore is reserved to the implementation in C, so every name under it was
  undefined behaviour waiting for a toolchain to claim it. **The same is true of a
  leading underscore followed by an uppercase letter, so an include guard or
  file-scope macro is `OZ_...` and never `_OZ_...`** — `_OZ_Q31_HELPERS` was the
  last of those and went in #417, and `tests/sdk_spliced_file_scope.rs` now
  requires a spliced prelude's `#define` to start `OZ`. `__objc_refcount_get` was
  its last survivor in the live tree and went in #418, replaced by
  `oz_retain_count`, which already did the same job. The prefix still appears
  in `src/runtime_legacy/` and `include/runtime_legacy/` (not compiled) and as one
  `#define` bridge in `tests/tools/oz2c_build.py`, which exists so behaviour
  drivers written against the old ABI stay unmodified — neither is a precedent
- ObjC ivars: underscore prefix (`_color`, `_model`)
- Use `#import` for ObjC headers, `#include` for C headers

### Commit Messages

Conventional commits, scoped to what changed: `feat(oz2c):`, `fix(oz2c):`,
`build:`, `docs:`, `samples:`. Append the issue number — `fix(oz2c): ... (#238)`.
Add `!` for a behavioural break.

The scope for transpiler work is `oz2c`. It documented `transpiler` here while
187 of the first 770 commits actually used the crate's old name, and #462
settled on one spelling; those older subjects stay as the historical record
they are, so a `git log --grep` on the scope alone splits at that boundary.
