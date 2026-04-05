/*
 * Behavioral spec derived from: ObjFW OFArrayTests.m
 * Verifies OZArray count, indexing, and out-of-bounds returns nil.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "ArrIdxTest_ozh.h"

void test_array_count(void)
{
	struct ArrIdxTest *t = ArrIdxTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	ArrIdxTest_run(t);
	TEST_ASSERT_EQUAL_UINT(3, ArrIdxTest_count(t));
	OZObject_release((struct OZObject *)t);
}

void test_array_first_last(void)
{
	struct ArrIdxTest *t = ArrIdxTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	ArrIdxTest_run(t);
	TEST_ASSERT_EQUAL_INT(10, ArrIdxTest_first(t));
	TEST_ASSERT_EQUAL_INT(30, ArrIdxTest_last(t));
	OZObject_release((struct OZObject *)t);
}

void test_array_out_of_bounds(void)
{
	struct ArrIdxTest *t = ArrIdxTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	ArrIdxTest_run(t);
	TEST_ASSERT_TRUE(ArrIdxTest_oobNil(t));
	OZObject_release((struct OZObject *)t);
}
