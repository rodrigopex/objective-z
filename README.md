# Objective-Z

Objective-C transpiler for Zephyr RTOS.

Converts `.m` sources to plain C via Clang AST analysis — no ObjC runtime needed. Packaged as a Zephyr module with a Platform Abstraction Layer (PAL) for zero-cost Zephyr integration.

## Motivation

### Why a transpiler?

Existing Objective-C runtimes — Apple libobjc, GNUstep libobjc2, ObjFW, mulle-objc — assume a general-purpose heap. All dispatch tables, class tables, selector tables, and object instances are `malloc`'d at runtime. This is fine for desktop/mobile but incompatible with deterministic embedded firmware on MCUs with 64–512 KB RAM and no MMU.

### Static-first design

Objective-Z inverts this: the transpiler converts `.m` files to plain C at build time via Clang JSON AST analysis. Dispatch tables are `const` vtable arrays in `.rodata` (FLASH), indexed by `class_id` — zero RAM overhead. When the receiver type is known at transpile time, protocol calls are resolved to direct function calls via compile-time dispatch (`OZ_SEND` macro with token concatenation). Object instances are served from per-class `k_mem_slab` pools (BSS), auto-generated from AST analysis. No heap allocation needed.

### No dynamic ObjC magic

Features that require unbounded runtime allocation — KVO, method swizzling, dynamic class creation, associated objects, weak references, message forwarding — are removed. Only the core language features that can be fully resolved at build time remain: classes, protocols, categories, properties, blocks, ARC.

### Zephyr-native

Built on Zephyr primitives (`k_mem_slab`, `SYS_INIT`, `k_spinlock_t`, `atomic_t`), not POSIX. No libc `malloc` dependency.

### Why not just C++?

C++ virtual dispatch is 2x slower than Objective-Z vtable dispatch on real hardware (20 vs 10 cycles, nRF52833 @ 64 MHz). Objective-Z also offers:

- **Familiar ObjC syntax** — `[obj method]`, `@property`, `@autoreleasepool`, `@synchronized`
- **Compile-time ARC** — automatic `retain`/`release` with scope-based cleanup, no manual `delete`
- **Protocol-based polymorphism** — protocol vtables with zero-cost conformance checking
- **Foundation classes** — `OZString`, `OZArray`, `OZDictionary`, `OZNumber` with fast enumeration
- **Slab allocation** — 4x faster than C++ `new`/`delete` (246 vs 995 cycles)

## How It Compares

All benchmarks on **nRF52833 DK** (ARM Cortex-M4F @ 64 MHz), DWT cycle counter, 10 000 iterations.

### Dispatch & Allocation

| Operation                      |  C++ | ObjC (transpiler) |
| ------------------------------ | ---: | ----------------: |
| Static / direct call           |    2 |               5–9 |
| Compile-time protocol dispatch |   — |               5–9 |
| Const vtable dispatch (polymorphic) |   20 |                19 |
| Slab / heap alloc+dealloc      |  995 |               228 |
| Atomic increment (refcount)    |   14 |                17 |

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
- **Static dispatch** — direct C function calls for non-protocol methods (5–9 cycles)
- **Compile-time dispatch** — protocol calls resolved to direct calls when receiver type known at transpile time (5–9 cycles)
- **Protocol vtable dispatch** — `const` vtable arrays in `.rodata` (zero RAM), 19 cycles polymorphic fallback for `id`-typed receivers
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
- **`+initialize`** — auto-called before `main()` via `SYS_INIT` (singleton pattern)
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
3. **Emit** (`emit.py`) — Generates per-class `.h`/`.c` files + `oz_dispatch.h`/`.c` (`const` vtable arrays, compile-time dispatch macros, slab definitions)

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
- **`oz_dispatch.h`** — class ID enum, `OZ_IMPL_*` compile-time dispatch macros, `OZ_SEND()` generic macro, `OZ_PROTOCOL_SEND_*` polymorphic fallback macros
- **`oz_dispatch.c`** — `const` vtable arrays (`OZ_PROTOCOL_RESOLVE_*`) in `.rodata`, class introspection tables

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
| `just bench`           | Run ObjC benchmark (build + flash)     |
| `just bench-cpp`       | Run C++ comparison benchmark           |
| `just bench-mem`       | Run memory comparison (C, C++, ObjC)   |
| `just test-bench`      | Run all benchmarks via twister (HW)    |
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

Cycle-accurate benchmarks on **nRF52833 DK** (ARM Cortex-M4F @ 64 MHz) using the DWT cycle counter. 10 000 iterations, 100 warmup, overhead-calibrated.

```sh
just board=nrf52833dk/nrf52833 bench       # ObjC transpiler benchmark
just board=nrf52833dk/nrf52833 bench-cpp   # C++ comparison benchmark
just board=nrf52833dk/nrf52833 bench-mem   # Memory comparison (C, C++, ObjC)
just test-bench                            # Run all via twister (hardware map)
```

