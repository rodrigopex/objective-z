/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file pools.c
 * @brief Pool definitions for static pool tests.
 *
 * OZ_DEFINE_POOL must be in a .c file (not .m) because it uses
 * Zephyr's K_MEM_SLAB_DEFINE and SYS_INIT macros.
 *
 * TestPooled : Object has: isa (4) + _refcount (4) + _tag (4) = 12 bytes.
 * We define 4 slots of 12 bytes each, 4-byte aligned.
 */
#include <objc/pool.h>

OZ_DEFINE_POOL(TestPooled, 12, 4, 4);
