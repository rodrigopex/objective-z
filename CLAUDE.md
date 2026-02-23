# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Objective-Z is an Objective-C runtime for Zephyr RTOS, ported from [djthorpe/objc](https://github.com/djthorpe/objc) (minimal GCC-compatible ObjC runtime). It is packaged as a Zephyr module (`objc/zephyr/module.yml`) and implements the GCC ObjC ABI (version 8), NOT the Apple/NeXT runtime.

## Build Commands

Default board: `qemu_cortex_m3`. Requires Zephyr SDK, west, and ARM cross-compiler.

| Command | Description |
|---------|-------------|
| `just build` / `just b` | Build default sample (hello_world) |
| `just rebuild` | Pristine rebuild |
| `just run` / `just r` | Run in QEMU |
| `just flash` / `just f` | Flash to hardware |
| `just monitor` / `just m` | Serial monitor via tio |
| `just clean` / `just c` | Remove build dir |

Build a specific sample: `just build project_dir=samples/hello_category`

Each sample registers the runtime via `ZEPHYR_EXTRA_MODULES` in its CMakeLists.txt and enables it with `CONFIG_OBJZ=y` in prj.conf.

## Architecture

### Runtime Module (`objc/`)

- **`src/api.h`** — All ABI structures: `objc_class`, `objc_method`, `objc_selector`, `objc_module`, etc.
- **`src/module.c`** — Entry point: `__objc_exec_class()` called by compiler for each ObjC module during static init (`STATIC_INIT_GNU`)
- **`src/message.c`** — Core dispatch: `objc_msg_lookup()` / `objc_msg_lookup_super()`, sends `+initialize` on first use
- **`src/class.c`** — Class table (32 slots), lazy resolution of superclasses and methods
- **`src/hash.c`** — Global method hash table (512 slots, open addressing), methods registered with and without type info
- **`src/category.c`** — Category table (32 slots), deferred loading until class is resolved
- **`src/malloc.c`** — Dedicated `sys_heap` (default 4096 bytes via `CONFIG_OBJZ_MEM_POOL_SIZE`) with spinlock, separate from Zephyr heap
- **`src/Object.m`** — Root class: alloc/init/dealloc/class/respondsToSelector
- **`src/NXConstantString.m`** — Backs `@"..."` literals; aliased to `NSString` under Clang

### Init Order

`SYS_INIT(objz_init, APPLICATION, 99)` initializes the heap. `STATIC_INIT_GNU` runs GCC static constructors which trigger `__objc_exec_class()` for module registration.

### Static Table Sizes

Class: 32, Category: 32, Protocol: 32, Statics: 32, Hash: 512. All statically allocated, no dynamic growth.

## Coding Conventions

### C/ObjC Style

- `.clang-format`: LLVM-based, **8-space tab indentation**, Linux braces, column limit 100, `InsertBraces: true`
- Use `/* comment */` for documentation, `/** comment */` for Doxygen (not `//`)
- Always use curly braces with `if`, even single-line blocks
- Avoid `typedef` for structs — use explicit `struct objc_xxx` names (exception: public API types like `id`, `SEL`, `Class` per ObjC spec)
- Internal functions: `__objc_` prefix (double underscore)
- ObjC ivars: underscore prefix (`_color`, `_model`)
- Manual memory management (no ARC)
- Use `#import` for ObjC headers, `#include` for C headers

### Commit Messages

Format: `subsystem: description` (e.g., `runtime: fix Object includes`, `samples: Add zbus_service sample`)
