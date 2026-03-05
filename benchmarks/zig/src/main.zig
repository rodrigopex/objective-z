// Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
// SPDX-License-Identifier: Apache-2.0
//
// Zig Benchmark
//
// Measures key Zig operations for comparison against the
// Objective-Z runtime benchmark and C++/Rust benchmarks.
// Same timing infrastructure: DWT-based, 10000 iterations,
// 100 warmup, overhead calibration.

const c = @cImport({
    @cInclude("autoconf.h");
    @cInclude("zephyr/kernel.h");
    @cInclude("zephyr/timing/timing.h");
    @cInclude("zephyr/sys/printk.h");
});

// ── Configuration ──────────────────────────────────────────────────

const ITERATIONS: u32 = 10_000;
const WARMUP: u32 = 100;

var timing_overhead_cycles: u64 = 0;

// ── Timing helpers ─────────────────────────────────────────────────

inline fn counter() c.timing_t {
    return c.timing_counter_get();
}

inline fn cycles(start: c.timing_t, end: c.timing_t) u64 {
    var s = start;
    var e = end;
    return c.timing_cycles_get(&s, &e);
}

fn calibrate() void {
    var total: u64 = 0;
    for (0..ITERATIONS) |_| {
        const s = counter();
        const e = counter();
        total += cycles(s, e);
    }
    timing_overhead_cycles = total / ITERATIONS;
}

fn benchFn(comptime desc: [*:0]const u8, comptime f: fn () void) void {
    for (0..WARMUP) |_| {
        f();
    }
    var total: u64 = 0;
    for (0..ITERATIONS) |_| {
        const s = counter();
        f();
        const e = counter();
        total += cycles(s, e);
    }

    var avg = total / ITERATIONS;
    if (avg > timing_overhead_cycles) {
        avg -= timing_overhead_cycles;
    }
    const ns = c.timing_cycles_to_ns(avg);
    c.printk("%-52s: %5llu cycles , %5llu ns\n", desc, @as(c_ulonglong, avg), @as(c_ulonglong, ns));
}

fn benchCtx(comptime desc: [*:0]const u8, ctx: anytype) void {
    for (0..WARMUP) |_| {
        ctx.call();
    }
    var total: u64 = 0;
    for (0..ITERATIONS) |_| {
        const s = counter();
        ctx.call();
        const e = counter();
        total += cycles(s, e);
    }

    var avg = total / ITERATIONS;
    if (avg > timing_overhead_cycles) {
        avg -= timing_overhead_cycles;
    }
    const ns = c.timing_cycles_to_ns(avg);
    c.printk("%-52s: %5llu cycles , %5llu ns\n", desc, @as(c_ulonglong, avg), @as(c_ulonglong, ns));
}

// ── Prevent optimization ───────────────────────────────────────────

fn doNotOptimize(val: anytype) void {
    _ = @as(*volatile @TypeOf(val), @constCast(@ptrCast(&val))).*;
}

// ── Interface (fat pointer vtable — idiomatic Zig dynamic dispatch) ─

const Benchable = struct {
    ptr: *anyopaque,
    vtable: *const VTable,

    const VTable = struct {
        nop: *const fn (*anyopaque) void,
    };

    fn nop(self: Benchable) void {
        self.vtable.nop(self.ptr);
    }

    fn init(comptime T: type, ptr: *T) Benchable {
        return .{
            .ptr = @ptrCast(ptr),
            .vtable = &.{
                .nop = @ptrCast(&T.nop),
            },
        };
    }
};

const BenchBase = struct {
    x: i32 = 0,

    fn nop(_: *BenchBase) void {
        asm volatile ("" ::: .{ .memory = true });
    }

    fn staticNop() void {
        asm volatile ("" ::: .{ .memory = true });
    }
};

const BenchChild = struct {
    base: BenchBase = .{},

    fn nop(_: *BenchChild) void {
        asm volatile ("" ::: .{ .memory = true });
    }
};

const BenchGrandChild = struct {
    child: BenchChild = .{},

    fn nop(_: *BenchGrandChild) void {
        asm volatile ("" ::: .{ .memory = true });
    }
};

// ── Benchmark: Interface Dispatch ──────────────────────────────────

fn benchDispatch() void {
    c.printk("\n--- Interface Dispatch ---\n");

    var base = BenchBase{};
    var child = BenchChild{};
    var gchild = BenchGrandChild{};

    const iface_base = Benchable.init(BenchBase, &base);
    const iface_child = Benchable.init(BenchChild, &child);
    const iface_gchild = Benchable.init(BenchGrandChild, &gchild);

    benchFn("Direct function call (baseline)", struct {
        fn call() void {
            var b: BenchBase = .{};
            b.nop();
        }
    }.call);

    const IfaceCallBase = struct {
        iface: Benchable,
        fn call(self: *const @This()) void {
            doNotOptimize(self.iface);
            self.iface.nop();
        }
    };
    benchCtx("Interface dispatch (depth=0)", &IfaceCallBase{ .iface = iface_base });
    benchCtx("Interface dispatch (depth=1)", &IfaceCallBase{ .iface = iface_child });
    benchCtx("Interface dispatch (depth=2)", &IfaceCallBase{ .iface = iface_gchild });

    benchFn("Static function call", struct {
        fn call() void {
            BenchBase.staticNop();
        }
    }.call);
}

// ── Benchmark: Object Lifecycle ────────────────────────────────────

