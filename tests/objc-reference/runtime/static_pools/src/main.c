/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for static allocation pools (pool.c).
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_pool_create_pooled(int tag);
extern int test_pool_get_tag(id obj);
extern void test_pool_release_pooled(id obj);
extern id test_pool_create_unpooled(int val);
extern int test_pool_get_val(id obj);
extern void test_pool_release_unpooled(id obj);

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(static_pools, NULL, NULL, NULL, NULL, NULL);

/* Pooled class allocates successfully */
ZTEST(static_pools, test_pool_alloc)
{
	id obj = test_pool_create_pooled(1);

	zassert_not_null(obj, "pooled alloc should succeed");
	zassert_equal(test_pool_get_tag(obj), 1, "tag should be 1");

	test_pool_release_pooled(obj);
}

/* Pooled object can be freed and tag is correct */
ZTEST(static_pools, test_pool_free)
{
	id obj = test_pool_create_pooled(42);

	zassert_not_null(obj, "alloc should succeed");
	zassert_equal(test_pool_get_tag(obj), 42, "tag should be 42");

	/* Release should return block to slab, not crash */
	test_pool_release_pooled(obj);
}

/* Non-pooled class falls back to heap */
ZTEST(static_pools, test_heap_fallback)
{
	id obj = test_pool_create_unpooled(77);

	zassert_not_null(obj, "unpooled (heap) alloc should succeed");
	zassert_equal(test_pool_get_val(obj), 77, "val should be 77");

	test_pool_release_unpooled(obj);
}

/* All 4 pool slots can be allocated */
ZTEST(static_pools, test_pool_capacity)
{
	id objs[4];

	for (int i = 0; i < 4; i++) {
		objs[i] = test_pool_create_pooled(i + 1);
		zassert_not_null(objs[i], "slot alloc should succeed");
		zassert_equal(test_pool_get_tag(objs[i]), i + 1,
			      "tag should match");
	}

	/* Release all */
	for (int i = 0; i < 4; i++) {
		test_pool_release_pooled(objs[i]);
	}
}

/* Slots are reusable after free */
ZTEST(static_pools, test_pool_alloc_free_cycle)
{
	/* Allocate all 4 slots */
	id objs[4];

	for (int i = 0; i < 4; i++) {
		objs[i] = test_pool_create_pooled(i);
		zassert_not_null(objs[i], "initial alloc should succeed");
	}

	/* Free all */
	for (int i = 0; i < 4; i++) {
		test_pool_release_pooled(objs[i]);
	}

	/* Allocate again — slots should be reused */
	for (int i = 0; i < 4; i++) {
		objs[i] = test_pool_create_pooled(100 + i);
		zassert_not_null(objs[i], "reuse alloc should succeed");
		zassert_equal(test_pool_get_tag(objs[i]), 100 + i,
			      "reused tag should match");
	}

	/* Cleanup */
	for (int i = 0; i < 4; i++) {
		test_pool_release_pooled(objs[i]);
	}
}
