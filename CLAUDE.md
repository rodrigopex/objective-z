# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Current version: v0.4.1**

Objective-Z is an Objective-C transpiler for Zephyr RTOS, packaged as a Zephyr module (`zephyr/module.yml`). Converts `.m` sources to plain C via Clang AST analysis — no ObjC runtime needed. Uses the Platform Abstraction Layer (PAL) for zero-cost Zephyr integration.

## Project instructions

- Use just for build automation
- Use semantic versioning
- All changes must validate by testing with `just test`

## Build Commands

Default board: `mps2/an385` (ARM). RISC-V: `qemu_riscv32`. Requires Zephyr SDK, west, and Clang (for AST analysis). RISC-V requires LLVM Clang (not Apple Clang) — auto-detected from Homebrew.

| Command                    | Description                        |
| -------------------------- | ---------------------------------- |
| `just build` / `just b`   | Build default sample (hello_world) |
| `just rebuild`             | Pristine rebuild                   |
| `just run` / `just r`     | Run in QEMU                        |
| `just flash` / `just f`   | Flash to hardware                  |
| `just monitor` / `just m` | Serial monitor via tio             |
| `just clean` / `just c`   | Remove build dir                   |
| `just test` / `just t`    | Run twister on all samples (ARM)   |
| `just test-transpiler`    | Run transpiler pytest suite        |
| `just test-behavior`      | Run compiled behavior tests        |
| `just test-adapted`       | Run adapted upstream tests         |
| `just transpile`          | Run OZ transpiler                  |
| `just ast-dump file`      | Clang JSON AST dump                |
| `just smoke`              | Run host-side PAL smoke test       |

Build a specific sample: `just project_dir=samples/arc_demo rebuild`
Build for RISC-V: `just board=qemu_riscv32 rebuild`

Each sample uses `ZEPHYR_EXTRA_MODULES` to register the module and enables it with `CONFIG_OBJZ=y` in prj.conf.

## Architecture

### Zephyr Module (root)

- **`zephyr/module.yml`** — Module definition, points cmake/kconfig to root
- **`west.yml`** — West manifest for Zephyr CI integration
- **`CMakeLists.txt`** — Includes `oz_transpile.cmake` when `CONFIG_OBJZ` is enabled
- **`Kconfig`** — `CONFIG_OBJZ` master enable, auto-selects `STATIC_INIT_GNU`

### OZ Transpiler (`tools/oz_transpile/`)

Primary compilation path: `.m -> clang -ast-dump=json -> oz_transpile -> .h + .c`. Generates plain C compilable by GCC alone.

- **`model.py`** — Dataclasses: OZModule, OZClass, OZMethod, OZIvar, OZType, OZProtocol, OZParam, DispatchKind
- **`collect.py`** — Pass 1: Clang JSON AST → OZModule (walks ObjCInterfaceDecl, ObjCImplementationDecl, etc.)
- **`resolve.py`** — Pass 2: hierarchy validation, topological class IDs, base_depth, dispatch classification (STATIC vs PROTOCOL)
- **`emit.py`** — Pass 3: OZModule → per-class `.h`/`.c` (with alloc/free/slab), `oz_dispatch.h`/`.c`
- **`__main__.py`** — CLI: `--input`, `--outdir`, `--root-class`, `--pool-sizes`, `--verbose`, `--strict`
- Tests: `python3 -m pytest tools/oz_transpile/tests/ -v` or `just test-transpiler`

### CMake Build Infrastructure (`cmake/`)

- **`oz_transpile.cmake`** — `objz_transpile_sources()` function: Clang AST dump → Python transpiler → generated C sources + PAL include paths
- **`ObjcClang.cmake`** — Clang detection (`objz_find_clang()`), target triple mapping, AST analysis flags, compile_commands.json generation for clangd IDE support

### Platform Abstraction Layer (`include/platform/`)

Zero-cost abstraction for transpiler-generated C:

- **`oz_platform.h`** — ifdef router (`OZ_PLATFORM_ZEPHYR` / `OZ_PLATFORM_HOST`)
- **`oz_platform_zephyr.h`** — Zephyr backend: k_mem_slab, Zephyr atomics, spinlock, printk
- **`oz_platform_host.h`** — Host backend: malloc-backed slab, C11 stdatomic, printf
- **`oz_platform_types.h`** — Shared type definitions
- **`oz_lock.h`** — OZLock RAII spinlock wrapper for `@synchronized`

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
- **OZArray.m**, **OZDictionary.m**, **OZNumber.m** — Collection/number classes
- **OZLog.c** — Pure C logging support for `%@` object specifier

### Legacy Runtime (`src/runtime_legacy/`, `include/runtime_legacy/`)

Retained as reference for transpiler development. Not compiled — the runtime compilation path has been retired. Includes message dispatch, ARC, refcounting, Foundation classes, and architecture-specific assembly trampolines.

### Test Infrastructure (`tests/`)

- **`tests/behavior/`** — Compiled behavior tests (Unity framework, host-side)
- **`tests/adapted/`** — Adapted upstream tests (LLVM, GNUstep, Apple spec)
- **`tests/zephyr/`** — Zephyr integration tests (`native_sim` + `ztest` + `twister`)
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
