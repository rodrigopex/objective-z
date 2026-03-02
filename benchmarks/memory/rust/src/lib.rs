/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Memory Benchmark: Rust
 *
 * Reports struct sizes, heap allocation overhead (via dedicated
 * sys_heap shim), and smart pointer costs.
 *
 * The zephyr crate provides #[global_allocator], so Box/Arc use
 * CONFIG_COMMON_LIBC_MALLOC_ARENA_SIZE. The heap_shim provides
 * an isolated sys_heap for precise allocation overhead measurement.
 */

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::hint::black_box;
use core::mem;
use core::sync::atomic::AtomicI32;

/* ── FFI: heap shim (isolated sys_heap for measurement) ───────── */

extern "C" {
    fn heap_shim_init();
    fn heap_shim_alloc(size: usize, align: usize) -> *mut u8;
    fn heap_shim_free(ptr: *mut u8);
    fn heap_shim_stats(allocated: *mut usize, free_bytes: *mut usize, max_allocated: *mut usize);
}

/* ── Heap stats helper ────────────────────────────────────────── */

struct HeapStats {
    allocated: usize,
    free_bytes: usize,
    max_allocated: usize,
}

fn get_heap_stats() -> HeapStats {
    let mut s = HeapStats {
        allocated: 0,
        free_bytes: 0,
        max_allocated: 0,
    };
    unsafe {
        heap_shim_stats(
            &mut s.allocated as *mut usize,
            &mut s.free_bytes as *mut usize,
            &mut s.max_allocated as *mut usize,
        );
    }
    s
}

/* ── Manual alloc/free on the measurement heap ────────────────── */

unsafe fn measure_alloc(size: usize, align: usize) -> *mut u8 {
    unsafe { heap_shim_alloc(size, align) }
}

unsafe fn measure_free(ptr: *mut u8) {
    unsafe { heap_shim_free(ptr) }
}

/* ── Trait + struct hierarchy ─────────────────────────────────── */

trait MemTrait {
    fn nop(&self);
    fn get_value(&self) -> i32 {
        0
    }
}

/*
 * MemBase: refcount (AtomicI32 = 4 bytes)
 * As a trait object (&dyn MemTrait): fat pointer = 2 * ptr = 8 bytes on stack
 */
struct MemBase {
    _refcount: AtomicI32,
}

impl MemTrait for MemBase {
    fn nop(&self) {
        black_box(());
    }
}

/*
 * MemChild: MemBase (4) + field_a (4) = 8 bytes
 */
struct MemChild {
    _base: MemBase,
    _field_a: i32,
}

impl MemTrait for MemChild {
    fn nop(&self) {
        black_box(());
    }
}

/*
 * MemGrandChild: MemChild (8) + field_b (4) = 12 bytes
 */
struct MemGrandChild {
    _child: MemChild,
    _field_b: i32,
}

impl MemTrait for MemGrandChild {
    fn nop(&self) {
        black_box(());
    }
}

/* ── Benchmark sections ───────────────────────────────────────── */

const N_BULK: usize = 20;

fn bench_object_sizes() {
    zephyr::printkln!("-- Object Sizes (mem::size_of) --");
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "Base (AtomicI32 refcount)",
        mem::size_of::<MemBase>()
    );
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "Child (base + 1 i32)",
        mem::size_of::<MemChild>()
    );
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "GrandChild (child + 1 i32)",
        mem::size_of::<MemGrandChild>()
    );
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "Pointer size",
        mem::size_of::<*const ()>()
    );
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "Dispatch mechanism (&dyn fat ptr)",
        mem::size_of::<&dyn MemTrait>()
    );
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "Refcount (AtomicI32)",
        mem::size_of::<AtomicI32>()
    );
}

fn bench_single_alloc() {
    zephyr::printkln!("\n-- Single Allocation (heap delta) --");

    unsafe {
        /* Base */
        let before = get_heap_stats();
        let ptr = measure_alloc(mem::size_of::<MemBase>(), mem::align_of::<MemBase>());
        let after = get_heap_stats();
        let delta = after.allocated - before.allocated;
        zephyr::printkln!(
            "  {:<40}: {:>4} bytes (sizeof {} + overhead {})",
            "Base object",
            delta,
            mem::size_of::<MemBase>(),
            delta - mem::size_of::<MemBase>()
        );
        measure_free(ptr);

        /* Child */
        let before = get_heap_stats();
        let ptr = measure_alloc(mem::size_of::<MemChild>(), mem::align_of::<MemChild>());
        let after = get_heap_stats();
        let delta = after.allocated - before.allocated;
        zephyr::printkln!(
            "  {:<40}: {:>4} bytes (sizeof {} + overhead {})",
            "Child object",
            delta,
            mem::size_of::<MemChild>(),
            delta - mem::size_of::<MemChild>()
        );
        measure_free(ptr);

        /* GrandChild */
        let before = get_heap_stats();
        let ptr = measure_alloc(
            mem::size_of::<MemGrandChild>(),
            mem::align_of::<MemGrandChild>(),
        );
        let after = get_heap_stats();
        let delta = after.allocated - before.allocated;
        zephyr::printkln!(
            "  {:<40}: {:>4} bytes (sizeof {} + overhead {})",
            "GrandChild object",
            delta,
            mem::size_of::<MemGrandChild>(),
            delta - mem::size_of::<MemGrandChild>()
        );
        measure_free(ptr);
    }
}

