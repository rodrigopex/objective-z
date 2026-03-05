// Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
// SPDX-License-Identifier: Apache-2.0
//
// Memory Benchmark: Zig
//
// Measures memory overhead of Zig struct-based OOP with
// interface dispatch (fat pointers) and atomic refcount.
// Uses a dedicated sys_heap via C shim for precise measurement.

const c = @cImport({
    @cInclude("autoconf.h");
    @cInclude("zephyr/sys/printk.h");
});

// ── Heap shim (C-side dedicated sys_heap) ──────────────────────────

extern fn heap_shim_init() void;
extern fn heap_shim_alloc(size: usize, alignment: usize) ?*anyopaque;
extern fn heap_shim_free(ptr: ?*anyopaque) void;
extern fn heap_shim_stats(allocated: *usize, free_bytes: *usize, max_allocated: *usize) void;

// ── Object hierarchy (mirrors C/C++/Rust benchmarks) ───────────────

const MemBase = struct {
    refcount: i32,
};

const MemChild = struct {
    base: MemBase,
    field_a: i32,
};

const MemGrandChild = struct {
    child: MemChild,
    field_b: i32,
};

// ── Interface (fat pointer — Zig's dynamic dispatch) ───────────────

const Dispatchable = struct {
    ptr: *anyopaque,
    vtable: *const VTable,

    const VTable = struct {
        nop: *const fn (*anyopaque) void,
    };
};

// ── Helpers ────────────────────────────────────────────────────────

fn heapAlloc(comptime T: type) ?*T {
    const ptr = heap_shim_alloc(@sizeOf(T), @alignOf(T));
    if (ptr) |p| {
        return @ptrCast(@alignCast(p));
    }
    return null;
}

fn heapFree(ptr: anytype) void {
    heap_shim_free(@ptrCast(ptr));
}

const N_BULK = 20;

// ── Benchmark sections ─────────────────────────────────────────────

fn benchObjectSizes() void {
    c.printk("-- Object Sizes (@sizeOf) --\n");
    c.printk("  %-40s: %4u bytes\n", "Base (refcount only, no vptr)",
        @as(c_uint, @sizeOf(MemBase)));
    c.printk("  %-40s: %4u bytes\n", "Child (base + 1 int)",
        @as(c_uint, @sizeOf(MemChild)));
    c.printk("  %-40s: %4u bytes\n", "GrandChild (child + 1 int)",
        @as(c_uint, @sizeOf(MemGrandChild)));
    c.printk("  %-40s: %4u bytes\n", "Pointer size",
        @as(c_uint, @sizeOf(*anyopaque)));
    c.printk("  %-40s: %4u bytes\n", "Interface (fat pointer: ptr + vtable*)",
        @as(c_uint, @sizeOf(Dispatchable)));
    c.printk("  %-40s: %4u bytes\n", "Refcount (i32)",
        @as(c_uint, @sizeOf(i32)));
}

fn benchSingleAlloc() void {
    c.printk("\n-- Single Allocation (heap delta) --\n");

    // Base
    var before: usize = 0;
    var after: usize = 0;
    var dummy: usize = 0;

    heap_shim_stats(&before, &dummy, &dummy);
    const b = heapAlloc(MemBase);
    heap_shim_stats(&after, &dummy, &dummy);
    var delta = after - before;
    c.printk("  %-40s: %4u bytes (sizeof %u + overhead %u)\n",
        "Base object",
        @as(c_uint, @truncate(delta)),
        @as(c_uint, @sizeOf(MemBase)),
        @as(c_uint, @truncate(delta - @sizeOf(MemBase))));
    if (b) |p| { heapFree(p); }

    // Child
    heap_shim_stats(&before, &dummy, &dummy);
    const ch = heapAlloc(MemChild);
    heap_shim_stats(&after, &dummy, &dummy);
    delta = after - before;
    c.printk("  %-40s: %4u bytes (sizeof %u + overhead %u)\n",
        "Child object",
        @as(c_uint, @truncate(delta)),
        @as(c_uint, @sizeOf(MemChild)),
        @as(c_uint, @truncate(delta - @sizeOf(MemChild))));
    if (ch) |p| { heapFree(p); }

    // GrandChild
    heap_shim_stats(&before, &dummy, &dummy);
    const gc = heapAlloc(MemGrandChild);
    heap_shim_stats(&after, &dummy, &dummy);
    delta = after - before;
    c.printk("  %-40s: %4u bytes (sizeof %u + overhead %u)\n",
        "GrandChild object",
        @as(c_uint, @truncate(delta)),
        @as(c_uint, @sizeOf(MemGrandChild)),
        @as(c_uint, @truncate(delta - @sizeOf(MemGrandChild))));
    if (gc) |p| { heapFree(p); }
}

