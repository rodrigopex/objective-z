/**
 * @file dispatch.h
 * @brief Global flat dispatch table public declarations.
 *
 * Defines the selector init entry type used by the build-time
 * generated dispatch_init.c.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once

#include <stdint.h>

/**
 * @brief Selector init entry for the generated dispatch_init.c.
 *
 * Each entry maps a selector name string to a numeric sel_id
 * assigned at build time by objz_gen_table_sizes.py.
 */
struct objz_sel_init_entry {
	const char *name;
	uint16_t sel_id;
};
