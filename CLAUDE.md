# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Objective-Z is an Objective-C runtime for Zephyr RTOS, packaged as a Zephyr module (`objc/zephyr/module.yml`). Uses the gnustep-2.0 ABI with Clang for ObjC compilation.

## Project instructions

- Use just for build automation
- Use semantic versioning
- All changes must validate by testing with `just test`

## Build Commands

Default board: `mps2/an385`. Requires Zephyr SDK, west, and Clang (for ObjC files).

| Command                    | Description                        |
| -------------------------- | ---------------------------------- |
| `just build` / `just b`   | Build default sample (hello_world) |
| `just rebuild`             | Pristine rebuild                   |
| `just run` / `just r`     | Run in QEMU                        |
| `just flash` / `just f`   | Flash to hardware                  |
| `just monitor` / `just m` | Serial monitor via tio             |
| `just clean` / `just c`   | Remove build dir                   |
| `just test` / `just t`    | Run twister on all samples         |

Build a specific sample: `just project_dir=samples/arc_demo rebuild`

Each sample registers the runtime via `ZEPHYR_EXTRA_MODULES` in its CMakeLists.txt and enables it with `CONFIG_OBJZ=y` in prj.conf.

## Architecture

### Runtime Module (`objc/`)

- **`include/objc/objc.h`** — Pure C runtime umbrella: assert, malloc, mutex, runtime
- **`include/objc/runtime.h`** — Runtime types (`id`, `SEL`, `Class`, `IMP`, `BOOL`), introspection functions
- **`src/api.h`** — ABI structures (`objc_class`, `objc_method`, `objc_selector`, `objc_init`), gnustep-2.0 types, `objc_class_flag_immortal` for statically-emitted objects
- **`src/load.c`** — Entry point: `__objc_load()` called via `.init_array` static constructor. Registers classes, categories, protocols, and fixes constant string isa pointers from gnustep-2.0 metadata sections.
- **`src/message.c`** — Core dispatch: `objc_msg_lookup()` / `objc_msg_lookup_super()`, lazy class resolution, sends `+initialize` on first use. Checks per-class dtable cache before global hash table.
- **`src/dtable.c`** — Per-class dispatch table cache (`CONFIG_OBJZ_DISPATCH_CACHE`). Two-tier allocation: static BSS pool (first N classes), heap fallback. Pointer-hash lookup with `strcmp` fallback.
- **`src/class.c`** — Class table (auto-sized), lazy resolution of superclasses, gnustep-2.0 non-fragile ivar offset fixup (skips immortal classes)
- **`src/hash.c`** — Global method hash table (auto-sized, open addressing). Slow path for cache misses.
- **`src/category.c`** — Category table (auto-sized), deferred loading until class is resolved. Flushes dispatch caches after loading.
- **`src/malloc.c`** — Dedicated `sys_heap` (default 4096 bytes via `CONFIG_OBJZ_MEM_POOL_SIZE`) with spinlock
- **`src/refcount.c`** — Atomic refcount core (pure C, Zephyr `atomic_inc/dec/get/set`). Guards immortal classes (OZString, Protocol).

### Foundation Layer (`objc/include/Foundation/`, `objc/src/Foundation/`)

- **`include/Foundation/Foundation.h`** — Umbrella header importing all Foundation classes
- **`src/Foundation/Object.m`** — Root class: alloc (sets rc=1)/init/dealloc/retain/release/autorelease/retainCount/class/respondsToSelector. Checks static pools before heap fallback.
- **`src/Foundation/Protocol.m`** — Protocol class: name, conformsTo, isEqual
- **`src/Foundation/OZString.m`** — Backs `@"..."` literals; aliased to `NSString` under Clang
- **`src/Foundation/OZMutableString.m`** — Heap-allocated mutable string for -description
- **`src/Foundation/OZAutoreleasePool.m`** — Per-thread pool stack (`__thread`), `@autoreleasepool {}` via `objc_autoreleasePoolPush/Pop`
- **`src/Foundation/OZLog.m`** — Formatted logging with `%@` object specifier
- **`src/Foundation/OZNumber.m`** — Boxed number class for `@42`, `@YES` literals (`CONFIG_OBJZ_NUMBERS`)
- **`src/Foundation/OZArray.m`** — Immutable array for `@[...]` literals (`CONFIG_OBJZ_COLLECTIONS`)
- **`src/Foundation/OZDictionary.m`** — Immutable dictionary for `@{...}` literals (`CONFIG_OBJZ_COLLECTIONS`)

### Clang / gnustep-2.0 Dispatch

- **`src/objc_msgSend.S`** — ARM Cortex-M Thumb-2 trampoline: `objc_msg_lookup(r0,r1)` + tail-call IMP
- **`cmake/ObjcClang.cmake`** — Clang cross-compilation with `-fobjc-runtime=gnustep-2.0`, provides `objz_target_sources()` (always ARC), `objz_compile_objc_sources()` (MRR, runtime only)
- **`cmake/objc_sections.ld`** — Linker script for gnustep-2.0 ObjC metadata sections (`__objc_selectors`, `__objc_classes`, etc.) with `__start_`/`__stop_` boundary symbols

### ARC Layer (always enabled)

- **`src/arc.c`** — ARC entry points: `objc_retain`, `objc_release`, `objc_storeStrong`, `objc_autorelease`, return-value optimization (ARM Thumb-2 `mov r7, r7` marker + TLS flag)
- **`include/objc/arc.h`** — ARC entry point declarations. Weak stubs panic.
- All user `.m` files compile with `-fobjc-arc` via `objz_target_sources()`. Runtime Foundation stays MRR.

### Static Allocation Pools (`CONFIG_OBJZ_STATIC_POOLS`)

- **`src/pool.c`** — Pool registry: maps class names to `K_MEM_SLAB` instances. `__objc_pool_get_slab()` API for runtime queries.
- **`include/objc/pool.h`** — `OZ_DEFINE_POOL(ClassName, block_size, count, align)` macro. Must be in a `.c` file (not `.m`).
- **`scripts/gen_pools.py`** — Auto-generates pool definitions from Clang AST analysis. All pools are auto-generated; no manual `pools.c` files needed.
- **`scripts/gen_table_sizes.py`** — Auto-computes runtime table sizes via tree-sitter source analysis. Generates `table_sizes.h`. No Clang AST dumps needed.
- **`scripts/requirements.txt`** — Python deps: `tree-sitter`, `tree-sitter-objc`.

### Init Order

`SYS_INIT(objz_init, APPLICATION, 99)` initializes the heap. Static pools register at priority 98. `STATIC_INIT_GNU` runs static constructors (`.init_array`) which trigger `__objc_load()` for gnustep-2.0 metadata registration.

### Static Table Sizes

Auto-computed via tree-sitter at build time. Kconfig defaults to 0 (auto); non-zero overrides. Statically allocated, no dynamic growth.

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

Conventional commits: `feat(runtime): description`, `fix(runtime): description`, `build: description`, `samples: description`
