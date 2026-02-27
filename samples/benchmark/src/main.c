/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Objective-Z Runtime Benchmark
 *
 * Measures key runtime operations with cycle-accurate timing on
 * ARM Cortex-M via Zephyr's DWT-based timing API.
 * All ObjC interactions go through extern C wrappers in bench_helpers.m.
 */
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>
#include <zephyr/logging/log.h>
#include <objc/runtime.h>
#include <objc/arc.h>
#include <objc/malloc.h>

LOG_MODULE_REGISTER(bench, LOG_LEVEL_INF);

/* ── Configuration ────────────────────────────────────────────────── */

#ifndef CONFIG_BENCHMARK_ITERATIONS
#define CONFIG_BENCHMARK_ITERATIONS 10000
#endif

#define WARMUP_ITERATIONS 100
#define ITERATIONS        CONFIG_BENCHMARK_ITERATIONS

/* ── Extern C wrappers from bench_helpers.m ───────────────────────── */

extern id bench_create_base(void);
extern id bench_create_child(void);
extern id bench_create_grandchild(void);
extern id bench_create_pooled(void);
extern void bench_nop(id obj);
extern int bench_get_value(id obj);
extern void bench_class_nop(void);
extern void bench_retain(id obj);
extern void bench_release(id obj);
extern void bench_dealloc(id obj);
extern void (*bench_get_nop_imp(id obj))(id, SEL);
extern SEL bench_get_nop_sel(void);
extern BOOL bench_responds_to_nop(id obj);
extern BOOL bench_responds_to_missing(id obj);
extern Class bench_get_class(id obj);
extern void bench_flush_cache(id obj);
extern void bench_ozlog_simple(void);
extern void bench_ozlog_int(void);
extern void bench_ozlog_string(void);
extern void bench_ozlog_objat(id obj);

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

/* ── Benchmark: Message Dispatch ──────────────────────────────────── */

static void bench_message_dispatch(void)
{
	printk("\n--- Message Dispatch ---\n");

	id base = bench_create_base();
	id child = bench_create_child();
	id gchild = bench_create_grandchild();

	/* Get IMP + SEL for direct call baseline */
	void (*direct_nop)(id, SEL) = bench_get_nop_imp(base);
	SEL nop_sel = bench_get_nop_sel();

	BENCH_LOOP("C function call (baseline)", direct_nop(base, nop_sel));

	BENCH_LOOP("objc_msgSend (instance method)", bench_nop(base));

	BENCH_LOOP("objc_msgSend (class method)", bench_class_nop());

	BENCH_LOOP("objc_msgSend (inherited depth=1)", bench_nop(child));

	BENCH_LOOP("objc_msgSend (inherited depth=2)", bench_nop(gchild));

	BENCH_LOOP("objc_msgSend (cold cache, depth=0)", {
		bench_flush_cache(base);
		bench_nop(base);
	});

	BENCH_LOOP("objc_msgSend (cold cache, depth=2)", {
		bench_flush_cache(gchild);
		bench_nop(gchild);
	});

	bench_dealloc(base);
	bench_dealloc(child);
	bench_dealloc(gchild);
}

/* ── Benchmark: Object Lifecycle ──────────────────────────────────── */

static void bench_object_lifecycle(void)
{
	printk("\n--- Object Lifecycle ---\n");

	BENCH_LOOP("alloc/init/release (heap, MRR)", {
		id obj = bench_create_base();
		bench_dealloc(obj);
	});

	BENCH_LOOP("alloc/init/release (static pool)", {
		id obj = bench_create_pooled();
		bench_dealloc(obj);
	});
}

/* ── Benchmark: Reference Counting ────────────────────────────────── */

static void bench_refcount(void)
{
	printk("\n--- Reference Counting ---\n");

	id obj = bench_create_base();

	BENCH_LOOP("retain", bench_retain(obj));

	/* Balance accumulated retains: warmup + measurement */
	for (int i = 0; i < WARMUP_ITERATIONS + ITERATIONS; i++) {
		bench_release(obj);
	}

	BENCH_LOOP("retain + release pair", {
		bench_retain(obj);
		bench_release(obj);
	});

	bench_dealloc(obj);
}

/* ── Benchmark: ARC ───────────────────────────────────────────────── */

static void bench_arc_ops(void)
{
	printk("\n--- ARC ---\n");

	id obj = bench_create_base();

	BENCH_LOOP("objc_retain", objc_retain(obj));

	/* Balance accumulated retains */
	for (int i = 0; i < WARMUP_ITERATIONS + ITERATIONS; i++) {
		objc_release(obj);
	}

	BENCH_LOOP("objc_release", {
		objc_retain(obj);
		objc_release(obj);
	});

	BENCH_LOOP("objc_storeStrong", {
		id slot = nil;
		objc_storeStrong(&slot, obj);
		objc_storeStrong(&slot, nil);
	});

	bench_dealloc(obj);
}

/* ── Benchmark: Introspection ─────────────────────────────────────── */

static void bench_introspection(void)
{
	printk("\n--- Introspection ---\n");

	id obj = bench_create_base();

	BENCH_LOOP("class_respondsToSelector (YES)", bench_responds_to_nop(obj));

	BENCH_LOOP("class_respondsToSelector (NO)", bench_responds_to_missing(obj));

	BENCH_LOOP("object_getClass", bench_get_class(obj));

	bench_dealloc(obj);
}

/* ── Benchmark: Blocks ────────────────────────────────────────────── */

#ifdef CONFIG_OBJZ_BLOCKS
#include <objc/blocks.h>

