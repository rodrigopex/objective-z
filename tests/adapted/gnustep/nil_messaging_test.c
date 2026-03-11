/*
 * Adapted from: GNUstep libobjc2 — Test/NilTest.m
 * License: MIT
 * Verifies nil receiver returns zero for various return types.
 */
#include "unity.h"
#include "Target_ozh.h"
#include "oz_mem_slabs.h"

void test_nil_retain_returns_null(void)
{
	struct OZObject *result = OZObject_retain((struct OZObject *)0);
	TEST_ASSERT_NULL(result);
}

void test_nil_release_no_crash(void)
{
	OZObject_release((struct OZObject *)0);
	TEST_PASS();
}

void test_nil_retaincount_returns_zero(void)
{
	TEST_ASSERT_EQUAL_UINT32(0, OZObject_retainCount((struct OZObject *)0));
}
