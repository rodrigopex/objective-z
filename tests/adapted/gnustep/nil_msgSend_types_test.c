/*
 * Adapted from: GNUstep libobjc2 — Test/objc_msgSend.m
 * Verifies nil messaging returns nil for retain, 0 for retainCount.
 */
#include "unity.h"
#include "NilMsgTest_ozh.h"

void test_nil_init_returns_nil(void)
{
	struct NilMsgTest *t = NilMsgTest_alloc();
	NilMsgTest_testNilMessaging(t);
	TEST_ASSERT_TRUE(NilMsgTest_initReturnsNil(t));
	OZObject_release((struct OZObject *)t);
}

void test_nil_isEqual_returns_no(void)
{
	struct NilMsgTest *t = NilMsgTest_alloc();
	NilMsgTest_testNilMessaging(t);
	TEST_ASSERT_TRUE(NilMsgTest_isEqualReturnsNo(t));
	OZObject_release((struct OZObject *)t);
}
