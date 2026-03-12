/* Behavior test: releasing nil is safe (no crash) */
#include "unity.h"
#include "Marker_ozh.h"

void test_release_nil_no_crash(void)
{
	OZObject_release((struct OZObject *)0);
	TEST_PASS();
}

void test_retain_nil_returns_null(void)
{
	struct OZObject *result = OZObject_retain((struct OZObject *)0);
	TEST_ASSERT_NULL(result);
}

void test_retain_count_nil_is_zero(void)
{
	TEST_ASSERT_EQUAL_UINT32(0, OZObject_retainCount((struct OZObject *)0));
}
