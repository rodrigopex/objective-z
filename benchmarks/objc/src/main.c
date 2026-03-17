/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Objective-Z Transpiler Benchmark
 *
 * Measures key transpiler-generated operations with cycle-accurate
 * timing on ARM Cortex-M via Zephyr's DWT-based timing API.
 * All ObjC classes are transpiled to plain C via objz_transpile_sources().
 */
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>
#include <zephyr/logging/log.h>

/* Transpiler-generated headers */
#include "oz_dispatch.h"
#include "OZObject_ozh.h"
#include "bench_helpers_ozh.h"
#include "BenchChild_ozh.h"
#include "BenchGrandChild_ozh.h"

/* OZLog declaration (implemented in src/OZLog.c) */
extern void OZLog(const char *fmt, ...);

LOG_MODULE_REGISTER(bench, LOG_LEVEL_INF);

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

/**
 * BENCH_LOOP: measure a code snippet over ITERATIONS.
 *
 * - Runs WARMUP_ITERATIONS warmup (discarded).
 * - Measures ITERATIONS, accumulates total cycles.
 * - Subtracts timing overhead, prints avg cycles and ns.
 */
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
		       (unsigned long long)_ns);                                                    \
	} while (0)

/* ── C function baseline (not through transpiler dispatch) ────────── */

static void c_nop(struct OZObject *self)
{
	(void)self;
}

/* ── Benchmark: Dispatch ──────────────────────────────────────────── */

static void bench_dispatch(void)
{
	printk("\n--- Dispatch ---\n");

	struct BenchBase *base = BenchBase_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)base);
	struct BenchChild *child = BenchChild_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)child);
	struct BenchGrandChild *gchild = BenchGrandChild_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)gchild);

	/* C function pointer call baseline (prevents inlining) */
	void (*volatile fn_ptr)(struct OZObject *) = c_nop;
	BENCH_LOOP("C function pointer (baseline)", fn_ptr((struct OZObject *)base));

	/* Static dispatch: direct call, type known at compile time */
	BENCH_LOOP("Static dispatch (direct call)", BenchBase_nop(base));

	/* Class method: transpiled to static C function */
	BENCH_LOOP("Class method (static function)", BenchBase_cls_classNop());

	/* Vtable dispatch via OZ_SEND_* at various inheritance depths */
	BENCH_LOOP("Vtable dispatch (depth=0)", OZ_PROTOCOL_SEND_nop((struct OZObject *)base));

	BENCH_LOOP("Vtable dispatch (depth=1)", OZ_PROTOCOL_SEND_nop((struct OZObject *)child));

	BENCH_LOOP("Vtable dispatch (depth=2)", OZ_PROTOCOL_SEND_nop((struct OZObject *)gchild));

	OZObject_release((struct OZObject *)base);
	OZObject_release((struct OZObject *)child);
	OZObject_release((struct OZObject *)gchild);
}

/* ── Benchmark: Object Lifecycle ──────────────────────────────────── */

static void bench_object_lifecycle(void)
{
	printk("\n--- Object Lifecycle ---\n");

	BENCH_LOOP("slab alloc + init + release", {
		struct BenchBase *obj = BenchBase_alloc();
		OZ_PROTOCOL_SEND_init((struct OZObject *)obj);
		OZObject_release((struct OZObject *)obj);
	});
}

/* ── Benchmark: Reference Counting ────────────────────────────────── */

static void bench_refcount(void)
{
	printk("\n--- Reference Counting ---\n");

	struct BenchBase *base = BenchBase_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)base);
	struct OZObject *obj = (struct OZObject *)base;

	BENCH_LOOP("OZObject_retain (atomic inc)", OZObject_retain(obj));

	/* Balance accumulated retains: warmup + measurement */
	for (int i = 0; i < WARMUP_ITERATIONS + ITERATIONS; i++) {
		OZObject_release(obj);
	}

	BENCH_LOOP("retain + release pair", {
		OZObject_retain(obj);
		OZObject_release(obj);
	});

	OZObject_release(obj);
}

