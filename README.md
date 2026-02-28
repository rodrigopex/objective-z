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
| `objc_msgSend` (cold cache, depth=0) | 560 | 22,400 |
| `objc_msgSend` (cold cache, depth=2) | 1,274 | 50,960 |

### Object Lifecycle

| Operation | Cached | No cache | Unit |
|---|---:|---:|---|
| alloc/init/release (heap) | 5,456 | 6,647 | cycles |
| alloc/init/release (static pool) | 3,373 | 4,564 | cycles |

### Reference Counting

| Operation | Cycles | ns |
|---|---:|---:|
| retain (via dispatch) | 240 | 9,600 |
| retain + release pair | 320 | 12,800 |
| `objc_retain` (ARC, direct C call) | 59 | 2,360 |
| `objc_release` (ARC) | 135 | 5,400 |
| `objc_storeStrong` (ARC) | 221 | 8,840 |

### Introspection

| Operation | Cached | No cache | Unit |
|---|---:|---:|---|
| `class_respondsToSelector` (YES) | 151 | 493 | cycles |
| `class_respondsToSelector` (NO) | 1,159 | 1,095 | cycles |
| `object_getClass` | 20 | 20 | cycles |

### Blocks

| Operation | Cycles | ns |
|---|---:|---:|
| C function pointer call (baseline) | 10 | 400 |
| Global block invocation | 20 | 800 |
| Heap block invocation (int capture) | 20 | 800 |
| `_Block_copy` + `_Block_release` (int capture) | 413 | 16,520 |
| `_Block_copy` (retain heap block) | 48 | 1,920 |

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

### Logging

Comparison of `printk`, Zephyr `LOG_INF` (minimal mode), and `OZLog` (50 iterations):

| Operation | Cycles | ns |
|---|---:|---:|
| `printk` (simple string) | 2,301 | 92,040 |
| `LOG_INF` (simple string) | 2,903 | 116,120 |
| `OZLog` (simple string) | 13,133 | 525,320 |
| `printk` (integer format) | 2,196 | 87,840 |
| `LOG_INF` (integer format) | 2,797 | 111,880 |
| `OZLog` (integer format) | 13,728 | 549,120 |
| `printk` (string format) | 2,039 | 81,560 |
| `LOG_INF` (string format) | 2,640 | 105,600 |
| `OZLog` (string format) | 13,738 | 549,520 |

### Memory Footprint

Runtime cost vs bare Zephyr (mps2/an385, benchmark sample with all features):

| Configuration | FLASH | RAM | FLASH delta | RAM delta |
|---|---:|---:|---:|---:|
| Bare Zephyr (no ObjC) | 12,172 B | 6,120 B | — | — |
| All features enabled | 38,568 B | 30,508 B | +26,396 B | +24,388 B |

Dispatch cache cost (`CONFIG_OBJZ_DISPATCH_CACHE`, default `y`):

| Metric | Cached | No cache | Delta |
|---|---:|---:|---:|
| FLASH | 38,568 B | 38,112 B | +456 B |
| RAM (BSS + data) | 30,508 B | 29,476 B | +1,032 B |

Blocks runtime cost (`CONFIG_OBJZ_BLOCKS`, default `n`):

| Metric | Blocks on | Blocks off | Delta |
|---|---:|---:|---:|
| FLASH | 38,568 B | 35,560 B | +3,008 B |
| RAM (BSS + data) | 30,508 B | 30,476 B | +32 B |

**Key takeaways:**

- **Dispatch cache cuts overhead from ~42x to ~16x** a direct C function call. The per-class dispatch table (`CONFIG_OBJZ_DISPATCH_CACHE`) resolves method lookups via pointer hashing after the first call. Cold-cache sends fall back to the global hash table with `strcmp` matching.
- **Inheritance depth is free** after warm-up: cached inherited methods (depth=1, depth=2) all resolve in ~208 cycles, the same as direct methods. The IMP is cached at the receiver's class level, eliminating the superclass chain walk. Without cache, each level adds ~300-400 cycles.
- **Cache cost:** +1,032 B RAM (static BSS pool for 8 dtables), +456 B FLASH (code). Configurable via `CONFIG_OBJZ_DISPATCH_CACHE_STATIC_COUNT` and `CONFIG_OBJZ_DISPATCH_TABLE_SIZE`.
- **ARC retain vs message dispatch**: `objc_retain` (59 cycles) vs `[obj retain]` (240 cycles cached) — ARC entry points bypass message dispatch entirely.
- **Static pools are ~38% faster** than heap allocation (`sys_heap` with spinlock).
- **Block invocation matches C function pointers** at 20 cycles (vs 10 for a raw `call`). The overhead comes from `_Block_copy` (stack-to-heap promotion): 413 cycles per copy, but retaining an already-heap block is only 48 cycles. Each heap block costs 32 B (56 B with `__block` variables due to the `Block_byref` structure).
- **OZLog vs printk**: OZLog is ~6-7x slower than bare `printk` due to the custom format parser, `@autoreleasepool` push/pop, and per-specifier `snprintk`. `LOG_INF` in minimal mode adds ~26-29% over `printk` (prefix formatting).
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
# Required (ARC is always enabled for user .m files)
CONFIG_OBJZ=y

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
| `CONFIG_OBJZ` | Enable Objective-C runtime (ARC always on) | — |
| `CONFIG_OBJZ_DISPATCH_CACHE` | Per-class dispatch table cache | `OBJZ` |
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

