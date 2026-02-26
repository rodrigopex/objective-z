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

- **`src/api.h`** — ABI structures (`objc_class`, `objc_method`, `objc_selector`, `objc_init`), gnustep-2.0 types
- **`src/load.c`** — Entry point: `__objc_load()` called via `.init_array` static constructor. Registers classes, categories, protocols, and fixes constant string isa pointers from gnustep-2.0 metadata sections.
- **`src/message.c`** — Core dispatch: `objc_msg_lookup()` / `objc_msg_lookup_super()`, lazy class resolution, sends `+initialize` on first use
- **`src/class.c`** — Class table (32 slots), lazy resolution of superclasses, gnustep-2.0 non-fragile ivar offset fixup
- **`src/hash.c`** — Global method hash table (512 slots, open addressing)
- **`src/category.c`** — Category table (32 slots), deferred loading until class is resolved
- **`src/malloc.c`** — Dedicated `sys_heap` (default 4096 bytes via `CONFIG_OBJZ_MEM_POOL_SIZE`) with spinlock
- **`src/Object.m`** — Root class: alloc/init/dealloc/class/respondsToSelector. Checks static pools before heap fallback.
- **`src/OZString.m`** — Backs `@"..."` literals; aliased to `NSString` under Clang

### Clang / gnustep-2.0 Dispatch

- **`src/objc_msgSend.S`** — ARM Cortex-M Thumb-2 trampoline: `objc_msg_lookup(r0,r1)` + tail-call IMP
- **`cmake/ObjcClang.cmake`** — Clang cross-compilation with `-fobjc-runtime=gnustep-2.0`, provides `objz_compile_objc_sources()`, `objz_compile_objc_arc_sources()`, `objz_target_sources()`, `objz_target_arc_sources()`
- **`cmake/objc_sections.ld`** — Linker script for gnustep-2.0 ObjC metadata sections (`__objc_selectors`, `__objc_classes`, etc.) with `__start_`/`__stop_` boundary symbols

### MRR Layer (`CONFIG_OBJZ_MRR`)

- **`src/refcount.c`** — Atomic refcount core (pure C, Zephyr `atomic_inc/dec/get/set`)
- **`src/OZObject.m`** — Managed root class: `+alloc` sets rc=1, `-retain/-release/-autorelease` delegate to refcount.c
- **`src/OZAutoreleasePool.m`** — Per-thread pool stack (`__thread`), `@autoreleasepool {}` via `objc_autoreleasePoolPush/Pop`
- **`include/objc/OZObject.h`** / **`include/objc/OZAutoreleasePool.h`** — Public headers

### ARC Layer (`CONFIG_OBJZ_ARC`)

- **`src/arc.c`** — ARC entry points: `objc_retain`, `objc_release`, `objc_storeStrong`, `objc_autorelease`, return-value optimization (ARM Thumb-2 `mov r7, r7` marker + TLS flag)
- **`include/objc/arc.h`** — ARC entry point declarations. Weak stubs panic.

### Static Allocation Pools (`CONFIG_OBJZ_STATIC_POOLS`)

- **`src/pool.c`** — Pool registry: maps class names to `K_MEM_SLAB` instances
- **`include/objc/pool.h`** — `OZ_DEFINE_POOL(ClassName, block_size, count, align)` macro. Must be in a `.c` file (not `.m`).

### Init Order

`SYS_INIT(objz_init, APPLICATION, 99)` initializes the heap. Static pools register at priority 98. `STATIC_INIT_GNU` runs static constructors (`.init_array`) which trigger `__objc_load()` for gnustep-2.0 metadata registration.

### Static Table Sizes

Class: 32, Category: 32, Protocol: 32, Hash: 512, Pool: 16. All statically allocated, no dynamic growth. Configurable via Kconfig.

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