/* ── Benchmark: Logging ───────────────────────────────────────────── */

/*
 * Fewer iterations for logging benchmarks to limit console output.
 * Each iteration produces a full printk/LOG_INF/OZLog line.
 */
#define LOG_ITERATIONS 50
#define LOG_WARMUP     5

#define LOG_BENCH_LOOP(desc, code)                                                                 \
	do {                                                                                       \
		timing_t _start, _end;                                                             \
		uint64_t _total_cycles = 0;                                                        \
                                                                                                   \
		for (int _w = 0; _w < LOG_WARMUP; _w++) {                                          \
			code;                                                                      \
		}                                                                                  \
                                                                                                   \
		for (int _i = 0; _i < LOG_ITERATIONS; _i++) {                                      \
			_start = timing_counter_get();                                              \
			code;                                                                      \
			_end = timing_counter_get();                                               \
			_total_cycles += timing_cycles_get(&_start, &_end);                        \
		}                                                                                  \
                                                                                                   \
		uint64_t _avg = _total_cycles / LOG_ITERATIONS;                                    \
		if (_avg > timing_overhead_cycles) {                                                \
			_avg -= timing_overhead_cycles;                                             \
		}                                                                                  \
		uint64_t _ns = timing_cycles_to_ns(_avg);                                          \
		printk("%-52s: %5llu cycles , %5llu ns\n", desc,                                   \
		       (unsigned long long)_avg, (unsigned long long)_ns);                          \
	} while (0)

static void bench_logging(void)
{
	printk("\n--- Logging ---\n");

	/* Simple string (no format args) */
	LOG_BENCH_LOOP("printk (simple string)",
		       printk("Hello benchmark\n"));

	LOG_BENCH_LOOP("LOG_INF (simple string)",
		       LOG_INF("Hello benchmark"));

	LOG_BENCH_LOOP("OZLog (simple string)",
		       OZLog("Hello benchmark"));

	/* Integer formatting */
	LOG_BENCH_LOOP("printk (integer format)",
		       printk("Value: %d\n", 42));

	LOG_BENCH_LOOP("LOG_INF (integer format)",
		       LOG_INF("Value: %d", 42));

	LOG_BENCH_LOOP("OZLog (integer format)",
		       OZLog("Value: %d", 42));

	/* String formatting */
	LOG_BENCH_LOOP("printk (string format)",
		       printk("Name: %s\n", "test"));

	LOG_BENCH_LOOP("LOG_INF (string format)",
		       LOG_INF("Name: %s", "test"));

	LOG_BENCH_LOOP("OZLog (string format)",
		       OZLog("Name: %s", "test"));
}

/* ── Main ─────────────────────────────────────────────────────────── */

int main(void)
{
	printk("=== Objective-Z Transpiler Benchmark ===\n");
	printk("Iterations: %d (warmup: %d)\n", ITERATIONS, WARMUP_ITERATIONS);

	timing_init();
	timing_start();

	calibrate_timing_overhead();
	printk("Timing overhead: %llu cycles\n", (unsigned long long)timing_overhead_cycles);

	bench_dispatch();
	bench_object_lifecycle();
	bench_refcount();
	bench_logging();

	timing_stop();

	printk("\n--- Object Sizes ---\n");
	printk("%-52s: %5zu bytes\n", "OZObject (class_id + refcount)",
	       sizeof(struct OZObject));
	printk("%-52s: %5zu bytes\n", "BenchBase (OZObject + int _x)",
	       sizeof(struct BenchBase));
	printk("%-52s: %5zu bytes\n", "BenchChild (BenchBase, no extra ivars)",
	       sizeof(struct BenchChild));
	printk("%-52s: %5zu bytes\n", "BenchGrandChild (BenchChild, no extra ivars)",
	       sizeof(struct BenchGrandChild));

	printk("\nPROJECT EXECUTION SUCCESSFUL\n");
	return 0;
}
