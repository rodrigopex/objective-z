/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * C++ Comprehensive Benchmark (OZ-070)
 *
 * 7 sections mirroring OZ: Allocation, Dispatch, Lifecycle,
 * Refcount, Properties/Sync, Collections+Blocks, Introspection.
 *
 * Same timing methodology as OZ benchmark:
 * DWT-based, explicit loops, overhead calibration.
 */
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>

#include <atomic>
#include <functional>
#include <memory>
#include <typeinfo>
#include <cstring>

#include "bench_classes.hpp"

/* ── Iteration tiers (match OZ benchmark) ─────────────────────────── */

#define FAST_ITERATIONS  50000
#define ITERATIONS       10000
#define SLOW_ITERATIONS   1000

/* ── Timing helpers ───────────────────────────────────────────────── */

static uint64_t timing_overhead_cycles;

static void calibrate_timing_overhead(void)
{
        timing_t start, end;
        uint64_t total = 0;

        for (int i = 0; i < ITERATIONS; i++) {
                start = timing_counter_get();
                end = timing_counter_get();
                total += timing_cycles_get(&start, &end);
        }
        timing_overhead_cycles = total / ITERATIONS;
}

static void bench_report(const char *desc, uint64_t total, int n)
{
        uint64_t avg = total / n;

        if (avg > timing_overhead_cycles) {
                avg -= timing_overhead_cycles;
        }
        uint64_t ns = timing_cycles_to_ns(avg);

        printk("  %-48s: %5llu cycles , %5llu ns\n", desc,
               (unsigned long long)avg, (unsigned long long)ns);
}

/* ── C function baseline ──────────────────────────────────────────── */

static void c_nop(void *self)
{
        (void)self;
        __asm__ volatile("" ::: "memory");
}

static void (*c_nop_ptr)(void *) = c_nop;

/* ── Section 1: Allocation ────────────────────────────────────────── */

/* Slab for placement-new (mirrors OZ k_mem_slab) */
K_MEM_SLAB_DEFINE(bench_slab, sizeof(BenchBase), 8, sizeof(void *));

static void bench_allocation(void)
{
        printk("\n--- 1. Allocation ---\n");
        timing_t s, e;
        uint64_t total;

        /* Value type on stack (vs OZ slab alloc) */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                s = timing_counter_get();
                BenchBase obj;
                obj.nop();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("Value type on stack (BenchBase)", total, SLOW_ITERATIONS);

        /* new/delete (heap, vs OZ heap alloc) */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                s = timing_counter_get();
                BenchBase *obj = new BenchBase();
                delete obj;
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("new/delete (heap)", total, SLOW_ITERATIONS);

        /* unique_ptr create/destroy */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                s = timing_counter_get();
                auto obj = std::make_unique<BenchBase>();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("unique_ptr create/destroy", total, SLOW_ITERATIONS);
}

/* ── Section 2: Dispatch ──────────────────────────────────────────── */

static void bench_dispatch(void)
{
        printk("\n--- 2. Dispatch ---\n");
        timing_t s, e;
        uint64_t total;

        BenchBase *volatile base = new BenchBase();
        BenchBase *volatile child = new BenchChild();
        BenchBase *volatile gchild = new BenchGrandChild();

        /* C function pointer baseline */
        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                s = timing_counter_get();
                c_nop_ptr(base);
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("C function pointer (baseline)", total, FAST_ITERATIONS);

        /* Direct (non-virtual) call baseline */
        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                BenchBase *p = base;
                s = timing_counter_get();
                p->BenchBase::nop();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("Direct call (non-virtual)", total, FAST_ITERATIONS);

        /* Static method */
        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                s = timing_counter_get();
                BenchBase::classNop();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("Static method call", total, FAST_ITERATIONS);

        /* Virtual dispatch at various depths */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                BenchBase *p = base;
                s = timing_counter_get();
                p->nop();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("Virtual dispatch (depth=0)", total, ITERATIONS);

        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                BenchBase *p = child;
                s = timing_counter_get();
                p->nop();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("Virtual dispatch (depth=1)", total, ITERATIONS);

        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                BenchBase *p = gchild;
                s = timing_counter_get();
                p->nop();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("Virtual dispatch (depth=2)", total, ITERATIONS);

        /* Non-capturing lambda (func ptr decay) */
        auto lambda = +[]() -> int {
                __asm__ volatile("" ::: "memory");
                return 0;
        };
        int (*volatile lptr)() = lambda;

        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                s = timing_counter_get();
                (void)lptr();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("Lambda (non-capturing, fn ptr decay)", total, FAST_ITERATIONS);

        /* std::function with capture */
        int capture_val = 42;
        std::function<int()> fn_cap = [capture_val]() -> int {
                __asm__ volatile("" ::: "memory");
                return capture_val;
        };
        std::function<int()> *volatile fn_ptr = &fn_cap;

        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                (void)(*fn_ptr)();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("std::function (int capture)", total, ITERATIONS);

        /* std::function copy + destroy */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                s = timing_counter_get();
                std::function<int()> fn_copy = *fn_ptr;
                (void)fn_copy();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("std::function copy + destroy", total, SLOW_ITERATIONS);

        delete base;
        delete child;
        delete gchild;
}

