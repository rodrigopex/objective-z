/* Behavior test: nil messaging returns 0 / NULL */
#include "unity.h"
#include "Dummy_ozh.h"
#include "oz_mem_slabs.h"

void test_retain_nil_returns_null(void)
{
	struct OZObject *result = OZObject_retain((struct OZObject *)0);
	TEST_ASSERT_NULL(result);
}

void test_release_nil_no_crash(void)
{
	OZObject_release((struct OZObject *)0);
	TEST_PASS();
}

void test_retain_count_nil_zero(void)
{
	TEST_ASSERT_EQUAL_UINT32(0, OZObject_retainCount((struct OZObject *)0));
}
