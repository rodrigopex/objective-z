/*
 * Behavioral spec: nil messaging returns zero.
 * Original test — no Apple code.
 */
#include "unity.h"
#include "Widget_ozh.h"
#include "oz_mem_slabs.h"

void test_nil_int_return_zero(void)
{
	/* Calling retain on nil returns NULL (0) */
	struct OZObject *result = OZObject_retain((struct OZObject *)0);
	TEST_ASSERT_NULL(result);
}

void test_nil_release_safe(void)
{
	OZObject_release((struct OZObject *)0);
	TEST_PASS();
}

void test_nil_retaincount_zero(void)
{
	uint32_t rc = OZObject_retainCount((struct OZObject *)0);
	TEST_ASSERT_EQUAL_UINT32(0, rc);
}
