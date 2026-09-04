# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Version: `tools/oz_static/Cargo.toml`** — the transpiler carries its own semantic
version, bumped in the same commit as the change it describes (patch for a fix, minor
for a new construct or a pre-1.0 break). The repo-level `VERSION` file sits at v0.5.99
and no longer moves: it tracked the outgoing Python pipeline through a scheme that tied
`PATCHLEVEL` to an issue id, which is retired. Don't bump it.

Objective-Z is an Objective-C transpiler for Zephyr RTOS, packaged as a Zephyr module
(`zephyr/module.yml`). Converts `.m` sources to plain C — no ObjC runtime needed. Uses the
Platform Abstraction Layer (PAL) for zero-cost Zephyr integration.

**The transpiler is `tools/oz_static/` (the `oz2c` binary, Rust).** It is the default
backend (`CONFIG_OBJZ_BACKEND_STATIC`) and where new work goes. The Python pipeline
`tools/oz_transpile/` still builds and its suites are still the only independent oracle
for behavioural equivalence, but it is the outgoing implementation: read it for
reference, don't extend it.

## Project instructions

- Use just for build automation
- Use semantic versioning on `tools/oz_static/Cargo.toml` (see Version above)
- All changes must validate by testing. `cargo test --manifest-path tools/oz_static/Cargo.toml`
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
don't rename an issue into that form. Older commits, closed issues and `PARITY.md` entries
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
| `just clean` / `just c`   | Remove build dir                   |
| `just test` / `just t`    | Run twister on all samples (ARM)   |
| `just test-riscv`          | Same samples on RISC-V (12 of 13; `gpio_demo` is ARM-only) |
| `just test-smp`            | Two cores, `qemu_cortex_a53/smp` — the only board that exercises real lock contention |
| `just test-boards`         | ARM + RISC-V, so neither hides an architecture-specific regression |
| `just test-all-boards`     | All three, including SMP |
| `just test-pedantic`       | ISO C constraint violations in generated C, on target. Reports; the host half is a gate in `corpus_parity.rs` |
| `just test-cross-backend` | Both backends over the same corpus, results diffed |
| `just test-behavior`      | Behavior corpus, 71 cases, through **oz_static**; `--compiler`/`--opt`/`--sanitize`/`--check-leaks` |
| `just test-adapted`       | 40 adapted upstream tests, through **oz_static** |
| `just test-behavior-python` / `test-adapted-python` | The same two corpora through the outgoing Python pipeline |
| `just test-transpiler`    | Python transpiler pytest suite (retiring with that backend) |
| `just transpile`          | Run the Python transpiler directly |

The Rust suite has no `just` recipe — run it directly:

```sh
cargo test --manifest-path tools/oz_static/Cargo.toml
```
| `just ast-dump file`      | Clang JSON AST dump                |
| `just smoke`              | Run host-side PAL smoke test       |

Build a specific sample: `just project_dir=samples/arc_demo rebuild`
Build for RISC-V: `just board=qemu_riscv32 rebuild`

Each sample uses `ZEPHYR_EXTRA_MODULES` to register the module and enables it with `CONFIG_OBJZ=y` in prj.conf.

## Architecture

### Zephyr Module (root)

- **`zephyr/module.yml`** — Module definition, points cmake/kconfig to root
- **`west.yml`** — West manifest for Zephyr CI integration
- **`CMakeLists.txt`** — Includes `oz_transpile.cmake` when `CONFIG_OBJZ` is enabled; that
  file dispatches to the backend `CONFIG_OBJZ_BACKEND` selects
- **`Kconfig`** — `CONFIG_OBJZ` master enable, auto-selects `STATIC_INIT_GNU`

### OZ Transpiler (`tools/oz_static/`) — the `oz2c` binary

Primary compilation path: `.m -> tree-sitter CST -> oz2c -> .h + .c`. Generates plain C
compilable by GCC alone. The source text is substituted in place rather than regenerated
from an AST, which is why unexpanded macros survive into the output.

- **`collect.rs`** — CST → `Program`: classes, ivars, methods, types, protocols
- **`emit.rs`** — in-place substitution; expression and statement rendering
- **`companion.rs`** — shared dispatch header/source, per-class slabs, allocators, boxed-literal builders
- **`arc.rs`** — scope-based ARC. A Clang JSON AST may be supplied via `--ast` as an
  *optional* secondary oracle for ivar ownership and method definedness; tree-sitter stays
  the primary frontend
- **`pools.rs`** — slab and element-pool sizing, counted from allocation sites
- **`staticbar.rs`** — accept/reject scan for the static subset
- **`imports.rs`** — `#import` resolution and per-origin provenance
- **`generics.rs`** — generic and protocol constraint checking
- **`model.rs`** — `Program`, `ClassInfo`, `Diagnostic`
- CLI: `--pool-sizes`, `--item-pool-size`, `--heap-support`, `--root-class`, `--ast`, `-I`
- Tests: `cargo test --manifest-path tools/oz_static/Cargo.toml`

Two standing design rules, easy to violate with good intentions:

- **It never silently degrades.** Anything outside the supported subset is a hard, *located*
  error. That is deliberate — do not add a soft-diagnostic or best-effort mode.
