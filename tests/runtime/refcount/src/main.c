/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for MRR reference counting (refcount.c, OZObject, OZAutoreleasePool).
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_create_sensor(int tag);
extern id test_retain(id obj);
extern void test_release(id obj);
extern id test_autorelease(id obj);
extern unsigned int test_retainCount(id obj);
extern void test_dealloc_obj(id obj);
extern void test_reset_dealloc_tracking(void);
extern void *test_pool_push(void);
extern void test_pool_pop(void *pool);
extern int test_get_tag(id obj);

/* ── Global dealloc tracking (defined in helpers.m) ─────────────── */

extern int g_dealloc_count;
extern int g_dealloc_order[16];
extern int g_dealloc_order_idx;

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(refcount, NULL, NULL, NULL, NULL, NULL);

/* Freshly allocated object has retainCount == 1 */
ZTEST(refcount, test_alloc_rc_one)
{
	id sensor = test_create_sensor(0);

	zassert_not_null(sensor, "alloc should succeed");
	zassert_equal(test_retainCount(sensor), 1,
		      "freshly allocated object should have retainCount 1");

	test_reset_dealloc_tracking();
	test_dealloc_obj(sensor);
}

/* retain increments retainCount */
ZTEST(refcount, test_retain_increments)
{
	id sensor = test_create_sensor(0);

	test_retain(sensor);
	zassert_equal(test_retainCount(sensor), 2,
		      "retain should increment retainCount to 2");

	/* Release back to 1, then release to dealloc */
	test_release(sensor);
	test_reset_dealloc_tracking();
	test_dealloc_obj(sensor);
}

/* release decrements retainCount */
ZTEST(refcount, test_release_decrements)
{
	id sensor = test_create_sensor(0);

	test_retain(sensor);
	zassert_equal(test_retainCount(sensor), 2,
		      "retainCount should be 2 after retain");

	test_release(sensor);
	zassert_equal(test_retainCount(sensor), 1,
		      "retainCount should be 1 after release");

	/* Final release triggers dealloc */
	test_reset_dealloc_tracking();
	test_dealloc_obj(sensor);
}

/* Releasing to zero triggers dealloc */
ZTEST(refcount, test_release_to_zero_deallocs)
{
	id sensor = test_create_sensor(0);

	test_reset_dealloc_tracking();
	test_release(sensor);

	zassert_equal(g_dealloc_count, 1,
		      "release to zero should trigger dealloc");
}

/* Retaining nil should not crash */
ZTEST(refcount, test_retain_nil)
{
	id result = test_retain(nil);

	zassert_is_null(result, "retain(nil) should return nil");
}

/* Releasing nil should not crash */
ZTEST(refcount, test_release_nil)
{
	test_release(nil);
	/* If we reach here, no crash occurred */
}

/* retainCount accuracy across multiple retains and releases */
ZTEST(refcount, test_retainCount_accuracy)
{
	id sensor = test_create_sensor(0);

	/* Retain 5 times: rc should be 6 */
	for (int i = 0; i < 5; i++) {
		test_retain(sensor);
	}
	zassert_equal(test_retainCount(sensor), 6,
		      "retainCount should be 6 after 5 retains");

	/* Release 3 times: rc should be 3 */
	for (int i = 0; i < 3; i++) {
		test_release(sensor);
	}
	zassert_equal(test_retainCount(sensor), 3,
		      "retainCount should be 3 after 3 releases");

	/* Release 3 more: should dealloc on the last one */
	test_reset_dealloc_tracking();
	for (int i = 0; i < 3; i++) {
		test_release(sensor);
	}
	zassert_equal(g_dealloc_count, 1,
		      "final release should trigger dealloc");
}

/* Autorelease drains on pool pop */
ZTEST(refcount, test_autorelease_drains)
{
	test_reset_dealloc_tracking();

	void *pool = test_pool_push();
	id sensor = test_create_sensor(0);

	test_autorelease(sensor);
	zassert_equal(g_dealloc_count, 0,
		      "should not dealloc before pool pop");

	test_pool_pop(pool);
	zassert_equal(g_dealloc_count, 1,
		      "pool pop should trigger dealloc of autoreleased object");
}

/* Nested pools: inner pop only drains inner objects */
ZTEST(refcount, test_nested_pools)
{
	test_reset_dealloc_tracking();

	void *outer = test_pool_push();
	id sensor1 = test_create_sensor(1);

	test_autorelease(sensor1);

	void *inner = test_pool_push();
	id sensor2 = test_create_sensor(2);

	test_autorelease(sensor2);

	/* Pop inner: only sensor2 should be deallocated */
	test_pool_pop(inner);
	zassert_equal(g_dealloc_count, 1,
		      "inner pool pop should dealloc only sensor2");

	/* Pop outer: sensor1 should now be deallocated */
	test_pool_pop(outer);
	zassert_equal(g_dealloc_count, 2,
		      "outer pool pop should dealloc sensor1");
}

/* Pool drains in LIFO order */
ZTEST(refcount, test_drain_lifo_order)
{
	test_reset_dealloc_tracking();

	void *pool = test_pool_push();

	id s1 = test_create_sensor(1);
	test_autorelease(s1);

	id s2 = test_create_sensor(2);
	test_autorelease(s2);

	id s3 = test_create_sensor(3);
	test_autorelease(s3);

	test_pool_pop(pool);

	zassert_equal(g_dealloc_count, 3,
		      "all three sensors should be deallocated");
	zassert_equal(g_dealloc_order[0], 3,
		      "first dealloc should be tag 3 (LIFO)");
	zassert_equal(g_dealloc_order[1], 2,
		      "second dealloc should be tag 2 (LIFO)");
	zassert_equal(g_dealloc_order[2], 1,
		      "third dealloc should be tag 1 (LIFO)");
}

/* Multiple retain/release cycle ending in dealloc */
ZTEST(refcount, test_multiple_retain_release)
{
	id sensor = test_create_sensor(0);

	/* Retain 10 times: rc should be 11 */
	for (int i = 0; i < 10; i++) {
		test_retain(sensor);
	}
	zassert_equal(test_retainCount(sensor), 11,
		      "retainCount should be 11 after 10 retains");

	/* Release 10 times: rc should be 1 */
	for (int i = 0; i < 10; i++) {
		test_release(sensor);
	}
	zassert_equal(test_retainCount(sensor), 1,
		      "retainCount should be 1 after 10 releases");

	/* Final release triggers dealloc */
	test_reset_dealloc_tracking();
	test_release(sensor);
	zassert_equal(g_dealloc_count, 1,
		      "final release should trigger dealloc");
}
