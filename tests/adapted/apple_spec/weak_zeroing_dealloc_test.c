/*
 * Behavioral spec derived from: Apple objc4/test/arr-weak.m
 * Verifies dealloc reclaims slab (weak zeroing spec — slab proxy).
 */
#include "unity.h"
#include "WeakTest_ozh.h"

void test_dealloc_reclaims_slab(void)
{
	struct WeakTest *t = WeakTest_alloc();
	WeakTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, WeakTest_reclaimOk(t));
	OZObject_release((struct OZObject *)t);
}
