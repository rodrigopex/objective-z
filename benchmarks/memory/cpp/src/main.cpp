/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Memory Benchmark: C++
 *
 * Virtual classes with std::atomic refcount. All allocations go
 * through a dedicated sys_heap via overridden operator new/delete.
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/sys_heap.h>
#include <zephyr/sys/printk.h>

#include <atomic>
#include <cstddef>
#include <cstring>
#include <functional>
#include <memory>
#include <new>
#include <zephyr/spinlock.h>

/* ── Dedicated heap ───────────────────────────────────────────── */

#define BENCH_HEAP_SIZE 8192

static char bench_heap_mem[BENCH_HEAP_SIZE] __aligned(8);
static struct sys_heap bench_heap;
static bool heap_ready;

static void heap_stats(struct sys_memory_stats *s)
{
	sys_heap_runtime_stats_get(&bench_heap, s);
}

/* ── Global operator new/delete → sys_heap ────────────────────── */

void *operator new(std::size_t size) noexcept
{
	if (!heap_ready) {
		return nullptr;
	}
	return sys_heap_alloc(&bench_heap, size == 0 ? 1 : size);
}

void *operator new[](std::size_t size) noexcept
{
	return operator new(size);
}

void operator delete(void *ptr) noexcept
{
	if (ptr && heap_ready) {
		sys_heap_free(&bench_heap, ptr);
	}
}

void operator delete[](void *ptr) noexcept
{
	operator delete(ptr);
}

void operator delete(void *ptr, std::size_t) noexcept
{
	operator delete(ptr);
}

void operator delete[](void *ptr, std::size_t) noexcept
{
	operator delete(ptr);
}

/* ── Class hierarchy ──────────────────────────────────────────── */

/*
 * MemBase: implicit vptr (4) + atomic<int> refcount (4) = 8 bytes
 */
class MemBase {
public:
	std::atomic<int> refcount{1};

	virtual ~MemBase() = default;
	virtual void nop() {}
	virtual int getValue() { return 0; }

	void retain()
	{
		refcount.fetch_add(1, std::memory_order_relaxed);
	}

	bool release()
	{
		return refcount.fetch_sub(1, std::memory_order_acq_rel) == 1;
	}
};

/*
 * MemChild: MemBase (8) + field_a (4) + padding (0) = 12 bytes
 */
class MemChild : public MemBase {
public:
	int field_a{0};
};

/*
 * MemGrandChild: MemChild (12) + field_b (4) = 16 bytes
 */
class MemGrandChild : public MemChild {
public:
	int field_b{0};
};

/* ── Benchmark sections ───────────────────────────────────────── */

#define N_BULK 20

static void bench_object_sizes()
{
	printk("-- Object Sizes (sizeof) --\n");
	printk("  %-40s: %4zu bytes\n", "Base (vptr + atomic<int>)",
	       sizeof(MemBase));
	printk("  %-40s: %4zu bytes\n", "Child (base + 1 int)",
	       sizeof(MemChild));
	printk("  %-40s: %4zu bytes\n", "GrandChild (child + 1 int)",
	       sizeof(MemGrandChild));
	printk("  %-40s: %4zu bytes\n", "Pointer size",
	       sizeof(void *));
	printk("  %-40s: %4zu bytes\n", "Dispatch mechanism (vptr)",
	       sizeof(void *));
	printk("  %-40s: %4zu bytes\n", "Refcount (atomic<int>)",
	       sizeof(std::atomic<int>));
}

static void bench_single_alloc()
{
	struct sys_memory_stats before, after;
	size_t delta;

	printk("\n-- Single Allocation (heap delta) --\n");

	/* Base */
	heap_stats(&before);
	volatile MemBase *b = new MemBase();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "Base object", delta, sizeof(MemBase), delta - sizeof(MemBase));
	delete const_cast<MemBase *>(b);

	/* Child */
	heap_stats(&before);
	volatile MemChild *ch = new MemChild();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "Child object", delta, sizeof(MemChild), delta - sizeof(MemChild));
	delete const_cast<MemChild *>(ch);

	/* GrandChild */
	heap_stats(&before);
	volatile MemGrandChild *gc = new MemGrandChild();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "GrandChild object", delta, sizeof(MemGrandChild),
	       delta - sizeof(MemGrandChild));
	delete const_cast<MemGrandChild *>(gc);
}

