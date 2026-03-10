/* Behavior test: retainCount returns correct value at each step */
#include "unity.h"
#include "Tracker.h"
#include "oz_mem_slabs.h"

void test_retain_count_query(void)
{
	struct Tracker *t = Tracker_alloc();
	TEST_ASSERT_EQUAL_UINT32(1, OZObject_retainCount((struct OZObject *)t));

	OZObject_retain((struct OZObject *)t);
	TEST_ASSERT_EQUAL_UINT32(2, OZObject_retainCount((struct OZObject *)t));

	OZObject_release((struct OZObject *)t);
	TEST_ASSERT_EQUAL_UINT32(1, OZObject_retainCount((struct OZObject *)t));

	OZObject_release((struct OZObject *)t);
}

void test_retain_count_nil_returns_zero(void)
{
	TEST_ASSERT_EQUAL_UINT32(0, OZObject_retainCount((struct OZObject *)0));
}
