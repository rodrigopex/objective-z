/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for ARC entry points (arc.c).
 *
 * Exercises the C-level ARC functions (objc_retain, objc_release,
 * objc_storeStrong, objc_retainAutorelease) and verifies that
 * ARC-compiled code correctly releases objects at end of scope.
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>
#include <objc/arc.h>

/* ── MRR helpers (defined in helpers.m) ────────────────────────── */

extern id test_arc_create_obj(void);
extern unsigned int test_arc_get_rc(id obj);
extern void test_arc_reset_count(void);
extern void *test_arc_pool_push(void);
extern void test_arc_pool_pop(void *p);

/* ── ARC helpers (defined in arc_helpers.m, compiled with -fobjc-arc) */

extern void test_arc_scope_cleanup(void);
extern void test_arc_atomic_property(void);

/* ── Property helpers (defined in helpers.m) ───────────────────── */

extern id test_prop_create(void);
extern ptrdiff_t test_prop_offset(void);
extern void *test_prop_read_ivar(id obj);
extern void test_prop_write_ivar(id obj, id val);

/* ── Dealloc tracking counter ──────────────────────────────────── */

extern int g_arc_dealloc_count;

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(arc, NULL, NULL, NULL, NULL, NULL);

/* objc_retain(nil) returns nil without crashing */
ZTEST(arc, test_arc_retain_nil)
{
	id result = objc_retain(nil);

	zassert_is_null(result, "objc_retain(nil) should return nil");
}

/* objc_release(nil) does not crash */
ZTEST(arc, test_arc_release_nil)
{
	objc_release(nil);

	/* If we get here, it did not crash */
	zassert_true(true, "objc_release(nil) should not crash");
}

/* objc_retain increments refcount, objc_release decrements it */
ZTEST(arc, test_arc_retain_release)
{
	id obj = test_arc_create_obj();

	zassert_not_null(obj, "alloc should succeed");
	zassert_equal(test_arc_get_rc(obj), 1, "initial rc should be 1");

	objc_retain(obj);
	zassert_equal(test_arc_get_rc(obj), 2, "rc after retain should be 2");

	objc_release(obj);
	zassert_equal(test_arc_get_rc(obj), 1, "rc after release should be 1");

	/* Final cleanup */
	objc_release(obj);
}

/* objc_release triggers dealloc when refcount reaches 0 */
ZTEST(arc, test_arc_release_deallocs)
{
	test_arc_reset_count();

	id obj = test_arc_create_obj();

	zassert_not_null(obj, "alloc should succeed");
	zassert_equal(g_arc_dealloc_count, 0, "no dealloc yet");

	objc_release(obj);
	zassert_equal(g_arc_dealloc_count, 1,
		      "dealloc should be called when rc reaches 0");
}

/*
 * objc_storeStrong: swap location from obj_a to obj_b.
 *
 * storeStrong retains the new value and releases the old one.
 *   - storeStrong(&loc, a): retain(a) rc 1->2, store a
 *   - storeStrong(&loc, b): retain(b) rc 1->2, store b, release(a) rc 2->1
 */
ZTEST(arc, test_storeStrong_swap)
{
	id obj_a = test_arc_create_obj();
	id obj_b = test_arc_create_obj();
	id loc = nil;

	zassert_equal(test_arc_get_rc(obj_a), 1, "obj_a initial rc should be 1");
	zassert_equal(test_arc_get_rc(obj_b), 1, "obj_b initial rc should be 1");

	/* Store obj_a into loc */
	objc_storeStrong(&loc, obj_a);
	zassert_equal(test_arc_get_rc(obj_a), 2,
		      "obj_a rc should be 2 after storeStrong");

	/* Swap to obj_b — retains b, releases a */
	objc_storeStrong(&loc, obj_b);
	zassert_equal(test_arc_get_rc(obj_a), 1,
		      "obj_a rc should be 1 after swap");
	zassert_equal(test_arc_get_rc(obj_b), 2,
		      "obj_b rc should be 2 after swap");

	/* Cleanup: clear loc (releases b: rc 2->1), then release both */
	objc_storeStrong(&loc, nil);
	objc_release(obj_a);
	objc_release(obj_b);
}

/*
 * objc_storeStrong with same object is a no-op.
 *
 * storeStrong checks val == old and returns immediately if equal.
 */