### Dispatch

Transpiler uses `const` vtable arrays in `.rodata` indexed by `class_id`. When the receiver type is known at transpile time, protocol calls are resolved to direct function calls at zero cost. For truly polymorphic calls (`id`-typed receivers), a single `const` array dereference — no hash lookups, no cache, zero RAM:

| Operation                         | Cycles |   ns |
| --------------------------------- | -----: | ---: |
| C function pointer (baseline)     |      8 |  125 |
| Static dispatch (direct call)     |      9 |  140 |
| Class method (static function)    |      5 |   78 |
| Const vtable dispatch (depth=0)   |     19 |  296 |
| Const vtable dispatch (depth=1)   |     19 |  296 |
| Const vtable dispatch (depth=2)   |     19 |  296 |

> **Key result:** With compile-time dispatch, most protocol calls resolve to direct function calls (5–9 cycles) when the receiver type is known at transpile time — same cost as static dispatch. Const vtable dispatch (19 cycles, depth-independent) is the polymorphic fallback for `id`-typed receivers. Vtable arrays are in `.rodata` (FLASH) — zero RAM overhead. C++ virtual calls (20 cycles) use vptr indirection from RAM.

### Object Lifecycle

| Operation                     | Cycles |    ns |
| ----------------------------- | -----: | ----: |
| slab alloc + init + release   |    228 | 3,562 |

> Slab-based allocation (228 cycles) is **4.4x faster** than C++ `new`/`delete` (995 cycles) and **4.7x faster** than `unique_ptr` create/destroy (1,072 cycles). No heap metadata overhead — `alloc`/`free` are direct slab operations.

### Reference Counting

| Operation                       | Cycles |  ns |
| ------------------------------- | -----: | --: |
| `OZObject_retain` (atomic inc)  |     17 | 265 |
| retain + release pair           |     44 | 687 |

> `OZObject_retain` (17 cycles) is close to raw `atomic_fetch_add` (14 cycles in C++) — only 3 cycles overhead for null check. Both are inline operations with no function call overhead.

### Logging

Comparison of `printk`, Zephyr `LOG_INF` (minimal mode), and `OZLog` (50 iterations):

| Operation                    |  Cycles |        ns |
| ---------------------------- | ------: | --------: |
| `printk` (simple string)     |  94,462 | 1,475,968 |
| `LOG_INF` (simple string)    | 111,138 | 1,736,531 |
| `OZLog` (simple string)      |  94,456 | 1,475,875 |
| `printk` (integer format)    |  61,100 |   954,687 |
| `LOG_INF` (integer format)   |  77,777 | 1,215,265 |
| `OZLog` (integer format)     |  61,104 |   954,750 |
| `printk` (string format)     |  66,664 | 1,041,625 |
| `LOG_INF` (string format)    |  83,343 | 1,302,234 |
| `OZLog` (string format)      |  66,662 | 1,041,593 |

> On real hardware with UART output, logging is dominated by serial I/O (~1 ms per line). `OZLog` matches `printk` exactly — both use the same `printk` backend. `LOG_INF` adds ~17% overhead from the logging subsystem.

### Object Sizes

| Object                    |  Size |
| ------------------------- | ----: |
| OZObject (class_id + rc)  |   8 B |
| BenchBase (+ 1 int ivar)  |  12 B |
| BenchChild (no extra)     |  12 B |
| BenchGrandChild (no extra)|  12 B |

**Key takeaways:**

- **Compile-time dispatch eliminates vtable lookups** — most protocol calls resolve to direct function calls (5–9 cycles) at transpile time, same cost as static dispatch.
- **Const vtable dispatch matches C++** (19 vs 20 cycles) — only used for truly polymorphic `id`-typed receivers. Vtable arrays in `.rodata` (zero RAM).
- **Slab allocation is 4.4x faster** than C++ `new`/`delete` (228 vs 995 cycles). Per-class `k_mem_slab` pools have zero allocator overhead.
- **Inline ARC** adds only 3 cycles over raw atomics (17 vs 14 cycles).
- **OZLog matches printk** — zero overhead over the underlying `printk` backend on real UART.

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

Side-by-side C++ vs Objective-Z on **nRF52833 DK** (ARM Cortex-M4F @ 64 MHz). All values in cycles.

#### Dispatch

| Operation                          | C++ | ObjC (transpiler) |
| ---------------------------------- | --: | ----------------: |
| C function pointer (baseline)      |   8 |                 8 |
| Direct / static / class method     |   2 |               5–9 |
| Compile-time protocol dispatch     |  — |               5–9 |
| Virtual / const vtable (depth=0)   |  20 |                19 |
| Virtual / const vtable (depth=1)   |  21 |                19 |
| Virtual / const vtable (depth=2)   |  20 |                19 |