fn benchLifecycle() void {
    c.printk("\n--- Object Lifecycle ---\n");

    benchFn("k_malloc + k_free (heap)", struct {
        fn call() void {
            const ptr = c.k_malloc(@sizeOf(BenchBase));
            if (ptr) |p| {
                const obj: *BenchBase = @ptrCast(@alignCast(p));
                obj.* = BenchBase{};
                doNotOptimize(obj);
                c.k_free(p);
            }
        }
    }.call);

    benchFn("Stack allocation (baseline)", struct {
        fn call() void {
            var obj = BenchBase{};
            doNotOptimize(&obj);
        }
    }.call);
}

// ── Benchmark: Reference Counting ──────────────────────────────────

fn benchRefcount() void {
    c.printk("\n--- Reference Counting ---\n");

    var refcount: i32 = 1;

    const Retain = struct {
        rc: *i32,
        fn call(self: *const @This()) void {
            _ = @atomicRmw(i32, self.rc, .Add, 1, .monotonic);
        }
    };
    benchCtx("Atomic increment (retain)", &Retain{ .rc = &refcount });

    @atomicStore(i32, &refcount, 1, .seq_cst);

    const RetainRelease = struct {
        rc: *i32,
        fn call(self: *const @This()) void {
            _ = @atomicRmw(i32, self.rc, .Add, 1, .monotonic);
            _ = @atomicRmw(i32, self.rc, .Sub, 1, .release);
        }
    };
    benchCtx("Atomic inc + dec pair (retain + release)", &RetainRelease{ .rc = &refcount });
}

// ── Benchmark: Introspection ───────────────────────────────────────

const ObjKind = enum { base, child, grandchild };

const TaggedObj = struct {
    kind: ObjKind,
    data: i32,
};

fn benchIntrospection() void {
    c.printk("\n--- Introspection ---\n");

    var child_obj = TaggedObj{ .kind = .child, .data = 0 };
    var base_obj = TaggedObj{ .kind = .base, .data = 0 };

    const TagCheck = struct {
        obj: *TaggedObj,
        fn call(self: *const @This()) void {
            doNotOptimize(self.obj);
            const is_child = self.obj.kind == .child;
            doNotOptimize(is_child);
        }
    };
    benchCtx("Tagged union check (hit)", &TagCheck{ .obj = &child_obj });
    benchCtx("Tagged union check (miss)", &TagCheck{ .obj = &base_obj });

    const TagSwitch = struct {
        obj: *TaggedObj,
        fn call(self: *const @This()) void {
            doNotOptimize(self.obj);
            const result: i32 = switch (self.obj.kind) {
                .base => 0,
                .child => 1,
                .grandchild => 2,
            };
            doNotOptimize(result);
        }
    };
    benchCtx("Tagged union switch dispatch", &TagSwitch{ .obj = &child_obj });
}

// ── Benchmark: Function Pointers ───────────────────────────────────

fn zigNop() i32 {
    asm volatile ("" ::: .{ .memory = true });
    return 0;
}

fn benchFunctionPointers() void {
    c.printk("\n--- Function Pointers ---\n");

    benchFn("Function pointer call", struct {
        fn call() void {
            const fptr: *const fn () i32 = &zigNop;
            doNotOptimize(fptr);
            _ = fptr();
        }
    }.call);

    const Closure = struct {
        captured: i32,

        fn invoke(self: *const @This()) i32 {
            asm volatile ("" ::: .{ .memory = true });
            return self.captured;
        }
    };

    var closure = Closure{ .captured = 42 };
    const ClosureCall = struct {
        cl: *Closure,
        fn call(self: *const @This()) void {
            doNotOptimize(self.cl);
            _ = self.cl.invoke();
        }
    };
    benchCtx("Struct closure invocation (int capture)", &ClosureCall{ .cl = &closure });

    const ErasedFn = struct {
        ptr: *anyopaque,
        invoke_fn: *const fn (*anyopaque) i32,
    };

    const erased_invoke = struct {
        fn f(p: *anyopaque) i32 {
            const cl: *Closure = @ptrCast(@alignCast(p));
            return cl.invoke();
        }
    }.f;

    var erased = ErasedFn{ .ptr = @ptrCast(&closure), .invoke_fn = &erased_invoke };
    const ErasedCall = struct {
        e: *ErasedFn,
        fn call(self: *const @This()) void {
            doNotOptimize(self.e);
            _ = self.e.invoke_fn(self.e.ptr);
        }
    };
    benchCtx("Type-erased callable (fat pointer)", &ErasedCall{ .e = &erased });

    c.printk("\n--- Function Pointers: Memory ---\n");
    c.printk("%-52s: %5u bytes\n", "Function pointer", @as(c_uint, @sizeOf(*const fn () i32)));
    c.printk("%-52s: %5u bytes\n", "Struct closure (int capture)", @as(c_uint, @sizeOf(Closure)));
    c.printk("%-52s: %5u bytes\n", "Type-erased callable (fat pointer)", @as(c_uint, @sizeOf(ErasedFn)));
    c.printk("%-52s: %5u bytes\n", "Benchable interface (fat pointer)", @as(c_uint, @sizeOf(Benchable)));
}

// ── Main ───────────────────────────────────────────────────────────

export fn zig_main() callconv(.c) void {
    c.printk("=== Zig Benchmark ===\n");
    c.printk("Iterations: %u (warmup: %u)\n", ITERATIONS, WARMUP);

    c.timing_init();
    c.timing_start();

    calibrate();
    c.printk("Timing overhead: %llu cycles\n", @as(c_ulonglong, timing_overhead_cycles));

    benchDispatch();
    benchLifecycle();
    benchRefcount();
    benchIntrospection();
    benchFunctionPointers();

    c.timing_stop();

    c.printk("\nPROJECT EXECUTION SUCCESSFUL\n");
}

pub fn panic(_: anytype, _: anytype, _: anytype) noreturn {
    while (true) {}
}
