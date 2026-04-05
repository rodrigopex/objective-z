/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 * Verifies scope-exit releases local objects (slab reuse proves it).
 */
#include "unity.h"
#include "ScopeTest_ozh.h"

void test_arc_scope_cleanup(void)
{
	struct ScopeTest *t = ScopeTest_alloc();
	ScopeTest_testScopeCleanup(t);
	TEST_ASSERT_EQUAL_INT(1, ScopeTest_canRealloc(t));
	OZObject_release((struct OZObject *)t);
}
