# Objective-Z

Objective-C runtime for Zephyr RTOS.

Ported from [djthorpe/objc](https://github.com/djthorpe/objc) (minimal GCC-compatible ObjC runtime), packaged as a Zephyr module. Supports GCC ABI v8, gnustep-1.7 ABI v9, and gnustep-1.7+ARC ABI v10.

## Features

- Class and instance method dispatch (`objc_msg_lookup` / `objc_msgSend`)
- Categories and protocols
- `@"..."` string literals (OZString / NSString alias under Clang)
- Manual Retain/Release (MRR) with `OZObject` root class
- Automatic Reference Counting (ARC) with `-fobjc-arc`
- `@autoreleasepool` blocks via per-thread pool stack
- Static allocation pools using Zephyr `K_MEM_SLAB` — zero heap allocation per class
- Zephyr zbus integration examples (pub/sub and request-response)
- ARM Cortex-M Thumb-2 `objc_msgSend` trampoline for gnustep-1.7 direct dispatch

## Prerequisites

- [Zephyr SDK](https://docs.zephyrproject.org/latest/develop/getting_started/index.html) and `west`
- Clang (for compiling `.m` files with gnustep-1.7 ABI)
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
| `mem_demo` | Manual Retain/Release lifecycle with `OZObject` | `+OBJZ_MRR` |
| `arc_demo` | Automatic Reference Counting, scoped cleanup | `+OBJZ_MRR +OBJZ_ARC` |
| `pool_demo` | Static allocation pools with `K_MEM_SLAB` | `+OBJZ_MRR +OBJZ_STATIC_POOLS` |
| `zbus_objc` | ObjC objects with Zephyr zbus pub/sub messaging | `+ZBUS` |
| `zbus_service` | Request-response service pattern over zbus | `+ZBUS` |
| `benchmark` | Cycle-accurate runtime performance benchmarks | `+OBJZ_MRR +OBJZ_ARC +OBJZ_STATIC_POOLS` |

Build a specific sample:

```sh
just project_dir=samples/arc_demo rebuild
just run
```

### hello_world

```objc
#import <objc/objc.h>
#include <zephyr/kernel.h>

@interface MyFirstObject : Object
- (void)greet;
+ (void)greet;
@end

@implementation MyFirstObject
- (void)greet { printk("Hello, world from object\n"); }
+ (void)greet { printk("Hello, world from class\n"); }
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

| Operation | Cycles | ns |
|---|---:|---:|
| C function call (baseline, cached IMP) | 13 | 520 |
| `objc_msgSend` (instance method) | 551 | 22,040 |
| `objc_msgSend` (class method) | 733 | 29,320 |
| `objc_msgSend` (inherited depth=1) | 868 | 34,720 |
| `objc_msgSend` (inherited depth=2) | 1,264 | 50,560 |

### Object Lifecycle

| Operation | Cycles | ns |
|---|---:|---:|
| alloc/init/release (heap, MRR) | 7,889 | 315,560 |
| alloc/init/release (static pool) | 6,276 | 251,040 |

### Reference Counting

| Operation | Cycles | ns |
|---|---:|---:|
| retain (MRR, via dispatch) | 1,037 | 41,480 |
| retain + release pair (MRR) | 2,132 | 85,280 |
| `objc_retain` (ARC, direct C call) | 45 | 1,800 |
| `objc_release` (ARC) | 109 | 4,360 |
| `objc_storeStrong` (ARC) | 196 | 7,840 |

### Introspection

| Operation | Cycles | ns |
|---|---:|---:|
| `class_respondsToSelector` (YES) | 493 | 19,720 |
| `class_respondsToSelector` (NO) | 1,604 | 64,160 |
| `object_getClass` | 20 | 800 |

**Key takeaways:**

- **Dispatch overhead is ~42x** a direct C function call — the runtime has no method cache, so every send walks the hash table with `strcmp` matching. This is a deliberate trade-off for minimal RAM usage on constrained MCUs.
- **Superclass chain cost**: each inheritance level adds ~300-400 cycles as the runtime traverses parent classes during lookup.
- **ARC vs MRR retain**: `objc_retain` (45 cycles) vs `[obj retain]` (1,037 cycles) — ARC entry points bypass message dispatch entirely, making them ~23x faster.
- **Static pools are ~20% faster** than heap allocation (`sys_heap` with spinlock).
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

# Include ObjC compilation helpers
include(${CMAKE_CURRENT_SOURCE_DIR}/../objc/cmake/ObjcClang.cmake)

# Compile .m sources with Clang
objz_target_sources(app src/main.m)

# For ARC-enabled sources, use instead:
# objz_target_arc_sources(app src/main.m)
```

### 3. prj.conf

```ini
# Required
CONFIG_OBJZ=y

# Optional: Manual Retain/Release
CONFIG_OBJZ_MRR=y

# Optional: Automatic Reference Counting (requires MRR)
CONFIG_OBJZ_ARC=y

# Optional: Static allocation pools
CONFIG_OBJZ_STATIC_POOLS=y
```

### 4. Write your .m file

```objc
#import <objc/objc.h>       /* Core runtime: Object, id, SEL, Class */
#import <objc/OZObject.h>   /* Managed root class (MRR/ARC) */
#include <zephyr/kernel.h>
```

Use `Object` as root class for lightweight objects. Use `OZObject` when you need reference counting (MRR or ARC).

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
| `CONFIG_OBJZ` | Enable Objective-C runtime | — |
| `CONFIG_OBJZ_MRR` | Manual Retain/Release with `OZObject` | `OBJZ` |
| `CONFIG_OBJZ_ARC` | Automatic Reference Counting | `OBJZ_MRR` |
| `CONFIG_OBJZ_STATIC_POOLS` | Per-class static allocation pools | `OBJZ` |

### Table Sizes

All tables are statically allocated. Tune via Kconfig if defaults are insufficient:

| Kconfig | Default | Description |
|---|---|---|
| `CONFIG_OBJZ_CLASS_TABLE_SIZE` | 32 | Max registered classes |
| `CONFIG_OBJZ_CATEGORY_TABLE_SIZE` | 32 | Max categories |
| `CONFIG_OBJZ_PROTOCOL_TABLE_SIZE` | 32 | Max protocols |
| `CONFIG_OBJZ_HASH_TABLE_SIZE` | 512 | Method hash table slots |
| `CONFIG_OBJZ_DISPATCH_TABLE_SIZE` | 64 | Per-class dispatch table (power-of-2) |
| `CONFIG_OBJZ_MEM_POOL_SIZE` | 4096 | Heap size in bytes |
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