fn benchBulkAlloc() void {
    c.printk("\n-- Bulk Allocation (%u objects) --\n", @as(c_uint, N_BULK));

    var before: usize = 0;
    var after: usize = 0;
    var dummy: usize = 0;

    // N_BULK Child objects
    var children: [N_BULK]?*MemChild = undefined;
    heap_shim_stats(&before, &dummy, &dummy);
    for (0..N_BULK) |i| {
        children[i] = heapAlloc(MemChild);
    }
    heap_shim_stats(&after, &dummy, &dummy);
    var delta = after - before;
    c.printk("  %-40s: %4u bytes (%u bytes/obj)\n",
        "20x Child total",
        @as(c_uint, @truncate(delta)),
        @as(c_uint, @truncate(delta / N_BULK)));
    for (0..N_BULK) |i| {
        if (children[i]) |p| { heapFree(p); }
    }

    // N_BULK GrandChild objects
    var grandchildren: [N_BULK]?*MemGrandChild = undefined;
    heap_shim_stats(&before, &dummy, &dummy);
    for (0..N_BULK) |i| {
        grandchildren[i] = heapAlloc(MemGrandChild);
    }
    heap_shim_stats(&after, &dummy, &dummy);
    delta = after - before;
    c.printk("  %-40s: %4u bytes (%u bytes/obj)\n",
        "20x GrandChild total",
        @as(c_uint, @truncate(delta)),
        @as(c_uint, @truncate(delta / N_BULK)));
    for (0..N_BULK) |i| {
        if (grandchildren[i]) |p| { heapFree(p); }
    }
}

fn benchRefcount() void {
    c.printk("\n-- Reference Counting --\n");
    c.printk("  %-40s: %4u bytes (inline in struct)\n",
        "Refcount overhead (i32)",
        @as(c_uint, @sizeOf(i32)));
    c.printk("  %-40s:    0 bytes\n",
        "Control block (none)");

    // Demonstrate retain/release
    var refcount: i32 = 1;
    _ = @atomicRmw(i32, &refcount, .Add, 1, .monotonic);
    c.printk("  %-40s: %4d\n",
        "After retain, refcount",
        @as(c_int, @atomicLoad(i32, &refcount, .seq_cst)));
    _ = @atomicRmw(i32, &refcount, .Sub, 1, .release);
    c.printk("  %-40s: %4d\n",
        "After release, refcount",
        @as(c_int, @atomicLoad(i32, &refcount, .seq_cst)));
}

fn benchHeapSummary() void {
    var allocated: usize = 0;
    var free_bytes: usize = 0;
    var max_allocated: usize = 0;

    heap_shim_stats(&allocated, &free_bytes, &max_allocated);
    c.printk("\n-- Heap Summary --\n");
    c.printk("  %-40s: %4u\n", "allocated_bytes", @as(c_uint, @truncate(allocated)));
    c.printk("  %-40s: %4u\n", "free_bytes", @as(c_uint, @truncate(free_bytes)));
    c.printk("  %-40s: %4u\n", "max_allocated_bytes", @as(c_uint, @truncate(max_allocated)));
}

// ── Main ───────────────────────────────────────────────────────────

export fn zig_main() callconv(.c) void {
    heap_shim_init();

    c.printk("=== Memory Benchmark: Zig ===\n\n");

    benchObjectSizes();
    benchSingleAlloc();
    benchBulkAlloc();
    benchRefcount();
    benchHeapSummary();

    c.printk("\nPROJECT EXECUTION SUCCESSFUL\n");
}

pub fn panic(_: anytype, _: anytype, _: anytype) noreturn {
    while (true) {}
}
