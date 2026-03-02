/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * C++ Benchmark
 *
 * Measures key C++ operations for comparison against the
 * Objective-Z runtime benchmark (samples/benchmark/).
 * Same timing infrastructure: DWT-based, 10000 iterations,
 * 100 warmup, overhead calibration.
 */
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>

#include <atomic>
#include <functional>
#include <memory>
#include <typeinfo>

#include "bench_classes.hpp"

/* ── Configuration ────────────────────────────────────────────────── */

#ifndef CONFIG_BENCHMARK_ITERATIONS
#define CONFIG_BENCHMARK_ITERATIONS 10000
#endif

#define WARMUP_ITERATIONS 100
#define ITERATIONS        CONFIG_BENCHMARK_ITERATIONS

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

#define BENCH_LOOP(desc, code)                                                                     \
        do {                                                                                       \
                timing_t _start, _end;                                                             \
                uint64_t _total_cycles = 0;                                                        \
                                                                                                   \
                for (int _w = 0; _w < WARMUP_ITERATIONS; _w++) {                                   \
                        code;                                                                      \
                }                                                                                  \
                                                                                                   \
                for (int _i = 0; _i < ITERATIONS; _i++) {                                          \
                        _start = timing_counter_get();                                             \
                        code;                                                                      \
                        _end = timing_counter_get();                                               \
                        _total_cycles += timing_cycles_get(&_start, &_end);                        \
                }                                                                                  \
                                                                                                   \
                uint64_t _avg = _total_cycles / ITERATIONS;                                        \
                if (_avg > timing_overhead_cycles) {                                                \
                        _avg -= timing_overhead_cycles;                                             \
                }                                                                                  \
                uint64_t _ns = timing_cycles_to_ns(_avg);                                          \
                printk("%-52s: %5llu cycles , %5llu ns\n", desc, (unsigned long long)_avg,         \
                       (unsigned long long)_ns);                                                   \
        } while (0)

/* ── Benchmark: Virtual Dispatch ──────────────────────────────────── */

static void bench_virtual_dispatch(void)
{
        printk("\n--- Virtual Dispatch ---\n");

        /* volatile prevents devirtualization */
        BenchBase *volatile base = new BenchBase();
        BenchBase *volatile child = new BenchChild();
        BenchBase *volatile gchild = new BenchGrandChild();

        /* Direct (non-virtual) call baseline via qualified call */
        void (BenchBase::*direct_nop)() = &BenchBase::nop;

        BENCH_LOOP("Direct call (baseline, non-virtual)", {
                BenchBase *p = base;
                p->BenchBase::nop();
        });

        BENCH_LOOP("Virtual method call (depth=0)", {
                BenchBase *p = base;
                p->nop();
        });

        BENCH_LOOP("Virtual method call (depth=1)", {
                BenchBase *p = child;
                p->nop();
        });

        BENCH_LOOP("Virtual method call (depth=2)", {
                BenchBase *p = gchild;
                p->nop();
        });

        BENCH_LOOP("Static method call", BenchBase::classNop());

        (void)direct_nop;
        delete base;
        delete child;
        delete gchild;
}

/* ── Benchmark: Object Lifecycle ──────────────────────────────────── */

/* K_MEM_SLAB for placement new (mirrors ObjC static pool) */
K_MEM_SLAB_DEFINE(pooled_slab, sizeof(PooledObj), 4, sizeof(void *));

static void bench_object_lifecycle(void)
{
        printk("\n--- Object Lifecycle ---\n");

        BENCH_LOOP("new/delete (heap)", {
                BenchBase *obj = new BenchBase();
                delete obj;
        });

        BENCH_LOOP("placement new + dtor + slab free (static pool)", {
                void *mem;
                k_mem_slab_alloc(&pooled_slab, &mem, K_NO_WAIT);
                PooledObj *obj = new (mem) PooledObj();
                obj->~PooledObj();
                k_mem_slab_free(&pooled_slab, mem);
        });

        BENCH_LOOP("unique_ptr create/destroy", {
                auto obj = std::make_unique<BenchBase>();
        });
}

/* ── Benchmark: Reference Counting ────────────────────────────────── */

