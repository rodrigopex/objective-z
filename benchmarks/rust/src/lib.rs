/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Rust Benchmark
 *
 * Measures key Rust operations for comparison against the
 * Objective-Z runtime benchmark (samples/benchmark/) and
 * C++ benchmark (benchmarks/cpp/).
 * Same timing infrastructure: DWT-based, 10000 iterations,
 * 100 warmup, overhead calibration.
 */
#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::any::{Any, TypeId};
use core::hint::black_box;
use core::mem;
use core::sync::atomic::{AtomicI32, Ordering};

/* ── FFI: timing shim (wraps Zephyr static inline timing functions) ── */

extern "C" {
    fn timing_shim_init();
    fn timing_shim_start();
    fn timing_shim_stop();
    fn timing_shim_counter_get() -> u32;
    fn timing_shim_cycles_between(start: u32, end: u32) -> u64;
    fn timing_shim_cycles_to_ns(cycles: u64) -> u64;
}

/* ── Configuration ── */

const ITERATIONS: u32 = 10_000;
const WARMUP: u32 = 100;

static mut TIMING_OVERHEAD_CYCLES: u64 = 0;

/* ── Timing helpers ── */

#[inline(always)]
fn counter() -> u32 {
    unsafe { timing_shim_counter_get() }
}

#[inline(always)]
fn cycles(start: u32, end: u32) -> u64 {
    unsafe { timing_shim_cycles_between(start, end) }
}

fn calibrate() {
    let mut total: u64 = 0;
    for _ in 0..ITERATIONS {
        let s = counter();
        let e = counter();
        total += cycles(s, e);
    }
    unsafe {
        TIMING_OVERHEAD_CYCLES = total / ITERATIONS as u64;
    }
}

fn bench(desc: &str, mut f: impl FnMut()) {
    for _ in 0..WARMUP {
        f();
    }
    let mut total: u64 = 0;
    for _ in 0..ITERATIONS {
        let s = counter();
        f();
        let e = counter();
        total += cycles(s, e);
    }

    let overhead = unsafe { TIMING_OVERHEAD_CYCLES };
    let mut avg = total / ITERATIONS as u64;
    if avg > overhead {
        avg -= overhead;
    }
    let ns = unsafe { timing_shim_cycles_to_ns(avg) };
    zephyr::printkln!("{:<52}: {:>5} cycles , {:>5} ns", desc, avg, ns);
}

/* ── Trait + structs (simulated class hierarchy via composition) ── */

trait Benchable {
    fn nop(&self);
}

struct BenchBase {
    _x: i32,
}

impl Benchable for BenchBase {
    fn nop(&self) {
        black_box(());
    }
}

impl BenchBase {
    fn static_nop() {
        black_box(());
    }
}

struct BenchChild {
    _base: BenchBase,
}

impl Benchable for BenchChild {
    fn nop(&self) {
        black_box(());
    }
}

struct BenchGrandChild {
    _child: BenchChild,
}

impl Benchable for BenchGrandChild {
    fn nop(&self) {
        black_box(());
    }
}

/* ── Benchmark: Trait Object Dispatch ── */

fn direct_nop() {
    black_box(());
}

fn bench_dispatch() {
    zephyr::printkln!("\n--- Trait Object Dispatch ---");

    let base = BenchBase { _x: 0 };
    let child = BenchChild {
        _base: BenchBase { _x: 0 },
    };
    let gchild = BenchGrandChild {
        _child: BenchChild {
            _base: BenchBase { _x: 0 },
        },
    };

    /* Create trait objects up front so compiler can't devirtualize */
    let tobj_base: &dyn Benchable = &base;
    let tobj_child: &dyn Benchable = &child;
    let tobj_gchild: &dyn Benchable = &gchild;

    bench("Direct function call (baseline)", || {
        black_box(direct_nop)();
    });

    bench("Trait object method call (depth=0)", || {
        let p = black_box(tobj_base);
        p.nop();
    });

    bench("Trait object method call (depth=1)", || {
        let p = black_box(tobj_child);
        p.nop();
    });

    bench("Trait object method call (depth=2)", || {
        let p = black_box(tobj_gchild);
        p.nop();
    });

    bench("Static function call", || {
        black_box(BenchBase::static_nop)();
    });
}

/* ── Benchmark: Object Lifecycle ── */

fn bench_lifecycle() {
    zephyr::printkln!("\n--- Object Lifecycle ---");

    bench("Box::new + drop (heap alloc/dealloc)", || {
        let obj = Box::new(BenchBase { _x: 0 });
        drop(black_box(obj));
    });

    bench("Box<dyn Trait> create + drop (type-erased)", || {
        let obj: Box<dyn Benchable> = Box::new(BenchBase { _x: 0 });
        drop(black_box(obj));
    });
}

/* ── Benchmark: Reference Counting ── */

