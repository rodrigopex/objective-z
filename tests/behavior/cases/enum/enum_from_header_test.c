/* Behavior test: enum constant used in method body comparison */
#include "unity.h"
#include "EnumHeaderTest_ozh.h"

void test_enum_high_priority(void)
{
	struct EnumHeaderTest *t = EnumHeaderTest_alloc();
	EnumHeaderTest_setPriority_(t, 10);
	TEST_ASSERT_TRUE(EnumHeaderTest_isHighPriority(t));
	OZObject_release((struct OZObject *)t);
}

void test_enum_low_priority(void)
{
	struct EnumHeaderTest *t = EnumHeaderTest_alloc();
	EnumHeaderTest_setPriority_(t, 1);
	TEST_ASSERT_FALSE(EnumHeaderTest_isHighPriority(t));
	OZObject_release((struct OZObject *)t);
}
