/*
 * Behavioral spec derived from: mulle-objc runtime lifecycle patterns
 * Verifies alloc → init → use → release lifecycle order.
 */
#include "unity.h"
#include "LifecycleTest_ozh.h"

void test_lifecycle_order(void)
{
	struct LifecycleTest *t = LifecycleTest_alloc();
	LifecycleTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, LifecycleTest_initStage(t));
	TEST_ASSERT_EQUAL_INT(2, LifecycleTest_useStage(t));
	OZObject_release((struct OZObject *)t);
}
