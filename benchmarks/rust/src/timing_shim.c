/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Timing shim for Rust FFI
 *
 * Wraps Zephyr's static inline timing functions as real symbols
 * callable from Rust via extern "C".
 */
#include <zephyr/timing/timing.h>
#include <stdint.h>

void timing_shim_init(void)
{
        timing_init();
}

void timing_shim_start(void)
{
        timing_start();
}

void timing_shim_stop(void)
{
        timing_stop();
}

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