## ARC Guide

Automatic Reference Counting (ARC) lets the compiler manage `retain`/`release` calls for you. ARC is always enabled — all `.m` files compiled via `objz_target_sources()` use `-fobjc-arc`.

### How it works

Under ARC, Clang inserts `objc_retain`/`objc_release` calls at compile time. You never call `retain`, `release`, or `autorelease` explicitly — the compiler does it.

```objc
#import <Foundation/Foundation.h>

@interface Sensor : Object
@property (nonatomic, strong) id delegate;
- (void)measure;
@end

@implementation Sensor
@synthesize delegate = _delegate;

- (void)measure
{
    OZLog("Measuring...");
}

- (void)dealloc
{
    OZLog("Sensor deallocated");
    /* ARC auto-inserts [super dealloc] — do NOT call it yourself */
}
@end

void demo(void)
{
    Sensor *s = [[Sensor alloc] init]; /* rc=1 */
    [s measure];
    /* ARC releases s here — dealloc fires automatically */
}
```

### Strong properties and `.cxx_destruct`

When a class has `strong` properties (or ivars), ARC generates a hidden `.cxx_destruct` method that releases them before `-dealloc` runs. This works through the entire class hierarchy:

```objc
@interface Driver : Object
@property (nonatomic, strong) Sensor *sensor;
@end

@implementation Driver
@synthesize sensor = _sensor;

- (void)dealloc
{
    OZLog("Driver deallocated");
    /* .cxx_destruct already released _sensor before we get here */
}
@end

void demo(void)
{
    Driver *d = [[Driver alloc] init];
    d.sensor = [[Sensor alloc] init];
    /* ARC releases d → .cxx_destruct releases sensor → both dealloc */
}
```

### `@autoreleasepool` — when you need it

ARC handles most cases, but `@autoreleasepool` is critical in **loops that create many temporary objects**. Without it, temporaries accumulate until the enclosing scope ends — a serious problem on memory-constrained embedded systems.

```objc
/* BAD: all 1000 temporaries live until function returns */
void process_bad(void)
{
    for (int i = 0; i < 1000; i++) {
        id tmp = [SomeFactory create]; /* autoreleased by factory */
        /* tmp stays alive... */
    }
    /* all 1000 objects released here — peak memory is huge */
}

/* GOOD: each iteration drains its pool */
void process_good(void)
{
    for (int i = 0; i < 1000; i++) {
        @autoreleasepool {
            id tmp = [SomeFactory create];
            /* tmp released at end of @autoreleasepool block */
        }
    }
    /* peak memory: only 1 object at a time */
}
```

Use `@autoreleasepool` when:
- **Loops** create temporary objects (factory methods, `-description`, string operations)
- **Worker threads** — each thread needs its own pool before any autorelease happens
- **Batch processing** — any code path that allocates many short-lived objects

### Retain cycles — the one thing ARC cannot fix

ARC has no weak references on this runtime. If two objects hold `strong` references to each other, neither can be deallocated:

```objc
@interface Node : Object
@property (nonatomic, strong) Node *next;
@end

void leak(void)
{
    Node *a = [[Node alloc] init];
    Node *b = [[Node alloc] init];
    a.next = b;
    b.next = a; /* cycle: a→b→a */
    /* ARC releases locals, but the cycle keeps both alive — LEAK */
}
```

Avoid cycles by breaking the reference chain before the owner goes out of scope:

```objc
void no_leak(void)
{
    Node *a = [[Node alloc] init];
    Node *b = [[Node alloc] init];
    a.next = b;
    b.next = a;

    /* Break cycle before scope exit */
    b.next = nil;
    /* Now: a→b, b→nil. ARC releases a → releases b → both dealloc */
}
```

### ARC rules summary

| Do | Don't |
|---|---|
| Use `objz_target_sources()` in CMake | Call `retain`, `release`, or `autorelease` |
| Let the compiler manage object lifetime | Call `[super dealloc]` — ARC inserts it |
| Use `@autoreleasepool` in loops/threads | Create strong reference cycles |
| Use `strong` properties for ownership | Mix MRR and ARC in the same `.m` file |
| Break cycles manually before scope exit | Assume temporaries are released immediately |

### Static pools with ARC

Static allocation pools (`CONFIG_OBJZ_STATIC_POOLS`) work transparently with ARC. Pools are auto-generated by `gen_pools.py` from Clang AST analysis — no manual `pools.c` files needed. The build system determines instance sizes and max concurrent counts automatically.

```objc
/* main.m — compiled with ARC */
void demo(void)
{
    Sensor *s = [[Sensor alloc] init]; /* allocated from slab */
    [s measure];
    /* ARC releases s → dealloc returns block to slab */
}
```

## License

Apache-2.0
