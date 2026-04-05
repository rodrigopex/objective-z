/*
 * Behavioral spec derived from: ObjFW OFObjectTests.m
 * Verifies OZObject equality contract: self-equal, distinct-not-equal.
 */
#include "unity.h"
#include "EqTest_ozh.h"

void test_object_equal_to_self(void)
{
	struct EqTest *t = EqTest_alloc();
	EqTest_run(t);
	TEST_ASSERT_TRUE(EqTest_selfEqual(t));
	OZObject_release((struct OZObject *)t);
}

void test_distinct_objects_not_equal(void)
{
	struct EqTest *t = EqTest_alloc();
	EqTest_run(t);
	TEST_ASSERT_TRUE(EqTest_distinctNotEqual(t));
	OZObject_release((struct OZObject *)t);
}