fn bench_bulk_alloc() {
    zephyr::printkln!("\n-- Bulk Allocation ({} objects) --", N_BULK);

    unsafe {
        /* N_BULK Child objects */
        let mut ptrs: [*mut u8; N_BULK] = [core::ptr::null_mut(); N_BULK];

        let before = get_heap_stats();
        for p in ptrs.iter_mut() {
            *p = measure_alloc(mem::size_of::<MemChild>(), mem::align_of::<MemChild>());
        }
        let after = get_heap_stats();
        let delta = after.allocated - before.allocated;
        zephyr::printkln!(
            "  {:<40}: {:>4} bytes ({} bytes/obj)",
            "20x Child total",
            delta,
            delta / N_BULK
        );
        for p in ptrs.iter() {
            measure_free(*p);
        }

        /* N_BULK GrandChild objects */
        let before = get_heap_stats();
        for p in ptrs.iter_mut() {
            *p = measure_alloc(
                mem::size_of::<MemGrandChild>(),
                mem::align_of::<MemGrandChild>(),
            );
        }
        let after = get_heap_stats();
        let delta = after.allocated - before.allocated;
        zephyr::printkln!(
            "  {:<40}: {:>4} bytes ({} bytes/obj)",
            "20x GrandChild total",
            delta,
            delta / N_BULK
        );
        for p in ptrs.iter() {
            measure_free(*p);
        }
    }
}

fn bench_smart_pointers() {
    zephyr::printkln!("\n-- Smart Pointer / Ref Counting --");

    /* sizeof pointer types on stack */
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "sizeof(Box<MemBase>)",
        mem::size_of::<Box<MemBase>>()
    );
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "sizeof(Arc<MemBase>)",
        mem::size_of::<Arc<MemBase>>()
    );
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes",
        "sizeof(Box<dyn MemTrait>) fat ptr",
        mem::size_of::<Box<dyn MemTrait>>()
    );

    /* Box: no control block, same as raw alloc */
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes (no ctrl block)",
        "Box<MemBase> heap cost",
        mem::size_of::<MemBase>()
    );

    /*
     * Arc: adds strong + weak counts (2 * usize = 8 bytes on 32-bit ARM)
     * Total heap = sizeof(MemBase) + 8 bytes control block + allocator overhead
     */
    let arc_inner_size = mem::size_of::<MemBase>() + 2 * mem::size_of::<usize>();
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes (obj {} + ctrl block {})",
        "Arc<MemBase> heap cost (logical)",
        arc_inner_size,
        mem::size_of::<MemBase>(),
        2 * mem::size_of::<usize>()
    );

    /* Manual AtomicI32: inline, no extra alloc */
    zephyr::printkln!(
        "  {:<40}: {:>4} bytes (inline, no ctrl block)",
        "Manual AtomicI32 refcount",
        mem::size_of::<AtomicI32>()
    );
}

fn bench_heap_summary() {
    let s = get_heap_stats();
    zephyr::printkln!("\n-- Heap Summary (measurement heap) --");
    zephyr::printkln!("  {:<40}: {:>4}", "allocated_bytes", s.allocated);
    zephyr::printkln!("  {:<40}: {:>4}", "free_bytes", s.free_bytes);
    zephyr::printkln!("  {:<40}: {:>4}", "max_allocated_bytes", s.max_allocated);
}

/* ── Main ─────────────────────────────────────────────────────── */

#[no_mangle]
extern "C" fn rust_main() {
    unsafe {
        heap_shim_init();
    }

    zephyr::printkln!("=== Memory Benchmark: Rust ===\n");

    bench_object_sizes();
    bench_single_alloc();
    bench_bulk_alloc();
    bench_smart_pointers();
    bench_heap_summary();

    zephyr::printkln!("\nPROJECT EXECUTION SUCCESSFUL");
}
