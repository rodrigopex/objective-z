/*
 * Behavioral spec derived from: ObjFW OFStringTests.m
 * Verifies OZString equality/hash contract.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "StrEqTest_ozh.h"

void test_string_equality(void)
{
	struct StrEqTest *t = StrEqTest_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)t);
	StrEqTest_run(t);
	TEST_ASSERT_TRUE(StrEqTest_equalResult(t));
	OZObject_release((struct OZObject *)t);
}