> With compile-time dispatch, most protocol calls resolve to direct function calls (5–9 cycles) — same cost as static dispatch. Const vtable dispatch (19 cycles, depth-independent) matches C++ virtual calls (20–21 cycles) and is only used for truly polymorphic `id`-typed receivers. Vtable arrays are `const` in `.rodata` — zero RAM overhead.

#### Object Lifecycle

| Operation                              |   C++ | ObjC (transpiler) |
| -------------------------------------- | ----: | ----------------: |
| Slab / heap alloc+dealloc              |   995 |               228 |
| Placement new + dtor + slab free       |   130 |                 — |
| `unique_ptr` create/destroy            | 1,072 |                 — |

> Transpiler slab alloc+init+release (228 cycles) is **4.4x faster** than C++ `new`/`delete` (995 cycles). Both C++ placement new (130 cycles) and ObjC slab (228 cycles) use `k_mem_slab` — the extra ObjC cycles cover `init` vtable call + ARC release.

#### Reference Counting

| Operation                     |   C++ | ObjC (transpiler) |
| ----------------------------- | ----: | ----------------: |
| Atomic increment              |    14 |                17 |
| Atomic inc + dec pair         |    24 |                44 |
| `shared_ptr` copy             | 1,140 |                 — |
| `shared_ptr` copy + reset     | 1,182 |                 — |

> `OZObject_retain` (17 cycles) is close to raw `atomic_fetch_add` (14 cycles) — 3 cycles for null check. C++ `shared_ptr` operations (1,140+ cycles) are 67x slower due to control block allocation and atomic operations on the control block.

#### Introspection (C++)

| Operation          | Cycles |
| ------------------ | -----: |
| `dynamic_cast` (hit)  |      2 |
| `dynamic_cast` (miss) |      2 |
| `typeid()`             |      4 |

#### Lambdas / std::function (C++)

| Operation                          | Cycles |
| ---------------------------------- | -----: |
| C function pointer call            |      8 |
| Non-capturing lambda (func ptr)    |      9 |
| `std::function` invocation         |     25 |
| `std::function` copy + destroy     |    102 |

### Memory Comparison

Per-object memory cost across C, C++, and Objective-Z on **nRF52833 DK**. C/C++ use a dedicated 8 KB `sys_heap`. ObjC uses per-class `k_mem_slab` pools (zero allocator overhead).

#### Object Sizes

| Metric                     |    C |  C++ | ObjC (transpiler) |
| -------------------------- | ---: | ---: | ----------------: |
| Base object (sizeof)       |  8 B |  8 B |               8 B |
| Child (+ 1 int)            | 12 B | 12 B |              12 B |
| GrandChild (+ 2 ints)      | 16 B | 16 B |              16 B |
| Dispatch mechanism         |  4 B |  4 B |       4 B (enum)  |
| Refcount field             |  4 B |  4 B |               4 B |

> Object sizes are identical — all embed a 4 B dispatch field + 4 B refcount. The transpiler uses `enum oz_class_id` as vtable index instead of a vptr.

#### Single Allocation

| Object type   |     C |   C++ | ObjC (transpiler) |
| ------------- | ----: | ----: | ----------------: |
| Base          | 16 B  | 16 B  |               8 B |
| Child         | 16 B  | 16 B  |              12 B |
| GrandChild    | 24 B  | 24 B  |              16 B |

> Transpiler slab allocation has **zero overhead** — block size equals `sizeof(struct)`. C/C++ `sys_heap` adds 4–8 B per allocation (chunk header).

#### Bulk Allocation (20 objects)

| Object type        |      C |    C++ | ObjC (transpiler) |
| ------------------ | -----: | -----: | ----------------: |
| 20x Child          | 320 B  | 320 B  |             240 B |
| 20x GrandChild     | 480 B  | 480 B  |             320 B |
| Per GrandChild avg |  24 B  |  24 B  |              16 B |

> 33% less memory per object with slab allocation (16 B vs 24 B) — zero heap metadata overhead.

#### Smart Pointers / Reference Counting (C++)

| Metric                          |               C++ |          ObjC |
| ------------------------------- | ----------------: | ------------: |
| `sizeof(unique_ptr)`            |               4 B |             — |
| `sizeof(shared_ptr)`            |               8 B |             — |
| `make_unique` heap cost         |              16 B |             — |
| `make_shared` heap cost         |    24 B (+ ctrl)  |             — |
| `shared_ptr(new T)` heap cost   | 40 B (2 allocs)   |             — |
| Manual `atomic<int>` refcount   |     4 B (inline)  |  4 B (inline) |

> ObjC stores the refcount inline (0 extra heap cost). C++ `make_shared` adds a ~16 B control block; `shared_ptr(new T)` does two allocations totaling 40 B.

## License

Apache-2.0