- **The Python pipeline is a reference, not an authority.** It has real defects (a
  double-release in synthesized dealloc, no variadic support, item-slot sizing that ignores
  loops); matching them would be a regression dressed as parity.

### Legacy Transpiler (`tools/oz_transpile/`, Python)

**Being retired.** Still selectable via `CONFIG_OBJZ_BACKEND_PYTHON`, but the corpora it
used to own now run through oz_static: `tests/tools/compile_and_run.py` takes
`--backend {static,python}`, and every CI job exercising the behaviour and adapted
corpora passes `--backend=static`. What it still uniquely provides is
`just test-cross-backend`, the independent behavioural oracle, which goes when it does.

An audit found nothing blocking removal on the construct side: all 71 behaviour cases and
all 40 adapted cases transpile *and run* through oz_static, and the Python backend
implements neither `@try` (it is listed in its own `_UNSUPPORTED_AST_KINDS`), reflection
selectors, `@selector`/`@protocol()` emission, nor variadics. Objective-C in a `#define`
body crashes it with a `RecursionError`, where oz_static rejects it with a located error
(#238). It has also been unable to build any sample on target since #267 left a deleted
`src/OZTimer.m` in its source list, and no gate noticed.

- `model.py` / `collect.py` / `resolve.py` / `emit.py` — 3-pass Clang-AST pipeline
- Tests: `just test-transpiler`

### CMake Build Infrastructure (`cmake/`)

- **`oz_transpile.cmake`** — `objz_transpile_sources()`: the entry point every sample calls.
  Dispatches on `CONFIG_OBJZ_BACKEND` to either backend
- **`oz_static.cmake`** — the Rust backend: builds `oz2c`, optionally dumps Clang ASTs for
  ARC facts, and emits generated sources into `oz_static_generated/`
- **`ObjcClang.cmake`** — Clang detection (`objz_find_clang()`), target triple mapping, AST analysis flags, compile_commands.json generation for clangd IDE support

### Platform Abstraction Layer (`include/platform/`)

Zero-cost abstraction for transpiler-generated C:

- **`oz_platform.h`** — ifdef router (`OZ_PLATFORM_ZEPHYR` / `OZ_PLATFORM_HOST`)
- **`oz_platform_zephyr.h`** — Zephyr backend: k_mem_slab, Zephyr atomics, spinlock, printk
- **`oz_platform_host.h`** — Host backend: malloc-backed slab, C11 stdatomic, printf
- **`oz_platform_types.h`** — Shared type definitions
- **`oz_lock.h`** — OZSpinLock RAII spinlock struct for `@synchronized`

All PAL functions are `static inline` — vanish at -O1+.

### OZ SDK Headers (`include/oz_sdk/`)

OZ Foundation class headers and system shims for Clang AST analysis:

- **`Foundation/`** — OZObject.h, OZString.h, OZQ31.h, OZArray.h, OZDictionary.h, OZLog.h, protocols, Foundation.h umbrella
- **`objc/`** — objc.h (runtime stub)
- **`assert.h`** — System shim for Clang AST (must stay at root for `#import <assert.h>` resolution)

### Transpiler Sources (`src/`)

ObjC implementations consumed by Clang AST analysis:

- **OZObject.m** — Root class
- **OZString.m** — String class
- **OZArray.m**, **OZDictionary.m**, **OZQ31.m** — Collection/fixed-point classes
- **OZLog.c** — Pure C logging support for `%@` object specifier

### Legacy Runtime (`src/runtime_legacy/`, `include/runtime_legacy/`)

Retained as reference for transpiler development. Not compiled — the runtime compilation path has been retired. Includes message dispatch, ARC, refcounting, Foundation classes, and architecture-specific assembly trampolines.

### Test Infrastructure (`tests/`)

- **`tests/behavior/`** — 72 compiled behavior tests across 16 categories (Unity framework, host-side)
- **`tests/adapted/`** — 40 adapted upstream tests across 6 sources (LLVM, GNUstep, Apple, Bucket B, ObjFW, mulle-objc)
- **`tests/zephyr/`** — 18 Zephyr integration cases in 5 ztest suites (`native_sim` +
  `ztest` + `twister`), over C committed under `tests/zephyr/generated/`. That C is
  **oz_static's output** since the port, so a green run says something about the
  default backend; `scripts/regen_zephyr_tests.py` regenerates it and the
  `generated-freshness` CI job fails if the tree is stale
- **`tests/objc-reference/`** — Legacy runtime tests (reference only, not compiled)

## Coding Conventions

### C/ObjC Style

- `.clang-format`: LLVM-based, **8-space tab indentation**, Linux braces, column limit 100, `InsertBraces: true`
- Use `/* comment */` for documentation, `/** comment */` for Doxygen (not `//`)
- Always use curly braces with `if`, even single-line blocks
- Avoid `typedef` for structs — use explicit `struct objc_xxx` names (exception: public API types like `id`, `SEL`, `Class` per ObjC spec)
- Internal functions: `__objc_` prefix (double underscore)
- ObjC ivars: underscore prefix (`_color`, `_model`)
- Use `#import` for ObjC headers, `#include` for C headers

### Commit Messages

Conventional commits: `feat(transpiler): description`, `fix(transpiler): description`, `build: description`, `samples: description`
