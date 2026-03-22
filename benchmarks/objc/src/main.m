/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Objective-Z Comprehensive Benchmark (OZ-070)
 *
 * Pure Objective-C benchmark — all measured code goes through
 * the OZ transpiler. Explicit timing loops (no C macros) to
 * work around OZ-071.
 *
 * 7 sections: Allocation, Dispatch, Lifecycle, Refcount,
 *             Properties/Sync, Foundation, Introspection.
 *
 * NOTE: Objective-Z supports single inheritance only.
 */

#import <Foundation/Foundation.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>

/* ── Protocol to force vtable (PROTOCOL) dispatch ─────────────────── */

@protocol Benchable
- (void)nop;
- (int)getValue;
@end

/* ── BenchBase (depth=0) ──────────────────────────────────────────── */

@interface BenchBase : OZObject <Benchable> {
	int _x;
}
@property(nonatomic, assign) int value;
@property(assign) int atomicValue;
- (void)nop;
- (int)getValue;
+ (void)classNop;
- (void)syncNop;
- (OZArray *)createBenchArray;
- (OZDictionary *)createBenchDict;
- (OZString *)benchKey;
@end

@implementation BenchBase
@synthesize value = _value;
@synthesize atomicValue = _atomicValue;

- (void)nop
{
}

- (int)getValue
{
	return _x;
}

+ (void)classNop
{
}

- (void)syncNop
{
	@synchronized(self) {
	}
}

- (OZArray *)createBenchArray
{
	return @[@0, @1, @2, @3, @4, @5, @6, @7, @8, @9];
}

- (OZDictionary *)createBenchDict
{
	return @{@"bench_key": @42};
}

- (OZString *)benchKey
{
	return @"bench_key";
}

@end

/* ── BenchChild (depth=1) ─────────────────────────────────────────── */

@interface BenchChild : BenchBase
@end

@implementation BenchChild
@end

/* ── BenchGrandChild (depth=2) ────────────────────────────────────── */

@interface BenchGrandChild : BenchChild
@end

@implementation BenchGrandChild
@end

/* ── Iteration tiers ──────────────────────────────────────────────── */

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

/* Function pointer stored as global to prevent devirtualization */
static void (*c_nop_ptr)(void *) = c_nop;

/* ── Block baseline (OZ emits blocks as static C functions) ───────── */

static int block_nop_fn(void)
{
	__asm__ volatile("" ::: "memory");
	return 0;
}

/* ── Heap buffer for OZHeap benchmark (module-level for alignment) ── */

static char oz_heap_buf[4096];

/* ── Section 1: Allocation ────────────────────────────────────────── */

