/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Memory Benchmark: Plain C
 *
 * Simulates OOP via struct + function-pointer vtable and manual
 * atomic refcount. All allocations go through a dedicated sys_heap
 * with CONFIG_SYS_HEAP_RUNTIME_STATS for precise measurement.
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/sys_heap.h>
#include <zephyr/sys/printk.h>
#include <zephyr/sys/atomic.h>
#include <stddef.h>
#include <string.h>

/* ── Dedicated heap ───────────────────────────────────────────── */

#define BENCH_HEAP_SIZE 8192

static char bench_heap_mem[BENCH_HEAP_SIZE] __aligned(8);
static struct sys_heap bench_heap;

static void *bench_alloc(size_t size)
{
	return sys_heap_alloc(&bench_heap, size);
}

static void bench_free(void *ptr)
{
	sys_heap_free(&bench_heap, ptr);
}

static void heap_stats(struct sys_memory_stats *s)
{
	sys_heap_runtime_stats_get(&bench_heap, s);
}

/* ── VTable-based object hierarchy ────────────────────────────── */

struct c_base;

struct c_vtable {
	void (*nop)(struct c_base *self);
	int (*get_value)(struct c_base *self);
};

/*
 * c_base: vtable pointer (4) + atomic refcount (4) = 8 bytes
 */
struct c_base {
	const struct c_vtable *vt;
	atomic_t refcount;
};

static void c_base_nop(struct c_base *self)
{
	(void)self;
}

static int c_base_get_value(struct c_base *self)
{
	(void)self;
	return 0;
}

static const struct c_vtable c_base_vt = {
	.nop = c_base_nop,
	.get_value = c_base_get_value,
};

static struct c_base *c_base_alloc(void)
{
	struct c_base *obj = bench_alloc(sizeof(struct c_base));

	if (obj) {
		memset(obj, 0, sizeof(*obj));
		obj->vt = &c_base_vt;
		atomic_set(&obj->refcount, 1);
	}
	return obj;
}

/*
 * c_child: c_base (8) + field_a (4) = 12 bytes
 */
struct c_child {
	struct c_base base;
	int field_a;
};

static const struct c_vtable c_child_vt = {
	.nop = c_base_nop,
	.get_value = c_base_get_value,
};

static struct c_child *c_child_alloc(void)
{
	struct c_child *obj = bench_alloc(sizeof(struct c_child));

	if (obj) {
		memset(obj, 0, sizeof(*obj));
		obj->base.vt = &c_child_vt;
		atomic_set(&obj->base.refcount, 1);
	}
	return obj;
}

/*
 * c_grandchild: c_child (12) + field_b (4) = 16 bytes
 */
struct c_grandchild {
	struct c_child child;
	int field_b;
};

static const struct c_vtable c_grandchild_vt = {
	.nop = c_base_nop,
	.get_value = c_base_get_value,
};

static struct c_grandchild *c_grandchild_alloc(void)
{
	struct c_grandchild *obj = bench_alloc(sizeof(struct c_grandchild));

	if (obj) {
		memset(obj, 0, sizeof(*obj));
		obj->child.base.vt = &c_grandchild_vt;
		atomic_set(&obj->child.base.refcount, 1);
	}
	return obj;
}

/* ── Retain / Release ─────────────────────────────────────────── */

static void c_retain(struct c_base *obj)
{
	atomic_inc(&obj->refcount);
}

static void c_release(struct c_base *obj)
{
	if (atomic_dec(&obj->refcount) == 1) {
		bench_free(obj);
	}
}

/* ── Benchmark sections ───────────────────────────────────────── */

#define N_BULK 20

