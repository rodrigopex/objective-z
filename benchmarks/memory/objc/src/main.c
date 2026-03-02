/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Memory Benchmark: Objective-C
 *
 * All ObjC allocations go through the runtime's dedicated sys_heap
 * (CONFIG_OBJZ_MEM_POOL_SIZE=8192). Measured via objc_stats() deltas.
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <objc/runtime.h>
#include <objc/malloc.h>

/* ── Extern wrappers from mem_helpers.m ───────────────────────── */

extern id mem_create_base(void);
extern id mem_create_child(void);
extern id mem_create_grandchild(void);
extern void mem_release(id obj);
extern size_t mem_sizeof_base(void);
extern size_t mem_sizeof_child(void);
extern size_t mem_sizeof_grandchild(void);

#define N_BULK 20

/* ── Benchmark sections ───────────────────────────────────────── */

static void bench_object_sizes(void)
{
	printk("-- Object Sizes (class_getInstanceSize) --\n");
	printk("  %-40s: %4zu bytes\n", "Base (isa + refcount)",
	       mem_sizeof_base());
	printk("  %-40s: %4zu bytes\n", "Child (base + 1 int)",
	       mem_sizeof_child());
	printk("  %-40s: %4zu bytes\n", "GrandChild (child + 1 int)",
	       mem_sizeof_grandchild());
	printk("  %-40s: %4zu bytes\n", "Pointer size (id)",
	       sizeof(id));
	printk("  %-40s: %4zu bytes\n", "Dispatch mechanism (isa pointer)",
	       sizeof(id));
	printk("  %-40s: %4s\n", "Refcount",
	       "inline in Object (atomic_t, 4 bytes)");
}

static void bench_single_alloc(void)
{
	struct sys_memory_stats before, after;
	size_t delta, obj_size;

	printk("\n-- Single Allocation (heap delta) --\n");

	/* Base */
	objc_stats(&before);
	id b = mem_create_base();
	objc_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	obj_size = mem_sizeof_base();
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "Base object", delta, obj_size, delta - obj_size);
	mem_release(b);

	/* Child */
	objc_stats(&before);
	id ch = mem_create_child();
	objc_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	obj_size = mem_sizeof_child();
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "Child object", delta, obj_size, delta - obj_size);
	mem_release(ch);

	/* GrandChild */
	objc_stats(&before);
	id gc = mem_create_grandchild();
	objc_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	obj_size = mem_sizeof_grandchild();
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "GrandChild object", delta, obj_size, delta - obj_size);
	mem_release(gc);
}

static void bench_bulk_alloc(void)
{
	struct sys_memory_stats before, after;
	size_t delta;

	printk("\n-- Bulk Allocation (%d objects) --\n", N_BULK);

	/* N_BULK Child objects */
	id children[N_BULK];

	objc_stats(&before);
	for (int i = 0; i < N_BULK; i++) {
		children[i] = mem_create_child();
	}
	objc_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (%zu bytes/obj)\n",
	       "20x Child total", delta, delta / N_BULK);

	for (int i = 0; i < N_BULK; i++) {
		mem_release(children[i]);
	}

	/* N_BULK GrandChild objects */
	id grandchildren[N_BULK];

	objc_stats(&before);
	for (int i = 0; i < N_BULK; i++) {
		grandchildren[i] = mem_create_grandchild();
	}
	objc_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (%zu bytes/obj)\n",
	       "20x GrandChild total", delta, delta / N_BULK);

	for (int i = 0; i < N_BULK; i++) {
		mem_release(grandchildren[i]);
	}
}

static void bench_arc_overhead(void)
{
	printk("\n-- ARC / Reference Counting --\n");
	printk("  %-40s: %4zu bytes (inline in Object)\n",
	       "Refcount overhead (atomic_t)", sizeof(atomic_t));
	printk("  %-40s:    0 bytes\n",
	       "Control block (none, refcount is inline)");
	printk("  %-40s:    0 bytes\n",
	       "ARC extra per-object cost (uses inline rc)");
}

static void bench_heap_summary(void)
{
	struct sys_memory_stats stats;

	objc_stats(&stats);
	printk("\n-- Heap Summary --\n");
	printk("  %-40s: %4zu\n", "allocated_bytes", stats.allocated_bytes);
	printk("  %-40s: %4zu\n", "free_bytes", stats.free_bytes);
	printk("  %-40s: %4zu\n", "max_allocated_bytes", stats.max_allocated_bytes);
}

/* ── Main ─────────────────────────────────────────────────────── */

int main(void)
{
	printk("=== Memory Benchmark: Objective-C ===\n\n");

	bench_object_sizes();
	bench_single_alloc();
	bench_bulk_alloc();
	bench_arc_overhead();
	bench_heap_summary();

	printk("\nPROJECT EXECUTION SUCCESSFUL\n");
	return 0;
}