ZTEST(arc, test_storeStrong_same)
{
	id obj = test_arc_create_obj();
	id loc = nil;

	/* First store: retain obj (rc 1->2) */
	objc_storeStrong(&loc, obj);
	zassert_equal(test_arc_get_rc(obj), 2,
		      "rc should be 2 after first storeStrong");

	/* Same object: should be a no-op */
	objc_storeStrong(&loc, obj);
	zassert_equal(test_arc_get_rc(obj), 2,
		      "rc should still be 2 after storing same object");

	/* Cleanup */
	objc_storeStrong(&loc, nil);
	objc_release(obj);
}

/*
 * objc_storeStrong(&loc, nil) clears the location and releases the
 * held object.
 */
ZTEST(arc, test_storeStrong_nil)
{
	test_arc_reset_count();

	id obj = test_arc_create_obj();
	id loc = nil;

	/* Store obj into loc: retain (rc 1->2) */
	objc_storeStrong(&loc, obj);
	zassert_equal(test_arc_get_rc(obj), 2,
		      "rc should be 2 after storeStrong");

	/* Clear loc: releases obj (rc 2->1) */
	objc_storeStrong(&loc, nil);
	zassert_is_null(loc, "loc should be nil after clearing");
	zassert_equal(test_arc_get_rc(obj), 1,
		      "rc should be 1 after storeStrong nil");

	/* Final release triggers dealloc */
	objc_release(obj);
	zassert_equal(g_arc_dealloc_count, 1,
		      "dealloc should be called after final release");
}

/*
 * objc_retainAutorelease: retain + autorelease.
 *
 * Push a pool, create obj (rc=1), retainAutorelease (rc=2, one
 * owned by pool), pop pool (autorelease fires, rc=1).
 */
ZTEST(arc, test_retainAutorelease)
{
	void *pool = test_arc_pool_push();

	id obj = test_arc_create_obj();

	zassert_equal(test_arc_get_rc(obj), 1, "initial rc should be 1");

	id ret = objc_retainAutorelease(obj);
	zassert_equal(ret, obj, "retainAutorelease should return same object");
	zassert_equal(test_arc_get_rc(obj), 2,
		      "rc should be 2 after retainAutorelease");

	/* Pop pool — the autoreleased reference is released */
	test_arc_pool_pop(pool);
	zassert_equal(test_arc_get_rc(obj), 1,
		      "rc should be 1 after pool drain");

	/* Cleanup */
	objc_release(obj);
}

/*
 * ARC scope cleanup: an ARC-compiled function that creates a local
 * object should release it when the function returns.
 */
ZTEST(arc, test_arc_scope_cleanup)
{
	test_arc_reset_count();
	zassert_equal(g_arc_dealloc_count, 0, "count should start at 0");

	test_arc_scope_cleanup();

	zassert_equal(g_arc_dealloc_count, 1,
		      "ARC should release object when scope ends");
}

/* ── Property accessor tests ───────────────────────────────────── */

ZTEST_SUITE(arc_property, NULL, NULL, NULL, NULL, NULL);

/* objc_getProperty(nil, ...) returns nil */
ZTEST(arc_property, test_getProperty_nil)
{
	ptrdiff_t off = test_prop_offset();
	id result = objc_getProperty(nil, NULL, off, NO);

	zassert_is_null(result, "getProperty(nil) should return nil");
}

/* objc_setProperty(nil, ...) does not crash */
ZTEST(arc_property, test_setProperty_nil)
{
	ptrdiff_t off = test_prop_offset();
	id dummy = test_arc_create_obj();

	objc_setProperty(nil, NULL, off, dummy, NO, NO);

	/* If we get here, it did not crash */
	zassert_true(true, "setProperty(nil) should not crash");

	objc_release(dummy);
}

/* Non-atomic getProperty reads the ivar directly */
ZTEST(arc_property, test_getProperty_nonatomic)
{
	id obj = test_prop_create();
	id val = test_arc_create_obj();
	ptrdiff_t off = test_prop_offset();

	/* Write directly into the ivar */
	test_prop_write_ivar(obj, val);

	id result = objc_getProperty(obj, NULL, off, NO);

	zassert_equal(result, val,
		      "non-atomic getProperty should return ivar value");

	/* Cleanup: nil out ivar before releasing obj */
	test_prop_write_ivar(obj, nil);
	objc_release(val);
	objc_release(obj);
}