static void bench_object_sizes(void)
{
	printk("-- Object Sizes (sizeof) --\n");
	printk("  %-40s: %4zu bytes\n", "Base (vtable* + refcount)",
	       sizeof(struct c_base));
	printk("  %-40s: %4zu bytes\n", "Child (base + 1 int)",
	       sizeof(struct c_child));
	printk("  %-40s: %4zu bytes\n", "GrandChild (child + 1 int)",
	       sizeof(struct c_grandchild));
	printk("  %-40s: %4zu bytes\n", "Pointer size",
	       sizeof(void *));
	printk("  %-40s: %4zu bytes\n", "VTable pointer",
	       sizeof(const struct c_vtable *));
	printk("  %-40s: %4zu bytes\n", "Refcount (atomic_t)",
	       sizeof(atomic_t));
}

static void bench_single_alloc(void)
{
	struct sys_memory_stats before, after;
	size_t delta;

	printk("\n-- Single Allocation (heap delta) --\n");

	/* Base */
	heap_stats(&before);
	struct c_base *b = c_base_alloc();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "Base object", delta, sizeof(struct c_base),
	       delta - sizeof(struct c_base));
	c_release(b);

	/* Child */
	heap_stats(&before);
	struct c_child *ch = c_child_alloc();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "Child object", delta, sizeof(struct c_child),
	       delta - sizeof(struct c_child));
	c_release(&ch->base);

	/* GrandChild */
	heap_stats(&before);
	struct c_grandchild *gc = c_grandchild_alloc();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "GrandChild object", delta, sizeof(struct c_grandchild),
	       delta - sizeof(struct c_grandchild));
	c_release(&gc->child.base);
}

static void bench_bulk_alloc(void)
{
	struct sys_memory_stats before, after;
	size_t delta;

	printk("\n-- Bulk Allocation (%d objects) --\n", N_BULK);

	/* N_BULK Child objects */
	struct c_child *children[N_BULK];

	heap_stats(&before);
	for (int i = 0; i < N_BULK; i++) {
		children[i] = c_child_alloc();
	}
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (%zu bytes/obj)\n",
	       "20x Child total", delta, delta / N_BULK);

	for (int i = 0; i < N_BULK; i++) {
		c_release(&children[i]->base);
	}

	/* N_BULK GrandChild objects */
	struct c_grandchild *grandchildren[N_BULK];

	heap_stats(&before);
	for (int i = 0; i < N_BULK; i++) {
		grandchildren[i] = c_grandchild_alloc();
	}
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (%zu bytes/obj)\n",
	       "20x GrandChild total", delta, delta / N_BULK);

	for (int i = 0; i < N_BULK; i++) {
		c_release(&grandchildren[i]->child.base);
	}
}

static void bench_refcount(void)
{
	printk("\n-- Reference Counting --\n");
	printk("  %-40s: %4zu bytes (inline in struct)\n",
	       "Refcount overhead (atomic_t)", sizeof(atomic_t));
	printk("  %-40s:    0 bytes\n",
	       "Control block (none)");

	/* Demonstrate retain/release works */
	struct c_base *obj = c_base_alloc();

	c_retain(obj);
	printk("  %-40s: %4ld\n",
	       "After retain, refcount", (long)atomic_get(&obj->refcount));
	c_release(obj);
	printk("  %-40s: %4ld\n",
	       "After release, refcount", (long)atomic_get(&obj->refcount));
	c_release(obj); /* final release, frees */
}

static void bench_heap_summary(void)
{
	struct sys_memory_stats stats;

	heap_stats(&stats);
	printk("\n-- Heap Summary --\n");
	printk("  %-40s: %4zu\n", "allocated_bytes", stats.allocated_bytes);
	printk("  %-40s: %4zu\n", "free_bytes", stats.free_bytes);
	printk("  %-40s: %4zu\n", "max_allocated_bytes", stats.max_allocated_bytes);
}

/* ── Main ─────────────────────────────────────────────────────── */

int main(void)
{
	sys_heap_init(&bench_heap, bench_heap_mem, BENCH_HEAP_SIZE);

	printk("=== Memory Benchmark: C ===\n\n");

	bench_object_sizes();
	bench_single_alloc();
	bench_bulk_alloc();
	bench_refcount();
	bench_heap_summary();

	printk("\nPROJECT EXECUTION SUCCESSFUL\n");
	return 0;
}