extern int (*bench_get_c_func(void))(void);
extern void *bench_get_global_block(void);
extern int bench_invoke_int_block(void *blk);
extern unsigned long bench_block_size_int_capture(void);
extern unsigned long bench_block_size_obj_capture(void);
extern unsigned long bench_block_size_byref(void);
extern void *bench_copy_int_block(int value);
extern void *bench_copy_obj_block(id obj);
extern void *bench_copy_byref_block(void);
extern void bench_release_block(void *blk);
extern void *bench_block_copy(void *blk);

static void bench_blocks_memory(void)
{
	printk("\n--- Blocks: Memory (C func ptr vs Block) ---\n");
	printk("%-52s: %5zu bytes\n", "C function pointer",
	       sizeof(int (*)(void)));
	printk("%-52s: %5zu bytes\n", "Block pointer (reference)",
	       sizeof(void *));
	printk("%-52s: %5zu bytes\n", "Block literal (struct Block_layout)",
	       sizeof(struct Block_layout));
	printk("%-52s: %5lu bytes\n", "Block + int capture (descriptor->size)",
	       bench_block_size_int_capture());
	printk("%-52s: %5lu bytes\n", "Block + ObjC object capture (descriptor->size)",
	       bench_block_size_obj_capture());
	printk("%-52s: %5lu bytes\n", "Block + __block int (descriptor->size)",
	       bench_block_size_byref());

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	struct sys_memory_stats before, after;

	/* Measure heap cost of _Block_copy (int capture) */
	objc_stats(&before);
	void *blk = bench_copy_int_block(42);
	objc_stats(&after);
	printk("%-52s: %5zu bytes\n", "Heap cost: _Block_copy (int capture)",
	       after.allocated_bytes - before.allocated_bytes);
	bench_release_block(blk);

	/* Measure heap cost of _Block_copy (ObjC object capture) */
	id obj = bench_create_base();
	objc_stats(&before);
	blk = bench_copy_obj_block(obj);
	objc_stats(&after);
	printk("%-52s: %5zu bytes\n", "Heap cost: _Block_copy (obj capture)",
	       after.allocated_bytes - before.allocated_bytes);
	bench_release_block(blk);
	bench_dealloc(obj);

	/* Measure heap cost of _Block_copy (__block variable) */
	objc_stats(&before);
	blk = bench_copy_byref_block();
	objc_stats(&after);
	printk("%-52s: %5zu bytes\n", "Heap cost: _Block_copy (__block int)",
	       after.allocated_bytes - before.allocated_bytes);
	bench_release_block(blk);
#endif
}

static void bench_blocks_perf(void)
{
	printk("\n--- Blocks: Performance ---\n");

	/* C function pointer invocation baseline */
	int (*c_func)(void) = bench_get_c_func();

	BENCH_LOOP("C function pointer call", (void)c_func());

	/* Global block invocation */
	void *global_blk = bench_get_global_block();

	BENCH_LOOP("Global block invocation", bench_invoke_int_block(global_blk));

	/* Heap block invocation (int capture) */
	void *heap_blk = bench_copy_int_block(7);

	BENCH_LOOP("Heap block invocation (int capture)",
		   bench_invoke_int_block(heap_blk));
	bench_release_block(heap_blk);

	/* _Block_copy + _Block_release (int capture) */
	BENCH_LOOP("_Block_copy + _Block_release (int capture)", {
		void *b = bench_copy_int_block(7);
		bench_release_block(b);
	});

	/* _Block_copy retain (already on heap) */
	heap_blk = bench_copy_int_block(7);
	BENCH_LOOP("_Block_copy (retain heap block)", {
		void *b = bench_block_copy(heap_blk);
		bench_release_block(b);
	});
	bench_release_block(heap_blk);
}
#endif /* CONFIG_OBJZ_BLOCKS */

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
		       bench_ozlog_simple());

	/* Integer formatting */
	LOG_BENCH_LOOP("printk (integer format)",
		       printk("Value: %d\n", 42));

	LOG_BENCH_LOOP("LOG_INF (integer format)",
		       LOG_INF("Value: %d", 42));

	LOG_BENCH_LOOP("OZLog (integer format)",
		       bench_ozlog_int());

	/* String formatting */
	LOG_BENCH_LOOP("printk (string format)",
		       printk("Name: %s\n", "test"));

	LOG_BENCH_LOOP("LOG_INF (string format)",
		       LOG_INF("Name: %s", "test"));

	LOG_BENCH_LOOP("OZLog (string format)",
		       bench_ozlog_string());

	/* OZLog-only: %@ object description */
	id obj = bench_create_base();

	LOG_BENCH_LOOP("OZLog (%@ object format)",
		       bench_ozlog_objat(obj));

	bench_dealloc(obj);
}

/* ── Main ─────────────────────────────────────────────────────────── */

int main(void)
{
	printk("=== Objective-Z Runtime Benchmark ===\n");
	printk("Iterations: %d (warmup: %d)\n", ITERATIONS, WARMUP_ITERATIONS);

	timing_init();
	timing_start();

	calibrate_timing_overhead();
	printk("Timing overhead: %llu cycles\n", (unsigned long long)timing_overhead_cycles);

	bench_message_dispatch();
	bench_object_lifecycle();
	bench_refcount();
	bench_arc_ops();
	bench_introspection();

#ifdef CONFIG_OBJZ_BLOCKS
	bench_blocks_memory();
	bench_blocks_perf();
#endif

	bench_logging();

	timing_stop();

	printk("\n--- Memory ---\n");
#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	struct sys_memory_stats heap_stats;

	objc_stats(&heap_stats);
	printk("ObjC heap: %zu allocated, %zu free, %zu max allocated\n",
	       heap_stats.allocated_bytes, heap_stats.free_bytes,
	       heap_stats.max_allocated_bytes);
#endif

	printk("\nPROJECT EXECUTION SUCCESSFUL\n");
	return 0;
}
