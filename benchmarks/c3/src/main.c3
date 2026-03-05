/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * C3 Benchmark
 *
 * Measures key C3 operations for comparison against the
 * Objective-Z runtime benchmark and C++/Rust/Zig benchmarks.
 * Same timing infrastructure: DWT-based, 10000 iterations,
 * 100 warmup, overhead calibration.
 */
module c3bench;

/* ── C extern declarations ─────────────────────────────────────── */

/* Zephyr real symbols */
extern fn void timing_init();
extern fn void timing_start();
extern fn void timing_stop();
extern fn void *k_malloc(usz size);
extern fn void k_free(void *ptr);
extern fn int printk(char *fmt, ...);

/* Timing shim (wraps static inline Zephyr helpers) */
extern fn uint timing_shim_counter_get();
extern fn ulong timing_shim_cycles_between(uint start, uint end);
extern fn ulong timing_shim_cycles_to_ns(ulong cycles);

/* ── Configuration ─────────────────────────────────────────────── */

const uint ITERATIONS = 10000;
const uint WARMUP = 100;

ulong timing_overhead_cycles;
int nop_sink;

/* ── Timing helpers ────────────────────────────────────────────── */

fn void calibrate() {
    ulong total = 0;
    for (uint i = 0; i < ITERATIONS; i++) {
        uint s = timing_shim_counter_get();
        uint e = timing_shim_counter_get();
        total += timing_shim_cycles_between(s, e);
    }
    timing_overhead_cycles = total / ITERATIONS;
}

/* LLVM memory orderings: 2=Monotonic, 4=Release, 5=AcqRel, 6=SeqCst */
const MONOTONIC = 2;
const RELEASE = 4;
const SEQ_CST = 6;

macro @bench_loop(char *desc; @body()) {
    /* Warmup */
    for (uint _w = 0; _w < WARMUP; _w++) {
        @body();
    }

    ulong _total = 0;
    for (uint _i = 0; _i < ITERATIONS; _i++) {
        uint _start = timing_shim_counter_get();
        @body();
        uint _end = timing_shim_counter_get();
        _total += timing_shim_cycles_between(_start, _end);
    }

    ulong _avg = _total / ITERATIONS;
    if (_avg > timing_overhead_cycles) {
        _avg -= timing_overhead_cycles;
    }
    ulong _ns = timing_shim_cycles_to_ns(_avg);
    printk("%-52s: %5llu cycles , %5llu ns\n", desc, _avg, _ns);
}

/* ── Interface (C3 dynamic dispatch via @dynamic + any) ────────── */

interface Benchable {
    fn void nop();
}

struct BenchBase (Benchable) {
    int x;
}

fn void BenchBase.nop(&self) @dynamic {
    $$volatile_store(&nop_sink, self.x);
}

fn void static_nop() {
    $$volatile_store(&nop_sink, 0);
}

struct BenchChild (Benchable) {
    inline BenchBase base;
    int y;
}

fn void BenchChild.nop(&self) @dynamic {
    $$volatile_store(&nop_sink, self.y);
}

struct BenchGrandChild (Benchable) {
    inline BenchChild child;
    int z;
}

fn void BenchGrandChild.nop(&self) @dynamic {
    $$volatile_store(&nop_sink, self.z);
}

/* ── Benchmark: Interface Dispatch ─────────────────────────────── */

fn void bench_dispatch() {
    printk("\n--- Interface Dispatch ---\n");

    BenchBase base_obj = { .x = 0 };
    BenchChild child_obj = { .base = { .x = 0 }, .y = 0 };
    BenchGrandChild gchild_obj = { .child = { .base = { .x = 0 }, .y = 0 }, .z = 0 };

    /* Direct function call baseline */
    @bench_loop("Direct function call (baseline)") {
        base_obj.nop();
    };

    /* Interface dispatch via any */
    any base_any = &base_obj;
    any child_any = &child_obj;
    any gchild_any = &gchild_obj;

    @bench_loop("Interface dispatch (depth=0)") {
        Benchable b = (Benchable)base_any;
        b.nop();
    };

    @bench_loop("Interface dispatch (depth=1)") {
        Benchable b = (Benchable)child_any;
        b.nop();
    };

    @bench_loop("Interface dispatch (depth=2)") {
        Benchable b = (Benchable)gchild_any;
        b.nop();
    };

    @bench_loop("Static function call") {
        static_nop();
    };
}

