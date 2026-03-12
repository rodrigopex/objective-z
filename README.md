# Objective-Z

Objective-C transpiler for Zephyr RTOS.

Converts `.m` sources to plain C via Clang AST analysis — no ObjC runtime needed. Packaged as a Zephyr module with a Platform Abstraction Layer (PAL) for zero-cost Zephyr integration.

## Motivation

### Why a transpiler?

Existing Objective-C runtimes — Apple libobjc, GNUstep libobjc2, ObjFW, mulle-objc — assume a general-purpose heap. All dispatch tables, class tables, selector tables, and object instances are `malloc`'d at runtime. This is fine for desktop/mobile but incompatible with deterministic embedded firmware on MCUs with 64–512 KB RAM and no MMU.

### Static-first design

Objective-Z inverts this: the transpiler converts `.m` files to plain C at build time via Clang JSON AST analysis. All dispatch tables are statically sized vtable arrays indexed by `class_id`. Object instances are served from per-class `k_mem_slab` pools (BSS), auto-generated from AST analysis. No heap allocation needed.

### No dynamic ObjC magic

Features that require unbounded runtime allocation — KVO, method swizzling, dynamic class creation, associated objects, weak references, message forwarding — are removed. Only the core language features that can be fully resolved at build time remain: classes, protocols, categories, properties, blocks, ARC.

### Zephyr-native

Built on Zephyr primitives (`k_mem_slab`, `SYS_INIT`, `k_spinlock_t`, `atomic_t`), not POSIX. No libc `malloc` dependency.

### Why not just C++?

