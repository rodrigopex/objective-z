/*
 * Behavioral spec derived from: ObjFW OFStringTests.m
 * Verifies OZString length returns correct count and cString is valid.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "StrLenTest_ozh.h"

void test_string_length(void)
{
	struct StrLenTest *t = StrLenTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	StrLenTest_run(t);
	TEST_ASSERT_EQUAL_UINT(5, StrLenTest_len(t));
	OZObject_release((struct OZObject *)t);
}

void test_string_cstring_valid(void)
{
	struct StrLenTest *t = StrLenTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	StrLenTest_run(t);
	TEST_ASSERT_TRUE(StrLenTest_cStringValid(t));
	OZObject_release((struct OZObject *)t);
}