/* ── Section 3: Object Lifecycle ──────────────────────────────────── */

static void bench_lifecycle(void)
{
        printk("\n--- 3. Object Lifecycle ---\n");
        timing_t s, e;
        uint64_t total;

        /* new/delete */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                s = timing_counter_get();
                BenchBase *obj = new BenchBase();
                delete obj;
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("new + delete", total, SLOW_ITERATIONS);

        /* placement new + slab (mirrors OZ slab alloc) */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                void *mem;
                s = timing_counter_get();
                k_mem_slab_alloc(&bench_slab, &mem, K_NO_WAIT);
                PooledObj *obj = new (mem) PooledObj();
                obj->~PooledObj();
                k_mem_slab_free(&bench_slab, mem);
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("placement new + slab + dtor + free", total, SLOW_ITERATIONS);

        /* unique_ptr */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                s = timing_counter_get();
                auto obj = std::make_unique<BenchBase>();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("make_unique create/destroy", total, SLOW_ITERATIONS);
}

/* ── Section 4: Reference Counting ────────────────────────────────── */

static void bench_refcount(void)
{
        printk("\n--- 4. Reference Counting ---\n");
        timing_t s, e;
        uint64_t total;

        BenchBase *obj = new BenchBase();

        /* atomic increment (retain) */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                obj->retain();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("atomic inc (retain)", total, ITERATIONS);

        /* Balance accumulated retains */
        obj->refcount.fetch_sub(ITERATIONS, std::memory_order_relaxed);

        /* retain + release pair */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                obj->retain();
                obj->refcount.fetch_sub(1, std::memory_order_acq_rel);
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("atomic inc + dec pair", total, ITERATIONS);

        /* shared_ptr copy (retain equivalent) */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                auto sp = std::make_shared<BenchBase>();
                s = timing_counter_get();
                auto sp2 = sp;
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("shared_ptr copy", total, SLOW_ITERATIONS);

        /* shared_ptr copy + reset */
        total = 0;
        for (int i = 0; i < SLOW_ITERATIONS; i++) {
                auto sp = std::make_shared<BenchBase>();
                s = timing_counter_get();
                auto sp2 = sp;
                sp2.reset();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("shared_ptr copy + reset", total, SLOW_ITERATIONS);

        delete obj;
}

/* ── Section 5: Properties / Synchronization ──────────────────────── */

static void bench_properties_sync(void)
{
        printk("\n--- 5. Properties / Synchronization ---\n");
        timing_t s, e;
        uint64_t total;

        BenchBase *obj = new BenchBase();

        /* property get (nonatomic) */
        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                s = timing_counter_get();
                (void)obj->value();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("property get (nonatomic)", total, FAST_ITERATIONS);

        /* property set (nonatomic) */
        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                s = timing_counter_get();
                obj->setValue(42);
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("property set (nonatomic)", total, FAST_ITERATIONS);

        /* property get (atomic, k_spinlock — same as OZ) */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                (void)obj->atomicValue();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("property get (atomic, k_spinlock)", total, ITERATIONS);

        /* property set (atomic, k_spinlock) */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                obj->setAtomicValue(42);
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("property set (atomic, k_spinlock)", total, ITERATIONS);

        /* synchronized (k_spinlock, same primitive as OZ) */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                obj->syncNop();
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("synchronized (k_spinlock)", total, ITERATIONS);

        delete obj;
}

/* ── Section 6: Collections + Blocks ──────────────────────────────── */

/* Slab for BoxedInt (mirrors OZNumber slab pool) */
K_MEM_SLAB_DEFINE(boxed_slab, sizeof(BoxedInt), 16, sizeof(void *));

static void bench_collections_blocks(void)
{
        printk("\n--- 6. Collections + Blocks ---\n");
        timing_t s, e;
        uint64_t total;

        /* ── Value-based (idiomatic C++ vs OZArray) ─── */

        /* Array create (value-based, 10 ints on stack) */
        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                s = timing_counter_get();
                int arr[10] = {0, 1, 2, 3, 4, 5, 6, 7, 8, 9};
                (void)arr[5];
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("int[10] create + access (value)", total, FAST_ITERATIONS);

        /* Array iteration (value-based, 10 ints) */
        int val_arr[10] = {0, 1, 2, 3, 4, 5, 6, 7, 8, 9};

        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                volatile int sum = 0;
                for (int j = 0; j < 10; j++) {
                        sum += val_arr[j];
                }
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("int[10] iteration (value)", total, ITERATIONS);

        /* ── Boxed pointer-based (fair OZArray comparison) ─── */

        /* Create 10 boxed ints from slab */
        BoxedInt *items[10];

        for (int i = 0; i < 10; i++) {
                void *mem;
                k_mem_slab_alloc(&boxed_slab, &mem, K_NO_WAIT);
                items[i] = static_cast<BoxedInt *>(mem);
                items[i]->val = i;
        }

        /* Random access (boxed) */
        total = 0;
        for (int i = 0; i < FAST_ITERATIONS; i++) {
                s = timing_counter_get();
                (void)items[5]->val;
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("BoxedInt*[10] access (slab-pooled)", total, FAST_ITERATIONS);

        /* Iteration (boxed) */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                volatile int32_t sum = 0;
                for (int j = 0; j < 10; j++) {
                        sum += items[j]->val;
                }
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("BoxedInt*[10] iteration (slab-pooled)", total, ITERATIONS);

        /* Iteration via polymorphic iterator (fair OZ for-in comparison) */
        BoxedArray boxed_arr(items, 10);

        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                s = timing_counter_get();
                volatile int32_t sum = 0;
                for (auto it = boxed_arr.begin(); it != boxed_arr.end(); ++it) {
                        sum += (*it)->val;
                }
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("BoxedArray iterator (virtual ++/*)", total, ITERATIONS);

        for (int i = 0; i < 10; i++) {
                k_mem_slab_free(&boxed_slab, items[i]);
        }

        /* ── Blocks (lambda / std::function, from Section 2) ─── */
        printk("  (lambda/std::function benchmarks in Section 2 Dispatch)\n");
}

/* ── Section 7: Introspection (RTTI) ──────────────────────────────── */

static void bench_introspection(void)
{
        printk("\n--- 7. Introspection (RTTI) ---\n");
        timing_t s, e;
        uint64_t total;

        BenchBase *volatile child_as_base = new BenchChild();
        BenchBase *volatile plain_base = new BenchBase();

        /* dynamic_cast hit */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                BenchBase *p = child_as_base;
                s = timing_counter_get();
                volatile auto *r = dynamic_cast<BenchChild *>(p);
                (void)r;
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("dynamic_cast (hit)", total, ITERATIONS);

        /* dynamic_cast miss */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                BenchBase *p = plain_base;
                s = timing_counter_get();
                volatile auto *r = dynamic_cast<BenchChild *>(p);
                (void)r;
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("dynamic_cast (miss)", total, ITERATIONS);

        /* typeid */
        total = 0;
        for (int i = 0; i < ITERATIONS; i++) {
                BenchBase *p = child_as_base;
                s = timing_counter_get();
                const auto &ti = typeid(*p);
                volatile const char *name = ti.name();
                (void)name;
                e = timing_counter_get();
                total += timing_cycles_get(&s, &e);
        }
        bench_report("typeid() + name()", total, ITERATIONS);

        delete child_as_base;
        delete plain_base;
}

/* ── Object Sizes ─────────────────────────────────────────────────── */

static void print_sizes(void)
{
        printk("\n--- Object Sizes ---\n");
        printk("  %-48s: %5zu bytes\n", "BenchBase (vptr + refcount + props + lock)",
               sizeof(BenchBase));
        printk("  %-48s: %5zu bytes\n", "BenchChild (BenchBase, no extra)",
               sizeof(BenchChild));
        printk("  %-48s: %5zu bytes\n", "BenchGrandChild",
               sizeof(BenchGrandChild));
        printk("  %-48s: %5zu bytes\n", "BoxedInt (int32_t)",
               sizeof(BoxedInt));
        printk("  %-48s: %5zu bytes\n", "std::shared_ptr<BenchBase>",
               sizeof(std::shared_ptr<BenchBase>));
        printk("  %-48s: %5zu bytes\n", "std::unique_ptr<BenchBase>",
               sizeof(std::unique_ptr<BenchBase>));
        printk("  %-48s: %5zu bytes\n", "std::function<int()>",
               sizeof(std::function<int()>));
        printk("  %-48s: %5zu bytes\n", "k_spinlock",
               sizeof(struct k_spinlock));
        printk("  %-48s: %5zu bytes\n", "Pointer size",
               sizeof(void *));
        printk("  %-48s: %5s\n", "Dispatch mechanism",
               "vptr -> vtable -> function (2 indirection)");
}

/* ── Main ─────────────────────────────────────────────────────────── */

int main(void)
{
        printk("=== C++ Benchmark (OZ-070) ===\n");
        printk("Board: %s\n", CONFIG_BOARD);
        printk("Iterations: fast=%d, normal=%d, slow=%d\n",
               FAST_ITERATIONS, ITERATIONS, SLOW_ITERATIONS);

        timing_init();
        timing_start();

        calibrate_timing_overhead();
        printk("Timing overhead: %llu cycles\n",
               (unsigned long long)timing_overhead_cycles);

        bench_allocation();
        bench_dispatch();
        bench_lifecycle();
        bench_refcount();
        bench_properties_sync();
        bench_collections_blocks();
        bench_introspection();

        timing_stop();

        print_sizes();

        printk("\nPROJECT EXECUTION SUCCESSFUL\n");
        return 0;
}
