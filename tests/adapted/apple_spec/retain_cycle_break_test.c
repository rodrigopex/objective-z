/*
 * Behavioral spec derived from: Apple ARC documentation
 * Verifies two objects can be created, used, and released cleanly.
 */
#include "unity.h"
#include "CycleTest_ozh.h"

void test_objects_usable_before_release(void)
{
	struct CycleTest *t = CycleTest_alloc();
	CycleTest_run(t);
	TEST_ASSERT_EQUAL_INT(1, CycleTest_aTag(t));
	TEST_ASSERT_EQUAL_INT(2, CycleTest_bTag(t));
	OZObject_release((struct OZObject *)t);
}