/* Non-atomic setProperty stores value, retains new, releases old */
ZTEST(arc_property, test_setProperty_nonatomic)
{
	id obj = test_prop_create();
	id val = test_arc_create_obj();
	ptrdiff_t off = test_prop_offset();

	zassert_equal(test_arc_get_rc(val), 1, "val initial rc should be 1");

	objc_setProperty(obj, NULL, off, val, NO, NO);
	zassert_equal(test_arc_get_rc(val), 2,
		      "val rc should be 2 after setProperty");

	id stored = test_prop_read_ivar(obj);

	zassert_equal(stored, val,
		      "ivar should hold the stored value");

	/* Clear property (releases val: rc 2->1) */
	objc_setProperty(obj, NULL, off, nil, NO, NO);
	zassert_equal(test_arc_get_rc(val), 1,
		      "val rc should be 1 after clearing property");

	objc_release(val);
	objc_release(obj);
}

/* Atomic getProperty retains + autoreleases the returned value */
ZTEST(arc_property, test_getProperty_atomic)
{
	void *pool = test_arc_pool_push();
	id obj = test_prop_create();
	id val = test_arc_create_obj();
	ptrdiff_t off = test_prop_offset();

	/* Manually set ivar and retain for ownership */
	objc_retain(val);
	test_prop_write_ivar(obj, val);

	zassert_equal(test_arc_get_rc(val), 2,
		      "val rc should be 2 (alloc + manual retain)");

	id result = objc_getProperty(obj, NULL, off, YES);

	zassert_equal(result, val,
		      "atomic getProperty should return ivar value");
	/* retain inside lock + autorelease = rc goes up by 1 then pool owns it */
	zassert_equal(test_arc_get_rc(val), 3,
		      "val rc should be 3 (alloc + ivar + autorelease-retained)");

	/* Drain pool: autoreleased ref released (rc 3->2) */
	test_arc_pool_pop(pool);
	zassert_equal(test_arc_get_rc(val), 2,
		      "val rc should be 2 after pool drain");

	/* Cleanup */
	test_prop_write_ivar(obj, nil);
	objc_release(val); /* ivar ownership */
	objc_release(val); /* alloc ownership */
	objc_release(obj);
}

/* Atomic setProperty swaps under lock, retains new, releases old */
ZTEST(arc_property, test_setProperty_atomic)
{
	id obj = test_prop_create();
	id val = test_arc_create_obj();
	ptrdiff_t off = test_prop_offset();

	zassert_equal(test_arc_get_rc(val), 1, "val initial rc should be 1");

	objc_setProperty(obj, NULL, off, val, YES, NO);
	zassert_equal(test_arc_get_rc(val), 2,
		      "val rc should be 2 after atomic setProperty");

	id stored = test_prop_read_ivar(obj);

	zassert_equal(stored, val,
		      "ivar should hold the stored value");

	/* Clear property (releases val: rc 2->1) */
	objc_setProperty(obj, NULL, off, nil, YES, NO);
	zassert_equal(test_arc_get_rc(val), 1,
		      "val rc should be 1 after clearing property");

	objc_release(val);
	objc_release(obj);
}

/* Atomic setProperty: overwrite A with B; A refcount drops, B rises */
ZTEST(arc_property, test_setProperty_atomic_overwrites)
{
	id obj = test_prop_create();
	id val_a = test_arc_create_obj();
	id val_b = test_arc_create_obj();
	ptrdiff_t off = test_prop_offset();

	objc_setProperty(obj, NULL, off, val_a, YES, NO);
	zassert_equal(test_arc_get_rc(val_a), 2,
		      "val_a rc should be 2 after set");

	objc_setProperty(obj, NULL, off, val_b, YES, NO);
	zassert_equal(test_arc_get_rc(val_a), 1,
		      "val_a rc should be 1 after overwrite");
	zassert_equal(test_arc_get_rc(val_b), 2,
		      "val_b rc should be 2 after overwrite");

	/* Cleanup */
	objc_setProperty(obj, NULL, off, nil, YES, NO);
	objc_release(val_a);
	objc_release(val_b);
	objc_release(obj);
}

/*
 * ARC integration: exercise actual Clang codegen for
 * @property (atomic, strong) id thing;
 */
ZTEST(arc_property, test_arc_atomic_property)
{
	test_arc_reset_count();

	test_arc_atomic_property();

	zassert_equal(g_arc_dealloc_count, 2,
		      "both PropHolder and stored object should be deallocated");
}