static void bench_bulk_alloc()
{
	struct sys_memory_stats before, after;
	size_t delta;

	printk("\n-- Bulk Allocation (%d objects) --\n", N_BULK);

	/* N_BULK Child objects */
	MemChild *children[N_BULK];

	heap_stats(&before);
	for (int i = 0; i < N_BULK; i++) {
		children[i] = new MemChild();
	}
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (%zu bytes/obj)\n",
	       "20x Child total", delta, delta / N_BULK);

	for (int i = 0; i < N_BULK; i++) {
		delete children[i];
	}

	/* N_BULK GrandChild objects */
	MemGrandChild *grandchildren[N_BULK];

	heap_stats(&before);
	for (int i = 0; i < N_BULK; i++) {
		grandchildren[i] = new MemGrandChild();
	}
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (%zu bytes/obj)\n",
	       "20x GrandChild total", delta, delta / N_BULK);

	for (int i = 0; i < N_BULK; i++) {
		delete grandchildren[i];
	}
}

static void bench_smart_pointers()
{
	struct sys_memory_stats before, after;
	size_t delta;

	printk("\n-- Smart Pointer / Ref Counting --\n");

	/* sizeof the pointer types */
	printk("  %-40s: %4zu bytes\n", "sizeof(unique_ptr<MemBase>)",
	       sizeof(std::unique_ptr<MemBase>));
	printk("  %-40s: %4zu bytes\n", "sizeof(shared_ptr<MemBase>)",
	       sizeof(std::shared_ptr<MemBase>));

	/* unique_ptr: no control block, just the object */
	heap_stats(&before);
	auto up = std::make_unique<MemBase>();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (sizeof %zu + overhead %zu)\n",
	       "make_unique<MemBase> heap cost", delta, sizeof(MemBase),
	       delta - sizeof(MemBase));
	up.reset();

	/* make_shared: object + control block in single allocation */
	heap_stats(&before);
	auto sp = std::make_shared<MemBase>();
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (obj %zu + ctrl block ~%zu)\n",
	       "make_shared<MemBase> heap cost", delta, sizeof(MemBase),
	       delta - sizeof(MemBase));
	sp.reset();

	/* shared_ptr(new T): object + separate control block (2 allocs) */
	heap_stats(&before);
	std::shared_ptr<MemBase> sp2(new MemBase());
	heap_stats(&after);
	delta = after.allocated_bytes - before.allocated_bytes;
	printk("  %-40s: %4zu bytes (2 allocations)\n",
	       "shared_ptr(new MemBase) heap cost", delta);
	sp2.reset();

	/* Manual refcount: inline, no extra allocation */
	printk("  %-40s: %4zu bytes (inline, no ctrl block)\n",
	       "Manual atomic<int> refcount", sizeof(std::atomic<int>));
}

static void bench_heap_summary()
{
	struct sys_memory_stats stats;

	heap_stats(&stats);
	printk("\n-- Heap Summary --\n");
	printk("  %-40s: %4zu\n", "allocated_bytes", stats.allocated_bytes);
	printk("  %-40s: %4zu\n", "free_bytes", stats.free_bytes);
	printk("  %-40s: %4zu\n", "max_allocated_bytes", stats.max_allocated_bytes);
}

static void bench_stl_sizes()
{
	printk("\n-- STL / Utility Sizes --\n");
	printk("  %-40s: %4zu bytes\n", "std::unique_ptr<MemBase>",
	       sizeof(std::unique_ptr<MemBase>));
	printk("  %-40s: %4zu bytes\n", "std::shared_ptr<MemBase>",
	       sizeof(std::shared_ptr<MemBase>));
	printk("  %-40s: %4zu bytes\n", "std::function<int()>",
	       sizeof(std::function<int()>));
	printk("  %-40s: %4zu bytes\n", "k_spinlock",
	       sizeof(struct k_spinlock));
}

/* ── Main ─────────────────────────────────────────────────────── */

int main(void)
{
	sys_heap_init(&bench_heap, bench_heap_mem, BENCH_HEAP_SIZE);
	heap_ready = true;

	printk("=== Memory Benchmark: C++ ===\n\n");

	bench_object_sizes();
	bench_single_alloc();
	bench_bulk_alloc();
	bench_smart_pointers();
	bench_stl_sizes();
	bench_heap_summary();

	heap_ready = false;
	printk("\nPROJECT EXECUTION SUCCESSFUL\n");
	return 0;
}
