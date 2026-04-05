/*
 * Adapted from: tests/objc-reference/runtime/static_pools/src/main.c
 * Verifies slab block is returned on free and reusable.
 */
#include "unity.h"
#include "PoolReuseTest_ozh.h"

void test_slab_pool_reuse(void)
{
	struct PoolReuseTest *t = PoolReuseTest_alloc();
	PoolReuseTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, PoolReuseTest_reuseOk(t));
	OZObject_release((struct OZObject *)t);
}