static void bench_refcount(void)
{
        printk("\n--- Reference Counting ---\n");

        BenchBase *obj = new BenchBase();

        BENCH_LOOP("atomic increment (retain)", obj->retain());

        /* Balance accumulated retains */
        int balance = WARMUP_ITERATIONS + ITERATIONS;
        obj->refcount.fetch_sub(balance, std::memory_order_relaxed);

        BENCH_LOOP("atomic inc + dec pair (retain + release)", {
                obj->retain();
                obj->refcount.fetch_sub(1, std::memory_order_acq_rel);
        });

        BENCH_LOOP("shared_ptr copy (retain equivalent)", {
                auto sp = std::make_shared<BenchBase>();
                auto sp2 = sp;
        });

        BENCH_LOOP("shared_ptr copy + reset (retain + release)", {
                auto sp = std::make_shared<BenchBase>();
                auto sp2 = sp;
                sp2.reset();
        });

        delete obj;
}

/* ── Benchmark: Introspection (RTTI) ──────────────────────────────── */

static void bench_introspection(void)
{
        printk("\n--- Introspection (RTTI) ---\n");

        BenchBase *volatile child_as_base = new BenchChild();
        BenchBase *volatile plain_base = new BenchBase();

        BENCH_LOOP("dynamic_cast (hit)", {
                BenchBase *p = child_as_base;
                volatile auto *r = dynamic_cast<BenchChild *>(p);
                (void)r;
        });

        BENCH_LOOP("dynamic_cast (miss)", {
                BenchBase *p = plain_base;
                volatile auto *r = dynamic_cast<BenchChild *>(p);
                (void)r;
        });

        BENCH_LOOP("typeid()", {
                BenchBase *p = child_as_base;
                volatile auto &ti = typeid(*p);
                (void)ti;
        });

        delete child_as_base;
        delete plain_base;
}

/* ── Benchmark: Lambdas / std::function ───────────────────────────── */

static int c_func_nop(void)
{
        __asm__ volatile("" ::: "memory");
        return 0;
}

static void bench_lambdas(void)
{
        printk("\n--- Lambdas / std::function ---\n");

        /* C function pointer baseline */
        int (*volatile fptr)(void) = c_func_nop;

        BENCH_LOOP("C function pointer call", { (void)fptr(); });

        /* Non-capturing lambda (decays to func ptr) */
        auto lambda_nocap = +[]() -> int {
                __asm__ volatile("" ::: "memory");
                return 0;
        };
        int (*volatile lptr)(void) = lambda_nocap;

        BENCH_LOOP("Non-capturing lambda (func ptr decay)", { (void)lptr(); });

        /* std::function with int capture */
        int capture_val = 42;
        std::function<int()> fn_cap = [capture_val]() -> int {
                __asm__ volatile("" ::: "memory");
                return capture_val;
        };
        std::function<int()> *volatile fn_ptr = &fn_cap;

        BENCH_LOOP("std::function invocation (int capture)", { (void)(*fn_ptr)(); });

        /* std::function copy + destroy */
        BENCH_LOOP("std::function copy + destroy", {
                std::function<int()> fn_copy = *fn_ptr;
                (void)fn_copy();
        });

        /* Memory sizes */
        printk("\n--- Lambdas / std::function: Memory ---\n");
        printk("%-52s: %5zu bytes\n", "C function pointer", sizeof(int (*)(void)));
        printk("%-52s: %5zu bytes\n", "Non-capturing lambda (func ptr)", sizeof(lambda_nocap));

        auto lambda_int = [capture_val]() -> int { return capture_val; };
        printk("%-52s: %5zu bytes\n", "Lambda with int capture", sizeof(lambda_int));

        printk("%-52s: %5zu bytes\n", "std::function<int()>", sizeof(std::function<int()>));
}

/* ── Main ─────────────────────────────────────────────────────────── */

int main(void)
{
        printk("=== C++ Benchmark ===\n");
        printk("Iterations: %d (warmup: %d)\n", ITERATIONS, WARMUP_ITERATIONS);

        timing_init();
        timing_start();

        calibrate_timing_overhead();
        printk("Timing overhead: %llu cycles\n", (unsigned long long)timing_overhead_cycles);

        bench_virtual_dispatch();
        bench_object_lifecycle();
        bench_refcount();
        bench_introspection();
        bench_lambdas();

        timing_stop();

        printk("\nPROJECT EXECUTION SUCCESSFUL\n");
        return 0;
}