static void bench_allocation(void)
{
	printk("\n--- 1. Allocation ---\n");
	timing_t s, e;
	uint64_t total;

	/* slab alloc + init + release (BenchBase) */
	total = 0;
	for (int i = 0; i < SLOW_ITERATIONS; i++) {
		s = timing_counter_get();
		BenchBase *obj = [[BenchBase alloc] init];
		[obj release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("slab alloc + init + release (BenchBase)", total, SLOW_ITERATIONS);

	/* slab alloc + init + release (BenchChild) */
	total = 0;
	for (int i = 0; i < SLOW_ITERATIONS; i++) {
		s = timing_counter_get();
		BenchChild *obj = [[BenchChild alloc] init];
		[obj release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("slab alloc + init + release (BenchChild)", total, SLOW_ITERATIONS);

	/* slab alloc + init + release (BenchGrandChild) */
	total = 0;
	for (int i = 0; i < SLOW_ITERATIONS; i++) {
		s = timing_counter_get();
		BenchGrandChild *obj = [[BenchGrandChild alloc] init];
		[obj release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("slab alloc + init + release (GrandChild)", total, SLOW_ITERATIONS);

	/* heap alloc + init + release (BenchBase via OZHeap) */
	OZHeap *heap = [[OZHeap alloc] initWithBuffer:oz_heap_buf size:sizeof(oz_heap_buf)];

	total = 0;
	for (int i = 0; i < SLOW_ITERATIONS; i++) {
		s = timing_counter_get();
		BenchBase *obj = [[BenchBase allocWithHeap:heap] init];
		[obj release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("heap alloc + init + release (BenchBase)", total, SLOW_ITERATIONS);

	[heap release];
}

/* ── Section 2: Dispatch ──────────────────────────────────────────── */

static void bench_dispatch(void)
{
	printk("\n--- 2. Dispatch ---\n");
	timing_t s, e;
	uint64_t total;

	BenchBase *base = [[BenchBase alloc] init];
	BenchChild *child = [[BenchChild alloc] init];
	BenchGrandChild *gchild = [[BenchGrandChild alloc] init];

	/* C function pointer baseline (global ptr prevents devirtualization) */
	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		c_nop_ptr((__bridge void *)base);
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("C function pointer (baseline)", total, FAST_ITERATIONS);

	/* Static dispatch: type known at compile time */
	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		[base nop];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("Static dispatch [base nop]", total, FAST_ITERATIONS);

	/* Class method */
	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		[BenchBase classNop];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("Class method [BenchBase classNop]", total, FAST_ITERATIONS);

	/* Vtable dispatch at various inheritance depths */
	id<Benchable> poly0 = base;
	id<Benchable> poly1 = child;
	id<Benchable> poly2 = gchild;

	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		[poly0 nop];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("Vtable dispatch (depth=0)", total, ITERATIONS);

	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		[poly1 nop];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("Vtable dispatch (depth=1)", total, ITERATIONS);

	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		[poly2 nop];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("Vtable dispatch (depth=2)", total, ITERATIONS);

	/* Block invocation (transpiled to static C function ptr) */
	int (^blk)(void) = ^{ return block_nop_fn(); };

	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		(void)blk();
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("Block invocation (static fn ptr)", total, FAST_ITERATIONS);

	[base release];
	[child release];
	[gchild release];
}

/* ── Section 3: Object Lifecycle ──────────────────────────────────── */

static void bench_lifecycle(void)
{
	printk("\n--- 3. Object Lifecycle ---\n");
	timing_t s, e;
	uint64_t total;

	/* alloc + init + release */
	total = 0;
	for (int i = 0; i < SLOW_ITERATIONS; i++) {
		s = timing_counter_get();
		BenchBase *obj = [[BenchBase alloc] init];
		[obj release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("alloc + init + release", total, SLOW_ITERATIONS);

	/* alloc + init + retain + 2x release */
	total = 0;
	for (int i = 0; i < SLOW_ITERATIONS; i++) {
		s = timing_counter_get();
		BenchBase *obj = [[BenchBase alloc] init];
		[obj retain];
		[obj release];
		[obj release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("alloc + init + retain + 2x release", total, SLOW_ITERATIONS);
}

/* ── Section 4: Reference Counting ────────────────────────────────── */

static void bench_refcount(void)
{
	printk("\n--- 4. Reference Counting ---\n");
	timing_t s, e;
	uint64_t total;

	BenchBase *base = [[BenchBase alloc] init];

	/* retain (atomic inc) */
	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		[base retain];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("[obj retain] (atomic inc)", total, ITERATIONS);

	/* Balance accumulated retains */
	for (int i = 0; i < ITERATIONS; i++) {
		[base release];
	}

	/* retain + release pair */
	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		[base retain];
		[base release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("retain + release pair", total, ITERATIONS);

	[base release];
}

/* ── Section 5: Properties / Synchronization ──────────────────────── */

static void bench_properties_sync(void)
{
	printk("\n--- 5. Properties / Synchronization ---\n");
	timing_t s, e;
	uint64_t total;

	BenchBase *obj = [[BenchBase alloc] init];

	/* property get (nonatomic) */
	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		(void)[obj value];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("property get (nonatomic)", total, FAST_ITERATIONS);

	/* property set (nonatomic) */
	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		[obj setValue:42];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("property set (nonatomic)", total, FAST_ITERATIONS);

	/* property get (atomic) */
	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		(void)[obj atomicValue];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("property get (atomic)", total, ITERATIONS);

	/* property set (atomic) */
	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		[obj setAtomicValue:42];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("property set (atomic)", total, ITERATIONS);

	/* @synchronized (empty critical section) */
	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		[obj syncNop];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("@synchronized (empty critical section)", total, ITERATIONS);

	[obj release];
}

/* ── Section 6: Foundation Operations ─────────────────────────────── */

static void bench_foundation(void)
{
	printk("\n--- 6. Foundation Operations ---\n");
	timing_t s, e;
	uint64_t total;

	/* OZNumber box + unbox */
	total = 0;
	for (int i = 0; i < SLOW_ITERATIONS; i++) {
		s = timing_counter_get();
		OZNumber *n = @42;
		volatile int32_t v = [n int32Value];
		(void)v;
		[n release];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("OZNumber box + unbox (int32)", total, SLOW_ITERATIONS);

	/* OZNumber unbox only */
	OZNumber *num = @99;

	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		(void)[num int32Value];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("OZNumber int32Value (unbox only)", total, FAST_ITERATIONS);

	[num release];

	/* OZArray random access */
	BenchBase *helper = [[BenchBase alloc] init];
	OZArray *arr = [helper createBenchArray];

	total = 0;
	for (int i = 0; i < FAST_ITERATIONS; i++) {
		s = timing_counter_get();
		(void)[arr objectAtIndex:5];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("OZArray objectAtIndex: (random access)", total, FAST_ITERATIONS);

	/* OZArray for-in iteration (10 items) */
	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		volatile int32_t sum = 0;
		for (OZNumber *n in arr) {
			sum += [n int32Value];
		}
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("OZArray for-in iteration (10 items)", total, ITERATIONS);

	/* OZArray raw loop via objectAtIndex: (10 items) */
	unsigned int arr_count = [arr count];

	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		volatile int32_t sum = 0;
		for (unsigned int j = 0; j < arr_count; j++) {
			OZNumber *n = [arr objectAtIndex:j];
			sum += [n int32Value];
		}
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("OZArray raw loop objectAtIndex: (10)", total, ITERATIONS);

	/* OZDictionary lookup (key from benchKey to avoid OZ-072 dup string) */
	OZDictionary *dict = [helper createBenchDict];
	OZString *key = [helper benchKey];

	total = 0;
	for (int i = 0; i < ITERATIONS; i++) {
		s = timing_counter_get();
		(void)[dict objectForKey:key];
		e = timing_counter_get();
		total += timing_cycles_get(&s, &e);
	}
	bench_report("OZDictionary objectForKey: (lookup)", total, ITERATIONS);

	[dict release];
	[arr release];
	[helper release];
}

/*
 * Section 7 (Introspection) omitted from ObjC benchmark.
 * OZ introspection uses generated C functions (oz_isKindOfClass, oz_name)
 * that aren't accessible from ObjC at Clang parse time.
 * C++ benchmark includes RTTI (dynamic_cast, typeid) for comparison.
 * TODO: Add [obj isKindOfClass:] and [obj className] to OZ SDK.
 */

/* ── Object Sizes ─────────────────────────────────────────────────── */

static void print_sizes(void)
{
	printk("\n--- Object Sizes ---\n");
	printk("  %-48s: %5zu bytes\n", "OZObject (class_id + refcount)",
	       sizeof(OZObject));
	printk("  %-48s: %5zu bytes\n", "BenchBase (OZObject + props + ivar)",
	       sizeof(BenchBase));
	printk("  %-48s: %5zu bytes\n", "BenchChild (BenchBase, no extra)",
	       sizeof(BenchChild));
	printk("  %-48s: %5zu bytes\n", "BenchGrandChild",
	       sizeof(BenchGrandChild));
	printk("  %-48s: %5zu bytes\n", "OZString",
	       sizeof(OZString));
	printk("  %-48s: %5zu bytes\n", "OZNumber",
	       sizeof(OZNumber));
	printk("  %-48s: %5zu bytes\n", "OZArray",
	       sizeof(OZArray));
	printk("  %-48s: %5zu bytes\n", "OZDictionary",
	       sizeof(OZDictionary));
	printk("  %-48s: %5zu bytes\n", "Pointer size",
	       sizeof(void *));
	printk("  %-48s: %5s\n", "Dispatch mechanism",
	       "class_id enum + const vtable array");
}

/* ── Main ─────────────────────────────────────────────────────────── */

int main(void)
{
	printk("=== Objective-Z Benchmark (OZ-070) ===\n");
	printk("Board: %s\n", CONFIG_BOARD);
	printk("Iterations: fast=%d, normal=%d, slow=%d\n",
	       FAST_ITERATIONS, ITERATIONS, SLOW_ITERATIONS);
	printk("NOTE: Single inheritance only (ObjC limitation)\n");

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
	bench_foundation();
	/* bench_introspection() — TODO: pending OZ SDK isKindOfClass:/className */

	timing_stop();

	print_sizes();

	printk("\nPROJECT EXECUTION SUCCESSFUL\n");
	return 0;
}