fn bench_refcount() {
    zephyr::printkln!("\n--- Reference Counting ---");

    let rc = AtomicI32::new(1);

    bench("Atomic increment (retain equivalent)", || {
        rc.fetch_add(1, Ordering::Relaxed);
    });
    /* Balance accumulated retains */
    rc.store(1, Ordering::Relaxed);

    bench("Atomic inc + dec pair (retain + release)", || {
        rc.fetch_add(1, Ordering::Relaxed);
        rc.fetch_sub(1, Ordering::AcqRel);
    });

    let arc_obj = Arc::new(0i32);

    bench("Arc::clone (retain equivalent)", || {
        let clone = arc_obj.clone();
        mem::forget(clone);
    });
    /* Strong count is now elevated; fine for benchmark */

    bench("Arc::clone + drop (retain + release)", || {
        let clone = arc_obj.clone();
        drop(black_box(clone));
    });
}

/* ── Benchmark: Introspection ── */

fn bench_introspection() {
    zephyr::printkln!("\n--- Introspection ---");

    let child: Box<dyn Any> = Box::new(BenchChild {
        _base: BenchBase { _x: 0 },
    });
    let base: Box<dyn Any> = Box::new(BenchBase { _x: 0 });

    bench("Any::downcast_ref (hit)", || {
        let r = black_box(&*child).downcast_ref::<BenchChild>();
        black_box(r);
    });

    bench("Any::downcast_ref (miss)", || {
        let r = black_box(&*base).downcast_ref::<BenchChild>();
        black_box(r);
    });

    bench("TypeId::of (type identity)", || {
        let tid = TypeId::of::<BenchChild>();
        black_box(tid);
    });
}

/* ── Benchmark: Closures ── */

fn c_func_nop() -> i32 {
    black_box(0)
}

fn bench_closures() {
    zephyr::printkln!("\n--- Closures ---");

    /* Function pointer baseline */
    let fptr: fn() -> i32 = c_func_nop;
    bench("Function pointer call", || {
        let f = black_box(fptr);
        black_box(f());
    });

    /* Non-capturing closure (coerces to fn ptr) */
    let nc: fn() -> i32 = || -> i32 { black_box(0) };
    bench("Non-capturing closure (fn ptr coercion)", || {
        let f = black_box(nc);
        black_box(f());
    });

    /* &dyn Fn with int capture */
    let capture_val = 42i32;
    let closure_cap = move || -> i32 { black_box(capture_val) };
    let dyn_ref: &dyn Fn() -> i32 = &closure_cap;
    bench("&dyn Fn invocation (int capture)", || {
        let f: &dyn Fn() -> i32 = black_box(dyn_ref);
        black_box(f());
    });

    /* Box<dyn Fn> create + invoke + drop (must capture to force heap alloc) */
    bench("Box<dyn Fn> create + invoke + drop", || {
        let v = black_box(capture_val);
        let f: Box<dyn Fn() -> i32> = black_box(Box::new(move || -> i32 { black_box(v) }));
        black_box(f());
    });

    /* Memory sizes */
    zephyr::printkln!("\n--- Closures: Memory ---");
    zephyr::printkln!(
        "{:<52}: {:>5} bytes",
        "Function pointer",
        mem::size_of::<fn() -> i32>()
    );

    let nc_closure = || -> i32 { 0 };
    zephyr::printkln!(
        "{:<52}: {:>5} bytes",
        "Non-capturing closure",
        mem::size_of_val(&nc_closure)
    );

    zephyr::printkln!(
        "{:<52}: {:>5} bytes",
        "Closure with int capture",
        mem::size_of_val(&closure_cap)
    );

    zephyr::printkln!(
        "{:<52}: {:>5} bytes",
        "&dyn Fn() -> i32 (fat pointer)",
        mem::size_of::<&dyn Fn() -> i32>()
    );

    zephyr::printkln!(
        "{:<52}: {:>5} bytes",
        "Box<dyn Fn() -> i32> (fat pointer)",
        mem::size_of::<Box<dyn Fn() -> i32>>()
    );
}

/* ── Main ── */

#[no_mangle]
extern "C" fn rust_main() {
    zephyr::printkln!("=== Rust Benchmark ===");
    zephyr::printkln!("Iterations: {} (warmup: {})", ITERATIONS, WARMUP);

    unsafe {
        timing_shim_init();
        timing_shim_start();
    }

    calibrate();
    zephyr::printkln!(
        "Timing overhead: {} cycles",
        unsafe { TIMING_OVERHEAD_CYCLES }
    );

    bench_dispatch();
    bench_lifecycle();
    bench_refcount();
    bench_introspection();
    bench_closures();

    unsafe {
        timing_shim_stop();
    }

    zephyr::printkln!("\nPROJECT EXECUTION SUCCESSFUL");
}