C++ virtual dispatch matches Objective-Z vtable dispatch at 16 cycles (see [benchmarks](#benchmark)). Objective-Z trades raw static dispatch speed for:

- **Familiar ObjC syntax** — `[obj method]`, `@property`, `@autoreleasepool`, `@synchronized`
- **Compile-time ARC** — automatic `retain`/`release` with scope-based cleanup, no manual `delete`
- **Protocol-based polymorphism** — protocol vtables with zero-cost conformance checking
- **Foundation classes** — `OZString`, `OZArray`, `OZDictionary`, `OZNumber` with fast enumeration
- **Slab allocation** — 8x faster than C++ `new`/`delete` (320 vs 2,548 cycles)

### Why not just Rust?

Rust is excellent for embedded but:

- **Zephyr integration** is still maturing (zephyr-lang-rust module required)
- **Ownership model** has a learning curve; ObjC ARC is simpler for teams already using C
- **Binary size** — Rust benchmarks show 22.3 KB FLASH / 30.8 KB RAM vs transpiler's 16.9 KB / 9.9 KB
- **Vtable dispatch** — Rust trait objects (22 cycles) are slightly slower than ObjC vtable (16 cycles)
- **Object allocation** — `Box::new` + drop (2,496 cycles) is 7.8x slower than slab alloc (320 cycles)

## How It Compares

### Dispatch & Allocation

| Operation                      |    C |  C++ | Rust |  Zig | ObjC (transpiler) |
| ------------------------------ | ---: | ---: | ---: | ---: | ----------------: |
| Static / direct call           |   13 |   ~0 |   ~1 |   ~0 |              7–10 |
| Virtual / vtable dispatch      |    — |   16 |   22 |   16 |                16 |
| Slab / heap alloc+dealloc      |    — | 2548 | 2496 | 2054 |               320 |
| Atomic increment (refcount)    |    — |   29 |   16 |   16 |                39 |

### Foundation Classes

| Class          | Description                                   |
| -------------- | --------------------------------------------- |
| `OZObject`     | Root class — alloc, init, dealloc, retain/release, isEqual |
| `OZString`     | Immutable strings — cStr, length, isEqual     |
| `OZArray`      | Immutable arrays — count, objectAtIndex, for-in enumeration |
| `OZDictionary` | Immutable dictionaries — count, objectForKey, for-in enumeration |
| `OZNumber`     | Tagged union — int8/16/32 (signed/unsigned), float, BOOL |
| `OZLog`        | printf-style logging with `%@` object specifier |

## Features

- **Three-pass transpiler** — Clang JSON AST → collect → resolve → emit → pure C
- **Static dispatch** — direct C function calls for non-protocol methods (7–10 cycles)
- **Protocol vtable dispatch** — indexed vtable arrays, 16 cycles, depth-independent
- **Compile-time ARC** — scope-based retain/release, auto-dealloc, break/continue cleanup
- **Categories** — merged at AST collection time
- **`@property` / `@synthesize`** — atomic and strong semantics
- **`@synchronized`** — RAII spinlock via OZLock
- **`@autoreleasepool`** — scoped memory management
- **Blocks** — non-capturing blocks transpiled to static C functions
- **`__block` variables** — promoted to file-scope static
- **Fast enumeration** — `for (id obj in collection)` via IteratorProtocol
- **Boxed literals** — `@42`, `@3.14f`, `@YES`
- **Collection literals** — `@[a, b, c]`, `@{key: value}`
- **Subscript syntax** — `array[0]`, `dict[@"key"]`
- **Lightweight generics** — typed collections
- **`+initialize`** — called once on first class message (singleton pattern)
- **Per-class slab pools** — auto-generated from AST analysis, zero heap overhead
- **Platform Abstraction Layer** — zero-cost `static inline` with Zephyr and host backends
- **clangd IDE support** — auto-generated `compile_commands.json`

## Prerequisites

- Zephyr SDK + west (see [Zephyr Getting Started](https://docs.zephyrproject.org/latest/develop/getting_started/index.html))
- Clang (for AST analysis — Apple Clang works for ARM, Homebrew LLVM for RISC-V)
- Python 3
- [just](https://github.com/casey/just) (build automation)

## Quick Start

```sh
# Clone and enter
git clone https://github.com/peixotooo/objective-z.git
cd objective-z

# Build the hello_world sample
just rebuild

# Run in QEMU
just run
```

Expected output:

```
Hello, world from class
Hello, world from object
```

## Samples

12 samples under `samples/`, each demonstrating different transpiler features:

| Sample                 | Description                                        |
| ---------------------- | -------------------------------------------------- |
| `hello_world`          | Basic class and instance method dispatch            |
| `hello_category`       | Category extensions (adding methods to classes)     |
| `arc_demo`             | ARC lifecycle, scoped cleanup, singletons, threads  |
| `mem_demo`             | ARC memory management, autorelease pools            |
| `pool_demo`            | Static slab pools, `@autoreleasepool`, `@synchronized` |
| `transpiled_blocks`    | Blocks, `__block` variables, fast enumeration       |
| `transpiled_literals`  | Boxed literals (`@42`) and collection literals (`@[]`, `@{}`) |
| `transpiled_generics`  | Lightweight generics with typed collections         |
| `transpiled_led`       | LED control demo (OZLed class)                      |
| `gpio_demo`            | GPIO input/output with Zephyr devicetree            |
| `zbus_objc`            | Zephyr zbus pub/sub messaging                       |
| `zbus_service`         | Request-response service pattern                    |

Build a specific sample:

```sh
just project_dir=samples/arc_demo rebuild
just run
```

### hello_world

```objc
#import <Foundation/Foundation.h>

@interface MyFirstObject: OZObject
- (void)greet;
+ (void)greet;
@end

@implementation MyFirstObject

- (void)greet
{
    OZLog("Hello, world from object");
}

+ (void)greet
{
    OZLog("Hello, world from class");
}

@end

int main(void)
{
    [MyFirstObject greet];

    MyFirstObject *hello = [[MyFirstObject alloc] init];
    [hello greet];

    return 0;
}
```

The transpiler converts this to plain C: `MyFirstObject_greet(self)` for instance methods, `MyFirstObject_class_greet()` for class methods, and `OZObject_slab_alloc()`/`OZObject_init()` for object creation. The generated code compiles with GCC — no ObjC compiler or runtime needed at build time.

## Architecture

```
.m sources → Clang JSON AST → oz_transpile (Python) → .h + .c → GCC → binary
```

### Transpiler Pipeline

Three-pass architecture in `tools/oz_transpile/`:

1. **Collect** (`collect.py`) — Walks Clang JSON AST nodes, builds `OZModule` with classes, methods, ivars, protocols, categories
2. **Resolve** (`resolve.py`) — Validates hierarchy, assigns topological class IDs, computes `base_depth`, classifies dispatch (STATIC vs PROTOCOL)
3. **Emit** (`emit.py`) — Generates per-class `.h`/`.c` files + `oz_dispatch.h`/`.c` (vtable arrays, slab definitions)

### Platform Abstraction Layer

Zero-cost `static inline` abstraction in `include/platform/`:

| Header                  | Purpose                                        |
| ----------------------- | ---------------------------------------------- |
| `oz_platform.h`         | `#ifdef` router (Zephyr vs Host)               |
| `oz_platform_zephyr.h`  | `k_mem_slab`, Zephyr atomics, `k_spinlock_t`, `printk` |
| `oz_platform_host.h`    | malloc-backed slab, C11 `stdatomic`, `printf`  |
| `oz_platform_types.h`   | Shared type definitions                        |
| `oz_lock.h`             | OZLock RAII spinlock for `@synchronized`        |
| `oz_assert.h`           | Assertion macros                               |

All PAL functions vanish at `-O1+` — zero runtime overhead.

### Generated Code

For each class, the transpiler emits:

- **`ClassName.h`** — struct definition, method prototypes, vtable extern
- **`ClassName.c`** — method implementations, vtable array, slab pool definition
- **`oz_dispatch.h`** — class ID enum, vtable type, dispatch macros
- **`oz_dispatch.c`** — global vtable arrays

## Using in Your Project

### 1. Directory layout

```
my_app/
├── CMakeLists.txt
├── prj.conf
├── src/
│   └── main.m
└── ../objective-z/   # Objective-Z module
```

### 2. CMakeLists.txt

```cmake
cmake_minimum_required(VERSION 3.20.0)

# Register Objective-Z as an extra module
set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../objective-z/")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(my_app)

# Transpile .m sources to C (ARC always enabled)
objz_transpile_sources(app src/main.m)
```

### 3. prj.conf

```ini
CONFIG_OBJZ=y
```

That's it. The transpiler automatically includes Foundation classes (OZObject, OZString, OZArray, OZDictionary, OZNumber) and generates slab pools for all classes found in the AST.

### 4. Write your .m file

```objc
#import <Foundation/Foundation.h>

@interface Sensor: OZObject {
    int _value;
}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Sensor
- (void)setValue:(int)v { _value = v; }
- (int)value { return _value; }

- (void)dealloc
{
    OZLog("Sensor dealloc (value=%d)", _value);
}
@end

int main(void)
{
    Sensor *s = [[Sensor alloc] init];
    [s setValue:42];
    OZLog("value=%d", [s value]);
    /* ARC releases s here → dealloc fires */
    return 0;
}
```

### 5. Build

```sh
west build -p -b mps2/an385 .
```

### CMake API

```
objz_transpile_sources(<target> <source1.m> [source2.m ...]
    [ROOT_CLASS <name>]
    [POOL_SIZES <Class1=N,Class2=M,...>]
    [INCLUDE_DIRS <dir1> [dir2 ...]]
)
```

| Parameter      | Default    | Description                              |
| -------------- | ---------- | ---------------------------------------- |
| `ROOT_CLASS`   | `OZObject` | Root class name for hierarchy resolution |
| `POOL_SIZES`   | auto       | Override slab pool sizes per class       |
| `INCLUDE_DIRS` | —          | Additional include directories for AST   |

## Configuration

`CONFIG_OBJZ` is the only Kconfig option. It enables the transpiler pipeline and auto-selects `STATIC_INIT_GNU`.

Supported architectures:

- ARM Cortex-M
- ARM Cortex-A
- RISC-V 32/64-bit (requires LLVM Clang, not Apple Clang)

## Build Commands

Requires [just](https://github.com/casey/just). Default board: `mps2/an385`.

| Command                | Description                            |
| ---------------------- | -------------------------------------- |
| `just build` / `just b`  | Incremental build                   |
| `just rebuild`         | Pristine rebuild                       |
| `just run` / `just r`    | Run in QEMU                         |
| `just flash` / `just f`  | Flash to hardware                   |
| `just monitor` / `just m` | Serial monitor (tio)               |
| `just clean` / `just c`  | Remove build directory               |
| `just test` / `just t`   | Run twister on all samples (ARM)    |
| `just test-transpiler` | Run transpiler pytest suite            |
| `just test-behavior`   | Run compiled behavior tests            |
| `just test-adapted`    | Run adapted upstream tests             |
| `just smoke`           | Run host-side PAL smoke test           |
| `just bench`           | Run ObjC benchmark                     |
| `just bench-cpp`       | Run C++ comparison benchmark           |
| `just bench-rust`      | Run Rust comparison benchmark          |
| `just bench-zig`       | Run Zig comparison benchmark           |
| `just bench-c3`        | Run C3 comparison benchmark (RISC-V)  |
| `just bench-mem`       | Run memory comparison (all languages)  |
| `just transpile`       | Run OZ transpiler directly             |
| `just ast-dump file`   | Clang JSON AST dump                    |

Override defaults:

```sh
just project_dir=samples/arc_demo board=nucleo_f429zi rebuild
just board=qemu_riscv32 rebuild   # RISC-V target
```

## Limitations

See [docs/LIMITATIONS.md](docs/LIMITATIONS.md) for the full list. Key limitations:

- **No `switch`/`case`** — use `if`/`else if` chains
- **Non-capturing blocks only** — blocks that capture local variables produce a diagnostic error
- **No boxed expressions** — `@(expr)` not supported; use explicit `OZNumber` initializers
- **No `typedef`** — use explicit types
- **No `@try`/`@catch`/`@throw`** — exception handling not supported
- **No dynamic dispatch** for non-protocol methods — all resolved statically
- **OZNumber**: 8/16/32-bit integers and float only (no int64/double)

## ARC Guide

Automatic Reference Counting (ARC) is always enabled. The transpiler inserts `retain`/`release` calls at compile time — you never call them manually.

### How it works

```objc
#import <Foundation/Foundation.h>

@interface Sensor: OZObject
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

When a class has `strong` properties (or ivars), ARC generates a hidden `.cxx_destruct` method that releases them before `-dealloc` runs:

```objc
@interface Driver: OZObject
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

### `@autoreleasepool`

Critical in loops that create temporary objects — without it, temporaries accumulate until the enclosing scope ends:

```objc
/* BAD: all 1000 temporaries live until function returns */
void process_bad(void)
{
    for (int i = 0; i < 1000; i++) {
        id tmp = [SomeFactory create];
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

- **Loops** create temporary objects
- **Worker threads** — each thread needs its own pool
- **Batch processing** — any code path that allocates many short-lived objects

### Retain cycles

ARC has no weak references (`__weak` panics at runtime). If two objects hold `strong` references to each other, neither can be deallocated:

```objc
/* PROBLEM: direct cycle — Parent ↔ Child */
@interface Parent: OZObject
@property (nonatomic) Child *child;   /* strong by default */
@end

@interface Child: OZObject
@property (nonatomic) Parent *parent; /* strong — creates cycle! */
@end
```

#### Fix: use `__unsafe_unretained`

```objc
@interface Child: OZObject
@property (nonatomic, unsafe_unretained) Parent *parent; /* non-owning */
@end
```

> **Caution:** `__unsafe_unretained` pointers are not zeroed on dealloc. Ensure the owner outlives the child, or set the back-reference to `nil` before the owner is released.

#### Alternative: break the cycle manually

```objc
void no_leak(void)
{
    Node *a = [[Node alloc] init];
    Node *b = [[Node alloc] init];
    a.next = b;
    b.next = a;

    b.next = nil; /* break cycle before scope exit */
    /* ARC releases a → releases b → both dealloc */
}
```

### ARC rules summary

| Do                                      | Don't                                          |
| --------------------------------------- | ---------------------------------------------- |
| Use `objz_transpile_sources()` in CMake | Call `retain`, `release`, or `autorelease`      |
| Let the compiler manage object lifetime | Call `[super dealloc]` — ARC inserts it         |
| Use `@autoreleasepool` in loops/threads | Create strong reference cycles                  |
| Use `strong` properties for ownership   | Assume temporaries are released immediately     |
| Break cycles manually before scope exit | Use `__weak` (not supported, panics at runtime) |

## Benchmark

Cycle-accurate benchmarks using the DWT cycle counter. Results from QEMU (mps2/an385, ARM Cortex-M3, 25 MHz):

```sh
just bench       # ObjC transpiler benchmark
just bench-cpp   # C++ comparison benchmark
just bench-rust  # Rust comparison benchmark
just bench-zig   # Zig comparison benchmark
just bench-mem   # Memory comparison (C, C++, Rust, Zig, ObjC)
```

### Dispatch

Transpiler uses compile-time vtable arrays indexed by `class_id`. No hash lookups, no cache — a single array dereference per dispatch:

| Operation                         | Cycles |    ns |
| --------------------------------- | -----: | ----: |
| C function pointer (baseline)     |     13 |   520 |
| Static dispatch (direct call)     |     10 |   400 |
| Class method (static function)    |      7 |   280 |
| Vtable dispatch (depth=0)         |     16 |   640 |
| Vtable dispatch (depth=1)         |     16 |   640 |
| Vtable dispatch (depth=2)         |     16 |   640 |

> **Key result:** Vtable dispatch (16 cycles) matches C++ virtual calls exactly. Inheritance depth is free — all depths resolve in identical cycles. Static dispatch (10 cycles) and class methods (7 cycles) inline to near-zero overhead.

### Object Lifecycle

| Operation                     | Cycles |     ns |
| ----------------------------- | -----: | -----: |
| slab alloc + init + release   |    320 | 12,800 |

> Slab-based allocation (320 cycles) is **14x faster** than legacy heap alloc (4,474 cycles) and **6.7x faster** than legacy static pools (2,151 cycles). No message dispatch overhead — `alloc`/`free` are direct slab operations, `init` is a vtable call.

### Reference Counting

| Operation                       | Cycles |    ns |
| ------------------------------- | -----: | ----: |
| `OZObject_retain` (atomic inc)  |     39 | 1,560 |
| retain + release pair           |     90 | 3,600 |

> `OZObject_retain` (39 cycles) is **1.5x faster** than legacy `objc_retain` (58 cycles) and **6.2x faster** than legacy retain-via-dispatch (240 cycles). The inline implementation avoids function call overhead and immortal-object guards.

### Logging

Comparison of `printk`, Zephyr `LOG_INF` (minimal mode), and `OZLog` (50 iterations):

| Operation                    | Cycles |      ns |
| ---------------------------- | -----: | ------: |
| `printk` (simple string)     |  2,301 |  92,040 |
| `LOG_INF` (simple string)    |  2,906 | 116,240 |
| `OZLog` (simple string)      |  3,325 | 133,000 |
| `printk` (integer format)    |  2,196 |  87,840 |
| `LOG_INF` (integer format)   |  2,797 | 111,880 |
| `OZLog` (integer format)     |  3,917 | 156,680 |
| `printk` (string format)     |  2,039 |  81,560 |
| `LOG_INF` (string format)    |  2,640 | 105,600 |
| `OZLog` (string format)      |  3,924 | 156,960 |

### Object Sizes

| Object                    |  Size |
| ------------------------- | ----: |
| OZObject (class_id + rc)  |   8 B |
| BenchBase (+ 1 int ivar)  |  12 B |
| BenchChild (no extra)     |  12 B |
| BenchGrandChild (no extra)|  12 B |

### Memory Footprint

Transpiler binary size (mps2/an385, benchmark sample):

| Configuration         |    FLASH |     RAM |
| --------------------- | -------: | ------: |
| Transpiler benchmark  | 21,684 B | 9,940 B |

**Key takeaways:**

- **Vtable dispatch matches C++** at 16 cycles per indirect call. No hash lookups, no cache misses — just an array index.
- **Slab allocation is 14x faster** than legacy heap (320 vs 4,474 cycles). Per-class `k_mem_slab` pools have zero allocator overhead.
- **Inline ARC is 1.5x faster** than legacy `objc_retain` (39 vs 58 cycles) and 6.2x faster than retain-via-dispatch (39 vs 240 cycles).
- **Binary size: 21.7 KB FLASH, 9.9 KB RAM** — 45% less FLASH and 62% less RAM than the legacy runtime benchmark (39.6 KB / 26.0 KB).
- **OZLog vs printk**: OZLog is ~1.4x `printk` for simple strings. `LOG_INF` in minimal mode adds ~26% over `printk`.
- **QEMU caveat**: these are instruction-accurate counts, not true cycle-accurate. Real hardware numbers will differ, but relative comparisons hold.

### Legacy Runtime Reference

<details>
<summary>Legacy runtime benchmark data (retained for comparison)</summary>

The following data is from the retired legacy ObjC runtime (`objc_msgSend`, heap allocation, ARC runtime). These benchmarks no longer build — the legacy runtime compilation path has been retired in favor of the transpiler.

#### Message Dispatch (Legacy)

With flat dispatch table (`CONFIG_OBJZ_FLAT_DISPATCH=y`):

| Operation                              | Cycles |    ns |
| -------------------------------------- | -----: | ----: |
| C function call (baseline, cached IMP) |     13 |   520 |
| `objc_msgSend` (instance method)       |    205 | 8,200 |
| `objc_msgSend` (class method)          |    212 | 8,480 |
| `objc_msgSend` (inherited depth=1)     |    205 | 8,200 |
| `objc_msgSend` (inherited depth=2)     |    205 | 8,200 |

Without flat dispatch (`CONFIG_OBJZ_FLAT_DISPATCH=n`):

| Operation                              | Cycles |     ns |
| -------------------------------------- | -----: | -----: |
| C function call (baseline, cached IMP) |     13 |    520 |
| `objc_msgSend` (instance method)       |    560 | 22,400 |
| `objc_msgSend` (class method)          |    743 | 29,720 |
| `objc_msgSend` (inherited depth=1)     |    887 | 35,480 |
| `objc_msgSend` (inherited depth=2)     |  1,328 | 53,120 |

#### Object Lifecycle (Legacy)

| Operation                        | Cycles |      ns |
| -------------------------------- | -----: | ------: |
| alloc/init/release (heap)        |  4,474 | 178,960 |
| alloc/init/release (static pool) |  2,151 |  86,040 |

#### Reference Counting (Legacy)

| Operation                          | Cycles |     ns |
| ---------------------------------- | -----: | -----: |
| retain (via dispatch)              |    240 |  9,600 |
| retain + release pair              |    320 | 12,800 |
| `objc_retain` (ARC, direct C call) |     58 |  2,320 |
| `objc_release` (ARC)               |    135 |  5,400 |
| `objc_storeStrong` (ARC)           |    221 |  8,840 |

#### Introspection (Legacy)

| Operation                        | Cycles |     ns |
| -------------------------------- | -----: | -----: |
| `class_respondsToSelector` (YES) |    148 |  5,920 |
| `class_respondsToSelector` (NO)  |    461 | 18,440 |
| `object_getClass`                |     20 |    800 |

#### Blocks (Legacy)

| Operation                                      | Cycles |      ns |
| ---------------------------------------------- | -----: | ------: |
| C function pointer call (baseline)             |     10 |     400 |
| Global block invocation                        |     20 |     800 |
| Heap block invocation (int capture)            |     20 |     800 |
| `_Block_copy` + `_Block_release` (int capture) |  3,060 | 122,400 |
| `_Block_copy` (retain heap block)              |    154 |   6,160 |

#### Block Memory (Legacy)

| Metric                                        | Size |
| --------------------------------------------- | ---: |
| C function pointer                            |  4 B |
| Block pointer (reference)                     |  4 B |
| `struct Block_layout`                         | 20 B |
| Block + int capture (descriptor size)         | 24 B |
| Block + ObjC object capture (descriptor size) | 24 B |
| Block + `__block` int (descriptor size)       | 24 B |
| Heap cost: `_Block_copy` (int capture)        | 32 B |
| Heap cost: `_Block_copy` (obj capture)        | 32 B |
| Heap cost: `_Block_copy` (`__block` int)      | 56 B |

#### Logging (Legacy)

| Operation                    | Cycles |      ns |
| ---------------------------- | -----: | ------: |
| `printk` (simple string)     |  2,301 |  92,040 |
| `LOG_INF` (simple string)    |  2,903 | 116,120 |
| `OZLog` (simple string)      |  3,280 | 131,200 |
| `printk` (integer format)    |  2,196 |  87,840 |
| `LOG_INF` (integer format)   |  2,797 | 111,880 |
| `OZLog` (integer format)     |  3,883 | 155,320 |
| `printk` (string format)     |  2,039 |  81,560 |
| `LOG_INF` (string format)    |  2,640 | 105,600 |
| `OZLog` (string format)      |  3,892 | 155,680 |
| `OZLog` (`%@` object format) |  8,480 | 339,200 |

#### Memory Footprint (Legacy)

| Configuration         |    FLASH |      RAM | FLASH delta | RAM delta |
| --------------------- | -------: | -------: | ----------: | --------: |
| Bare Zephyr (no ObjC) | 12,104 B |  6,120 B |           — |         — |
| All features enabled  | 39,568 B | 26,020 B |   +27,464 B | +19,900 B |

Flat dispatch table cost:

| Metric           | Flat dispatch | No flat dispatch |    Delta |
| ---------------- | ------------: | ---------------: | -------: |
| FLASH            |      39,568 B |         38,384 B | +1,184 B |
| RAM (BSS + data) |      26,020 B |         22,180 B | +3,840 B |

Blocks runtime cost:

| Metric           | Blocks on | Blocks off |    Delta |
| ---------------- | --------: | ---------: | -------: |
| FLASH            |  39,568 B |   36,576 B | +2,992 B |
| RAM (BSS + data) |  26,020 B |   25,996 B |    +24 B |

</details>

### C++ Comparison

Side-by-side C++ vs Objective-Z (same board, same iteration count, `just bench-cpp` vs `just bench`). All values in cycles. "ObjC (legacy)" column shows the retired runtime for reference.

#### Dispatch

| Operation                          | C++ | ObjC (transpiler) | ObjC (legacy) |
| ---------------------------------- | --: | ----------------: | ------------: |
| C function pointer (baseline)      |  13 |                13 |            13 |
| Static / class method              |  ~0 |              7–10 |           212 |
| Virtual / vtable dispatch (depth=0)|  16 |                16 |           205 |
| Virtual / vtable dispatch (depth=1)|  16 |                16 |           205 |
| Virtual / vtable dispatch (depth=2)|  16 |                16 |           205 |

> Transpiler vtable dispatch (16 cycles) matches C++ virtual calls exactly — both use a single indirect call through a function pointer array. Static/class methods inline to near-zero (7–10 cycles). Legacy `objc_msgSend` (205 cycles) did pointer-hash cache lookup + table index.

#### Object Lifecycle

| Operation                   |   C++ | ObjC (transpiler) | ObjC (legacy) |
| --------------------------- | ----: | ----------------: | ------------: |
| Slab / heap alloc+dealloc   | 2,548 |               320 |         4,474 |
| Static pool (slab)          |   170 |                 — |         2,151 |
| `unique_ptr` create/destroy | 2,612 |                 — |             — |

> Transpiler slab alloc+init+release (320 cycles) is **8x faster** than C++ `new`/`delete` (2,548 cycles). `k_mem_slab` is a fixed-size block pool with O(1) alloc/free — no heap metadata overhead.

#### Reference Counting

| Operation                 |   C++ | ObjC (transpiler) | ObjC (legacy) |
| ------------------------- | ----: | ----------------: | ------------: |
| Atomic increment          |    29 |                39 |            58 |
| Atomic inc + dec pair     |    52 |                90 |           135 |
| `shared_ptr` copy / —     | 2,740 |                 — |             — |

> Transpiler `OZObject_retain` (39 cycles) is competitive with raw `atomic_fetch_add` (29 cycles) — only 10 cycles overhead for null check. Both are inline operations with no function call or dispatch overhead.

### Rust Comparison

Side-by-side Rust vs Objective-Z (same board, same iteration count, `just bench-rust` vs `just bench`). All values in cycles. "ObjC (legacy)" column shows the retired runtime for reference.

**Prerequisites** (one-time setup):

```sh
west config manifest.project-filter "+zephyr-lang-rust,+nanopb"
west update
rustup target add thumbv7m-none-eabi
```

#### Dispatch (Rust)

| Operation                            | Rust | ObjC (transpiler) | ObjC (legacy) |
| ------------------------------------ | ---: | ----------------: | ------------: |
| Direct / C function call (baseline)  |   ~1 |                13 |            13 |
| Static function call                 |   ~1 |              7–10 |           212 |
| Trait object / vtable dispatch (d=0) |   22 |                16 |           205 |
| Trait object / vtable dispatch (d=1) |   22 |                16 |           205 |
| Trait object / vtable dispatch (d=2) |   22 |                16 |           205 |

> Transpiler vtable dispatch (16 cycles) is slightly faster than Rust trait objects (22 cycles) — both use indirect calls through function pointer arrays. Rust's 6-cycle overhead may be from fat pointer dereferencing. Legacy `objc_msgSend` was 9.3x slower.

#### Object Lifecycle (Rust)

| Operation                          |  Rust | ObjC (transpiler) | ObjC (legacy) |
| ---------------------------------- | ----: | ----------------: | ------------: |
| Slab / `Box::new` + drop (heap)   | 2,496 |               320 |         4,474 |
| `Box<dyn Trait>` create + drop     | 2,538 |                 — |             — |

> Transpiler slab alloc+init+release (320 cycles) is **7.8x faster** than Rust `Box::new` + drop (2,496 cycles). Slab allocation is O(1) with no heap metadata.

#### Reference Counting (Rust)

| Operation                | Rust | ObjC (transpiler) | ObjC (legacy) |
| ------------------------ | ---: | ----------------: | ------------: |
| Atomic increment         |   16 |                39 |            58 |
| Atomic inc + dec pair    |   38 |                90 |           135 |
| `Arc::clone`             |   22 |                 — |            58 |
| `Arc::clone` + drop      |   58 |                 — |           135 |

> Raw `AtomicI32::fetch_add` (16 cycles) vs transpiler `OZObject_retain` (39 cycles) — 2.4x gap due to null check + function call overhead. Both are inline operations. Legacy was 3.6x slower than Rust.

### Zig Comparison

Side-by-side Zig vs Objective-Z (same board, same iteration count, `just bench-zig` vs `just bench`). All values in cycles. "ObjC (legacy)" column shows the retired runtime for reference.

Zig uses fat-pointer interfaces (ptr + vtable\*) for dynamic dispatch — similar to Rust trait objects. No inheritance; "depth" refers to struct composition depth, which does not affect dispatch cost. Zig calls Zephyr C APIs directly via `@cImport` (no FFI shim needed).

#### Dispatch (Zig)

| Operation                        |  Zig | ObjC (transpiler) | ObjC (legacy) |
| -------------------------------- | ---: | ----------------: | ------------: |
| Direct function call (baseline)  |   ~0 |                13 |            13 |
| Static function call             |   ~0 |              7–10 |           212 |
| Interface / vtable dispatch (d=0)|   16 |                16 |           205 |
| Interface / vtable dispatch (d=1)|   12 |                16 |           205 |
| Interface / vtable dispatch (d=2)|   13 |                16 |           205 |

> Transpiler vtable dispatch (16 cycles) matches Zig interface dispatch (12–16 cycles) — both use function pointer array indirection. Legacy `objc_msgSend` was 12.8x–15.8x slower.

#### Object Lifecycle (Zig)

| Operation                        |  Zig | ObjC (transpiler) | ObjC (legacy) |
| -------------------------------- | ---: | ----------------: | ------------: |
| Slab / `k_malloc` + `k_free`    | 2,054 |               320 |         4,474 |
| Stack allocation (baseline)      |   10 |                 — |             — |

> Transpiler slab alloc+init+release (320 cycles) is **6.4x faster** than Zig `k_malloc` + `k_free` (2,054 cycles). Slab pools avoid heap overhead entirely.

#### Reference Counting (Zig)

| Operation                     |  Zig | ObjC (transpiler) | ObjC (legacy) |
| ----------------------------- | ---: | ----------------: | ------------: |
| Atomic increment              |   16 |                39 |            58 |
| Atomic inc + dec pair         |   41 |                90 |           135 |

> Zig `@atomicRmw` (16 cycles) vs transpiler `OZObject_retain` (39 cycles) — 2.4x gap from null check overhead. Legacy was 3.6x slower than Zig.

### C3 Comparison

Side-by-side C3 vs Objective-Z on RISC-V (`just board=qemu_riscv32 bench-c3` vs `just board=qemu_riscv32 bench`). All values in cycles. C3 currently only supports `elf-riscv32` for freestanding targets (no ARM Cortex-M), so all comparisons use `qemu_riscv32`. Tables include both ObjC transpiler and legacy runtime data.

C3 uses `interface` + `@dynamic` methods for dynamic dispatch — runtime registration via `.init_array` constructors, dispatch via vtable lookup through `any` type. Calls Zephyr C APIs directly via `extern fn` declarations (timing shim needed only for `static inline` helpers).

#### Dispatch (C3)

| Operation                       |  C3 | ObjC (transpiler) | ObjC (legacy) | C3 vs transpiler |
| ------------------------------- | --: | ----------------: | ------------: | ---------------: |
| Direct function call (baseline) |  12 |                 2 |             2 | 6.0x             |
| Static function call            |  12 |                 2 |             2 | 6.0x             |
| Interface dispatch (depth=0)    |   4 |                 5 |            71 | 0.8x             |
| Interface dispatch (depth=1)    |   4 |                 5 |            71 | 0.8x             |
| Interface dispatch (depth=2)    |   4 |                 5 |            71 | 0.8x             |

> C3 interface dispatch (4 cycles) uses `@dynamic` vtable lookup via `any` — comparable to C++ virtual calls. ObjC transpiler vtable dispatch (5 cycles) is nearly identical — both use indexed vtable arrays. Depth has no effect for either approach. Legacy ObjC `objc_msg_lookup_sender` on RISC-V (71 cycles) was 14x slower due to hash-based slot dispatch. Direct/static calls show C3 volatile-store overhead (12 cycles) vs ObjC's bare C call (2 cycles).

#### Object Lifecycle (C3)

| Operation                  |  C3 | ObjC (transpiler) | ObjC (legacy) | C3 vs transpiler |
| -------------------------- | --: | ----------------: | ------------: | ---------------: |
| Alloc + init + release     | 624 |                95 |         1,245 | 6.6x slower      |
| Stack allocation (baseline)|  12 |                 — |             — | —                |

> ObjC transpiler slab alloc + init + release (95 cycles) is 6.6x faster than C3 `k_malloc` + `k_free` (624 cycles) and 13.1x faster than legacy ObjC (1,245 cycles). Slab allocation is O(1) — no free-list walk or fragmentation.

#### Reference Counting (C3)

| Operation             |  C3 | ObjC (transpiler) | ObjC (legacy) | C3 vs transpiler |
| --------------------- | --: | ----------------: | ------------: | ---------------: |
| Atomic increment      |  12 |                 5 |            59 | 2.4x slower      |
| Atomic inc + dec pair |   1 |                11 |            85 | 0.1x             |

> ObjC transpiler `OZObject_retain` (5 cycles) is 2.4x faster than C3 `$$atomic_fetch_add` (12 cycles) — both are inline atomics, but C3 has volatile-store overhead. The C3 inc+dec pair (1 cycle) suggests optimizer combining. Legacy ObjC retain (59 cycles) added immortal-object guard + function call overhead.

#### Introspection (C3)

| Operation              |  C3 | ObjC (legacy) | Ratio |
| ---------------------- | --: | ------------: | ----- |
| `any.type` check (hit) |  12 |            43 | 3.6x  |
| `any.type` check (miss)|  12 |           128 | 10.7x |

> C3 `any` type stores a typeid alongside the pointer — type checks are a simple integer comparison (12 cycles, same cost hit or miss). ObjC `respondsToSelector:` is asymmetric: 43 cycles on hit (found in dispatch table) vs 128 cycles on miss (full method list walk).

#### Function Pointers (C3)

| Operation                          |  C3 | ObjC (legacy) | Ratio |
| ---------------------------------- | --: | ------------: | ----- |
| Function pointer call              |   1 |             2 | 0.5x  |
| Struct closure invocation          |   1 |             4 | 0.3x  |

> Plain function pointers (1 cycle) and struct closure invocation (1 cycle) are at or below timing resolution — effectively zero overhead. C3 closures are struct methods with a captured field.

#### Function Pointer Memory (C3)

| Metric                       |  C3 | ObjC (legacy) |
| ---------------------------- | --: | ------------: |
| Function pointer             | 4 B |           4 B |
| Struct closure (int capture) | 4 B |          24 B |
| `any` type (ptr + typeid)   | 8 B |          20 B |

> C3 struct closures with captures are 4 B (only the captured data). The `any` type (ptr + typeid = 8 B) is C3's runtime type erasure, comparable to Zig's fat pointers. ObjC blocks carry a fixed `Block_layout` header (20 B minimum).

### Memory Comparison

Per-object memory cost across C, C++, Rust, Zig, and Objective-C (`just bench-mem`). All values from the same board (mps2/an385, ARM Cortex-M3). C/C++/Rust/Zig use a dedicated 8 KB `sys_heap`. The transpiler ObjC column uses per-class `k_mem_slab` pools (zero allocator overhead). The legacy ObjC column (in parentheses) used `sys_heap`.

#### Object Sizes

| Metric                     |    C |  C++ | Rust |  Zig | ObjC (transpiler) | ObjC (legacy) |
| -------------------------- | ---: | ---: | ---: | ---: | ----------------: | ------------: |
| Base object (sizeof)       |  8 B |  8 B |  4 B |  4 B |               8 B |           8 B |
| Child (+ 1 int)            | 12 B | 12 B |  8 B |  8 B |              12 B |          12 B |
| GrandChild (+ 2 ints)      | 16 B | 16 B | 12 B | 12 B |              16 B |          16 B |
| Dispatch mechanism         |  4 B |  4 B |  8 B |  8 B |       4 B (enum)  |    4 B (isa)  |
| Refcount field             |  4 B |  4 B |  4 B |  4 B |               4 B |           4 B |

> Object sizes are identical between transpiler and legacy — both embed a 4 B dispatch field + 4 B refcount. The transpiler uses an `enum oz_class_id` (4 B) as vtable index instead of an isa pointer. Rust and Zig structs have no embedded dispatch pointer — dispatch goes through fat pointers (8 B) on the stack.

#### Single Allocation

| Object type   |     C |   C++ |  Rust |   Zig | ObjC (transpiler) | ObjC (legacy) |
| ------------- | ----: | ----: | ----: | ----: | ----------------: | ------------: |
| Base          | 16 B  | 16 B  |  8 B  |  8 B  |               8 B |          16 B |
| Child         | 16 B  | 16 B  | 16 B  | 16 B  |              12 B |          16 B |
| GrandChild    | 24 B  | 24 B  | 16 B  | 16 B  |              16 B |          24 B |

> Transpiler slab allocation has **zero overhead** per object — block size equals `sizeof(struct)`. Legacy and C/C++ used `sys_heap` which adds ~8 B per allocation (chunk header). The transpiler matches or beats Rust/Zig for per-object memory.

#### Bulk Allocation (20 objects)

| Object type        |      C |    C++ |   Rust |    Zig | ObjC (transpiler) | ObjC (legacy) |
| ------------------ | -----: | -----: | -----: | -----: | ----------------: | ------------: |
| 20x Child          | 320 B  | 320 B  | 320 B  | 320 B  |             240 B |         320 B |
| 20x GrandChild     | 480 B  | 480 B  | 320 B  | 320 B  |             320 B |         480 B |
| Per GrandChild avg |  24 B  |  24 B  |  16 B  |  16 B  |              16 B |          24 B |

> Transpiler slab allocation (16 B/GrandChild) matches Rust/Zig — zero allocator overhead means per-object cost equals `sizeof`. Legacy ObjC and C/C++ paid 8 B overhead per allocation.

#### Smart Pointers / Reference Counting

| Metric                          |        C |               C++ |        Rust |       Zig |          ObjC |
| ------------------------------- | -------: | ----------------: | ----------: | --------: | ------------: |
| `sizeof` pointer on stack       |      4 B |     4 B (unique)  |   4 B (Box) |       4 B |           4 B |
|                                 |        — |     8 B (shared)  |   4 B (Arc) |         — |             — |
| Control block (heap)            |      0 B | ~16 B (make_shared) | ~8 B (Arc) |       0 B |          0 B |
| Refcount storage                | inline   |            inline |  ctrl block |    inline |        inline |

> ObjC (both transpiler and legacy), C, and Zig store the refcount inline (0 extra heap). The transpiler uses `oz_atomic_t` (4 B inline in `OZObject`).

#### Binary Size

| Metric |       C |     C++ |     Zig |    Rust | ObjC (transpiler) | ObjC (legacy) |
| ------ | ------: | ------: | ------: | ------: | ----------------: | ------------: |
| FLASH  | 14.1 KB | 15.6 KB | 18.7 KB | 22.3 KB |           16.9 KB |       29.8 KB |
| RAM    | 17.4 KB | 21.4 KB | 20.9 KB | 30.8 KB |            9.9 KB |       25.4 KB |

> Transpiler binary size (16.9 KB FLASH, 9.9 KB RAM) is competitive with C/C++. The 43% FLASH and 61% RAM reduction vs legacy comes from eliminating the runtime (class tables, message dispatch, ARC runtime, Foundation classes). Only generated vtable arrays, slab definitions, and PAL headers remain.

## License

Apache-2.0
