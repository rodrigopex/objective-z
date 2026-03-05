/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Heap shim for Zig memory benchmark
 *
 * Provides a dedicated sys_heap instance with runtime stats.
 * Zig calls these via @cImport extern declarations.
 */

#include <zephyr/sys/sys_heap.h>
#include <stddef.h>
#include <stdint.h>

#define BENCH_HEAP_SIZE 8192

static char bench_heap_mem[BENCH_HEAP_SIZE] __aligned(8);
static struct sys_heap bench_heap;

void heap_shim_init(void)
{
	sys_heap_init(&bench_heap, bench_heap_mem, BENCH_HEAP_SIZE);
}

void *heap_shim_alloc(size_t size, size_t align)
{
	return sys_heap_aligned_alloc(&bench_heap, align, size);
}

void heap_shim_free(void *ptr)
{
	if (ptr) {
		sys_heap_free(&bench_heap, ptr);
	}
}

void heap_shim_stats(size_t *allocated, size_t *free_bytes, size_t *max_allocated)
{
	struct sys_memory_stats s;

	sys_heap_runtime_stats_get(&bench_heap, &s);
	*allocated = s.allocated_bytes;
	*free_bytes = s.free_bytes;
	*max_allocated = s.max_allocated_bytes;
}
