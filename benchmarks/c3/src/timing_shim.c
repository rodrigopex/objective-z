/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Timing shim for C3 FFI
 *
 * Only wraps Zephyr's static inline timing helpers that have no
 * real linker symbol. Real functions (timing_init, timing_start,
 * timing_stop, k_malloc, k_free, printk) are called directly
 * from C3 via extern fn.
 */
#include <zephyr/kernel.h>
#include <zephyr/timing/timing.h>
#include <stdint.h>

uint32_t timing_shim_counter_get(void)
{
	return (uint32_t)timing_counter_get();
}

uint64_t timing_shim_cycles_between(uint32_t start, uint32_t end)
{
	timing_t s = start;
	timing_t e = end;
	return timing_cycles_get(&s, &e);
}

uint64_t timing_shim_cycles_to_ns(uint64_t cycles)
{
	return timing_cycles_to_ns(cycles);
}
