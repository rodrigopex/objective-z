/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file pools.c
 * @brief Static pool definitions and slab stat wrappers for ARC intensive tests.
 *
 * Must be a .c file (not .m) because OZ_DEFINE_POOL uses K_MEM_SLAB_DEFINE_STATIC
 * which requires GCC compilation.
 */
#include <objc/pool.h>

/*
 * ArcPoolObj layout: isa(4) + _refcount(4) + _tag(4) = 12 bytes
 * 8 blocks, 4-byte aligned
 */
OZ_DEFINE_POOL(ArcPoolObj, 12, 8, 4);

/* ── Slab stat wrappers ──────────────────────────────────────────── */

uint32_t test_pool_slab_used(void)
{
	return k_mem_slab_num_used_get(&_objz_pool_ArcPoolObj);
}

uint32_t test_pool_slab_free(void)
{
	return k_mem_slab_num_free_get(&_objz_pool_ArcPoolObj);
}
