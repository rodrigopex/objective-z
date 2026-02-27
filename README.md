# Objective-Z

Objective-C runtime for Zephyr RTOS.

Ported from [djthorpe/objc](https://github.com/djthorpe/objc) (minimal GCC-compatible ObjC runtime), packaged as a Zephyr module. Uses the gnustep-2.0 ABI with Clang for ObjC compilation.

## Features

- Class and instance method dispatch (`objc_msg_lookup` / `objc_msgSend`)
- Categories and protocols
- `@"..."` string literals (OZString / NSString alias under Clang)
- Boxed literals (`@42`, `@YES`, `@3.14`) and collection literals (`@[...]`, `@{...}`)
- Blocks (closures) with `-fblocks` — global, stack, and heap blocks with `__block` variable support
- Fast enumeration (`for...in` loops) on OZArray and OZDictionary
- `enumerateObjectsUsingBlock:` for block-based iteration
- Manual Retain/Release (MRR) built into the `Object` root class
- Automatic Reference Counting (ARC) with `-fobjc-arc`
- `@autoreleasepool` blocks via per-thread pool stack
- Static allocation pools using Zephyr `K_MEM_SLAB` — zero heap allocation per class
- `OZLog()` with `%@` format specifier and `-description` support
- Zephyr zbus integration examples (pub/sub and request-response)
- ARM Cortex-M Thumb-2 `objc_msgSend` trampoline for gnustep-2.0 direct dispatch

## Prerequisites

- [Zephyr SDK](https://docs.zephyrproject.org/latest/develop/getting_started/index.html) and `west`
- Clang (for compiling `.m` files with gnustep-2.0 ABI)
- [just](https://github.com/casey/just) (build automation, optional)

## Quick Start

Build and run the hello_world sample in QEMU:

```sh
# Build
west build -p -b mps2/an385 samples/hello_world

# Run in QEMU
west build -t run
```

Or with `just`:

```sh
just rebuild
just run
```

Expected output:

```
Hello, world from class
Hello, world from object
```

Exit QEMU with `Ctrl+A`, then `x`.

## Samples

| Sample | Description | Kconfig |
|---|---|---|
| `hello_world` | Basic class/instance method dispatch | `CONFIG_OBJZ=y` |
| `hello_category` | Categories (method extensions on existing classes) | `CONFIG_OBJZ=y` |
| `mem_demo` | Manual Retain/Release lifecycle | `CONFIG_OBJZ=y` |
| `arc_demo` | Automatic Reference Counting, scoped cleanup | `+OBJZ_ARC` |
| `pool_demo` | Static allocation pools with `K_MEM_SLAB` | `+OBJZ_STATIC_POOLS` |
| `literals_demo` | Boxed literals and collection literals (`@42`, `@[...]`, `@{...}`) | `+OBJZ_LITERALS` |
| `blocks_demo` | Blocks, `__block` variables, fast enumeration, `enumerateObjectsUsingBlock:` | `+OBJZ_BLOCKS +OBJZ_LITERALS` |
| `zbus_objc` | ObjC objects with Zephyr zbus pub/sub messaging | `+ZBUS` |
| `zbus_service` | Request-response service pattern over zbus | `+ZBUS` |
| `benchmark` | Cycle-accurate runtime performance benchmarks | `+OBJZ_ARC +OBJZ_BLOCKS +OBJZ_STATIC_POOLS` |

Build a specific sample:

```sh
just project_dir=samples/arc_demo rebuild
just run
```

### hello_world

```objc
#import <objc/objc.h>

@interface MyFirstObject : Object
- (void)greet;
+ (void)greet;
@end

@implementation MyFirstObject
- (void)greet { OZLog("Hello, world from object"); }
+ (void)greet { OZLog("Hello, world from class"); }
@end

int main(void) {
    [MyFirstObject greet];
    MyFirstObject *hello = [[MyFirstObject alloc] init];
    [hello greet];
    [hello dealloc];
    return 0;
}
```

## Benchmark

The `benchmark` sample measures key runtime operations with cycle-accurate timing using the DWT cycle counter. Results below are from QEMU (mps2/an385, ARM Cortex-M3, 25 MHz):

```sh
just project_dir=samples/benchmark rebuild
just run
```

### Message Dispatch

With dispatch cache (`CONFIG_OBJZ_DISPATCH_CACHE=y`, default):

| Operation | Cycles | ns |
|---|---:|---:|
| C function call (baseline, cached IMP) | 13 | 520 |
| `objc_msgSend` (instance method) | 208 | 8,320 |
| `objc_msgSend` (class method) | 215 | 8,600 |
| `objc_msgSend` (inherited depth=1) | 208 | 8,320 |
| `objc_msgSend` (inherited depth=2) | 208 | 8,320 |
| `objc_msgSend` (cold cache, depth=0) | 2,400 | 96,000 |
| `objc_msgSend` (cold cache, depth=2) | 3,120 | 124,800 |

Without dispatch cache (`CONFIG_OBJZ_DISPATCH_CACHE=n`):

| Operation | Cycles | ns |
|---|---:|---:|
| C function call (baseline, cached IMP) | 13 | 520 |
| `objc_msgSend` (instance method) | 551 | 22,040 |
| `objc_msgSend` (class method) | 733 | 29,320 |
| `objc_msgSend` (inherited depth=1) | 868 | 34,720 |
| `objc_msgSend` (inherited depth=2) | 1,264 | 50,560 |

### Object Lifecycle

| Operation | Cached | No cache | Unit |
|---|---:|---:|---|
| alloc/init/release (heap, MRR) | 3,908 | 6,970 | cycles |
| alloc/init/release (static pool) | 1,927 | 4,989 | cycles |

### Reference Counting

| Operation | Cached | No cache | Unit |
|---|---:|---:|---|
| retain (MRR, via dispatch) | 269 | 1,018 | cycles |
| retain + release pair (MRR) | 544 | 2,093 | cycles |
| `objc_retain` (ARC, direct C call) | 58 | 58 | cycles |
| `objc_release` (ARC) | 135 | 135 | cycles |
| `objc_storeStrong` (ARC) | 221 | 221 | cycles |

### Introspection

| Operation | Cached | No cache | Unit |
|---|---:|---:|---|
| `class_respondsToSelector` (YES) | 151 | 493 | cycles |
| `class_respondsToSelector` (NO) | 1,159 | 1,095 | cycles |
| `object_getClass` | 20 | 20 | cycles |

### Blocks

| Operation | Cycles | ns |
|---|---:|---:|
| C function pointer call (baseline) | 11 | 440 |
| Global block invocation | 20 | 800 |
| Heap block invocation (int capture) | 20 | 800 |
| `_Block_copy` + `_Block_release` (int capture) | 2,900 | 116,000 |
| `_Block_copy` (retain heap block) | 154 | 6,160 |

### Block Memory

| Metric | Size |
|---|---:|
| C function pointer | 4 B |
| Block pointer (reference) | 4 B |
| `struct Block_layout` | 20 B |
| Block + int capture (descriptor size) | 24 B |
| Block + ObjC object capture (descriptor size) | 24 B |
| Block + `__block` int (descriptor size) | 24 B |
| Heap cost: `_Block_copy` (int capture) | 32 B |
| Heap cost: `_Block_copy` (obj capture) | 32 B |
| Heap cost: `_Block_copy` (`__block` int) | 56 B |

### Memory Footprint

Dispatch cache cost (`CONFIG_OBJZ_DISPATCH_CACHE`, default `y`):

| Metric | Cached | No cache | Delta |
|---|---:|---:|---:|
| FLASH | 31,916 B | 31,596 B | +320 B |
| RAM (BSS + data) | 30,592 B | 29,568 B | +1,024 B |
| ObjC heap peak | 288 B | 48 B | +240 B |

Blocks runtime cost (`CONFIG_OBJZ_BLOCKS`, default `n`):

| Metric | Blocks on | Blocks off | Delta |
|---|---:|---:|---:|
| FLASH | 34,596 B | 31,916 B | +2,680 B |
| RAM (BSS + data) | 30,624 B | 30,592 B | +32 B |

**Key takeaways:**

- **Dispatch cache cuts overhead from ~42x to ~16x** a direct C function call. The per-class dispatch table (`CONFIG_OBJZ_DISPATCH_CACHE`) resolves method lookups via pointer hashing after the first call. Cold-cache sends fall back to the global hash table with `strcmp` matching.
- **Inheritance depth is free** after warm-up: cached inherited methods (depth=1, depth=2) all resolve in ~208 cycles, the same as direct methods. The IMP is cached at the receiver's class level, eliminating the superclass chain walk. Without cache, each level adds ~300-400 cycles.
- **Cache cost:** +1,024 B RAM (static BSS pool for 8 dtables), +240 B heap (overflow dtables), +320 B FLASH (code). Configurable via `CONFIG_OBJZ_DISPATCH_CACHE_STATIC_COUNT` and `CONFIG_OBJZ_DISPATCH_TABLE_SIZE`.
- **ARC vs MRR retain**: `objc_retain` (58 cycles) vs `[obj retain]` (269 cycles cached, 1,018 uncached) — ARC entry points bypass message dispatch entirely.
- **Static pools are ~51% faster** than heap allocation (`sys_heap` with spinlock).
- **Block invocation matches C function pointers** at 20 cycles (vs 11 for a raw `call`). The overhead comes from `_Block_copy` (stack-to-heap promotion): 2,900 cycles per copy, but retaining an already-heap block is only 154 cycles. Each heap block costs 32 B (56 B with `__block` variables due to the `Block_byref` structure).
- **QEMU caveat**: these are instruction-accurate counts, not true cycle-accurate. Real hardware numbers will differ, but relative comparisons hold.

## Using in Your Project

### 1. Directory layout

Place the `objc/` runtime alongside your application (or use west to manage it):

```
my_app/
├── CMakeLists.txt
├── prj.conf
├── src/
│   └── main.m
└── ../objc/          # Objective-Z runtime module
```

### 2. CMakeLists.txt

```cmake
cmake_minimum_required(VERSION 3.20.0)

# Register Objective-Z as an extra module
set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../objc/")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(my_app)

# Compile .m sources with Clang
objz_target_sources(app src/main.m)

# For ARC-enabled sources, use instead:
# objz_target_arc_sources(app src/main.m)
```

### 3. prj.conf

```ini
# Required (MRR is enabled by default)
CONFIG_OBJZ=y

# Optional: Automatic Reference Counting
CONFIG_OBJZ_ARC=y

# Optional: Blocks (closures)
CONFIG_OBJZ_BLOCKS=y

# Optional: Boxed/collection literals (@42, @[...], @{...})
CONFIG_OBJZ_LITERALS=y

# Optional: Static allocation pools
CONFIG_OBJZ_STATIC_POOLS=y
```

### 4. Write your .m file

```objc
#import <objc/objc.h>       /* Core runtime: Object, id, SEL, Class */
#include <zephyr/kernel.h>
```

Use `Object` as the root class. It provides retain/release/autorelease out of the box.

### 5. Build

```sh
west build -p -b mps2/an385 .
```

## CMake Helpers

| Function | Description |
|---|---|
| `objz_target_sources(target, files...)` | Routes `.m` to Clang, `.c` to GCC |
| `objz_target_arc_sources(target, files...)` | Same as above but adds `-fobjc-arc` to `.m` files |
| `objz_compile_objc_sources(target, files...)` | Compile `.m` files only with Clang |
| `objz_compile_objc_arc_sources(target, files...)` | Compile `.m` files only with ARC |

## Configuration

### Feature Flags

| Kconfig | Description | Depends on |
|---|---|---|
| `CONFIG_OBJZ` | Enable Objective-C runtime (includes MRR) | — |
| `CONFIG_OBJZ_DISPATCH_CACHE` | Per-class dispatch table cache | `OBJZ` |
| `CONFIG_OBJZ_ARC` | Automatic Reference Counting | `OBJZ` |
| `CONFIG_OBJZ_BLOCKS` | Blocks (closures) with `-fblocks` | `OBJZ` |
| `CONFIG_OBJZ_LITERALS` | Boxed literals and collection literals | `OBJZ` |
| `CONFIG_OBJZ_STATIC_POOLS` | Per-class static allocation pools | `OBJZ` |

### Table Sizes

All tables are statically allocated. Tune via Kconfig if defaults are insufficient:

| Kconfig | Default | Description |
|---|---|---|
| `CONFIG_OBJZ_CLASS_TABLE_SIZE` | 32 | Max registered classes |
| `CONFIG_OBJZ_CATEGORY_TABLE_SIZE` | 32 | Max categories |
| `CONFIG_OBJZ_PROTOCOL_TABLE_SIZE` | 32 | Max protocols |
| `CONFIG_OBJZ_HASH_TABLE_SIZE` | 512 | Method hash table slots |
| `CONFIG_OBJZ_DISPATCH_TABLE_SIZE` | 16 | Entries per dispatch cache (power-of-2) |
| `CONFIG_OBJZ_DISPATCH_CACHE_STATIC_COUNT` | 8 | Static dtable blocks in BSS |
| `CONFIG_OBJZ_MEM_POOL_SIZE` | 4096 | Heap size in bytes |
| `CONFIG_OBJZ_LOG_BUFFER_SIZE` | 128 | OZLog format buffer size |
| `CONFIG_OBJZ_STATIC_POOL_TABLE_SIZE` | 16 | Max static pool registrations |

## Build Commands

Requires [just](https://github.com/casey/just). Default board: `mps2/an385`.

| Command | Description |
|---|---|
| `just build` | Incremental build |
| `just rebuild` | Pristine rebuild |
| `just run` | Run in QEMU |
| `just flash` | Flash to hardware |
| `just monitor` | Serial monitor (tio) |
| `just clean` | Remove build directory |
| `just test` | Run twister on all samples |

Override defaults:

```sh
just project_dir=samples/arc_demo board=nucleo_f429zi rebuild
just flash
```

## License

Apache-2.0
