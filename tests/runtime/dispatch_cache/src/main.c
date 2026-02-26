/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Tests for per-class dispatch table cache.
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── Extern helpers from helpers.m ───────────────────────────────── */

extern id test_cache_create_base(void);
extern id test_cache_create_child(void);
extern id test_cache_create_grandchild(void);
extern id test_cache_create_peer(void);
extern void test_cache_dealloc(id obj);
extern int test_cache_call_value(id obj);
extern int test_cache_call_shared(id obj);
extern int test_cache_call_class_value(void);
extern int test_cache_call_child_only(id obj);

ZTEST_SUITE(dispatch_cache, NULL, NULL, NULL, NULL, NULL);

/**
 * Direct method dispatch works and returns correct value.
 * Second call exercises cache hit path.
 */
ZTEST(dispatch_cache, test_direct_method)
{
	id obj = test_cache_create_base();

	zassert_not_null(obj, "alloc failed");

	/* First call: cache miss, populates dtable */
	int v1 = test_cache_call_value(obj);

	zassert_equal(v1, 10, "first call returned %d", v1);

	/* Second call: cache hit */
	int v2 = test_cache_call_value(obj);

	zassert_equal(v2, 10, "cached call returned %d", v2);

	test_cache_dealloc(obj);
}

/**
 * Class method dispatch works with cache.
 */
ZTEST(dispatch_cache, test_class_method)
{
	int v1 = test_cache_call_class_value();

	zassert_equal(v1, 42, "first class call returned %d", v1);

	int v2 = test_cache_call_class_value();

	zassert_equal(v2, 42, "cached class call returned %d", v2);
}

/**
 * Inherited method at depth=1 resolves correctly.
 * Cache stores the IMP at the receiver's class level.
 */
ZTEST(dispatch_cache, test_inherited_depth1)
{
	id child = test_cache_create_child();

	zassert_not_null(child, "alloc failed");

	/* -value is inherited from CacheBase */
	int v1 = test_cache_call_value(child);

	zassert_equal(v1, 10, "inherited call returned %d", v1);

	/* Second call should hit child's dtable */
	int v2 = test_cache_call_value(child);

	zassert_equal(v2, 10, "cached inherited call returned %d", v2);

	/* Child's own method also works */
	int own = test_cache_call_child_only(child);

	zassert_equal(own, 20, "child own method returned %d", own);

	test_cache_dealloc(child);
}

/**
 * Inherited method at depth=2 resolves correctly.
 */
ZTEST(dispatch_cache, test_inherited_depth2)
{
	id gchild = test_cache_create_grandchild();

	zassert_not_null(gchild, "alloc failed");

	/* -value is defined in CacheBase, inherited through CacheChild */
	int v1 = test_cache_call_value(gchild);

	zassert_equal(v1, 10, "depth=2 inherited returned %d", v1);

	int v2 = test_cache_call_value(gchild);

	zassert_equal(v2, 10, "cached depth=2 returned %d", v2);

	test_cache_dealloc(gchild);
}

/**
 * Category override returns the overridden value.
 * The category replaces CacheBase -shared (100 -> 999).
 */
ZTEST(dispatch_cache, test_category_override)
{
	id obj = test_cache_create_base();

	zassert_not_null(obj, "alloc failed");

	/* Category should have replaced -shared */
	int v1 = test_cache_call_shared(obj);

	zassert_equal(v1, 999, "category override returned %d", v1);

	/* Cached call should also return overridden value */
	int v2 = test_cache_call_shared(obj);

	zassert_equal(v2, 999, "cached category override returned %d", v2);

	test_cache_dealloc(obj);
}

/**
 * Two classes with the same selector name do not cross-contaminate.
 * CacheBase -value returns 10, CachePeer -value returns 77.
 */
ZTEST(dispatch_cache, test_no_cross_contamination)
{
	id base = test_cache_create_base();
	id peer = test_cache_create_peer();

	zassert_not_null(base, "base alloc failed");
	zassert_not_null(peer, "peer alloc failed");

	/* Populate base's cache */
	int bv = test_cache_call_value(base);

	zassert_equal(bv, 10, "base value %d", bv);

	/* Populate peer's cache — must not return base's value */
	int pv = test_cache_call_value(peer);

	zassert_equal(pv, 77, "peer value %d", pv);

	/* Verify again after both caches are warm */
	zassert_equal(test_cache_call_value(base), 10, "base cached");
	zassert_equal(test_cache_call_value(peer), 77, "peer cached");

	test_cache_dealloc(base);
	test_cache_dealloc(peer);
}

/**
 * Inherited -shared is overridden by category on CacheBase.
 * CacheGrandChild inheriting through the chain should see 999.
 * CachePeer has its own -shared returning 200.
 */
ZTEST(dispatch_cache, test_inherited_category_override)
{
	id gchild = test_cache_create_grandchild();
	id peer = test_cache_create_peer();

	zassert_not_null(gchild, "gchild alloc failed");
	zassert_not_null(peer, "peer alloc failed");

	int gv = test_cache_call_shared(gchild);

	zassert_equal(gv, 999, "inherited category override returned %d", gv);

	int pv = test_cache_call_shared(peer);

	zassert_equal(pv, 200, "peer shared returned %d", pv);

	test_cache_dealloc(gchild);
	test_cache_dealloc(peer);
}