/* ── Benchmark: Object Lifecycle ───────────────────────────────── */

fn void bench_lifecycle() {
    printk("\n--- Object Lifecycle ---\n");

    @bench_loop("k_malloc + k_free (heap)") {
        void *ptr = k_malloc(BenchBase.sizeof);
        if (ptr != null) {
            BenchBase *obj = (BenchBase *)ptr;
            obj.x = 0;
            $$volatile_store(&nop_sink, obj.x);
            k_free(ptr);
        }
    };

    @bench_loop("Stack allocation (baseline)") {
        BenchBase obj = { .x = 0 };
        $$volatile_store(&nop_sink, obj.x);
    };
}

/* ── Benchmark: Reference Counting ─────────────────────────────── */

fn void bench_refcount() {
    printk("\n--- Reference Counting ---\n");

    int refcount = 1;

    @bench_loop("Atomic increment (retain)") {
        $$atomic_fetch_add(&refcount, 1, false, MONOTONIC, int.alignof);
    };

    /* Reset */
    $$atomic_store(&refcount, 1, false, SEQ_CST);

    @bench_loop("Atomic inc + dec pair (retain + release)") {
        $$atomic_fetch_add(&refcount, 1, false, MONOTONIC, int.alignof);
        $$atomic_fetch_sub(&refcount, 1, false, RELEASE, int.alignof);
    };
}

/* ── Benchmark: Introspection ──────────────────────────────────── */

fn void bench_introspection() {
    printk("\n--- Introspection ---\n");

    BenchBase base_obj = { .x = 0 };
    BenchChild child_obj = { .base = { .x = 0 }, .y = 0 };

    any child_any = &child_obj;
    any base_any = &base_obj;

    @bench_loop("any.type check (hit)") {
        bool is_child = child_any.type == BenchChild.typeid;
        $$volatile_store(&nop_sink, (int)is_child);
    };

    @bench_loop("any.type check (miss)") {
        bool is_child = base_any.type == BenchChild.typeid;
        $$volatile_store(&nop_sink, (int)is_child);
    };
}

/* ── Benchmark: Function Pointers ──────────────────────────────── */

fn int c3_nop() {
    $$volatile_store(&nop_sink, 0);
    return 0;
}

alias FnPtr = fn int();

struct ManualClosure {
    int captured;
}

fn int ManualClosure.invoke(&self) {
    $$volatile_store(&nop_sink, self.captured);
    return self.captured;
}

fn void bench_function_pointers() {
    printk("\n--- Function Pointers ---\n");

    FnPtr fptr = &c3_nop;

    @bench_loop("Function pointer call") {
        $$volatile_store(&nop_sink, 0);
        fptr();
    };

    /* C3 lambdas do not capture, so we use a struct-based closure */
    ManualClosure closure = { .captured = 42 };

    @bench_loop("Struct closure invocation (int capture)") {
        $$volatile_store(&nop_sink, closure.captured);
        closure.invoke();
    };

    printk("\n--- Function Pointers: Memory ---\n");
    printk("%-52s: %5u bytes\n", "Function pointer", (uint)FnPtr.sizeof);
    printk("%-52s: %5u bytes\n", "Struct closure (int capture)", (uint)ManualClosure.sizeof);
    printk("%-52s: %5u bytes\n", "any type (ptr + typeid)", (uint)any.sizeof);
}

/* ── Main ──────────────────────────────────────────────────────── */

fn void c3_main() @export("c3_main") {
    printk("=== C3 Benchmark ===\n");
    printk("Iterations: %u (warmup: %u)\n", ITERATIONS, WARMUP);

    timing_init();
    timing_start();

    calibrate();
    printk("Timing overhead: %llu cycles\n", timing_overhead_cycles);

    bench_dispatch();
    bench_lifecycle();
    bench_refcount();
    bench_introspection();
    bench_function_pointers();

    timing_stop();

    printk("\nPROJECT EXECUTION SUCCESSFUL\n");
}
